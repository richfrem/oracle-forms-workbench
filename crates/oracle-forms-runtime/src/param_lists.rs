//! Parameter list built-ins: `CREATE_PARAMETER_LIST`, `GET_PARAMETER_LIST`,
//! `DESTROY_PARAMETER_LIST`, `ADD_PARAMETER`, `DELETE_PARAMETER`, `GET_PARAMETER_ATTR`,
//! `SET_PARAMETER_ATTR`.
//!
//! Semantics follow Forms: a list id is the list's name; failures set `FORM_FAILURE` and
//! let the trigger continue. Keys are case-insensitive and unique within a list. A
//! `DATA_PARAMETER` holds the *name of a record group*, which must exist when the
//! parameter is added or changed. `GET_PARAMETER_ATTR` returns its two OUT values
//! (`TEXT_PARAMETER`/`DATA_PARAMETER`, and the value) through `RuntimeObjects::pending_out`.
//!
//! Purpose: Module implementation (param_lists.rs).
//! Key Input Dependencies: crate::objects, crate::builtins
//! Index: dispatch, list_name, create, add, set_attr, get_attr, destroy

use crate::builtins::BuiltinResult;
use crate::objects::{ParamEntry, ParamKind, ParameterList};
use crate::session::FormSession;
use crate::value::OracleValue;

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

/// Resolves a list id (name) to the stored key, or a failure message.
fn list_name(session: &FormSession, args: &[OracleValue], idx: usize, who: &str) -> Result<String, String> {
    let Some(raw) = text(args, idx) else {
        return Err(format!("{who}: parameter list id is NULL"));
    };
    let key = raw.to_ascii_uppercase();
    if session.objects.parameter_lists.contains_key(&key) {
        Ok(key)
    } else {
        Err(format!("{who}: parameter list {raw} does not exist"))
    }
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "CREATE_PARAMETER_LIST" => create(args, session),
        "GET_PARAMETER_LIST" => match text(args, 0) {
            Some(n) if session.objects.parameter_lists.contains_key(&n.to_ascii_uppercase()) => BuiltinResult::Value(OracleValue::varchar2(n.to_ascii_uppercase())),
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        "DESTROY_PARAMETER_LIST" => destroy(args, session),
        "ADD_PARAMETER" => add(args, session),
        "DELETE_PARAMETER" => match (list_name(session, args, 0, "DELETE_PARAMETER"), text(args, 1)) {
            (Ok(l), Some(key)) => {
                let list = session.objects.parameter_lists.get_mut(&l).expect("checked");
                match list.position(&key) {
                    Some(i) => {
                        list.entries.remove(i);
                        BuiltinResult::Void
                    }
                    None => BuiltinResult::Failure(format!("DELETE_PARAMETER: parameter {key} not found in list {l}")),
                }
            }
            (Err(e), _) => BuiltinResult::Failure(e),
            (_, None) => BuiltinResult::Failure("DELETE_PARAMETER requires a key".into()),
        },
        "GET_PARAMETER_ATTR" => get_attr(args, session),
        "SET_PARAMETER_ATTR" => set_attr(args, session),
        _ => return None,
    })
}

fn create(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let name = match text(args, 0) {
        Some(n) if !n.trim().is_empty() => n.trim().to_ascii_uppercase(),
        _ => session.objects.next_anonymous_name("PARAMLIST"),
    };
    if session.objects.parameter_lists.contains_key(&name) {
        return BuiltinResult::Failure(format!("CREATE_PARAMETER_LIST: parameter list {name} already exists"));
    }
    session.objects.parameter_lists.insert(name.clone(), ParameterList { name: name.clone(), entries: Vec::new() });
    BuiltinResult::Value(OracleValue::varchar2(name))
}

fn destroy(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    match list_name(session, args, 0, "DESTROY_PARAMETER_LIST") {
        Ok(l) => {
            session.objects.parameter_lists.remove(&l);
            BuiltinResult::Void
        }
        Err(e) => BuiltinResult::Failure(e),
    }
}

/// Checks the value of a `DATA_PARAMETER`: it must name an existing record group.
fn check_data_value(session: &FormSession, kind: ParamKind, value: &str, who: &str) -> Result<(), String> {
    if kind == ParamKind::Data && !session.objects.record_groups.contains_key(&value.to_ascii_uppercase()) {
        return Err(format!("{who}: record group {value} does not exist (DATA_PARAMETER needs a record group)"));
    }
    Ok(())
}

fn add(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let l = match list_name(session, args, 0, "ADD_PARAMETER") {
        Ok(l) => l,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let (Some(key), Some(kind_text)) = (text(args, 1), text(args, 2)) else {
        return BuiltinResult::Failure("ADD_PARAMETER requires a key and a parameter type".into());
    };
    let Some(kind) = ParamKind::from_constant(&kind_text) else {
        return BuiltinResult::Failure(format!("ADD_PARAMETER: {kind_text} is not TEXT_PARAMETER or DATA_PARAMETER"));
    };
    let value = text(args, 3).unwrap_or_default();
    if let Err(e) = check_data_value(session, kind, &value, "ADD_PARAMETER") {
        return BuiltinResult::Failure(e);
    }
    let list = session.objects.parameter_lists.get_mut(&l).expect("checked");
    if list.position(&key).is_some() {
        return BuiltinResult::Failure(format!("ADD_PARAMETER: parameter {key} already exists in list {l}"));
    }
    list.entries.push(ParamEntry { key, kind, value });
    BuiltinResult::Void
}

fn set_attr(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let l = match list_name(session, args, 0, "SET_PARAMETER_ATTR") {
        Ok(l) => l,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let (Some(key), Some(kind_text)) = (text(args, 1), text(args, 2)) else {
        return BuiltinResult::Failure("SET_PARAMETER_ATTR requires a key and a parameter type".into());
    };
    let Some(kind) = ParamKind::from_constant(&kind_text) else {
        return BuiltinResult::Failure(format!("SET_PARAMETER_ATTR: {kind_text} is not TEXT_PARAMETER or DATA_PARAMETER"));
    };
    let value = text(args, 3).unwrap_or_default();
    if let Err(e) = check_data_value(session, kind, &value, "SET_PARAMETER_ATTR") {
        return BuiltinResult::Failure(e);
    }
    let list = session.objects.parameter_lists.get_mut(&l).expect("checked");
    match list.position(&key) {
        Some(i) => {
            list.entries[i].kind = kind;
            list.entries[i].value = value;
            BuiltinResult::Void
        }
        None => BuiltinResult::Failure(format!("SET_PARAMETER_ATTR: parameter {key} not found in list {l}")),
    }
}

fn get_attr(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let l = match list_name(session, args, 0, "GET_PARAMETER_ATTR") {
        Ok(l) => l,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let Some(key) = text(args, 1) else {
        return BuiltinResult::Failure("GET_PARAMETER_ATTR requires a key".into());
    };
    let list = session.objects.parameter_lists.get(&l).expect("checked");
    match list.position(&key) {
        Some(i) => {
            let e = list.entries[i].clone();
            session.objects.pending_out = vec![OracleValue::varchar2(e.kind.constant()), OracleValue::varchar2(e.value)];
            BuiltinResult::Void
        }
        None => BuiltinResult::Failure(format!("GET_PARAMETER_ATTR: parameter {key} not found in list {l}")),
    }
}
