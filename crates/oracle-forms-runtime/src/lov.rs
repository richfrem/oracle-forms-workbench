//! List of values: `SHOW_LOV`, `LIST_VALUES` (and the default `KEY-LISTVAL`),
//! `FIND_LOV`, `GET_LOV_PROPERTY` / `SET_LOV_PROPERTY`, plus validate-from-list.
//!
//! A LOV is a design-time object (`LOV` element) that points at a record group and maps
//! some of its columns to return items. Showing a LOV: (re)populate the record group
//! (when it is not populated yet, or the LOV refreshes automatically, the Forms default),
//! send the rows to the user through the session's interaction handler
//! ([`InteractionRequest::ShowLov`]) and, when the user picks a row, assign each mapped
//! column to its return item. `SHOW_LOV` returns TRUE when a row was picked and FALSE when
//! the user cancelled, there were no rows, or no user is attached (batch runs).
//!
//! Validate-from-list: for an item with a LOV and `ValidateFromList=true`, a typed value
//! must match a value in the item's own LOV column. An exact match fills the other return
//! items; otherwise the values that start with the typed text are offered (a single one is
//! taken automatically, several are shown to the user); no match raises
//! `FRM-40212: Invalid value for field <item>`.
//!
//! Purpose: Module implementation (lov.rs).
//! Key Input Dependencies: crate::objects, crate::record_groups, crate::interpreter
//! Index: LovColumn, dispatch, find_lov, group_name, prepare_rows, show, apply_row, list_values, validate_from_list, cell_text

use crate::builtins::BuiltinResult;
use crate::interpreter::{InteractionRequest, InteractionResponse};
use crate::record_groups;
use crate::session::FormSession;
use crate::value::OracleValue;
use oracle_forms_core::model::LovInfo;
use serde::{Deserialize, Serialize};

/// A column of the list shown to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LovColumn {
    pub name: String,
    pub title: String,
    pub width: Option<i32>,
}

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

fn find_lov(session: &FormSession, name: &str) -> Option<LovInfo> {
    session.module.lovs.iter().find(|l| l.name.eq_ignore_ascii_case(name.trim())).cloned()
}

/// Record group of a LOV (`SET_LOV_PROPERTY(GROUP_NAME)` overrides the design value).
fn group_name(session: &FormSession, lov: &LovInfo) -> Option<String> {
    session
        .objects
        .lov_overrides
        .get(&lov.name.to_ascii_uppercase())
        .and_then(|m| m.get("GROUP_NAME"))
        .and_then(|v| v.as_text())
        .or_else(|| lov.record_group_name.clone())
}

fn override_bool(session: &FormSession, lov: &LovInfo, prop: &str, design: Option<bool>) -> Option<bool> {
    session
        .objects
        .lov_overrides
        .get(&lov.name.to_ascii_uppercase())
        .and_then(|m| m.get(prop))
        .and_then(|v| v.as_text())
        .map(|t| t.eq_ignore_ascii_case("PROPERTY_TRUE") || t.eq_ignore_ascii_case("TRUE") || t == "4")
        .or(design)
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "FIND_LOV" => match text(args, 0).and_then(|n| find_lov(session, &n)) {
            Some(l) => BuiltinResult::Value(OracleValue::varchar2(l.name.to_ascii_uppercase())),
            None => BuiltinResult::Value(OracleValue::Null),
        },
        "SHOW_LOV" => match text(args, 0).and_then(|n| find_lov(session, &n).ok_or(n).ok()) {
            Some(lov) => match show(session, &lov, None, None) {
                Ok(picked) => BuiltinResult::Value(OracleValue::boolean(picked)),
                Err(e) => BuiltinResult::Failure(e),
            },
            None => BuiltinResult::Failure(format!("SHOW_LOV: LOV {} does not exist", text(args, 0).unwrap_or_default())),
        },
        "LIST_VALUES" => list_values(args, session),
        "SET_LOV_PROPERTY" => match (text(args, 0), text(args, 1)) {
            (Some(l), Some(p)) if find_lov(session, &l).is_some() => {
                session
                    .objects
                    .lov_overrides
                    .entry(l.to_ascii_uppercase())
                    .or_default()
                    .insert(p.to_ascii_uppercase(), args.get(2).cloned().unwrap_or(OracleValue::Null));
                BuiltinResult::Void
            }
            (Some(l), _) => BuiltinResult::Failure(format!("SET_LOV_PROPERTY: LOV {l} does not exist")),
            _ => BuiltinResult::Failure("SET_LOV_PROPERTY requires a LOV, a property and a value".into()),
        },
        "GET_LOV_PROPERTY" => match (text(args, 0).and_then(|l| find_lov(session, &l)), text(args, 1).map(|p| p.to_ascii_uppercase())) {
            (Some(lov), Some(p)) => {
                let over = session.objects.lov_overrides.get(&lov.name.to_ascii_uppercase()).and_then(|m| m.get(&p)).cloned();
                BuiltinResult::Value(over.unwrap_or_else(|| match p.as_str() {
                    "GROUP_NAME" => group_name(session, &lov).map(OracleValue::varchar2).unwrap_or(OracleValue::Null),
                    "TITLE" => lov.title.clone().map(OracleValue::varchar2).unwrap_or(OracleValue::Null),
                    "AUTO_REFRESH" => OracleValue::boolean(lov.auto_refresh.unwrap_or(true)),
                    "FILTER_BEFORE_DISPLAY" => OracleValue::boolean(lov.filter_before_display.unwrap_or(false)),
                    "NAME" => OracleValue::varchar2(lov.name.clone()),
                    _ => OracleValue::Null,
                }))
            }
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        _ => return None,
    })
}

