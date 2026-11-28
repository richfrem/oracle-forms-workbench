//! The form stack of one live session: the root form, forms started with `CALL_FORM`
//! (modal, on a stack), forms started with `OPEN_FORM` (peers), `NEW_FORM` replacement,
//! `GO_FORM` / `CLOSE_FORM` / `EXIT_FORM`, and `:GLOBAL` carry-over.
//!
//! Everything runs on the session's single thread (Forms itself is single-threaded per
//! runtime), so a `CALL_FORM` is a nested call: the caller's trigger blocks inside the
//! navigation handler, which serves browser commands for the called form until it exits and
//! then returns. Reply routing is the same as for alerts: the request that started the call
//! is answered with the called form's startup state; the request that ends it is answered
//! (with the caller's refreshed state) after the caller's trigger has finished.
//!
//! Only one form runs at a time, so `:GLOBAL` values are kept in one application map that is
//! copied into a form before it runs and copied back after (`:PARAMETER.*` stay per form).
//! The `SESSION` mode of `OPEN_FORM` gives the new form its own database connection (except
//! for in-memory SQLite and the Mock backend, where a second connection would be a different,
//! empty database; that sharing is reported to the opener). If the second connection cannot be
//! created the form is NOT opened and the opener gets a message and `FORM_FAILURE`.
//!
//! Unsaved changes (round 3): `NEW_FORM` (`NO_ROLLBACK` / `TO_SAVEPOINT`) and `CLOSE_FORM` of a
//! form with changes in a database block ask the same Yes/No/Cancel question as `EXIT_FORM`;
//! Yes commits first (needs write opt-in, otherwise the navigation fails visibly), No discards,
//! Cancel keeps the form and abandons the navigation. `NEW_FORM ... FULL_ROLLBACK` discards
//! without asking. Real savepoint semantics beyond this are not modelled (`TO_SAVEPOINT` is
//! treated like `NO_ROLLBACK`; unverified against 14c).
//!
//! Startup failures are explicit (round 3): a form whose startup (`PRE-FORM`) fails is not left
//! half open. `OPEN_FORM` removes it and reports to the opener (message + `FORM_FAILURE`),
//! `NEW_FORM` keeps the issuing form (its old session is restored, `:GLOBAL` values too),
//! `CALL_FORM` returns an error so the caller's `CALL_FORM` fails (`FORM_FAILURE`).
//!
//! Purpose: Module implementation (form_app.rs).
//! Key Input Dependencies: crate::runtime_sessions, oracle_forms_runtime, oracle_forms_db
//! Index: ConnFactory, AppInit, Kind, Slot, App, run, fail, build_session, start_session, sync_in, sync_out, forms_meta, aggregate, serve, handle_command, handle_event, after_run, apply_nav, settle_changes, open_form, new_form, abort_child, close_form, call_form, new_connection

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use oracle_forms_db::{DatabaseDriver, DbBackend, DbConfig};
use oracle_forms_runtime::nav::{ask_to_save, form_module_name, CallSpec, FormLoader, LoadedForm, NavRequest, NewSpec, OpenSpec, Rollback};
use oracle_forms_runtime::{FormSession, InteractionRequest, InteractionResponse, OracleValue, ALERT_BUTTON1};
use serde_json::{json, Value};

use crate::runtime_sessions::{run_event, snapshot, Answer, Awaiting, Command, Cursors, Reply, SessionEvent, Shared, ALERT_TIMEOUT, STARTUP_TIMER_LOOKAHEAD_MS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Root,
    /// Started with `CALL_FORM`; the caller is suspended until this form exits.
    Called { caller: u32 },
    Opened,
}

pub(crate) struct Slot {
    id: u32,
    name: String,
    session: Option<FormSession>,
    cursor: Cursors,
    kind: Kind,
    missing: Vec<String>,
    hide_caller: bool,
    replace_menu: bool,
}

/// Test hook: creates the second database connection of `OPEN_FORM(..., SESSION)` instead of
/// the real driver (lets tests exercise the failing path without a database).
pub(crate) type ConnFactory = Arc<dyn Fn() -> Result<Arc<dyn DatabaseDriver>, String> + Send + Sync>;

