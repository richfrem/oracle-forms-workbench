//! Record group built-ins: `CREATE_GROUP`, `CREATE_GROUP_FROM_QUERY`, `POPULATE_GROUP`,
//! `POPULATE_GROUP_WITH_QUERY`, `FIND_GROUP`, `FIND_COLUMN`, `ADD_GROUP_COLUMN`,
//! `ADD_GROUP_ROW`, `GET_GROUP_CHAR/NUMBER/DATE_CELL`, `SET_GROUP_CHAR/NUMBER/DATE_CELL`,
//! `GET_GROUP_ROW_COUNT`, `DELETE_GROUP`, `DELETE_GROUP_ROW`, and the selection built-ins
//! `SET_GROUP_SELECTION(group, row)`, `RESET_GROUP_SELECTION(group[, row])`,
//! `UNSET_GROUP_SELECTION(group, row)`, `GET_GROUP_SELECTION(group, n)`,
//! `GET_GROUP_SELECTION_COUNT(group)`, `GET_GROUP_RECORD_NUMBER(group, n)`.
//! Selections are row numbers kept in selection order; `GET_GROUP_SELECTION` and
//! `GET_GROUP_RECORD_NUMBER` return the row number of the Nth selection (NULL when there is
//! none; in this model both give the same number, whether they differ in 14c is unverified).
//! `RESET_GROUP_SELECTION` with a row deselects it, without one clears all selections.
//! Deleting rows renumbers the selection, populating a group clears it.
//!
//! Semantics follow Forms: the `POPULATE_GROUP*` functions return `0` on success, `1403`
//! when the query returns no rows, or the Oracle error number; rows are numbered from 1;
//! cell getters/setters are type checked against the column type; only groups created at
//! run time can be deleted; `DELETE_GROUP_ROW(group, ALL_ROWS)` empties a group. Query
//! text may contain `:BLOCK.ITEM` / `:GLOBAL.X` binds. Design-time groups (from the module)
//! exist from session start; static ones are already populated.
//!
//! Purpose: Module implementation (record_groups.rs).
//! Key Input Dependencies: crate::objects, oracle_forms_db
//! Index: selection_builtin, run_query, populate_rows, populate, cell_target, dispatch, create_group, add_column, add_row, get_cell, set_cell, delete_row

use crate::builtins::BuiltinResult;
use crate::objects::{GroupColumn, GroupColumnType, RecordGroup};
use crate::session::{block_on_future, FormSession};
use crate::value::OracleValue;
use oracle_forms_db::{rewrite_binds, BoundSql, QueryResult};

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

fn key(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// Runs `sql` against the session database with `:BLOCK.ITEM` style binds resolved from the
/// session. The error is `(oracle error number, message)`.
pub fn run_query(session: &mut FormSession, sql: &str) -> Result<QueryResult, (i64, String)> {
    let Some(driver) = session.database_driver.clone() else {
        return Err((1, "a record group query needs an attached database".into()));
    };
    let rewritten = rewrite_binds(sql, driver.backend(), 1);
    let binds = rewritten
        .refs
        .iter()
        .map(|r| (r.placeholder.clone(), session.get_bind_value(&r.reference).to_sql_value()))
        .collect();
    let bound = BoundSql { sql: rewritten.sql, binds };
    session.trace_log.push(format!("SQL: {}", bound.sql));
    block_on_future(driver.query(&bound)).map_err(|e| {
        let msg = e.to_string();
        let code = msg
            .find("ORA-")
            .and_then(|i| msg[i + 4..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<i64>().ok())
            .unwrap_or(1);
        (code, msg)
    })
}

/// A column value of a query record, looked up case-insensitively.
fn value_of<'a>(rec: &'a oracle_forms_db::model::QueryRecord, col: &str) -> Option<&'a serde_json::Value> {
    rec.values.get(col).or_else(|| rec.values.iter().find(|(k, _)| k.eq_ignore_ascii_case(col)).map(|(_, v)| v))
}

