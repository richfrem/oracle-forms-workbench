//! Built-ins that act on form objects the browser shows: list items (`CLEAR_LIST`,
//! `ADD_LIST_ELEMENT`, `DELETE_LIST_ELEMENT`, `POPULATE_LIST`, `GET_LIST_ELEMENT_*`), menu items
//! (`FIND_MENU_ITEM`, `SET_MENU_ITEM_PROPERTY`, `GET_MENU_ITEM_PROPERTY`) and record properties
//! (`GET_RECORD_PROPERTY`, `SET_RECORD_PROPERTY`).
//!
//! List elements start from the design-time elements of the item and are then kept per
//! session (`RuntimeObjects::list_elements`); the state sends changed lists to the UI.
//! Menu items accept any `MENU.ITEM` name when the form has no custom menu loaded (the default
//! Forms menu has standard items such as `FILE.EXIT`); with a menu loaded the full `MENU.ITEM` path must resolve inside the named menu (same item
//! name in two menus stays distinct; an unknown menu fails; a bare item name must be unique).
//! Menu state (`ENABLED`, `DISPLAYED`, `CHECKED`, `LABEL`) is kept and sent to the UI; the web
//! menu bar does not apply it yet. Record numbers are 1-based; `STATUS` values are
//! `NEW_STATUS`, `INSERT_STATUS`, `QUERY_STATUS`, `CHANGED_STATUS`.
//!
//! Purpose: Module implementation (ui_builtins.rs).
//! Key Input Dependencies: crate::objects, crate::record_groups, crate::record
//! Index: ListElement, dispatch, list_key, ensure_list, populate_list, menu_key, truthy, record_prop

use crate::builtins::BuiltinResult;
use crate::record::FormsRecordStatus;
use crate::record_groups;
use crate::session::FormSession;
use crate::value::OracleValue;

/// One element of a list item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListElement {
    pub label: String,
    pub value: String,
}

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

fn truthy(v: &OracleValue) -> bool {
    match v.as_text().map(|t| t.to_ascii_uppercase()).as_deref() {
        Some("PROPERTY_TRUE") | Some("TRUE") | Some("4") | Some("YES") => true,
        Some(_) => false,
        None => false,
    }
}