/// Everything the session thread needs to start.
pub(crate) struct AppInit {
    pub shared: Arc<Shared>,
    pub session_id: String,
    pub backend: String,
    pub driver: Option<Arc<dyn DatabaseDriver>>,
    pub db: Option<DbConfig>,
    pub allow_writes: bool,
    pub loader: Option<FormLoader>,
    pub connection_factory: Option<ConnFactory>,
    pub rx: mpsc::Receiver<Command>,
    pub answer_rx: mpsc::Receiver<(Answer, Reply)>,
}

pub(crate) struct App {
    slots: Vec<Slot>,
    next_id: u32,
    active: u32,
    /// `:GLOBAL.*` shared by all forms of the application.
    globals: HashMap<String, OracleValue>,
    ended: bool,
    shared: Arc<Shared>,
    session_id: String,
    backend: String,
    driver: Option<Arc<dyn DatabaseDriver>>,
    db: Option<DbConfig>,
    allow_writes: bool,
    loader: Option<FormLoader>,
    connection_factory: Option<ConnFactory>,
    rx: Rc<mpsc::Receiver<Command>>,
    answer_rx: Arc<Mutex<mpsc::Receiver<(Answer, Reply)>>>,
}

type AppRef = Rc<RefCell<App>>;

/// Entry point of the session thread: starts the root form and serves commands until the
/// command channel closes.
pub(crate) fn run(init: AppInit, root: LoadedForm, params: Vec<(String, String)>, ready: Reply) {
    let app: AppRef = Rc::new(RefCell::new(App {
        slots: Vec::new(),
        next_id: 1,
        active: 1,
        globals: HashMap::new(),
        ended: false,
        shared: init.shared,
        session_id: init.session_id,
        backend: init.backend,
        driver: init.driver,
        db: init.db,
        allow_writes: init.allow_writes,
        loader: init.loader,
        connection_factory: init.connection_factory,
        rx: Rc::new(init.rx),
        answer_rx: Arc::new(Mutex::new(init.answer_rx)),
    }));
    *app.borrow().shared.reply_slot.lock().unwrap() = Some(ready);
    let id = register(&app, &root.module.key.name, Kind::Root, root.missing_libraries.clone());
    let session = build_session(&app, id, root, &params, None, false, None);
    let (session, err) = start_session(&app, session);
    put_back(&app, id, session);
    let mut state = aggregate(&app, id);
    if let Some(e) = err {
        state["error"] = json!(e);
    }
    let reply = app.borrow().shared.reply_slot.lock().unwrap().take();
    if let Some(r) = reply {
        let _ = r.send(state);
    }
    after_run(&app, id);
    serve(&app, None);
}

fn register(app: &AppRef, name: &str, kind: Kind, missing: Vec<String>) -> u32 {
    let mut a = app.borrow_mut();
    let id = a.next_id;
    a.next_id += 1;
    a.slots.push(Slot { id, name: name.to_string(), session: None, cursor: Cursors::default(), kind, missing, hide_caller: false, replace_menu: false });
    if matches!(kind, Kind::Root) {
        a.active = id;
    }
    id
}

fn put_back(app: &AppRef, id: u32, session: FormSession) {
    if let Some(s) = app.borrow_mut().slots.iter_mut().find(|s| s.id == id) {
        s.session = Some(session);
    }
}

