//! Timers: `CREATE_TIMER`, `DELETE_TIMER`, `SET_TIMER`, `FIND_TIMER`,
//! `GET_APPLICATION_PROPERTY(TIMER_NAME)` and the form-level `WHEN-TIMER-EXPIRED` trigger.
//!
//! Event model: Forms runs timer triggers when the form is idle, between user events. The
//! runtime has no thread of its own, so the host drives it: [`FormSession::fire_due_timers`]
//! runs the `WHEN-TIMER-EXPIRED` trigger once for every timer that has expired on the
//! session clock. The server calls it after startup (with a small look-ahead so the
//! common "run this right after the form opens" idiom, a timer of a few milliseconds
//! created in `WHEN-NEW-FORM-INSTANCE`, fires as part of the startup response), after every
//! browser event, and on the `tick` request the web UI polls while timers are pending.
//!
//! A repeating timer is rescheduled `interval` after it fired (no catch-up burst); a
//! non-repeating timer stays defined but inactive after firing until `SET_TIMER` restarts
//! it or `DELETE_TIMER` removes it (not verified against 14c). An interval of `0` is
//! accepted and means "as soon as the form is idle"; the Forms limit is 2147483648 ms.
//!
//! Purpose: Module implementation (timers.rs).
//! Key Input Dependencies: crate::objects, crate::session
//! Index: dispatch, create, set, fire_due_timers, next_timer_due_in_ms, MAX_INTERVAL_MS

use crate::builtins::BuiltinResult;
use crate::objects::Timer;
use crate::session::FormSession;
use crate::trigger::TriggerResult;
use crate::value::OracleValue;

/// Largest interval Forms accepts (milliseconds).
pub const MAX_INTERVAL_MS: i64 = 2_147_483_648;

fn text(args: &[OracleValue], idx: usize) -> Option<String> {
    args.get(idx).and_then(|v| v.as_text())
}