/// Loads query rows into `group`. Columns are derived from the result when the group has
/// none; otherwise result columns map to group columns by position.
fn populate_rows(group: &mut RecordGroup, result: &QueryResult) {
    if group.columns.is_empty() {
        for col in &result.columns {
            let present: Vec<&serde_json::Value> = result.records.iter().filter_map(|r| value_of(r, col)).filter(|v| !v.is_null()).collect();
            let numeric = !present.is_empty() && present.iter().all(|v| v.is_number());
            group.columns.push(GroupColumn { name: col.clone(), data_type: if numeric { GroupColumnType::Number } else { GroupColumnType::Char } });
        }
    }
    group.rows = result
        .records
        .iter()
        .map(|rec| {
            group
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let src = result.columns.get(i).map(String::as_str).unwrap_or(&c.name);
                    let raw = value_of(rec, src);
                    match raw {
                        None => OracleValue::Null,
                        Some(v) => {
                            let ov = OracleValue::from_json(v);
                            match c.data_type {
                                GroupColumnType::Number if !ov.is_null() => OracleValue::number_from_str(&ov.to_string_lossy()),
                                GroupColumnType::Char | GroupColumnType::Long if !ov.is_null() => OracleValue::varchar2(ov.to_string_lossy()),
                                _ => ov,
                            }
                        }
                    }
                })
                .collect()
        })
        .collect();
    group.populated = true;
    group.selection.clear();
}

/// Populates the group named `name` from `sql`; returns the Forms status number.
pub fn populate(session: &mut FormSession, name: &str, sql: &str) -> i64 {
    match run_query(session, sql) {
        Ok(result) => {
            let group = session.objects.record_groups.get_mut(&key(name)).expect("group checked by caller");
            populate_rows(group, &result);
            if result.records.is_empty() {
                1403
            } else {
                0
            }
        }
        Err((code, msg)) => {
            session.trace_log.push(format!("POPULATE_GROUP_FAILED: {msg}"));
            code
        }
    }
}

/// Resolves a cell target `GROUP.COLUMN` to (group key, column index).
fn cell_target(session: &FormSession, raw: &str, who: &str) -> Result<(String, usize), String> {
    let (g, c) = raw.split_once('.').ok_or_else(|| format!("{who}: {raw} is not a GROUP.COLUMN column id"))?;
    let gk = key(g);
    let group = session.objects.record_groups.get(&gk).ok_or_else(|| format!("{who}: record group {g} does not exist"))?;
    let ci = group.column_index(c).ok_or_else(|| format!("{who}: column {c} does not exist in record group {g}"))?;
    Ok((gk, ci))
}

fn group_arg(session: &FormSession, args: &[OracleValue], who: &str) -> Result<String, String> {
    let Some(raw) = text(args, 0) else {
        return Err(format!("{who}: record group id is NULL"));
    };
    let k = key(&raw);
    if session.objects.record_groups.contains_key(&k) {
        Ok(k)
    } else {
        Err(format!("{who}: record group {raw} does not exist"))
    }
}

fn status(code: i64) -> BuiltinResult {
    BuiltinResult::Value(OracleValue::int(code))
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "CREATE_GROUP" | "CREATE_GROUP_FROM_QUERY" => create_group(name, args, session),
        "FIND_GROUP" => match text(args, 0) {
            Some(n) if session.objects.record_groups.contains_key(&key(&n)) => BuiltinResult::Value(OracleValue::varchar2(key(&n))),
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        "FIND_COLUMN" => match text(args, 0).map(|t| cell_target(session, &t, "FIND_COLUMN")) {
            Some(Ok(_)) => BuiltinResult::Value(OracleValue::varchar2(text(args, 0).unwrap_or_default().to_ascii_uppercase())),
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        "POPULATE_GROUP" => match group_arg(session, args, name) {
            Ok(g) => match session.objects.record_groups.get(&g).and_then(|r| r.query.clone()) {
                Some(q) => status(populate(session, &g, &q)),
                None => BuiltinResult::Failure(format!("POPULATE_GROUP: record group {g} has no query (use POPULATE_GROUP_WITH_QUERY)")),
            },
            Err(e) => BuiltinResult::Failure(e),
        },
        "POPULATE_GROUP_WITH_QUERY" => match (group_arg(session, args, name), text(args, 1)) {
            (Ok(g), Some(q)) => status(populate(session, &g, &q)),
            (Err(e), _) => BuiltinResult::Failure(e),
            (_, None) => BuiltinResult::Failure("POPULATE_GROUP_WITH_QUERY requires a query".into()),
        },
        "ADD_GROUP_COLUMN" => add_column(args, session),
        "ADD_GROUP_ROW" => add_row(args, session),
        "GET_GROUP_ROW_COUNT" => match group_arg(session, args, name) {
            Ok(g) => status(session.objects.record_groups[&g].rows.len() as i64),
            Err(e) => BuiltinResult::Failure(e),
        },
        "GET_GROUP_CHAR_CELL" | "GET_GROUP_NUMBER_CELL" | "GET_GROUP_DATE_CELL" => get_cell(name, args, session),
        "SET_GROUP_CHAR_CELL" | "SET_GROUP_NUMBER_CELL" | "SET_GROUP_DATE_CELL" => set_cell(name, args, session),
        "DELETE_GROUP" => match group_arg(session, args, name) {
            Ok(g) if session.objects.record_groups[&g].runtime_created => {
                session.objects.record_groups.remove(&g);
                BuiltinResult::Void
            }
            Ok(g) => BuiltinResult::Failure(format!("DELETE_GROUP: record group {g} was created at design time and cannot be deleted")),
            Err(e) => BuiltinResult::Failure(e),
        },
        "DELETE_GROUP_ROW" => delete_row(args, session),
        "SET_GROUP_SELECTION" | "RESET_GROUP_SELECTION" | "UNSET_GROUP_SELECTION" | "GET_GROUP_SELECTION" | "GET_GROUP_SELECTION_COUNT" | "GET_GROUP_RECORD_NUMBER" => {
            selection_builtin(name, args, session)
        }
        _ => return None,
    })
}