/// Opens a second connection for `SESSION` mode. `Ok((driver, shared_note))`: `shared_note` is
/// set when the driver is the shared one on purpose (in-memory SQLite / Mock). A connection that
/// cannot be created is an error (never a silent fallback to the shared connection).
fn new_connection(app: &AppRef) -> Result<(Option<Arc<dyn DatabaseDriver>>, Option<&'static str>), String> {
    let (shared, cfg, factory) = {
        let a = app.borrow();
        (a.driver.clone(), a.db.clone(), a.connection_factory.clone())
    };
    if let Some(f) = factory {
        return f().map(|d| (Some(d), None));
    }
    let Some(cfg) = cfg else { return Ok((shared, None)) };
    let in_memory = cfg.backend == DbBackend::Sqlite && cfg.database_path.as_deref().map(|p| p == ":memory:").unwrap_or(true);
    if in_memory || matches!(cfg.backend, DbBackend::Mock) {
        return Ok((shared, Some("SESSION shares the database connection: in-memory SQLite and Mock backends have a single database")));
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let d = oracle_forms_db::create_driver(&cfg).map_err(|e| e.to_string())?;
        d.test_connection().await.map_err(|e| e.to_string())?;
        Ok((Some(Arc::<dyn DatabaseDriver>::from(d)), None))
    })
}

/// Reports a failed navigation to the form that asked for it: a message and `FORM_FAILURE`.
fn fail(app: &AppRef, id: u32, msg: String) {
    note(app, id, msg);
    if let Some(s) = app.borrow_mut().slots.iter_mut().find(|s| s.id == id).and_then(|s| s.session.as_mut()) {
        s.last_builtin_failed = true;
    }
}

/// Removes a half-started child form and restores `:GLOBAL` values from before its startup.
fn abort_child(app: &AppRef, child: u32, globals: HashMap<String, OracleValue>) {
    let mut a = app.borrow_mut();
    a.slots.retain(|s| s.id != child);
    a.globals = globals;
}

/// Builds a form session wired to the application: libraries, database, interaction and
/// navigation handlers, startup parameters.
fn build_session(app: &AppRef, id: u32, loaded: LoadedForm, params: &[(String, String)], calling: Option<String>, query_only: bool, own_driver: Option<Option<Arc<dyn DatabaseDriver>>>) -> FormSession {
    let (cancel, allow_writes, user, driver_shared, has_loader) = {
        let a = app.borrow();
        (a.shared.cancel.clone(), a.allow_writes, a.db.as_ref().and_then(|c| c.username.clone()), a.driver.clone(), a.loader.is_some())
    };
    let driver = own_driver.unwrap_or(driver_shared);
    let mut s = FormSession::new(loaded.module);
    s.cancel = cancel;
    for lib in &loaded.libraries {
        s.attach_library(lib);
    }
    if let Some(d) = driver {
        s.set_database_driver(d);
    }
    s.db_username = user;
    s.read_only = !allow_writes || query_only;
    s.host_policy = crate::host_exec::policy_from_env();
    s.form_id = id;
    s.calling_form = calling;
    for (k, v) in params {
        s.global_variables.insert(format!("PARAMETER.{}", k.to_ascii_uppercase()), OracleValue::varchar2(v.clone()));
    }
    s.set_interaction_handler(interaction_handler(app, id));
    if has_loader {
        let weak = Rc::downgrade(app);
        s.nav_handler = Some(Box::new(move |caller, spec| call_form(&weak, caller, spec)));
    }
    s
}

