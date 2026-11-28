//! Live form sessions: run a form's triggers and PL/SQL on the server and drive it from
//! the browser.
//!
//! Each session owns a `FormSession` on its own thread. Commands arrive over a channel and
//! are run one at a time. When PL/SQL calls `SHOW_ALERT`, the session thread pauses: the
//! pending alert is returned as the HTTP response, and the next `/alert` request carries
//! the user's answer back so the trigger continues exactly where it stopped.
//!
//! Sessions are read-only by default (queries run; DML, locks and commits are refused)
//! and expire after 30 minutes without use.
//!
//! Purpose: Live stateful form runtime session manager.
//! Key Input Dependencies: oracle_forms_core, oracle_forms_db, oracle_forms_runtime, serde, serde_json, tokio
//! Index: lov_pauses_the_event_and_the_answer_fills_the_return_items, lov_cancel_leaves_values_and_other_answers_are_refused, timers_fire_after_startup_and_through_tick, startup_overrun_returns_error_and_other_sessions_still_start, await_reply, SessionEvent, Command, Shared, SessionHandle, SessionManager, StartOptions, Cursors, new_id, reap_idle, remove, get, start, send_event, answer_alert, state, run_event, snapshot, conformance, sqlite, alert_pauses_trigger_and_answer_resumes_it, read_only_session_refuses_commit_but_queries_run

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oracle_forms_core::model::ModuleIndex;
use oracle_forms_db::DbConfig;
use oracle_forms_runtime::{FormSession, TriggerResult};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::oneshot;

const MAX_SESSIONS: usize = 32;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub(crate) const ALERT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_TRACE_LINES: usize = 400;
/// Timers due within this many milliseconds of startup completing fire with the startup response.
pub(crate) const STARTUP_TIMER_LOOKAHEAD_MS: i64 = 100;
/// How long one HTTP request waits for its session thread (startup parses the form and
/// libraries, so it gets longer). A session that overruns is cancelled and dropped; it can
/// never hold up other sessions or requests because every session runs on its own thread
/// and the request side only awaits a oneshot with a timeout.
const START_TIMEOUT: Duration = Duration::from_secs(90);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// A user action sent from the browser.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEvent {
    /// WHEN-BUTTON-PRESSED on a push button.
    Button { block: String, item: String, record: Option<usize> },
    /// The user typed a value into an item (then left it: WHEN-VALIDATE-ITEM fires).
    SetItem { block: String, item: String, value: Value, record: Option<usize> },
    /// Move the cursor to an item (fires navigation/validation triggers).
    Navigate { block: String, item: String, record: Option<usize> },
    ExecuteQuery { block: Option<String> },
    NextRecord,
    PreviousRecord,
    CreateRecord,
    DeleteRecord,
    Commit,
    ClearForm,
    /// Open the list of values of the current item (`LIST_VALUES`, the default `KEY-LISTVAL`).
    PickLov,
    /// Make a form of the application the active one (tab click).
    #[serde(rename_all = "camelCase")]
    ActivateForm { form_id: u32 },
    /// `DO_KEY(name)`: runs the KEY- trigger if defined, else the default action.
    Key { name: String },
}

pub(crate) type Reply = oneshot::Sender<Value>;

pub(crate) enum Command {
    /// A browser event for a form (`None`: the active form).
    Event(SessionEvent, Option<u32>, Reply),
    State(Reply),
    /// Run due timers (`WHEN-TIMER-EXPIRED`), then return the state.
    Tick(Reply),
}

/// The user's answer to a pending interaction.
pub(crate) enum Answer {
    Button(i64),
    Lov(Option<usize>),
}

/// Which interaction the session thread is waiting on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Awaiting {
    Alert,
    Lov,
}

pub(crate) struct Shared {
    /// Where the response for the request currently being processed should go.
    pub(crate) reply_slot: Mutex<Option<Reply>>,
    /// Answers to a pending alert or LOV, with the reply slot for the follow-up request.
    pub(crate) answer_tx: Mutex<mpsc::Sender<(Answer, Reply)>>,
    pub(crate) awaiting_alert: AtomicBool,
    /// The form whose question (alert / LOV) is waiting.
    pub(crate) awaiting_form: AtomicU32,
    /// What `awaiting_alert` is waiting for (valid while it is set).
    pub(crate) awaiting_kind: Mutex<Awaiting>,
    /// Set to abort the PL/SQL the session thread is running (shared with `FormSession`).
    pub(crate) cancel: Arc<AtomicBool>,
}

pub struct SessionHandle {
    tx: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    last_used: Mutex<Instant>,
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<SessionHandle>>>>,
    counter: Arc<AtomicU64>,
    start_timeout: Duration,
    command_timeout: Duration,
    /// Test hook: delay before the session thread starts working (simulates a slow startup).
    startup_delay: Duration,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self {
            sessions: Arc::default(),
            counter: Arc::default(),
            start_timeout: START_TIMEOUT,
            command_timeout: COMMAND_TIMEOUT,
            startup_delay: Duration::ZERO,
        }
    }
}