fn key(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// `REPEAT` / `NO_REPEAT` / `NO_CHANGE` (absent means the argument default).
fn parse_iterate(args: &[OracleValue], idx: usize) -> Result<Option<bool>, String> {
    match text(args, idx).map(|t| t.to_ascii_uppercase()) {
        None => Ok(None),
        Some(t) if t == "NO_CHANGE" => Ok(None),
        Some(t) if t == "REPEAT" => Ok(Some(true)),
        Some(t) if t == "NO_REPEAT" => Ok(Some(false)),
        Some(t) => Err(format!("{t} is not REPEAT, NO_REPEAT or NO_CHANGE")),
    }
}

fn parse_interval(args: &[OracleValue], idx: usize) -> Result<Option<i64>, String> {
    match args.get(idx) {
        None | Some(OracleValue::Null) => Ok(None),
        Some(v) => {
            if v.as_text().map(|t| t.eq_ignore_ascii_case("NO_CHANGE")).unwrap_or(false) {
                return Ok(None);
            }
            match v.as_i64() {
                Some(n) if (0..=MAX_INTERVAL_MS).contains(&n) => Ok(Some(n)),
                Some(n) => Err(format!("interval {n} ms is outside 0..{MAX_INTERVAL_MS}")),
                None => Err(format!("interval {} is not a number", v.to_string_lossy())),
            }
        }
    }
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    Some(match name {
        "CREATE_TIMER" => create(args, session),
        "SET_TIMER" => set(args, session),
        "DELETE_TIMER" => match text(args, 0) {
            Some(t) if session.objects.timers.remove(&key(&t)).is_some() => BuiltinResult::Void,
            Some(t) => BuiltinResult::Failure(format!("DELETE_TIMER: timer {t} does not exist")),
            None => BuiltinResult::Failure("DELETE_TIMER requires a timer name".into()),
        },
        "FIND_TIMER" => match text(args, 0) {
            Some(t) if session.objects.timers.contains_key(&key(&t)) => BuiltinResult::Value(OracleValue::varchar2(key(&t))),
            _ => BuiltinResult::Value(OracleValue::Null),
        },
        _ => return None,
    })
}

fn create(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(raw) = text(args, 0).filter(|t| !t.trim().is_empty()) else {
        return BuiltinResult::Failure("CREATE_TIMER requires a timer name".into());
    };
    let k = key(&raw);
    if session.objects.timers.contains_key(&k) {
        return BuiltinResult::Failure(format!("CREATE_TIMER: timer {k} already exists"));
    }
    let interval = match parse_interval(args, 1) {
        Ok(Some(n)) => n,
        Ok(None) => return BuiltinResult::Failure("CREATE_TIMER requires a number of milliseconds".into()),
        Err(e) => return BuiltinResult::Failure(format!("CREATE_TIMER: {e}")),
    };
    let repeat = match parse_iterate(args, 2) {
        Ok(r) => r.unwrap_or(true),
        Err(e) => return BuiltinResult::Failure(format!("CREATE_TIMER: {e}")),
    };
    let due = session.objects.now_ms() + interval;
    session.objects.timers.insert(k.clone(), Timer { name: k.clone(), interval_ms: interval, repeat, due_at_ms: Some(due) });
    BuiltinResult::Value(OracleValue::varchar2(k))
}

/// `SET_TIMER(timer, milliseconds, iterate)`: changes the settings and restarts the countdown.
fn set(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(raw) = text(args, 0) else {
        return BuiltinResult::Failure("SET_TIMER requires a timer name".into());
    };
    let k = key(&raw);
    if !session.objects.timers.contains_key(&k) {
        return BuiltinResult::Failure(format!("SET_TIMER: timer {k} does not exist"));
    }
    let interval = match parse_interval(args, 1) {
        Ok(v) => v,
        Err(e) => return BuiltinResult::Failure(format!("SET_TIMER: {e}")),
    };
    let repeat = match parse_iterate(args, 2) {
        Ok(v) => v,
        Err(e) => return BuiltinResult::Failure(format!("SET_TIMER: {e}")),
    };
    let now = session.objects.now_ms();
    let timer = session.objects.timers.get_mut(&k).expect("checked");
    if let Some(i) = interval {
        timer.interval_ms = i;
    }
    if let Some(r) = repeat {
        timer.repeat = r;
    }
    timer.due_at_ms = Some(now + timer.interval_ms);
    BuiltinResult::Void
}

impl FormSession {
    /// Runs `WHEN-TIMER-EXPIRED` for every timer due within `look_ahead_ms` of the session
    /// clock. Returns the names of the timers that fired, in order. Does nothing while a
    /// trigger is running (Forms fires timers only when idle).
    pub fn fire_due_timers(&mut self, look_ahead_ms: i64) -> Vec<String> {
        if self.is_inside_trigger() {
            return Vec::new();
        }
        let horizon = self.objects.now_ms() + look_ahead_ms;
        let mut due: Vec<(i64, String)> = self
            .objects
            .timers
            .values()
            .filter_map(|t| t.due_at_ms.filter(|d| *d <= horizon).map(|d| (d, t.name.clone())))
            .collect();
        due.sort();
        let mut fired = Vec::new();
        for (_, name) in due {
            let now = self.objects.now_ms();
            // The trigger of an earlier timer may have deleted or reset this one.
            let Some(timer) = self.objects.timers.get_mut(&name) else { continue };
            if !timer.due_at_ms.map(|d| d <= horizon).unwrap_or(false) {
                continue;
            }
            timer.due_at_ms = if timer.repeat { Some(now + timer.interval_ms.max(1)) } else { None };
            self.objects.expired_timer = Some(name.clone());
            self.trace_log.push(format!("TIMER_EXPIRED: {name}"));
            if let TriggerResult::UnhandledException(e) | TriggerResult::Failure(e) = self.dispatch_event("WHEN-TIMER-EXPIRED", None, None) {
                self.trace_log.push(format!("TIMER_TRIGGER_ERROR: {name}: {e}"));
            }
            fired.push(name);
        }
        fired
    }

    /// Milliseconds until the next active timer expires (`0` if already due); `None` when
    /// no timer is active. The UI uses this to decide whether to poll.
    pub fn next_timer_due_in_ms(&self) -> Option<i64> {
        let now = self.objects.now_ms();
        self.objects.timers.values().filter_map(|t| t.due_at_ms).min().map(|d| (d - now).max(0))
    }
}
