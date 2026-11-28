//! Runtime gap-closure tests: execution budgets / runaway protection, then parameter
//! lists, timers, LOV dialogs and record groups. All fixtures are synthetic.
//!
//! Purpose: Integration/conformance tests (runtime_gap_tests.rs).
//! Key Input Dependencies: oracle_forms_core, oracle_forms_db, oracle_forms_runtime, oracle_forms_xml, samples/conformance/conf_03_query_cascade_fmb.xml
//! Index: sample, unit, trigger, session_with, button, press, infinite_loop_in_package_routine_is_stopped_and_named, cancelled_session_stops_running_plsql, stray_end_in_program_unit_text_does_not_hang_session_creation, record_field_assignment_and_read_work

use oracle_forms_core::model::{ModuleKey, ModuleType, ProgramUnitInfo, ProgramUnitType, TriggerInfo, TriggerLevel};
use oracle_forms_db::SqliteDriver;
use oracle_forms_runtime::{FormSession, TriggerResult};
use oracle_forms_xml::FormsXmlParser;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn sample(name: &str) -> PathBuf {
    for p in ["", "..", "../.."] {
        for d in ["samples", "samples/conformance"] {
            let c = PathBuf::from(p).join(d).join(name);
            if c.is_file() {
                return c;
            }
        }
    }
    panic!("sample {name} not found");
}

fn unit(name: &str, kind: ProgramUnitType, source: &str) -> ProgramUnitInfo {
    ProgramUnitInfo {
        name: name.into(),
        unit_type: kind,
        line_count: source.lines().count(),
        first_line: source.lines().next().unwrap_or("").into(),
        inherited: None,
        text_encoding: Default::default(),
        text_ref: None,
        xml_ref: None,
        source_text: Some(source.into()),
    }
}

fn trigger(name: &str, block: Option<&str>, item: Option<&str>, source: &str) -> TriggerInfo {
    TriggerInfo {
        name: name.into(),
        level: match (block, item) {
            (Some(_), Some(_)) => TriggerLevel::Item,
            (Some(_), None) => TriggerLevel::Block,
            _ => TriggerLevel::Form,
        },
        block_name: block.map(Into::into),
        item_name: item.map(Into::into),
        first_line: source.lines().next().unwrap_or("").into(),
        line_count: source.lines().count(),
        inherited: None,
        text_encoding: Default::default(),
        text_ref: None,
        xml_ref: None,
        source_text: Some(source.into()),
        execution_hierarchy: None,
    }
}

fn session_with(units: Vec<ProgramUnitInfo>, triggers: Vec<TriggerInfo>) -> FormSession {
    let key = ModuleKey::new("GAP", ModuleType::Form);
    let cache = std::env::temp_dir().join(format!("ofw-gap-{}", std::process::id()));
    let mut module = FormsXmlParser::parse(&key, &sample("conf_03_query_cascade_fmb.xml"), &cache).unwrap();
    module.program_units = units;
    module.triggers.retain(|t| t.name != "WHEN-BUTTON-PRESSED");
    module.triggers.extend(triggers);
    let mut session = FormSession::new(module);
    session.set_database_driver(Arc::new(SqliteDriver::in_memory().unwrap()));
    session
}

fn button(source: &str) -> TriggerInfo {
    trigger("WHEN-BUTTON-PRESSED", Some("ORDERS"), Some("BTN_DELETE"), source)
}

fn press(session: &mut FormSession) -> TriggerResult {
    session.dispatch_event("WHEN-BUTTON-PRESSED", Some("BTN_DELETE"), Some("ORDERS"))
}

#[test]
fn infinite_loop_in_package_routine_is_stopped_and_named() {
    let mut s = session_with(
        vec![unit("SPIN", ProgramUnitType::Procedure, "PROCEDURE spin IS BEGIN LOOP NULL; END LOOP; END;")],
        vec![button("BEGIN spin; END;")],
    );
    let r = press(&mut s);
    assert!(!r.is_success());
    let msg = s.messages.join(" | ");
    assert!(msg.contains("Execution budget exceeded"), "{msg}");
    assert!(msg.contains("WHEN-BUTTON-PRESSED"), "trigger named: {msg}");
    assert!(msg.contains("SPIN"), "program unit named: {msg}");
    assert!(msg.contains("statement (after"), "statement kind named: {msg}");
}

#[test]
fn cancelled_session_stops_running_plsql() {
    let mut s = session_with(
        vec![unit("SPIN", ProgramUnitType::Procedure, "PROCEDURE spin IS BEGIN LOOP NULL; END LOOP; END;")],
        vec![button("BEGIN spin; END;")],
    );
    s.cancel.store(true, Ordering::SeqCst);
    assert!(!press(&mut s).is_success());
    assert!(s.messages.iter().any(|m| m.contains("cancelled")), "{:?}", s.messages);
}

#[test]
fn stray_end_in_program_unit_text_does_not_hang_session_creation() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let s = session_with(
            vec![unit("PAIR", ProgramUnitType::PackageBody, "PROCEDURE a IS BEGIN NULL; END;\nEND;\nEND pkg_x;")],
            vec![],
        );
        let _ = tx.send(s.module.program_units.len());
    });
    rx.recv_timeout(std::time::Duration::from_secs(20)).expect("session creation must terminate");
}

#[test]
fn record_field_assignment_and_read_work() {
    let mut s = session_with(
        vec![],
        vec![button("DECLARE r some_record_type; BEGIN r.id := 41; r.id := r.id + 1; :ORDERS.TOTAL_AMOUNT := r.id; END;")],
    );
    assert_eq!(press(&mut s), TriggerResult::Success, "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(s.get_bind_value("ORDERS.TOTAL_AMOUNT"), oracle_forms_runtime::OracleValue::int(42));
}