pub struct StartOptions {
    pub module: ModuleIndex,
    pub libraries: Vec<ModuleIndex>,
    pub missing_libraries: Vec<String>,
    pub db: Option<DbConfig>,
    pub allow_writes: bool,
    /// Startup parameters, available to the form as `:PARAMETER.NAME`.
    pub parameters: Vec<(String, String)>,
    /// Finds other forms by name (`CALL_FORM`, `OPEN_FORM`, `NEW_FORM`); `None` disables them.
    pub loader: Option<oracle_forms_runtime::FormLoader>,
}

impl SessionManager {
    fn new_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("s{}{:08x}", n, nanos ^ (std::process::id().rotate_left(13)))
    }

    /// Drops sessions idle longer than the timeout.
    pub fn reap_idle(&self) {
        let mut map = self.sessions.lock().unwrap();
        map.retain(|_, h| h.last_used.lock().unwrap().elapsed() < IDLE_TIMEOUT);
    }

    pub fn remove(&self, id: &str) -> bool {
        self.sessions.lock().unwrap().remove(id).is_some()
    }

    fn get(&self, id: &str) -> Option<Arc<SessionHandle>> {
        let handle = self.sessions.lock().unwrap().get(id).cloned()?;
        *handle.last_used.lock().unwrap() = Instant::now();
        Some(handle)
    }

    /// Starts a session thread, runs the form startup triggers, and returns the initial
    /// state.
    pub async fn start(&self, opts: StartOptions) -> Result<Value, String> {
        self.reap_idle();
        if self.sessions.lock().unwrap().len() >= MAX_SESSIONS {
            return Err(format!("Too many running sessions (limit {}); close one first", MAX_SESSIONS));
        }

        // Connect (and verify) before spawning anything.
        let driver = match &opts.db {
            Some(cfg) => {
                let d = oracle_forms_db::create_driver(cfg).map_err(|e| e.to_string())?;
                d.test_connection().await.map_err(|e| e.to_string())?;
                Some(Arc::<dyn oracle_forms_db::DatabaseDriver>::from(d))
            }
            None => None,
        };

        let id = self.new_id();
        let (tx, rx) = mpsc::channel::<Command>();
        let (answer_tx, answer_rx) = mpsc::channel::<(Answer, Reply)>();
        let shared = Arc::new(Shared {
            reply_slot: Mutex::new(None),
            answer_tx: Mutex::new(answer_tx),
            awaiting_alert: AtomicBool::new(false),
            awaiting_form: AtomicU32::new(0),
            awaiting_kind: Mutex::new(Awaiting::Alert),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let (ready_tx, ready_rx) = oneshot::channel::<Value>();

        let thread_shared = shared.clone();
        let startup_delay = self.startup_delay;
        let session_id = id.clone();
        let backend = opts.db.as_ref().map(|c| format!("{:?}", c.backend)).unwrap_or_else(|| "none".into());
        let init = crate::form_app::AppInit {
            shared: thread_shared,
            session_id,
            backend,
            driver,
            db: opts.db.clone(),
            allow_writes: opts.allow_writes,
            loader: opts.loader.clone(),
            rx,
            answer_rx,
        };
        let root = oracle_forms_runtime::LoadedForm { module: opts.module, libraries: opts.libraries, missing_libraries: opts.missing_libraries };
        let params = opts.parameters;
        std::thread::Builder::new()
            .name(format!("form-session-{}", id))
            .spawn(move || {
                std::thread::sleep(startup_delay);
                crate::form_app::run(init, root, params, ready_tx);
            })
            .map_err(|e| format!("Failed to start session thread: {}", e))?;

        let state = match tokio::time::timeout(self.start_timeout, ready_rx).await {
            Ok(r) => r.map_err(|_| "Session thread exited during startup".to_string())?,
            Err(_) => {
                // Runaway startup: ask the interpreter to stop; the session is never
                // registered, so nothing else can reach it.
                shared.cancel.store(true, Ordering::SeqCst);
                return Err(format!(
                    "Session startup did not finish within {} s and was cancelled (runaway startup trigger or unparsable library?)",
                    self.start_timeout.as_secs_f32()
                ));
            }
        };
        self.sessions.lock().unwrap().insert(
            id,
            Arc::new(SessionHandle { tx, shared, last_used: Mutex::new(Instant::now()) }),
        );
        Ok(state)
    }

    #[cfg(test)]
    pub async fn send_event(&self, id: &str, event: SessionEvent) -> Result<Value, (u16, String)> {
        self.send_event_to(id, event, None).await
    }

    /// Sends an event to a specific form of the application (`None`: the active form).
    pub async fn send_event_to(&self, id: &str, event: SessionEvent, form_id: Option<u32>) -> Result<Value, (u16, String)> {
        let handle = self.get(id).ok_or((404, "Session not found or expired".to_string()))?;
        if handle.shared.awaiting_alert.load(Ordering::SeqCst) {
            return Err((409, "An alert is waiting for an answer".to_string()));
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        handle
            .tx
            .send(Command::Event(event, form_id, reply_tx))
            .map_err(|_| (410, "Session has ended".to_string()))?;
        self.await_reply(id, &handle, reply_rx).await
    }

    /// Waits for the session thread's reply. On timeout the session is cancelled and
    /// removed (504) so a runaway cannot block the caller or be reused half-run.
    async fn await_reply(&self, id: &str, handle: &Arc<SessionHandle>, rx: oneshot::Receiver<Value>) -> Result<Value, (u16, String)> {
        match tokio::time::timeout(self.command_timeout, rx).await {
            Ok(r) => r.map_err(|_| (500, "Session thread stopped".to_string())),
            Err(_) => {
                handle.shared.cancel.store(true, Ordering::SeqCst);
                self.remove(id);
                Err((504, format!("The session did not answer within {} s; it was cancelled and closed", self.command_timeout.as_secs_f32())))
            }
        }
    }

    #[cfg(test)]
    pub async fn answer_alert(&self, id: &str, button: i64) -> Result<Value, (u16, String)> {
        self.answer(id, Answer::Button(button), Awaiting::Alert, "No alert is waiting for an answer", None).await
    }

    pub async fn answer_alert_for(&self, id: &str, button: i64, form_id: Option<u32>) -> Result<Value, (u16, String)> {
        self.answer(id, Answer::Button(button), Awaiting::Alert, "No alert is waiting for an answer", form_id).await
    }

    /// Answers a pending list of values: the index of the picked row, or `None` to cancel.
    #[cfg(test)]
    pub async fn answer_lov(&self, id: &str, selection: Option<usize>) -> Result<Value, (u16, String)> {
        self.answer(id, Answer::Lov(selection), Awaiting::Lov, "No list of values is waiting for an answer", None).await
    }

    pub async fn answer_lov_for(&self, id: &str, selection: Option<usize>, form_id: Option<u32>) -> Result<Value, (u16, String)> {
        self.answer(id, Answer::Lov(selection), Awaiting::Lov, "No list of values is waiting for an answer", form_id).await
    }

    /// Runs due timers and returns the state (the web UI polls this while timers are pending).
    pub async fn tick(&self, id: &str) -> Result<Value, (u16, String)> {
        let handle = self.get(id).ok_or((404, "Session not found or expired".to_string()))?;
        if handle.shared.awaiting_alert.load(Ordering::SeqCst) {
            return Err((409, "An alert or list of values is waiting for an answer".to_string()));
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        handle.tx.send(Command::Tick(reply_tx)).map_err(|_| (410, "Session has ended".to_string()))?;
        self.await_reply(id, &handle, reply_rx).await
    }

    async fn answer(&self, id: &str, answer: Answer, expected: Awaiting, none_waiting: &str, form_id: Option<u32>) -> Result<Value, (u16, String)> {
        let handle = self.get(id).ok_or((404, "Session not found or expired".to_string()))?;
        if !handle.shared.awaiting_alert.load(Ordering::SeqCst) || *handle.shared.awaiting_kind.lock().unwrap() != expected {
            return Err((409, none_waiting.to_string()));
        }
        if let Some(f) = form_id {
            if f != handle.shared.awaiting_form.load(Ordering::SeqCst) {
                return Err((409, format!("The pending question belongs to form {}, not form {}", handle.shared.awaiting_form.load(Ordering::SeqCst), f)));
            }
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        handle
            .shared
            .answer_tx
            .lock()
            .unwrap()
            .send((answer, reply_tx))
            .map_err(|_| (410, "Session has ended".to_string()))?;
        self.await_reply(id, &handle, reply_rx).await
    }

    pub async fn state(&self, id: &str) -> Result<Value, (u16, String)> {
        let handle = self.get(id).ok_or((404, "Session not found or expired".to_string()))?;
        if handle.shared.awaiting_alert.load(Ordering::SeqCst) {
            return Err((409, "An alert is waiting for an answer".to_string()));
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        handle.tx.send(Command::State(reply_tx)).map_err(|_| (410, "Session has ended".to_string()))?;
        self.await_reply(id, &handle, reply_rx).await
    }
}

/// Runs one browser event against the session. Returns an error message for the UI.
pub(crate) fn run_event(session: &mut FormSession, event: SessionEvent) -> Option<String> {
    let trig = |r: TriggerResult| match r {
        TriggerResult::Success => None,
        TriggerResult::Failure(m) => Some(format!("Trigger failed: {}", m)),
        TriggerResult::UnhandledException(m) => Some(format!("Unhandled exception: {}", m)),
    };
    let rt = |r: Result<(), oracle_forms_runtime::RuntimeError>| r.err().map(|e| e.to_string());
    let goto = |session: &mut FormSession, block: &str, item: &str, record: Option<usize>| -> Option<String> {
        if let Err(e) = session.go_item(&format!("{}.{}", block, item)) {
            return Some(e.to_string());
        }
        if let Some(r) = record {
            let current = session.system.cursor_record;
            if r + 1 != current {
                if let Err(e) = session.go_record(r + 1) {
                    return Some(e.to_string());
                }
            }
        }
        None
    };

    match event {
        SessionEvent::Button { block, item, record } => {
            // Clicking a push button moves input focus to it (Mouse Navigate = Yes, the
            // Forms default), then WHEN-BUTTON-PRESSED fires.
            let _ = session.go_item(&format!("{}.{}", block, item));
            if let Some(r) = record {
                if r + 1 != session.system.cursor_record {
                    let _ = session.go_block(&block);
                    if let Err(e) = session.go_record(r + 1) {
                        return Some(e.to_string());
                    }
                }
            }
            trig(session.press_button(&format!("{}.{}", block, item)))
        }
        SessionEvent::SetItem { block, item, value, record } => {
            if let Some(e) = goto(session, &block, &item, record) {
                return Some(e);
            }
            if let Err(e) = session.set_item_value(&format!("{}.{}", block, item), value) {
                return Some(e.to_string());
            }
            // Validate-from-list runs before the item's own validation trigger.
            if let Err(e) = oracle_forms_runtime::lov::validate_from_list(session, &block, &item) {
                session.messages.push(e.clone());
                return Some(e);
            }
            // Leaving the item validates it.
            match session.dispatch_event("WHEN-VALIDATE-ITEM", Some(&item), Some(&block)) {
                TriggerResult::Success => None,
                other => trig(other),
            }
        }
        SessionEvent::Navigate { block, item, record } => goto(session, &block, &item, record),
        SessionEvent::PickLov => rt(session.do_key("LIST_VALUES")),
        // Handled by the application (no form code runs).
        SessionEvent::ActivateForm { .. } => None,
        SessionEvent::ExecuteQuery { block } => {
            let b = block.or_else(|| session.active_block().map(str::to_string)).unwrap_or_default();
            if let Err(e) = session.go_block(&b) {
                return Some(e.to_string());
            }
            rt(session.do_key("EXECUTE_QUERY"))
        }
        SessionEvent::NextRecord => rt(session.do_key("NEXT_RECORD")),
        SessionEvent::PreviousRecord => rt(session.do_key("PREVIOUS_RECORD")),
        SessionEvent::CreateRecord => rt(session.do_key("CREATE_RECORD")),
        SessionEvent::DeleteRecord => rt(session.do_key("DELETE_RECORD")),
        SessionEvent::Commit => rt(session.do_key("COMMIT_FORM")),
        SessionEvent::ClearForm => rt(session.do_key("CLEAR_FORM")),
        SessionEvent::Key { name } => rt(session.do_key(&name)),
    }
}

/// Tracks how much of the message / trace log the browser has already seen.
#[derive(Default)]
pub(crate) struct Cursors {
    actions: usize,
    messages: usize,
    trace: usize,
}

pub(crate) fn snapshot(session: &FormSession, id: &str, backend: &str, cursor: &mut Cursors, missing_libraries: &[String]) -> Value {
    let blocks: Vec<Value> = session
        .module
        .blocks
        .iter()
        .map(|b| {
            let buf = session.blocks.get(&b.name.to_ascii_uppercase());
            let records: Vec<Value> = buf
                .map(|buf| {
                    buf.records
                        .iter()
                        .map(|r| json!({ "status": r.status.as_str(), "values": r.values }))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "name": b.name,
                "currentRecord": buf.map(|b| b.current_record_index).unwrap_or(0),
                "status": buf.map(|b| b.status().as_str()).unwrap_or("NEW"),
                "pendingDeletes": buf.map(|b| b.deleted_records.len()).unwrap_or(0),
                "records": records,
            })
        })
        .collect();

    let new_messages: Vec<&String> = session.messages.iter().skip(cursor.messages).collect();
    cursor.messages = session.messages.len();
    let start = cursor.trace.max(session.trace_log.len().saturating_sub(MAX_TRACE_LINES));
    let new_trace: Vec<&String> = session.trace_log.iter().skip(start).collect();
    cursor.trace = session.trace_log.len();
    cursor.actions = session.actions.len();

    let mut properties = serde_json::Map::new();
    for (item, props) in &session.item_properties {
        let mut m = serde_json::Map::new();
        for (p, v) in props {
            m.insert(p.clone(), v.to_json());
        }
        properties.insert(item.clone(), Value::Object(m));
    }
    json!({
        "sessionId": id,
        "moduleName": session.module.key.name,
        "backend": backend,
        "readOnly": session.read_only,
        "cursorBlock": session.system.cursor_block,
        "cursorItem": session.system.cursor_item,
        "cursorRecord": session.system.cursor_record,
        "formStatus": session.system.form_status.as_str(),
        "blocks": blocks,
        "itemProperties": properties,
        "messages": new_messages,
        "trace": new_trace,
        "exitRequested": session.exit_requested,
        "missingLibraries": missing_libraries,
        "alert": Value::Null,
        "lov": Value::Null,
        "listElements": session.objects.list_elements.iter().map(|(k, v)| (k.clone(), Value::Array(v.iter().map(|e| json!({ "label": e.label, "value": e.value })).collect()))).collect::<serde_json::Map<String, Value>>(),
        "menuItems": session.objects.menu_items.iter().map(|(k, m)| (k.clone(), Value::Object(m.iter().map(|(p, v)| (p.clone(), v.to_json())).collect()))).collect::<serde_json::Map<String, Value>>(),
        "actions": session.actions.iter().skip(cursor.actions).collect::<Vec<_>>(),
        "sandboxFiles": session.objects.files.iter().map(|(p, b)| json!({ "path": p, "size": b.len() })).collect::<Vec<_>>(),
        "nextTimerMs": session.next_timer_due_in_ms(),
        "error": Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oracle_forms_runtime::ALERT_BUTTON1;
    use oracle_forms_core::model::{ModuleKey, ModuleType};
    use std::path::PathBuf;

    fn conformance(name: &str) -> ModuleIndex {
        let path = PathBuf::from("../../samples/conformance").join(name);
        let cache = std::env::temp_dir().join(format!("ofw-sess-{}", std::process::id()));
        oracle_forms_xml::FormsXmlParser::parse(&ModuleKey::new(name.trim_end_matches("_fmb.xml").to_ascii_uppercase(), ModuleType::Form), &path, &cache).unwrap()
    }

    fn sqlite() -> Option<DbConfig> {
        Some(DbConfig { backend: oracle_forms_db::DbBackend::Sqlite, database_path: Some(":memory:".into()), ..Default::default() })
    }

    /// A session that cannot answer in time is cancelled, reported (not hung), and does not
    /// stop other sessions from starting.
    #[tokio::test]
    async fn startup_overrun_returns_error_and_other_sessions_still_start() {
        let mut slow = SessionManager::default();
        slow.start_timeout = Duration::from_millis(50);
        slow.startup_delay = Duration::from_millis(500);
        let err = slow
            .start(StartOptions { module: conformance("conf_03_query_cascade_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None })
            .await
            .unwrap_err();
        assert!(err.contains("did not finish"), "{err}");

        let ok = SessionManager::default()
            .start(StartOptions { module: conformance("conf_03_query_cascade_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None })
            .await
            .unwrap();
        assert!(ok["sessionId"].is_string());
    }

    fn lov_options() -> StartOptions {
        StartOptions { module: conformance("conf_20_lov_timers_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None }
    }

    #[tokio::test]
    async fn lov_pauses_the_event_and_the_answer_fills_the_return_items() {
        let mgr = SessionManager::default();
        let state = mgr.start(lov_options()).await.unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        assert!(state["lov"].is_null());

        mgr.send_event(&id, SessionEvent::Navigate { block: "CUSTOMERS".into(), item: "CUST_NAME".into(), record: None }).await.unwrap();
        mgr.send_event(&id, SessionEvent::SetItem { block: "CUSTOMERS".into(), item: "CUST_NAME".into(), value: json!("Zed"), record: Some(0) })
            .await
            .unwrap();
        // "Zed" is not in the list and validate-from-list is on: the event reports FRM-40212.
        let bad = mgr.send_event(&id, SessionEvent::SetItem { block: "CUSTOMERS".into(), item: "CUST_NAME".into(), value: json!("Zzz"), record: Some(0) }).await.unwrap();
        assert!(bad["error"].as_str().unwrap_or("").contains("FRM-40212"), "{bad}");

        mgr.send_event(&id, SessionEvent::SetItem { block: "CUSTOMERS".into(), item: "CUST_NAME".into(), value: json!(""), record: Some(0) }).await.ok();
        let paused = mgr.send_event(&id, SessionEvent::PickLov).await.unwrap();
        assert_eq!(paused["lov"]["lovName"], "LOV_CUST", "{paused}");
        assert_eq!(paused["lov"]["rows"].as_array().unwrap().len(), 3);
        assert_eq!(paused["lov"]["columns"][0]["title"], "Customer");

        // While the list is open other events and alert answers are refused.
        assert_eq!(mgr.send_event(&id, SessionEvent::NextRecord).await.unwrap_err().0, 409);
        assert_eq!(mgr.answer_alert(&id, ALERT_BUTTON1).await.unwrap_err().0, 409);

        let done = mgr.answer_lov(&id, Some(2)).await.unwrap();
        assert!(done["lov"].is_null(), "{done}");
        let customers = done["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "CUSTOMERS").unwrap();
        assert_eq!(customers["records"][0]["values"]["CUST_NAME"], "Birch Co", "{done}");
        assert_eq!(customers["records"][0]["values"]["CITY"], "Kelowna");
        assert!(mgr.remove(&id));
    }

    #[tokio::test]
    async fn lov_cancel_leaves_values_and_other_answers_are_refused() {
        let mgr = SessionManager::default();
        let state = mgr.start(lov_options()).await.unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        assert_eq!(mgr.answer_lov(&id, Some(0)).await.unwrap_err().0, 409, "no list is open");
        mgr.send_event(&id, SessionEvent::Navigate { block: "CUSTOMERS".into(), item: "ITEM_NAME".into(), record: None }).await.unwrap();
        let paused = mgr.send_event(&id, SessionEvent::PickLov).await.unwrap();
        assert_eq!(paused["lov"]["rows"].as_array().unwrap().len(), 5, "database rows: {paused}");
        let done = mgr.answer_lov(&id, None).await.unwrap();
        let customers = done["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "CUSTOMERS").unwrap();
        assert!(customers["records"][0]["values"]["ITEM_NAME"].is_null());
    }

    #[tokio::test]
    async fn timers_fire_after_startup_and_through_tick() {
        let mgr = SessionManager::default();
        let state = mgr.start(lov_options()).await.unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        let trace: Vec<String> = state["trace"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
        assert!(trace.iter().any(|t| t == "TIMER_EXPIRED: T_STARTUP"), "short startup timer fires with the startup response: {trace:?}");
        assert!(state["nextTimerMs"].is_null(), "a fired one-shot timer is no longer pending");

        let pressed = mgr.send_event(&id, SessionEvent::Button { block: "CUSTOMERS".into(), item: "BTN_TIMER".into(), record: None }).await.unwrap();
        assert!(pressed["nextTimerMs"].as_i64().unwrap() <= 30, "{pressed}");
        tokio::time::sleep(Duration::from_millis(60)).await;
        let ticked = mgr.tick(&id).await.unwrap();
        let trace: Vec<&str> = ticked["trace"].as_array().unwrap().iter().filter_map(|t| t.as_str()).collect();
        assert!(trace.contains(&"TIMER_EXPIRED: T_UI"), "{trace:?}");
        assert!(ticked["nextTimerMs"].is_null());
    }

    fn sample_loader() -> oracle_forms_runtime::FormLoader {
        std::sync::Arc::new(|name: &str| {
            let file = format!("{}_fmb.xml", name.to_ascii_lowercase());
            let path = PathBuf::from("../../samples/conformance").join(&file);
            if !path.is_file() {
                return Err(format!("{name} not found"));
            }
            let cache = std::env::temp_dir().join(format!("ofw-nav-{}", std::process::id()));
            let module = oracle_forms_xml::FormsXmlParser::parse(&ModuleKey::new(name.to_ascii_uppercase(), ModuleType::Form), &path, &cache).map_err(|e| e.to_string())?;
            Ok(oracle_forms_runtime::LoadedForm { module, libraries: vec![], missing_libraries: vec![] })
        })
    }

    async fn nav_session(allow_writes: bool, params: Vec<(String, String)>) -> (SessionManager, String, Value) {
        let mgr = SessionManager::default();
        let state = mgr
            .start(StartOptions { module: conformance("conf_21_nav_parent_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes, parameters: params, loader: Some(sample_loader()) })
            .await
            .unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        (mgr, id, state)
    }

    fn press(block: &str, item: &str) -> SessionEvent {
        SessionEvent::Button { block: block.into(), item: item.into(), record: None }
    }

    fn value(state: &Value, block: &str, item: &str) -> String {
        state["blocks"].as_array().unwrap().iter().find(|b| b["name"] == block).and_then(|b| b["records"][0]["values"][item].as_str()).unwrap_or("").to_string()
    }

    fn form_names(state: &Value) -> Vec<String> {
        state["forms"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test]
    async fn startup_parameters_and_system_current_form() {
        let (_m, _id, state) = nav_session(false, vec![("GREETING".into(), "hi".into())]).await;
        assert_eq!(value(&state, "MAIN", "RESULT"), "hi/CONF_21_NAV_PARENT", "{state}");
        assert_eq!(state["formId"], 1);
        assert_eq!(form_names(&state), vec!["CONF_21_NAV_PARENT"]);
    }

    #[tokio::test]
    async fn call_form_is_modal_passes_parameters_and_globals_and_returns_to_the_caller() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        let called = mgr.send_event(&id, press("MAIN", "BTN_CALL")).await.unwrap();
        assert_eq!(called["formId"], 2, "{called}");
        assert_eq!(value(&called, "C", "SEEN"), "hello", "the parameter list became :PARAMETER.MSG");
        let forms = called["forms"].as_array().unwrap();
        assert_eq!(forms.len(), 2);
        assert_eq!(forms[0]["suspended"], true);
        assert_eq!(forms[1]["modal"], true);

        // The suspended caller cannot be driven; the error names the reason.
        let refused = mgr.send_event_to(&id, press("MAIN", "BTN_OPEN"), Some(1)).await.unwrap();
        assert!(refused["error"].as_str().unwrap().contains("suspended"), "{refused}");

        let back = mgr.send_event_to(&id, press("C", "BTN_EXIT"), Some(2)).await.unwrap();
        assert_eq!(back["formId"], 1, "{back}");
        assert_eq!(form_names(&back), vec!["CONF_21_NAV_PARENT"]);
        assert_eq!(
            value(&back, "MAIN", "RESULT"),
            "P>C>back|hello:CONF_21_NAV_PARENT",
            "the caller's trigger resumed after CALL_FORM and saw the child's :GLOBAL"
        );
    }

    #[tokio::test]
    async fn call_form_can_nest_and_a_missing_form_fails_visibly() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        mgr.send_event(&id, press("MAIN", "BTN_CALL")).await.unwrap();
        let nested = mgr.send_event(&id, press("C", "BTN_NEST")).await.unwrap();
        assert_eq!(nested["formId"], 3, "{nested}");
        assert_eq!(form_names(&nested).len(), 3);
        let c = mgr.send_event(&id, press("E", "BTN_EXIT")).await.unwrap();
        assert_eq!(c["formId"], 2);
        assert_eq!(value(&c, "C", "SEEN"), "nest-returned");
        mgr.send_event(&id, press("C", "BTN_EXIT")).await.unwrap();

        let missing = mgr.send_event(&id, press("MAIN", "BTN_CALL_MISSING")).await.unwrap();
        assert_eq!(value(&missing, "MAIN", "RESULT"), "load failed", "{missing}");
        assert!(missing["messages"].to_string().contains("no_such_form") || missing["messages"].to_string().contains("not found"), "{missing}");
    }

    #[tokio::test]
    async fn query_only_call_starts_the_form_read_only() {
        let (mgr, id, state) = nav_session(true, vec![]).await;
        assert_eq!(state["readOnly"], false);
        let called = mgr.send_event(&id, press("MAIN", "BTN_CALL_QO")).await.unwrap();
        assert_eq!(called["readOnly"], true, "{called}");
        assert_eq!(called["forms"][0]["hidden"], true, "HIDE hides the caller");
        assert_eq!(called["forms"][1]["replaceMenu"], true);
        let back = mgr.send_event(&id, press("C", "BTN_EXIT")).await.unwrap();
        assert_eq!(back["readOnly"], false);
    }

    #[tokio::test]
    async fn open_form_creates_a_peer_that_can_be_activated_found_and_closed() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        let opened = mgr.send_event(&id, press("MAIN", "BTN_OPEN")).await.unwrap();
        assert_eq!(opened["formId"], 2, "ACTIVATE makes the new form active: {opened}");
        assert_eq!(opened["forms"].as_array().unwrap().len(), 2);
        assert_eq!(opened["forms"][0]["suspended"], false, "OPEN_FORM does not suspend the opener");
        assert_eq!(value(&opened, "E", "INFO"), "CONF_21_NAV_PEER");

        // Back to the parent (tab click), whose trigger text shows the trail.
        let parent = mgr.send_event(&id, SessionEvent::ActivateForm { form_id: 1 }).await.unwrap();
        assert_eq!(parent["formId"], 1);
        assert_eq!(value(&parent, "MAIN", "RESULT"), "P>opened");
        let found = mgr.send_event(&id, press("MAIN", "BTN_FIND")).await.unwrap();
        assert_eq!(value(&found, "MAIN", "RESULT"), "peer open");

        // A second OPEN of the same form is refused with a message.
        let again = mgr.send_event(&id, press("MAIN", "BTN_OPEN")).await.unwrap();
        assert!(again["messages"].to_string().contains("already open"), "{again}");

        let closed = mgr.send_event(&id, press("MAIN", "BTN_CLOSE")).await.unwrap();
        assert_eq!(form_names(&closed), vec!["CONF_21_NAV_PARENT"]);
        let found = mgr.send_event(&id, press("MAIN", "BTN_FIND")).await.unwrap();
        assert_eq!(value(&found, "MAIN", "RESULT"), "peer closed");
    }

    #[tokio::test]
    async fn open_form_no_activate_and_session_mode_and_go_form_and_exit_of_a_peer() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        let bg = mgr.send_event(&id, press("MAIN", "BTN_OPEN_BG")).await.unwrap();
        assert_eq!(bg["formId"], 1, "NO_ACTIVATE keeps the opener active: {bg}");
        assert_eq!(bg["forms"].as_array().unwrap().len(), 2);
        let gone = mgr.send_event(&id, press("MAIN", "BTN_GO")).await.unwrap();
        assert_eq!(gone["activeForm"], 2, "GO_FORM moves focus to the peer: {gone}");
        let exited = mgr.send_event_to(&id, press("E", "BTN_EXIT"), Some(2)).await.unwrap();
        assert_eq!(form_names(&exited), vec!["CONF_21_NAV_PARENT"], "EXIT_FORM in an opened form closes just that form");
        assert_eq!(exited["activeForm"], 1);
    }

    #[tokio::test]
    async fn new_form_replaces_the_current_form_and_keeps_globals() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        let replaced = mgr.send_event(&id, press("MAIN", "BTN_NEW")).await.unwrap();
        assert_eq!(form_names(&replaced), vec!["CONF_21_NAV_REPL"], "{replaced}");
        assert_eq!(value(&replaced, "R", "MARK"), "P>R", ":GLOBAL carried over to the replacement");
        assert_eq!(replaced["formId"], 1);
    }

    #[tokio::test]
    async fn exit_form_asks_to_save_database_changes_and_honours_the_answer() {
        // CONF_08 has a database block; changing it makes the form "changed".
        let mgr = SessionManager::default();
        let state = mgr
            .start(StartOptions { module: conformance("conf_08_commit_pipeline_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None })
            .await
            .unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        mgr.send_event(&id, SessionEvent::ExecuteQuery { block: Some("ITEMS".into()) }).await.unwrap();
        mgr.send_event(&id, SessionEvent::SetItem { block: "ITEMS".into(), item: "ITEM_NAME".into(), value: json!("edited"), record: Some(0) }).await.unwrap();
        let asked = mgr.send_event(&id, SessionEvent::Key { name: "EXIT_FORM".into() }).await.unwrap();
        assert_eq!(asked["alert"]["alertName"], "EXIT_FORM_SAVE_CHANGES", "{asked}");
        assert_eq!(asked["formId"], 1);
        // Cancel keeps the form open.
        let kept = mgr.answer_alert_for(&id, oracle_forms_runtime::ALERT_BUTTON3, Some(1)).await.unwrap();
        assert_eq!(kept["exitRequested"], false, "{kept}");
        // Asking again and answering No ends the session without writing.
        mgr.send_event(&id, SessionEvent::Key { name: "EXIT_FORM".into() }).await.unwrap();
        assert_eq!(mgr.answer_alert_for(&id, oracle_forms_runtime::ALERT_BUTTON2, Some(9)).await.unwrap_err().0, 409, "wrong form id");
        let ended = mgr.answer_alert_for(&id, oracle_forms_runtime::ALERT_BUTTON2, Some(1)).await.unwrap();
        assert_eq!(ended["exitRequested"], true, "{ended}");
    }

    #[tokio::test]
    async fn exit_form_in_the_root_ends_the_application() {
        let (mgr, id, _s) = nav_session(false, vec![]).await;
        let ended = mgr.send_event(&id, press("MAIN", "BTN_EXIT")).await.unwrap();
        assert_eq!(ended["exitRequested"], true, "{ended}");
        let after = mgr.send_event(&id, press("MAIN", "BTN_CALL")).await.unwrap();
        assert!(after["error"].as_str().unwrap().contains("ended"), "{after}");
    }

    #[tokio::test]
    async fn alert_pauses_trigger_and_answer_resumes_it() {
        let mgr = SessionManager::default();
        let state = mgr
            .start(StartOptions { module: conformance("conf_03_query_cascade_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None })
            .await
            .unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();

        let paused = mgr
            .send_event(&id, SessionEvent::Button { block: "ORDERS".into(), item: "BTN_DELETE".into(), record: None })
            .await
            .unwrap();
        assert_eq!(paused["alert"]["alertName"], "CONFIRM_DELETE", "{paused}");
        assert_eq!(paused["sessionId"], id);

        // While the alert is open, other events are refused.
        let busy = mgr.send_event(&id, SessionEvent::NextRecord).await.unwrap_err();
        assert_eq!(busy.0, 409);

        let done = mgr.answer_alert(&id, ALERT_BUTTON1).await.unwrap();
        assert!(done["alert"].is_null(), "{done}");
        let orders = done["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "ORDERS").unwrap();
        assert_eq!(orders["records"][0]["values"]["ORDER_STATUS"], "CANCELLED");
        assert!(done["messages"].as_array().unwrap().iter().any(|m| m == "Order cancelled."), "{done}");
        assert!(mgr.remove(&id));
    }

    #[tokio::test]
    async fn read_only_session_refuses_commit_but_queries_run() {
        let mgr = SessionManager::default();
        let state = mgr
            .start(StartOptions { module: conformance("conf_08_commit_pipeline_fmb.xml"), libraries: vec![], missing_libraries: vec![], db: sqlite(), allow_writes: false, parameters: vec![], loader: None })
            .await
            .unwrap();
        let id = state["sessionId"].as_str().unwrap().to_string();
        let q = mgr.send_event(&id, SessionEvent::ExecuteQuery { block: Some("ITEMS".into()) }).await.unwrap();
        assert!(q["error"].is_null(), "{q}");
        let items = q["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "ITEMS").unwrap();
        assert!(items["records"].as_array().unwrap().len() >= 2, "{items}");

        let edited = mgr
            .send_event(&id, SessionEvent::SetItem { block: "ITEMS".into(), item: "ITEM_NAME".into(), value: json!("changed"), record: Some(0) })
            .await
            .unwrap();
        assert!(edited["error"].is_null(), "{edited}");
        let commit = mgr.send_event(&id, SessionEvent::Commit).await.unwrap();
        assert!(commit["error"].as_str().unwrap_or("").contains("read-only"), "{commit}");
    }
}