fn bool_text(b: bool) -> OracleValue {
    OracleValue::varchar2(if b { "TRUE" } else { "FALSE" })
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "CLEAR_LIST" | "ADD_LIST_ELEMENT" | "DELETE_LIST_ELEMENT" | "POPULATE_LIST" | "GET_LIST_ELEMENT_COUNT" | "GET_LIST_ELEMENT_LABEL"
        | "GET_LIST_ELEMENT_VALUE" | "GET_LIST_ELEMENT_INDEX" => list_builtin(name, args, session),
        "FIND_MENU_ITEM" => match menu_key(session, &text(args, 0).unwrap_or_default()) {
            Ok(k) => BuiltinResult::Value(OracleValue::varchar2(k)),
            Err(_) => BuiltinResult::Value(OracleValue::Null),
        },
        "SET_MENU_ITEM_PROPERTY" | "GET_MENU_ITEM_PROPERTY" => menu_builtin(name, args, session),
        "GET_RECORD_PROPERTY" | "SET_RECORD_PROPERTY" => record_prop(name, args, session),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// List items
// ---------------------------------------------------------------------------

/// `BLOCK.ITEM` key of a list argument (a name, or the item's id which is the same text).
fn list_key(session: &FormSession, raw: &str) -> Result<String, String> {
    let clean = raw.trim().trim_start_matches(':').to_ascii_uppercase();
    let (block, item) = match clean.split_once('.') {
        Some((b, i)) => (b.to_string(), i.to_string()),
        None => (session.active_block().unwrap_or_default().to_ascii_uppercase(), clean.clone()),
    };
    let exists = session
        .block_info(&block)
        .map(|b| b.items.iter().any(|i| i.name.eq_ignore_ascii_case(&item)))
        .unwrap_or(false);
    if exists {
        Ok(format!("{block}.{item}"))
    } else {
        Err(format!("list item {raw} does not exist"))
    }
}

/// Makes sure the list has its working copy (from the design-time elements) and returns the key.
fn ensure_list(session: &mut FormSession, raw: &str, who: &str) -> Result<String, String> {
    let key = list_key(session, raw).map_err(|e| format!("{who}: {e}"))?;
    if !session.objects.list_elements.contains_key(&key) {
        let (b, i) = key.split_once('.').expect("key has a dot");
        let design: Vec<ListElement> = session
            .block_info(b)
            .and_then(|bi| bi.items.iter().find(|x| x.name.eq_ignore_ascii_case(i)))
            .map(|it| it.list_elements.iter().map(|e| ListElement { label: e.name.clone(), value: e.value.clone().unwrap_or_else(|| e.name.clone()) }).collect())
            .unwrap_or_default();
        session.objects.list_elements.insert(key.clone(), design);
    }
    Ok(key)
}

fn index_arg(args: &[OracleValue], idx: usize) -> Option<usize> {
    args.get(idx).and_then(|v| v.as_i64()).filter(|n| *n >= 1).map(|n| n as usize)
}

fn list_builtin(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let key = match ensure_list(session, &text(args, 0).unwrap_or_default(), name) {
        Ok(k) => k,
        Err(e) => return BuiltinResult::Failure(e),
    };
    match name {
        "CLEAR_LIST" => {
            session.objects.list_elements.get_mut(&key).expect("ensured").clear();
            session.set_bind_value(&key, OracleValue::Null);
            BuiltinResult::Void
        }
        "ADD_LIST_ELEMENT" => {
            let (Some(at), Some(label)) = (index_arg(args, 1), text(args, 2)) else {
                return BuiltinResult::Failure("ADD_LIST_ELEMENT requires an index (from 1) and a label".into());
            };
            let value = text(args, 3).unwrap_or_default();
            let list = session.objects.list_elements.get_mut(&key).expect("ensured");
            if at > list.len() + 1 {
                return BuiltinResult::Failure(format!("ADD_LIST_ELEMENT: index {at} is beyond the end of the list ({} elements)", list.len()));
            }
            list.insert(at - 1, ListElement { label, value });
            BuiltinResult::Void
        }
        "DELETE_LIST_ELEMENT" => {
            let list = session.objects.list_elements.get_mut(&key).expect("ensured");
            match index_arg(args, 1) {
                Some(i) if i <= list.len() => {
                    list.remove(i - 1);
                    BuiltinResult::Void
                }
                _ => BuiltinResult::Failure(format!("DELETE_LIST_ELEMENT: index is not between 1 and {}", list.len())),
            }
        }
        "POPULATE_LIST" => populate_list(&key, args, session),
        "GET_LIST_ELEMENT_COUNT" => BuiltinResult::Value(OracleValue::int(session.objects.list_elements[&key].len() as i64)),
        "GET_LIST_ELEMENT_LABEL" | "GET_LIST_ELEMENT_VALUE" => {
            let list = &session.objects.list_elements[&key];
            match index_arg(args, 1).and_then(|i| list.get(i - 1)) {
                Some(e) => BuiltinResult::Value(OracleValue::varchar2(if name.ends_with("LABEL") { e.label.clone() } else { e.value.clone() })),
                None => BuiltinResult::Failure(format!("{name}: index is not between 1 and {}", list.len())),
            }
        }
        _ => {
            // GET_LIST_ELEMENT_INDEX: 0 when the value is not in the list.
            let wanted = text(args, 1).unwrap_or_default();
            let pos = session.objects.list_elements[&key].iter().position(|e| e.value == wanted);
            BuiltinResult::Value(OracleValue::int(pos.map(|p| p as i64 + 1).unwrap_or(0)))
        }
    }
}

/// `POPULATE_LIST(list, group)`: label = first column, value = second (or the label).
fn populate_list(key: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(g) = text(args, 1).map(|t| t.trim().to_ascii_uppercase()) else {
        return BuiltinResult::Failure("POPULATE_LIST requires a record group".into());
    };
    let Some(group) = session.objects.record_groups.get(&g) else {
        return BuiltinResult::Failure(format!("POPULATE_LIST: record group {g} does not exist"));
    };
    if group.columns.is_empty() || (!group.populated && group.query.is_none()) {
        return BuiltinResult::Failure(format!("POPULATE_LIST: record group {g} has no columns or data"));
    }
    if !group.populated {
        let q = group.query.clone().expect("checked");
        let code = record_groups::populate(session, &g, &q);
        if code != 0 && code != 1403 {
            return BuiltinResult::Failure(format!("POPULATE_LIST: the query of record group {g} failed (ORA-{code:05})"));
        }
    }
    let group = &session.objects.record_groups[&g];
    let elements: Vec<ListElement> = group
        .rows
        .iter()
        .map(|r| {
            let label = r.first().map(|v| v.to_string_lossy()).unwrap_or_default();
            let value = r.get(1).map(|v| v.to_string_lossy()).unwrap_or_else(|| label.clone());
            ListElement { label, value }
        })
        .collect();
    session.objects.list_elements.insert(key.to_string(), elements);
    BuiltinResult::Void
}

// ---------------------------------------------------------------------------
// Menu items
// ---------------------------------------------------------------------------

/// `MENU.ITEM` key; with a custom menu loaded the item must exist in it.
fn menu_key(session: &FormSession, raw: &str) -> Result<String, String> {
    let clean = raw.trim().trim_start_matches(':').to_ascii_uppercase();
    if clean.is_empty() {
        return Err("menu item name is empty".into());
    }
    if session.module.menus.is_empty() {
        return Ok(clean);
    }
    match clean.split_once('.') {
        Some((menu, item)) => {
            let Some(m) = session.module.menus.iter().find(|m| m.name.eq_ignore_ascii_case(menu)) else {
                return Err(format!("menu {menu} does not exist"));
            };
            if m.items.iter().any(|i| i.name.eq_ignore_ascii_case(item)) {
                Ok(clean)
            } else {
                Err(format!("menu item {item} does not exist in menu {menu}"))
            }
        }
        None => {
            // A bare item name must identify exactly one menu.
            let owners: Vec<&str> = session.module.menus.iter().filter(|m| m.items.iter().any(|i| i.name.eq_ignore_ascii_case(&clean))).map(|m| m.name.as_str()).collect();
            match owners.as_slice() {
                [one] => Ok(format!("{}.{}", one.to_ascii_uppercase(), clean)),
                [] => Err(format!("menu item {raw} does not exist")),
                _ => Err(format!("menu item {raw} is ambiguous (it exists in menus {}); qualify it as MENU.ITEM", owners.join(", "))),
            }
        }
    }
}

fn menu_builtin(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let key = match menu_key(session, &text(args, 0).unwrap_or_default()) {
        Ok(k) => k,
        Err(e) => return BuiltinResult::Failure(format!("{name}: {e}")),
    };
    let prop = text(args, 1).unwrap_or_default().to_ascii_uppercase();
    if name == "SET_MENU_ITEM_PROPERTY" {
        let value = args.get(2).cloned().unwrap_or(OracleValue::Null);
        let stored = match prop.as_str() {
            "ENABLED" | "DISPLAYED" | "CHECKED" => bool_text(truthy(&value)),
            "LABEL" | "ICON_NAME" => value,
            _ => return BuiltinResult::Failure(format!("{name}: property {prop} is not supported (ENABLED, DISPLAYED, CHECKED, LABEL, ICON_NAME)")),
        };
        session.objects.menu_items.entry(key).or_default().insert(prop, stored);
        return BuiltinResult::Void;
    }
    let set = session.objects.menu_items.get(&key).and_then(|m| m.get(&prop)).cloned();
    BuiltinResult::Value(set.unwrap_or_else(|| match prop.as_str() {
        "ENABLED" | "DISPLAYED" => bool_text(true),
        "CHECKED" => bool_text(false),
        "LABEL" => {
            let (menu, item) = key.split_once('.').unwrap_or(("", &key));
            session
                .module
                .menus
                .iter()
                .filter(|m| m.name.eq_ignore_ascii_case(menu))
                .flat_map(|m| m.items.iter())
                .find(|i| i.name.eq_ignore_ascii_case(item))
                .and_then(|i| i.label.clone())
                .map(OracleValue::varchar2)
                .unwrap_or(OracleValue::Null)
        }
        _ => OracleValue::Null,
    }))
}

// ---------------------------------------------------------------------------
// Record properties
// ---------------------------------------------------------------------------

fn status_text(s: FormsRecordStatus) -> &'static str {
    match s {
        FormsRecordStatus::New => "NEW",
        FormsRecordStatus::Insert => "INSERT",
        FormsRecordStatus::Query => "QUERY",
        FormsRecordStatus::Changed => "CHANGED",
    }
}

