//! Multi-form navigation built-ins: `CALL_FORM`, `OPEN_FORM`, `NEW_FORM`, `GO_FORM`,
//! `CLOSE_FORM`, `FIND_FORM`, `EXIT_FORM` (with commit modes) and the form-stack
//! introspection that real forms use (`GET_APPLICATION_PROPERTY(CALLING_FORM)`,
//! `:SYSTEM.CURRENT_FORM`, `GET_FORM_PROPERTY`).
//!
//! The runtime crate owns no file system and no threads. A form that asks to start another
//! form either
//! * blocks: `CALL_FORM` invokes the session's [`NavHandler`] (installed by the host), which
//!   loads the module, runs the called form until it exits, and only then returns, so the
//!   caller's trigger resumes exactly where it stopped (like `SHOW_ALERT`); or
//! * queues: `OPEN_FORM`, `NEW_FORM`, `GO_FORM` and `CLOSE_FORM` push a [`NavRequest`] that
//!   the host applies once the running trigger has finished (Forms starts opened forms when
//!   the opening trigger ends).
//! Module resolution is a host callback ([`FormLoader`]).
//!
//! Parameter lists passed to a started form are consumed into the new form's
//! `:PARAMETER.*` values (text parameters only; a `DATA_PARAMETER` is refused for forms).
//! Arguments are parsed against each built-in's own signature (`CALL_FORM(form, display,
//! switch_menu, query_mode, paramlist)`, `OPEN_FORM(form, activate_mode, session_mode, paramlist)`,
//! `NEW_FORM(form, rollback_mode, query_mode, paramlist)`); a wrong constant, a mode in another
//! mode's slot, a misplaced parameter list or an extra argument fails with `FORM_FAILURE`
//! and starts nothing.
//! `QUERY_ONLY` starts the form read-only. `HIDE` and `DO_REPLACE` are carried to the host as
//! flags (tab/menu presentation).
//!
//! Purpose: Module implementation (nav.rs).
//! Key Input Dependencies: crate::objects (parameter lists), crate::builtins
//! Index: LoadedForm, FormLoader, CallSpec, OpenSpec, NewSpec, NavRequest, NavHandler, ExitMode, dispatch, collect_params, Signature, is_mode_constant, parse_signature, form_module_name, exit_form, ask_to_save

use crate::builtins::BuiltinResult;
use crate::interpreter::{InteractionRequest, InteractionResponse};
use crate::objects::ParamKind;
use crate::session::FormSession;
use crate::value::OracleValue;
use oracle_forms_core::model::ModuleIndex;
use std::sync::Arc;

/// A module resolved by the host together with its attached libraries.
pub struct LoadedForm {
    pub module: ModuleIndex,
    pub libraries: Vec<ModuleIndex>,
    pub missing_libraries: Vec<String>,
}

/// Host callback that finds a form by the name used in `CALL_FORM('name')`.
pub type FormLoader = Arc<dyn Fn(&str) -> Result<LoadedForm, String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct CallSpec {
    pub form: String,
    pub hide: bool,
    pub replace_menu: bool,
    pub query_only: bool,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenSpec {
    pub form: String,
    pub activate: bool,
    /// `SESSION`: the new form gets its own database session.
    pub new_session: bool,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rollback {
    ToSavepoint,
    NoRollback,
    FullRollback,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSpec {
    pub form: String,
    pub rollback: Rollback,
    pub query_only: bool,
    pub params: Vec<(String, String)>,
}

/// Navigation the host applies after the running trigger finishes.
#[derive(Debug, Clone, PartialEq)]
pub enum NavRequest {
    Open(OpenSpec),
    New(NewSpec),
    GoForm(String),
    CloseForm(String),
}

/// Runs a called form to completion on behalf of `CALL_FORM`. `Err` is a load failure.
pub type NavHandler = Box<dyn FnMut(&mut FormSession, &CallSpec) -> Result<(), String>>;

/// How `EXIT_FORM` treats unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitMode {
    AskCommit,
    DoCommit,
    NoCommit,
    NoValidate,
}

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

/// `dir/NAME.fmx` -> `NAME`.
pub fn form_module_name(raw: &str) -> String {
    let base = raw.trim().rsplit(['/', '\\']).next().unwrap_or("").to_string();
    match base.rsplit_once('.') {
        Some((stem, ext)) if ["fmx", "fmb", "xml"].contains(&ext.to_ascii_lowercase().as_str()) => stem.to_string(),
        _ => base,
    }
}

/// Positional signature of a navigation built-in after the form name: the constants each
/// slot accepts; the parameter list is the slot right after them.
struct Signature {
    who: &'static str,
    slots: &'static [(&'static str, &'static [&'static str])],
}