fn create_group(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(raw) = text(args, 0).filter(|t| !t.trim().is_empty()) else {
        return BuiltinResult::Failure(format!("{name} requires a record group name"));
    };
    let k = key(&raw);
    if session.objects.record_groups.contains_key(&k) {
        return BuiltinResult::Failure(format!("{name}: record group {k} already exists"));
    }
    let query = if name == "CREATE_GROUP_FROM_QUERY" { text(args, 1) } else { None };
    if name == "CREATE_GROUP_FROM_QUERY" && query.is_none() {
        return BuiltinResult::Failure("CREATE_GROUP_FROM_QUERY requires a query".into());
    }
    session.objects.record_groups.insert(k.clone(), RecordGroup { name: k.clone(), query, runtime_created: true, ..Default::default() });
    BuiltinResult::Value(OracleValue::varchar2(k))
}

fn add_column(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let g = match group_arg(session, args, "ADD_GROUP_COLUMN") {
        Ok(g) => g,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let (Some(col), Some(ty)) = (text(args, 1), text(args, 2)) else {
        return BuiltinResult::Failure("ADD_GROUP_COLUMN requires a column name and a column type".into());
    };
    let Some(data_type) = GroupColumnType::from_constant(&ty) else {
        return BuiltinResult::Failure(format!("ADD_GROUP_COLUMN: {ty} is not CHAR_COLUMN, NUMBER_COLUMN, DATE_COLUMN or LONG_COLUMN"));
    };
    let group = session.objects.record_groups.get_mut(&g).expect("checked");
    if !group.rows.is_empty() {
        return BuiltinResult::Failure(format!("ADD_GROUP_COLUMN: record group {g} already has rows"));
    }
    if group.column_index(&col).is_some() {
        return BuiltinResult::Failure(format!("ADD_GROUP_COLUMN: column {col} already exists in record group {g}"));
    }
    group.columns.push(GroupColumn { name: col.trim().to_string(), data_type });
    BuiltinResult::Value(OracleValue::varchar2(format!("{}.{}", g, col.trim().to_ascii_uppercase())))
}

fn add_row(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let g = match group_arg(session, args, "ADD_GROUP_ROW") {
        Ok(g) => g,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let group = session.objects.record_groups.get_mut(&g).expect("checked");
    if group.columns.is_empty() {
        return BuiltinResult::Failure(format!("ADD_GROUP_ROW: record group {g} has no columns"));
    }
    let blank = vec![OracleValue::Null; group.columns.len()];
    let at = match args.get(1).and_then(|v| v.as_text()).map(|t| t.to_ascii_uppercase()) {
        None => group.rows.len(),
        Some(t) if t == "END_OF_GROUP" => group.rows.len(),
        Some(t) => match t.parse::<usize>() {
            Ok(n) if n >= 1 && n <= group.rows.len() + 1 => n - 1,
            _ => return BuiltinResult::Failure(format!("ADD_GROUP_ROW: row {t} is out of range for record group {g}")),
        },
    };
    group.rows.insert(at, blank);
    group.selection_after_row_inserted(at + 1);
    group.populated = true;
    BuiltinResult::Void
}

fn row_index(group: &RecordGroup, args: &[OracleValue], idx: usize, who: &str) -> Result<usize, String> {
    match args.get(idx).and_then(|v| v.as_i64()) {
        Some(n) if n >= 1 && (n as usize) <= group.rows.len() => Ok(n as usize - 1),
        Some(n) => Err(format!("{who}: row {n} is out of range (record group has {} rows)", group.rows.len())),
        None => Err(format!("{who}: row number is required")),
    }
}

fn cell_type(name: &str) -> GroupColumnType {
    if name.contains("NUMBER") {
        GroupColumnType::Number
    } else if name.contains("DATE") {
        GroupColumnType::Date
    } else {
        GroupColumnType::Char
    }
}

fn type_compatible(col: GroupColumnType, want: GroupColumnType) -> bool {
    col == want || (want == GroupColumnType::Char && col == GroupColumnType::Long)
}

fn get_cell(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let (g, ci) = match text(args, 0).ok_or_else(|| format!("{name}: column id is NULL")).and_then(|t| cell_target(session, &t, name)) {
        Ok(t) => t,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let group = &session.objects.record_groups[&g];
    let ri = match row_index(group, args, 1, name) {
        Ok(r) => r,
        Err(e) => return BuiltinResult::Failure(e),
    };
    if !type_compatible(group.columns[ci].data_type, cell_type(name)) {
        return BuiltinResult::Failure(format!("{name}: column {} is not of the matching column type", group.columns[ci].name));
    }
    BuiltinResult::Value(group.rows[ri][ci].clone())
}

fn set_cell(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let (g, ci) = match text(args, 0).ok_or_else(|| format!("{name}: column id is NULL")).and_then(|t| cell_target(session, &t, name)) {
        Ok(t) => t,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let group = session.objects.record_groups.get_mut(&g).expect("checked");
    let ri = match row_index(group, args, 1, name) {
        Ok(r) => r,
        Err(e) => return BuiltinResult::Failure(e),
    };
    if !type_compatible(group.columns[ci].data_type, cell_type(name)) {
        return BuiltinResult::Failure(format!("{name}: column {} is not of the matching column type", group.columns[ci].name));
    }
    let raw = args.get(2).cloned().unwrap_or(OracleValue::Null);
    group.rows[ri][ci] = match (cell_type(name), raw.is_null()) {
        (_, true) => OracleValue::Null,
        (GroupColumnType::Number, false) => match raw.as_decimal() {
            Some(d) => OracleValue::Number(d),
            None => return BuiltinResult::Failure(format!("{name}: {} is not a number", raw.to_string_lossy())),
        },
        (GroupColumnType::Date, false) => OracleValue::Date(raw.to_string_lossy()),
        _ => OracleValue::varchar2(raw.to_string_lossy()),
    };
    BuiltinResult::Void
}

fn delete_row(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let g = match group_arg(session, args, "DELETE_GROUP_ROW") {
        Ok(g) => g,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let group = session.objects.record_groups.get_mut(&g).expect("checked");
    if text(args, 1).map(|t| t.eq_ignore_ascii_case("ALL_ROWS")).unwrap_or(false) {
        group.rows.clear();
        group.selection.clear();
        return BuiltinResult::Void;
    }
    match row_index(group, args, 1, "DELETE_GROUP_ROW") {
        Ok(r) => {
            group.rows.remove(r);
            group.selection_after_row_removed(r + 1);
            BuiltinResult::Void
        }
        Err(e) => BuiltinResult::Failure(e),
    }
}

/// The selection built-ins (see the module docs for the semantics).
fn selection_builtin(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let g = match group_arg(session, args, name) {
        Ok(g) => g,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let group = session.objects.record_groups.get_mut(&g).expect("checked");
    match name {
        "GET_GROUP_SELECTION_COUNT" => status(group.selection.len() as i64),
        "RESET_GROUP_SELECTION" if args.get(1).map(|v| v.is_null()).unwrap_or(true) => {
            group.selection.clear();
            BuiltinResult::Void
        }
        "SET_GROUP_SELECTION" | "RESET_GROUP_SELECTION" | "UNSET_GROUP_SELECTION" => match row_index(group, args, 1, name) {
            Ok(r) => {
                if name == "SET_GROUP_SELECTION" {
                    group.select(r + 1);
                } else {
                    group.deselect(r + 1);
                }
                BuiltinResult::Void
            }
            Err(e) => BuiltinResult::Failure(e),
        },
        _ => match args.get(1).and_then(|v| v.as_i64()) {
            Some(n) if n >= 1 => BuiltinResult::Value(group.selection.get(n as usize - 1).map(|r| OracleValue::int(*r as i64)).unwrap_or(OracleValue::Null)),
            _ => BuiltinResult::Failure(format!("{name}: the selection number must be 1 or more")),
        },
    }
}