/// `SHOW_ALERT` / `SHOW_LOV`: hand the question to the waiting HTTP request, then block until
/// the browser answers.
fn interaction_handler(app: &AppRef, form_id: u32) -> oracle_forms_runtime::InteractionHandler {
    let weak: Weak<RefCell<App>> = Rc::downgrade(app);
    let (shared, answer_rx, session_id) = {
        let a = app.borrow();
        (a.shared.clone(), a.answer_rx.clone(), a.session_id.clone())
    };
    Box::new(move |req: &InteractionRequest| {
        let forms = weak.upgrade().map(|a| forms_meta(&a)).unwrap_or(Value::Null);
        let (kind, mut payload) = match req {
            InteractionRequest::ShowAlert { alert_name, message, button1, button2, button3, default_button } => (
                Awaiting::Alert,
                json!({ "alert": {
                    "alertName": alert_name, "message": message,
                    "button1": button1, "button2": button2, "button3": button3, "defaultButton": default_button,
                } }),
            ),
            InteractionRequest::ShowLov { lov_name, title, columns, rows, filter } => (
                Awaiting::Lov,
                json!({ "lov": { "lovName": lov_name, "title": title, "columns": columns, "rows": rows, "filter": filter } }),
            ),
        };
        payload["sessionId"] = json!(session_id);
        payload["formId"] = json!(form_id);
        payload["forms"] = forms;
        *shared.awaiting_kind.lock().unwrap() = kind;
        shared.awaiting_form.store(form_id, Ordering::SeqCst);
        shared.awaiting_alert.store(true, Ordering::SeqCst);
        if let Some(reply) = shared.reply_slot.lock().unwrap().take() {
            let _ = reply.send(payload);
        }
        let answer = answer_rx.lock().unwrap().recv_timeout(ALERT_TIMEOUT);
        shared.awaiting_alert.store(false, Ordering::SeqCst);
        let default = |req: &InteractionRequest| match req {
            InteractionRequest::ShowAlert { default_button, .. } => InteractionResponse::AlertButton(ALERT_BUTTON1 + (*default_button as i64).saturating_sub(1)),
            InteractionRequest::ShowLov { .. } => InteractionResponse::LovSelection(None),
        };
        match (answer, kind) {
            (Ok((Answer::Button(b), reply)), Awaiting::Alert) => {
                *shared.reply_slot.lock().unwrap() = Some(reply);
                InteractionResponse::AlertButton(b)
            }
            (Ok((Answer::Lov(sel), reply)), Awaiting::Lov) => {
                *shared.reply_slot.lock().unwrap() = Some(reply);
                InteractionResponse::LovSelection(sel)
            }
            (Ok((_, reply)), _) => {
                *shared.reply_slot.lock().unwrap() = Some(reply);
                default(req)
            }
            (Err(_), _) => default(req),
        }
    })
}