const CALL_FORM_SIG: Signature = Signature {
    who: "CALL_FORM",
    slots: &[("display", &["HIDE", "NO_HIDE"]), ("switch_menu", &["DO_REPLACE", "NO_REPLACE"]), ("query_mode", &["QUERY_ONLY", "NO_QUERY_ONLY"])],
};
const OPEN_FORM_SIG: Signature = Signature {
    who: "OPEN_FORM",
    slots: &[("activate_mode", &["ACTIVATE", "NO_ACTIVATE"]), ("session_mode", &["SESSION", "NO_SESSION"])],
};
const NEW_FORM_SIG: Signature = Signature {
    who: "NEW_FORM",
    slots: &[("rollback_mode", &["TO_SAVEPOINT", "NO_ROLLBACK", "FULL_ROLLBACK"]), ("query_mode", &["QUERY_ONLY", "NO_QUERY_ONLY"])],
};

fn is_mode_constant(t: &str) -> bool {
    let u = t.to_ascii_uppercase();
    [&CALL_FORM_SIG, &OPEN_FORM_SIG, &NEW_FORM_SIG].iter().any(|s| s.slots.iter().any(|(_, c)| c.contains(&u.as_str())))
}

/// Parses `args[1..]` against the signature: each slot takes only its own constants (or NULL /
/// omitted for the default), the next slot is the parameter list, anything beyond is an error.
/// Short form: `F(form, paramlist)` is accepted when the lone argument is not a mode constant
/// and names an existing parameter list. Returns the upper-cased modes and the list name.
fn parse_signature(sig: &Signature, args: &[OracleValue], session: &FormSession) -> Result<(Vec<String>, Option<String>), String> {
    let who = sig.who;
    if args.len() == 2 {
        if let Some(t) = args[1].as_text() {
            if !is_mode_constant(&t) && session.objects.parameter_lists.contains_key(&t.to_ascii_uppercase()) {
                return Ok((Vec::new(), Some(t)));
            }
        }
    }
    let mut modes = Vec::new();
    let mut list = None;
    for (i, a) in args.iter().enumerate().skip(1) {
        let Some(t) = a.as_text() else { continue };
        let slot = i - 1;
        if let Some((label, allowed)) = sig.slots.get(slot) {
            let up = t.to_ascii_uppercase();
            if !allowed.contains(&up.as_str()) {
                let hint = if is_mode_constant(&up) { "a mode for another argument" } else { "not a valid constant" };
                return Err(format!("{who}: '{t}' is {hint}; the {label} argument accepts {}", allowed.join(" or ")));
            }
            modes.push(up);
        } else if slot == sig.slots.len() {
            if is_mode_constant(&t) {
                return Err(format!("{who}: '{t}' is a mode constant, but argument {} must be a parameter list", i + 1));
            }
            list = Some(t);
        } else {
            return Err(format!("{who}: too many arguments (argument {} is not part of the signature)", i + 1));
        }
    }
    Ok((modes, list))
}

/// Text parameters of the named list (`None` name: no parameters).
fn collect_params(session: &FormSession, list: Option<&str>, who: &str) -> Result<Vec<(String, String)>, String> {
    let Some(name) = list else { return Ok(Vec::new()) };
    let Some(pl) = session.objects.parameter_lists.get(&name.to_ascii_uppercase()) else {
        return Err(format!("{who}: parameter list {name} does not exist"));
    };
    let mut out = Vec::new();
    for e in &pl.entries {
        if e.kind == ParamKind::Data {
            return Err(format!("{who}: parameter {} is a DATA_PARAMETER; forms accept only text parameters", e.key));
        }
        out.push((e.key.to_ascii_uppercase(), e.value.clone()));
    }
    Ok(out)
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "CALL_FORM" | "OPEN_FORM" | "NEW_FORM" => start_form(name, args, session),
        "GO_FORM" | "CLOSE_FORM" => match text(args, 0).map(|t| form_module_name(&t)).filter(|t| !t.is_empty()) {
            Some(f) => {
                session.nav_queue.push(if name == "GO_FORM" { NavRequest::GoForm(f) } else { NavRequest::CloseForm(f) });
                BuiltinResult::Void
            }
            None => BuiltinResult::Failure(format!("{name} requires a form name")),
        },
        "FIND_FORM" => match text(args, 0).map(|t| form_module_name(&t)) {
            Some(f) if session.open_forms.iter().any(|o| o.eq_ignore_ascii_case(&f)) => BuiltinResult::Value(OracleValue::varchar2(f.to_ascii_uppercase())),
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        "EXIT_FORM" => exit_form(args, session),
        "GET_FORM_PROPERTY" => {
            let prop = text(args, 1).unwrap_or_default().to_ascii_uppercase();
            BuiltinResult::Value(match prop.as_str() {
                "FORM_NAME" | "NAME" => OracleValue::varchar2(session.module.key.name.clone()),
                "FIRST_BLOCK" => session.module.blocks.first().map(|b| OracleValue::varchar2(b.name.clone())).unwrap_or(OracleValue::Null),
                "LAST_BLOCK" => session.module.blocks.last().map(|b| OracleValue::varchar2(b.name.clone())).unwrap_or(OracleValue::Null),
                "CURRENT_BLOCK" => session.active_block().map(OracleValue::varchar2).unwrap_or(OracleValue::Null),
                "CALLING_FORM" => session.calling_form.clone().map(OracleValue::varchar2).unwrap_or(OracleValue::Null),
                _ => OracleValue::Null,
            })
        }
        _ => return None,
    })
}