/// `GET_RECORD_PROPERTY(record_number, block, property)` / `SET_RECORD_PROPERTY(.., value)`.
fn record_prop(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let (Some(n), Some(block), Some(prop)) = (index_arg(args, 0), text(args, 1), text(args, 2)) else {
        return BuiltinResult::Failure(format!("{name} requires a record number (from 1), a block and a property"));
    };
    let bkey = block.trim().to_ascii_uppercase();
    let prop = prop.to_ascii_uppercase();
    let Some(buf) = session.blocks.get_mut(&bkey) else {
        return BuiltinResult::Failure(format!("{name}: block {block} does not exist"));
    };
    let current = buf.current_record_index;
    let Some(rec) = buf.records.get_mut(n - 1) else {
        return BuiltinResult::Failure(format!("{name}: record {n} does not exist in block {block}"));
    };
    if name == "GET_RECORD_PROPERTY" {
        return match prop.as_str() {
            "STATUS" => BuiltinResult::Value(OracleValue::varchar2(status_text(rec.status))),
            "CURRENT_RECORD" => BuiltinResult::Value(bool_text(current == n - 1)),
            "VISUAL_ATTRIBUTE" => BuiltinResult::Value(session.objects.record_visuals.get(&format!("{bkey}.{n}")).cloned().map(OracleValue::varchar2).unwrap_or(OracleValue::Null)),
            _ => BuiltinResult::Failure(format!("{name}: property {prop} is not supported (STATUS, CURRENT_RECORD, VISUAL_ATTRIBUTE)")),
        };
    }
    let value = text(args, 3).unwrap_or_default().to_ascii_uppercase();
    match prop.as_str() {
        "STATUS" => {
            rec.status = match value.as_str() {
                "NEW_STATUS" => FormsRecordStatus::New,
                "INSERT_STATUS" => FormsRecordStatus::Insert,
                "QUERY_STATUS" => {
                    rec.initial_values = rec.values.clone();
                    FormsRecordStatus::Query
                }
                "CHANGED_STATUS" => FormsRecordStatus::Changed,
                other => return BuiltinResult::Failure(format!("SET_RECORD_PROPERTY: {other} is not NEW_STATUS, INSERT_STATUS, QUERY_STATUS or CHANGED_STATUS")),
            };
            session.refresh_system_status();
            BuiltinResult::Void
        }
        "VISUAL_ATTRIBUTE" => {
            session.objects.record_visuals.insert(format!("{bkey}.{n}"), text(args, 3).unwrap_or_default());
            BuiltinResult::Void
        }
        _ => BuiltinResult::Failure(format!("SET_RECORD_PROPERTY: property {prop} is not supported (STATUS, VISUAL_ATTRIBUTE)")),
    }
}