/// The text of a record group cell for filtering and display.
fn cell_text(v: &OracleValue) -> String {
    if v.is_null() {
        String::new()
    } else {
        v.to_string_lossy()
    }
}

/// Makes sure the LOV's record group holds current rows and returns the display columns
/// plus the rows as (group row index, values per display column).
fn prepare_rows(session: &mut FormSession, lov: &LovInfo) -> Result<(String, Vec<LovColumn>, Vec<usize>, Vec<Vec<OracleValue>>), String> {
    let Some(gname) = group_name(session, lov) else {
        return Err(format!("LOV {} has no record group", lov.name));
    };
    let gkey = gname.to_ascii_uppercase();
    let (populated, query) = match session.objects.record_groups.get(&gkey) {
        Some(g) => (g.populated, g.query.clone()),
        None => return Err(format!("LOV {}: record group {gname} does not exist", lov.name)),
    };
    let refresh = override_bool(session, lov, "AUTO_REFRESH", lov.auto_refresh).unwrap_or(true);
    if let Some(q) = query.filter(|_| !populated || refresh) {
        let code = record_groups::populate(session, &gkey, &q);
        if code != 0 && code != 1403 {
            return Err(format!("LOV {}: query for record group {gname} failed (ORA-{code:05})", lov.name));
        }
    }
    let group = &session.objects.record_groups[&gkey];
    // LOV columns: the mappings, in order; without mappings every group column is shown.
    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    if lov.mappings.is_empty() {
        for (i, c) in group.columns.iter().enumerate() {
            columns.push(LovColumn { name: c.name.clone(), title: c.name.clone(), width: None });
            indexes.push(i);
        }
    } else {
        for m in &lov.mappings {
            let Some(i) = group.column_index(&m.name) else {
                return Err(format!("LOV {}: column {} is not a column of record group {gname}", lov.name, m.name));
            };
            columns.push(LovColumn { name: m.name.clone(), title: m.title.clone().unwrap_or_else(|| m.name.clone()), width: m.display_width });
            indexes.push(i);
        }
    }
    let row_ids: Vec<usize> = (0..group.rows.len()).collect();
    let rows = group.rows.iter().map(|r| indexes.iter().map(|&i| r[i].clone()).collect()).collect();
    Ok((gkey, columns, row_ids, rows))
}

/// Assigns the picked row's mapped columns to their return items.
fn apply_row(session: &mut FormSession, lov: &LovInfo, columns: &[LovColumn], row: &[OracleValue]) {
    for (m, _) in lov.mappings.iter().zip(columns) {
        let (Some(item), Some(pos)) = (m.return_item.as_deref(), columns.iter().position(|c| c.name.eq_ignore_ascii_case(&m.name))) else {
            continue;
        };
        let item = item.trim_start_matches(':');
        let value = row.get(pos).cloned().unwrap_or(OracleValue::Null);
        let target = if item.contains('.') {
            item.to_string()
        } else {
            format!("{}.{}", session.active_block().unwrap_or_default(), item)
        };
        if let Err(e) = session.set_item_value(&target, value.to_json()) {
            session.messages.push(e.to_string());
        }
    }
}