fn start_form(name: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(form) = text(args, 0).map(|t| form_module_name(&t)).filter(|t| !t.is_empty()) else {
        return BuiltinResult::Failure(format!("{name} requires a form name"));
    };
    let sig = match name {
        "CALL_FORM" => &CALL_FORM_SIG,
        "OPEN_FORM" => &OPEN_FORM_SIG,
        _ => &NEW_FORM_SIG,
    };
    let (modes, list) = match parse_signature(sig, args, session) {
        Ok(p) => p,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let params = match collect_params(session, list.as_deref(), name) {
        Ok(p) => p,
        Err(e) => return BuiltinResult::Failure(e),
    };
    let has = |w: &str| modes.iter().any(|m| m == w);
    match name {
        "CALL_FORM" => {
            let spec = CallSpec { form: form.clone(), hide: has("HIDE"), replace_menu: has("DO_REPLACE"), query_only: has("QUERY_ONLY"), params };
            let Some(mut handler) = session.nav_handler.take() else {
                return BuiltinResult::Unsupported("CALL_FORM (this session has no form loader; start it through the server)".into());
            };
            session.trace_log.push(format!("CALL_FORM: {form}"));
            let outcome = handler(session, &spec);
            session.nav_handler = Some(handler);
            match outcome {
                Ok(()) => BuiltinResult::Void,
                Err(e) => BuiltinResult::Failure(format!("CALL_FORM: {e}")),
            }
        }
        "OPEN_FORM" => {
            session.trace_log.push(format!("OPEN_FORM: {form}"));
            session.nav_queue.push(NavRequest::Open(OpenSpec { form, activate: !has("NO_ACTIVATE"), new_session: has("SESSION"), params }));
            BuiltinResult::Void
        }
        _ => {
            let rollback = if has("FULL_ROLLBACK") {
                Rollback::FullRollback
            } else if has("NO_ROLLBACK") {
                Rollback::NoRollback
            } else {
                Rollback::ToSavepoint
            };
            session.trace_log.push(format!("NEW_FORM: {form}"));
            session.nav_queue.push(NavRequest::New(NewSpec { form, rollback, query_only: has("QUERY_ONLY"), params }));
            BuiltinResult::Void
        }
    }
}

/// `EXIT_FORM [(ASK_COMMIT | DO_COMMIT | NO_COMMIT | NO_VALIDATE)]`.
fn exit_form(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let mode = match text(args, 0).map(|t| t.to_ascii_uppercase()).as_deref() {
        Some("DO_COMMIT") => ExitMode::DoCommit,
        Some("NO_COMMIT") => ExitMode::NoCommit,
        Some("NO_VALIDATE") => ExitMode::NoValidate,
        _ => ExitMode::AskCommit,
    };
    let changed = session.has_unsaved_changes();
    match mode {
        ExitMode::NoCommit | ExitMode::NoValidate => {}
        ExitMode::DoCommit => {
            if changed {
                if let Err(e) = session.commit() {
                    return BuiltinResult::Failure(format!("EXIT_FORM: {e}"));
                }
            }
        }
        ExitMode::AskCommit => {
            if changed {
                match ask_to_save(session) {
                    Some(true) => {
                        if let Err(e) = session.commit() {
                            return BuiltinResult::Failure(format!("EXIT_FORM: {e}"));
                        }
                    }
                    Some(false) => {}
                    None => return BuiltinResult::Void, // Cancel: stay in the form
                }
            }
        }
    }
    session.exit_requested = true;
    BuiltinResult::Void
}

/// Asks "save changes?" through the interaction handler: `Some(true)` yes, `Some(false)` no,
/// `None` cancel. Without a user attached the answer is no (batch runs must not write).
fn ask_to_save(session: &mut FormSession) -> Option<bool> {
    let request = InteractionRequest::ShowAlert {
        alert_name: "EXIT_FORM_SAVE_CHANGES".into(),
        message: "Do you want to save the changes you have made?".into(),
        button1: "Yes".into(),
        button2: Some("No".into()),
        button3: Some("Cancel".into()),
        default_button: 1,
    };
    let answer = session.interaction_handler.as_mut().map(|h| h(&request));
    match answer {
        Some(InteractionResponse::AlertButton(code)) if code == crate::builtins::ALERT_BUTTON1 => Some(true),
        Some(InteractionResponse::AlertButton(code)) if code == crate::builtins::ALERT_BUTTON2 => Some(false),
        Some(InteractionResponse::AlertButton(_)) => None,
        Some(_) => Some(true),
        None => {
            session.trace_log.push("EXIT_FORM: unsaved changes discarded (no user attached)".into());
            Some(false)
        }
    }
}