/// Copies the application's `:GLOBAL` values and form list into a form before it runs.
fn sync_in(app: &AppRef, s: &mut FormSession) {
    let a = app.borrow();
    let params: Vec<(String, OracleValue)> = s.global_variables.iter().filter(|(k, _)| k.starts_with("PARAMETER.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    s.global_variables = a.globals.clone();
    s.global_variables.extend(params);
    s.open_forms = a.slots.iter().map(|x| x.name.clone()).collect();
}

/// Copies a form's `:GLOBAL` values back into the application after it ran.
fn sync_out(app: &AppRef, s: &FormSession) {
    app.borrow_mut().globals = s.global_variables.iter().filter(|(k, _)| !k.starts_with("PARAMETER.")).map(|(k, v)| (k.clone(), v.clone())).collect();
}

/// Runs the startup triggers (and short startup timers) of a freshly built form.
fn start_session(app: &AppRef, mut s: FormSession) -> (FormSession, Option<String>) {
    sync_in(app, &mut s);
    let err = s.start().err().map(|e| e.to_string());
    if err.is_none() {
        s.fire_due_timers(STARTUP_TIMER_LOOKAHEAD_MS);
    }
    sync_out(app, &s);
    (s, err)
}

/// `[{id, name, kind, active, modal, suspended, hidden, replaceMenu}]` for the tab strip.
fn forms_meta(app: &AppRef) -> Value {
    let a = match app.try_borrow() {
        Ok(a) => a,
        Err(_) => return Value::Null,
    };
    let top = top_called(&a);
    Value::Array(
        a.slots
            .iter()
            .map(|s| {
                let suspended = a.slots.iter().any(|o| matches!(o.kind, Kind::Called { caller } if caller == s.id));
                let hidden = a.slots.iter().any(|o| matches!(o.kind, Kind::Called { caller } if caller == s.id) && o.hide_caller);
                json!({
                    "id": s.id, "name": s.name,
                    "kind": match s.kind { Kind::Root => "root", Kind::Called { .. } => "called", Kind::Opened => "opened" },
                    "active": s.id == a.active, "modal": top == Some(s.id), "suspended": suspended, "hidden": hidden,
                    "replaceMenu": s.replace_menu,
                })
            })
            .collect(),
    )
}

fn top_called(a: &App) -> Option<u32> {
    a.slots.iter().rev().find(|s| matches!(s.kind, Kind::Called { .. })).map(|s| s.id)
}

/// Full state of form `id` plus the form list.
fn aggregate(app: &AppRef, id: u32) -> Value {
    let forms = forms_meta(app);
    let mut a = app.borrow_mut();
    let (sid, backend, active) = (a.session_id.clone(), a.backend.clone(), a.active);
    let Some(slot) = a.slots.iter_mut().find(|s| s.id == id) else {
        return json!({ "sessionId": sid, "error": "The form is no longer open", "forms": forms, "activeForm": active });
    };
    let mut state = match &slot.session {
        Some(s) => {
            let mut v = snapshot(s, &sid, &backend, &mut slot.cursor, &slot.missing);
            v["sourceFile"] = json!(s.module.source_file);
            v
        }
        None => json!({ "sessionId": sid, "error": "The form is busy" }),
    };
    state["formId"] = json!(id);
    state["sourceFile"] = json!(std::path::Path::new(&state["sourceFile"].as_str().unwrap_or("").to_string()).file_name().and_then(|n| n.to_str()).unwrap_or(""));
    state["forms"] = forms;
    state["activeForm"] = json!(active);
    state
}

/// Serves browser commands. With `until`, returns once that form has been closed (a nested
/// `CALL_FORM`); otherwise runs until the command channel closes.
fn serve(app: &AppRef, until: Option<u32>) {
    let rx = app.borrow().rx.clone();
    while let Ok(cmd) = rx.recv() {
        if handle_command(app, cmd, until) {
            return;
        }
    }
}

fn is_open(app: &AppRef, id: u32) -> bool {
    app.borrow().slots.iter().any(|s| s.id == id)
}

fn error_state(app: &AppRef, msg: &str) -> Value {
    let a = app.borrow();
    json!({ "sessionId": a.session_id, "error": msg, "exitRequested": a.ended })
}

/// Handles one command; true when the nested loop waiting for `until` should stop.
fn handle_command(app: &AppRef, cmd: Command, until: Option<u32>) -> bool {
    match cmd {
        Command::State(reply) => {
            let active = app.borrow().active;
            let _ = reply.send(aggregate(app, active));
            false
        }
        Command::Tick(reply) => {
            tick_all(app);
            let active = app.borrow().active;
            let _ = reply.send(aggregate(app, active));
            false
        }
        Command::Event(ev, form_id, reply) => handle_event(app, ev, form_id, reply, until),
    }
}

/// Fires due timers on every form that is not suspended.
fn tick_all(app: &AppRef) {
    let ids: Vec<u32> = app.borrow().slots.iter().map(|s| s.id).collect();
    for id in ids {
        if app.borrow().ended || !runnable(app, id) {
            continue;
        }
        let Some(mut s) = take_session(app, id) else { continue };
        sync_in(app, &mut s);
        s.fire_due_timers(0);
        sync_out(app, &s);
        put_back(app, id, s);
        after_run(app, id);
    }
}

/// False for a form suspended by a `CALL_FORM`, or when a modal called form exists elsewhere.
fn runnable(app: &AppRef, id: u32) -> bool {
    let a = app.borrow();
    if !a.slots.iter().any(|s| s.id == id) {
        return false;
    }
    match top_called(&a) {
        Some(top) => top == id,
        None => true,
    }
}

fn take_session(app: &AppRef, id: u32) -> Option<FormSession> {
    app.borrow_mut().slots.iter_mut().find(|s| s.id == id).and_then(|s| s.session.take())
}

fn handle_event(app: &AppRef, ev: SessionEvent, form_id: Option<u32>, reply: Reply, until: Option<u32>) -> bool {
    if app.borrow().ended {
        let _ = reply.send(error_state(app, "Session has ended"));
        return false;
    }
    let target = match &ev {
        SessionEvent::ActivateForm { form_id } => *form_id,
        _ => form_id.unwrap_or(app.borrow().active),
    };
    if !is_open(app, target) {
        let _ = reply.send(error_state(app, "That form is not open"));
        return false;
    }
    if !runnable(app, target) {
        let _ = reply.send(error_state(app, "That form is suspended: a called form must exit first (CALL_FORM is modal)"));
        return false;
    }
    app.borrow_mut().active = target;
    let shared = app.borrow().shared.clone();
    let mut error = None;
    if !matches!(ev, SessionEvent::ActivateForm { .. }) {
        let Some(mut session) = take_session(app, target) else {
            let _ = reply.send(error_state(app, "The form is busy"));
            return false;
        };
        shared.cancel.store(false, Ordering::SeqCst);
        *shared.reply_slot.lock().unwrap() = Some(reply);
        sync_in(app, &mut session);
        error = run_event(&mut session, ev);
        session.fire_due_timers(0);
        sync_out(app, &session);
        put_back(app, target, session);
        after_run(app, target);
    } else {
        *shared.reply_slot.lock().unwrap() = Some(reply);
    }
    // A nested CALL_FORM loop ends when its form is gone: the reply waits for the caller's trigger.
    if until.map(|u| !is_open(app, u)).unwrap_or(false) {
        return true;
    }
    let reply = shared.reply_slot.lock().unwrap().take();
    if let Some(r) = reply {
        let active = app.borrow().active;
        let mut state = aggregate(app, active);
        if let Some(e) = error {
            state["error"] = json!(e);
        }
        let _ = r.send(state);
    }
    false
}

fn note(app: &AppRef, id: u32, msg: String) {
    if let Some(s) = app.borrow_mut().slots.iter_mut().find(|s| s.id == id).and_then(|s| s.session.as_mut()) {
        s.trace_log.push(format!("NAV: {msg}"));
        s.messages.push(msg);
    }
}

fn load(app: &AppRef, name: &str) -> Result<LoadedForm, String> {
    let loader = app.borrow().loader.clone();
    match loader {
        Some(l) => l(name),
        None => Err("this session cannot load other forms".into()),
    }
}

/// Applies the navigation a form queued and handles its exit request.
fn after_run(app: &AppRef, id: u32) {
    let (reqs, exit) = {
        let mut a = app.borrow_mut();
        match a.slots.iter_mut().find(|s| s.id == id).and_then(|s| s.session.as_mut()) {
            Some(s) => (std::mem::take(&mut s.nav_queue), s.exit_requested),
            None => return,
        }
    };
    if exit {
        close_form(app, id);
        return;
    }
    for r in reqs {
        apply_nav(app, id, r);
        if !is_open(app, id) {
            break;
        }
    }
}

fn apply_nav(app: &AppRef, origin: u32, req: NavRequest) {
    match req {
        NavRequest::Open(spec) => open_form(app, origin, spec),
        NavRequest::New(spec) => new_form(app, origin, spec),
        NavRequest::GoForm(name) => {
            let target = app.borrow().slots.iter().find(|s| s.name.eq_ignore_ascii_case(&name)).map(|s| s.id);
            match target {
                Some(t) if runnable(app, t) => app.borrow_mut().active = t,
                Some(_) => note(app, origin, format!("GO_FORM: form {name} is suspended by a CALL_FORM")),
                None => note(app, origin, format!("GO_FORM: form {name} is not open")),
            }
        }
        NavRequest::CloseForm(name) => {
            let target = app.borrow().slots.iter().find(|s| s.name.eq_ignore_ascii_case(&name) && !matches!(s.kind, Kind::Called { .. })).map(|s| s.id);
            match target {
                Some(t) => {
                    if settle_changes(app, t, origin, "CLOSE_FORM", "CLOSE_FORM_SAVE_CHANGES") {
                        close_form(app, t);
                    }
                }
                None => note(app, origin, format!("CLOSE_FORM: form {name} is not open (CALL_FORM forms end with EXIT_FORM)")),
            }
        }
    }
}

fn open_form(app: &AppRef, origin: u32, spec: OpenSpec) {
    let wanted = form_module_name(&spec.form);
    let loaded = match load(app, &wanted) {
        Ok(l) => l,
        Err(e) => return fail(app, origin, format!("OPEN_FORM: {e}")),
    };
    if app.borrow().slots.iter().any(|s| s.name.eq_ignore_ascii_case(&loaded.module.key.name)) {
        return fail(app, origin, format!("OPEN_FORM: form {} is already open", loaded.module.key.name));
    }
    let own = if spec.new_session {
        match new_connection(app) {
            Ok((driver, shared_note)) => {
                if let Some(t) = shared_note {
                    note(app, origin, format!("OPEN_FORM: {t}"));
                }
                Some(driver)
            }
            Err(e) => return fail(app, origin, format!("OPEN_FORM: form {} was not opened: the SESSION database connection could not be created: {e}", loaded.module.key.name)),
        }
    } else {
        None
    };
    let name = loaded.module.key.name.clone();
    let saved_globals = app.borrow().globals.clone();
    let missing = loaded.missing_libraries.clone();
    let id = register(app, &name, Kind::Opened, missing);
    let session = build_session(app, id, loaded, &spec.params, None, false, own);
    let (session, err) = start_session(app, session);
    if let Some(e) = err {
        drop(session);
        abort_child(app, id, saved_globals);
        return fail(app, origin, format!("OPEN_FORM: form {name} was not opened, its startup failed: {e}"));
    }
    put_back(app, id, session);
    if spec.activate {
        app.borrow_mut().active = id;
    }
    after_run(app, id);
}

/// Asks whether to save when form `id` has unsaved database changes, for a navigation issued by
/// `origin`. Returns true when the navigation may go ahead (nothing to save, saved, or discarded)
/// and false when it is abandoned (Cancel, or the commit failed / writes are not enabled; the
/// reason is reported to `origin` as a message and `FORM_FAILURE`).
fn settle_changes(app: &AppRef, id: u32, origin: u32, who: &str, alert: &str) -> bool {
    let Some(mut s) = take_session(app, id) else { return true };
    let mut outcome = Ok(true);
    if s.has_unsaved_changes() {
        sync_in(app, &mut s);
        outcome = match ask_to_save(&mut s, alert, who) {
            Some(true) => s.commit().map(|_| true).map_err(|e| format!("{who}: the changes could not be saved, the form stays open: {e}")),
            Some(false) => Ok(true),
            None => Ok(false),
        };
        sync_out(app, &s);
    }
    put_back(app, id, s);
    match outcome {
        Ok(true) => true,
        Ok(false) => {
            note(app, origin, format!("{who}: cancelled, the form stays open"));
            false
        }
        Err(e) => {
            fail(app, origin, e);
            false
        }
    }
}

/// `NEW_FORM`: the form that issued it is replaced (it keeps its place in the call chain). If
/// the new form's startup fails the issuing form is restored and the failure reported to it.
fn new_form(app: &AppRef, origin: u32, spec: NewSpec) {
    let wanted = form_module_name(&spec.form);
    let loaded = match load(app, &wanted) {
        Ok(l) => l,
        Err(e) => return fail(app, origin, format!("NEW_FORM: {e}")),
    };
    let calling = app.borrow().slots.iter().find(|s| s.id == origin).and_then(|s| s.session.as_ref()).and_then(|s| s.calling_form.clone());
    if spec.rollback == Rollback::FullRollback {
        if let Some(s) = app.borrow_mut().slots.iter_mut().find(|s| s.id == origin).and_then(|s| s.session.as_mut()) {
            s.clear_form();
        }
    } else if !settle_changes(app, origin, origin, "NEW_FORM", "NEW_FORM_SAVE_CHANGES") {
        return;
    }
    let saved_globals = app.borrow().globals.clone();
    let name = loaded.module.key.name.clone();
    // The old form stays aside until the new one has started, so a failed startup can be undone.
    let (old_session, old_name, old_missing, old_cursor) = {
        let mut a = app.borrow_mut();
        let Some(slot) = a.slots.iter_mut().find(|s| s.id == origin) else { return };
        let old = (slot.session.take(), std::mem::replace(&mut slot.name, name.clone()), std::mem::replace(&mut slot.missing, loaded.missing_libraries.clone()), std::mem::take(&mut slot.cursor));
        old
    };
    let session = build_session(app, origin, loaded, &spec.params, calling, spec.query_only, None);
    let (session, err) = start_session(app, session);
    if let Some(e) = err {
        drop(session);
        {
            let mut a = app.borrow_mut();
            a.globals = saved_globals;
            if let Some(slot) = a.slots.iter_mut().find(|s| s.id == origin) {
                slot.name = old_name;
                slot.missing = old_missing;
                slot.cursor = old_cursor;
                slot.session = old_session;
            }
        }
        return fail(app, origin, format!("NEW_FORM: form {name} was not started, its startup failed: {e}; the current form stays"));
    }
    put_back(app, origin, session);
    after_run(app, origin);
}

/// Ends a form (`EXIT_FORM`, `CLOSE_FORM`): a called form returns control to its caller, an
/// opened form disappears, and the root form ends the whole application.
fn close_form(app: &AppRef, id: u32) {
    let kind = app.borrow().slots.iter().find(|s| s.id == id).map(|s| s.kind);
    let Some(kind) = kind else { return };
    match kind {
        Kind::Root => {
            let mut a = app.borrow_mut();
            a.slots.retain(|s| s.id == id);
            a.active = id;
            a.ended = true;
            if let Some(s) = a.slots.iter_mut().find(|s| s.id == id).and_then(|s| s.session.as_mut()) {
                s.exit_requested = true;
            }
        }
        Kind::Called { caller } => {
            let mut a = app.borrow_mut();
            a.slots.retain(|s| s.id != id);
            a.active = caller;
        }
        Kind::Opened => {
            let mut a = app.borrow_mut();
            let was_active = a.active == id;
            a.slots.retain(|s| s.id != id);
            if was_active {
                a.active = a.slots.iter().rev().find(|s| !matches!(s.kind, Kind::Called { .. })).or(a.slots.first()).map(|s| s.id).unwrap_or(1);
            }
        }
    }
}

/// `CALL_FORM`: runs the called form inside the caller's trigger (see module docs).
fn call_form(weak: &Weak<RefCell<App>>, caller: &mut FormSession, spec: &CallSpec) -> Result<(), String> {
    let app = weak.upgrade().ok_or_else(|| "the session has ended".to_string())?;
    let wanted = form_module_name(&spec.form);
    let loaded = load(&app, &wanted)?;
    let caller_id = caller.form_id;
    sync_out(&app, caller);
    let missing = loaded.missing_libraries.clone();
    let id = register(&app, &loaded.module.key.name, Kind::Called { caller: caller_id }, missing);
    {
        let mut a = app.borrow_mut();
        a.active = id;
        if let Some(s) = a.slots.iter_mut().find(|s| s.id == id) {
            s.hide_caller = spec.hide;
            s.replace_menu = spec.replace_menu;
        }
    }
    let saved_globals = app.borrow().globals.clone();
    let name = app.borrow().slots.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
    let session = build_session(&app, id, loaded, &spec.params, Some(caller.module.key.name.clone()), spec.query_only, None);
    let (session, err) = start_session(&app, session);
    if let Some(e) = err {
        drop(session);
        abort_child(&app, id, saved_globals);
        app.borrow_mut().active = caller_id;
        sync_in(&app, caller);
        return Err(format!("form {name} was not called, its startup failed: {e}"));
    }
    put_back(&app, id, session);
    // The request that made the call is answered with the called form's startup state.
    let shared = app.borrow().shared.clone();
    let reply = shared.reply_slot.lock().unwrap().take();
    if let Some(r) = reply {
        let _ = r.send(aggregate(&app, id));
    }
    after_run(&app, id);
    if is_open(&app, id) {
        serve(&app, Some(id));
    }
    sync_in(&app, caller);
    app.borrow_mut().active = caller_id;
    Ok(())
}