/// Shows `lov` to the user. `filter` limits the rows to those whose key column starts
/// with it (case-insensitive); `key_item` names the item whose mapped column is the key
/// (default: the first column). Returns whether a row was picked.
pub fn show(session: &mut FormSession, lov: &LovInfo, filter: Option<&str>, key_item: Option<&str>) -> Result<bool, String> {
    let (_gkey, columns, _ids, mut rows) = prepare_rows(session, lov)?;
    let key_col = key_item
        .and_then(|it| lov.mappings.iter().position(|m| m.return_item.as_deref().map(|r| r.trim_start_matches(':').eq_ignore_ascii_case(it)).unwrap_or(false)))
        .unwrap_or(0);
    if let Some(f) = filter.filter(|f| !f.is_empty()) {
        let needle = f.to_ascii_lowercase();
        rows.retain(|r| r.get(key_col).map(|v| cell_text(v).to_ascii_lowercase().starts_with(&needle)).unwrap_or(false));
    }
    if rows.is_empty() {
        session.messages.push(format!("No values found for list of values {}", lov.name));
        session.trace_log.push(format!("LOV_EMPTY: {}", lov.name));
        return Ok(false);
    }
    let request = InteractionRequest::ShowLov {
        lov_name: lov.name.clone(),
        title: lov.title.clone().unwrap_or_else(|| lov.name.clone()),
        columns: columns.clone(),
        rows: rows.iter().map(|r| r.iter().map(OracleValue::to_json).collect()).collect(),
        filter: filter.map(str::to_string),
    };
    let answer = match session.interaction_handler.as_mut() {
        Some(handler) => handler(&request),
        None => {
            session.trace_log.push(format!("LOV_AUTO_CANCELLED: {} (no user attached)", lov.name));
            InteractionResponse::LovSelection(None)
        }
    };
    match answer {
        InteractionResponse::LovSelection(Some(i)) if i < rows.len() => {
            let row = rows[i].clone();
            apply_row(session, lov, &columns, &row);
            session.trace_log.push(format!("LOV_PICKED: {} row {}", lov.name, i + 1));
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `LIST_VALUES [(RESTRICT | NO_RESTRICT)]`: the LOV attached to the current item.
fn list_values(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(cursor) = session.system.cursor_item.clone() else {
        return BuiltinResult::Failure("LIST_VALUES: there is no current item".into());
    };
    let (block, item) = cursor.split_once('.').map(|(b, i)| (b.to_string(), i.to_string())).unwrap_or_default();
    let lov_name = session
        .block_info(&block)
        .and_then(|b| b.items.iter().find(|i| i.name.eq_ignore_ascii_case(&item)))
        .and_then(|i| i.lov_name.clone());
    let Some(lov) = lov_name.and_then(|n| find_lov(session, &n)) else {
        return BuiltinResult::Failure(format!("LIST_VALUES: item {cursor} has no list of values"));
    };
    let restrict = !text(args, 0).map(|t| t.eq_ignore_ascii_case("NO_RESTRICT")).unwrap_or(false);
    let current = session.get_bind_value(&cursor);
    let filter = if restrict && !current.is_null() { Some(current.to_string_lossy()) } else { None };
    match show(session, &lov, filter.as_deref(), Some(&item)) {
        Ok(_) => BuiltinResult::Void,
        Err(e) => BuiltinResult::Failure(e),
    }
}

/// Validate-from-list for `block.item` after the user changed its value. `Ok(())` when the
/// item has no list or the value is valid (other return items are filled in).
pub fn validate_from_list(session: &mut FormSession, block: &str, item: &str) -> Result<(), String> {
    let info = session
        .block_info(block)
        .and_then(|b| b.items.iter().find(|i| i.name.eq_ignore_ascii_case(item)))
        .cloned();
    let Some(info) = info else { return Ok(()) };
    let (Some(lov_name), Some(true)) = (info.lov_name.clone(), info.validate_from_list) else { return Ok(()) };
    let Some(lov) = find_lov(session, &lov_name) else { return Ok(()) };
    let path = format!("{}.{}", block, item);
    let value = session.get_bind_value(&path);
    if value.is_null() {
        return Ok(());
    }
    let typed = value.to_string_lossy();
    let (_g, columns, _ids, rows) = prepare_rows(session, &lov)?;
    let key_col = lov
        .mappings
        .iter()
        .position(|m| m.return_item.as_deref().map(|r| r.trim_start_matches(':').rsplit('.').next().unwrap_or("").eq_ignore_ascii_case(item)).unwrap_or(false))
        .unwrap_or(0);
    let invalid = || format!("FRM-40212: Invalid value for field {}.", item.to_ascii_uppercase());
    if let Some(row) = rows.iter().find(|r| r.get(key_col).map(|v| cell_text(v) == typed).unwrap_or(false)) {
        let row = row.clone();
        apply_row(session, &lov, &columns, &row);
        return Ok(());
    }
    let lower = typed.to_ascii_lowercase();
    let matches: Vec<&Vec<OracleValue>> = rows.iter().filter(|r| r.get(key_col).map(|v| cell_text(v).to_ascii_lowercase().starts_with(&lower)).unwrap_or(false)).collect();
    match matches.len() {
        0 => Err(invalid()),
        1 => {
            let row = matches[0].clone();
            apply_row(session, &lov, &columns, &row);
            Ok(())
        }
        _ => match show(session, &lov, Some(&typed), Some(item)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(invalid()),
            Err(e) => Err(e),
        },
    }
}
