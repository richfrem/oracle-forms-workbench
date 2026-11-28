//! Runtime gap-closure tests: execution budgets / runaway protection, then parameter
//! lists, timers, LOV dialogs and record groups. All fixtures are synthetic.
//!
//! Purpose: Integration/conformance tests (runtime_gap_tests.rs).
//! Key Input Dependencies: oracle_forms_core, oracle_forms_db, oracle_forms_runtime, oracle_forms_xml, samples/conformance/conf_03_query_cascade_fmb.xml
//! Index: sample, unit, trigger, session_with, button, press, infinite_loop_in_package_routine_is_stopped_and_named, cancelled_session_stops_running_plsql, stray_end_in_program_unit_text_does_not_hang_session_creation, record_field_assignment_and_read_work, objects_session, exec, global, parameter_list_lifecycle_and_out_parameters, parameter_list_errors_set_form_failure_and_continue, record_group_programmatic_cells_rows_and_types, record_group_queries_populate_and_report_status, design_time_record_groups_exist_at_startup, timers_create_find_set_delete_and_fire, startup_timer_with_short_delay_runs_after_startup, timers_do_not_fire_inside_a_running_trigger, lov_handler, show_lov_runs_query_applies_column_mappings_and_returns_true, show_lov_cancel_returns_false_and_leaves_items_alone, show_lov_without_a_user_is_cancelled_and_missing_lov_fails, list_values_uses_the_current_items_lov_and_restricts_by_typed_text, list_values_on_an_item_without_a_lov_fails_visibly, key_listval_trigger_overrides_the_default, validate_from_list_accepts_exact_completes_unique_prefix_and_rejects_unknown, validate_from_list_with_several_matches_shows_the_lov, button_trigger_can_use_show_lov_result, lov_overrides_change_the_group, form_names_lose_paths_and_extensions, open_new_go_close_queue_requests_with_parsed_modes_and_parameters, call_form_without_a_host_is_reported_and_data_parameters_are_refused, form_introspection_reports_the_stack, exit_form_modes_control_the_commit_and_the_prompt

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

// ---------------------------------------------------------------------------
// Runtime objects: parameter lists, record groups, timers, LOVs
// ---------------------------------------------------------------------------

use oracle_forms_runtime::{ExecutionStatus, Interpreter, InteractionRequest, InteractionResponse, OracleValue};
use std::sync::Mutex;

/// A session on the synthetic LOV/timer/record-group form with an in-memory database.
fn objects_session() -> FormSession {
    let key = ModuleKey::new("CONF20", ModuleType::Form);
    let cache = std::env::temp_dir().join(format!("ofw-gap20-{}", std::process::id()));
    let module = FormsXmlParser::parse(&key, &sample("conf_20_lov_timers_fmb.xml"), &cache).unwrap();
    let mut s = FormSession::new(module);
    s.set_database_driver(Arc::new(SqliteDriver::in_memory().unwrap()));
    s
}

/// Runs an anonymous PL/SQL block as a trigger body would run.
fn exec(s: &mut FormSession, code: &str) -> ExecutionStatus {
    let unit = oracle_forms_plsql::Parser::new(code).parse_unit().expect("parses");
    let mut interp = Interpreter::for_trigger(s, "TEST");
    interp.execute_unit(&unit, s).expect("runs")
}

fn global(s: &FormSession, name: &str) -> String {
    s.get_bind_value(&format!("GLOBAL.{name}")).to_string_lossy()
}

#[test]
fn parameter_list_lifecycle_and_out_parameters() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE pl ParamList; t VARCHAR2(30); v VARCHAR2(100); BEGIN \
           pl := CREATE_PARAMETER_LIST('PL1'); \
           ADD_PARAMETER(pl, 'dept', TEXT_PARAMETER, '10'); \
           ADD_PARAMETER(pl, 'grp', DATA_PARAMETER, 'RG_STATIC'); \
           GET_PARAMETER_ATTR(pl, 'DEPT', t, v); :GLOBAL.A := t || '=' || v; \
           GET_PARAMETER_ATTR(pl, 'grp', t, v); :GLOBAL.B := t || '=' || v; \
           SET_PARAMETER_ATTR(pl, 'dept', TEXT_PARAMETER, '20'); \
           GET_PARAMETER_ATTR(pl, 'dept', t, v); :GLOBAL.C := v; \
           IF ID_NULL(GET_PARAMETER_LIST('PL1')) THEN :GLOBAL.D := 'missing'; ELSE :GLOBAL.D := 'found'; END IF; \
           DELETE_PARAMETER(pl, 'dept'); \
           DESTROY_PARAMETER_LIST(pl); \
           IF ID_NULL(GET_PARAMETER_LIST('PL1')) THEN :GLOBAL.E := 'gone'; END IF; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "A"), "TEXT_PARAMETER=10");
    assert_eq!(global(&s, "B"), "DATA_PARAMETER=RG_STATIC");
    assert_eq!(global(&s, "C"), "20");
    assert_eq!(global(&s, "D"), "found");
    assert_eq!(global(&s, "E"), "gone");
}

#[test]
fn parameter_list_errors_set_form_failure_and_continue() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE pl ParamList; BEGIN pl := CREATE_PARAMETER_LIST('PL2'); \
           ADD_PARAMETER(pl, 'K', TEXT_PARAMETER, 'x'); \
           ADD_PARAMETER(pl, 'k', TEXT_PARAMETER, 'dup'); IF FORM_FAILURE THEN :GLOBAL.DUP := 'failed'; END IF; \
           ADD_PARAMETER(pl, 'D', DATA_PARAMETER, 'NO_SUCH_GROUP'); IF FORM_FAILURE THEN :GLOBAL.RG := 'failed'; END IF; \
           pl := CREATE_PARAMETER_LIST('PL2'); IF FORM_FAILURE THEN :GLOBAL.NEW := 'failed'; END IF; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None));
    assert_eq!(global(&s, "DUP"), "failed");
    assert_eq!(global(&s, "RG"), "failed");
    assert_eq!(global(&s, "NEW"), "failed");
    assert_eq!(s.objects.parameter_lists["PL2"].entries.len(), 1, "failed adds leave the list unchanged");
}

#[test]
fn record_group_programmatic_cells_rows_and_types() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE g RecordGroup; c1 GroupColumn; x NUMBER; BEGIN \
           g := CREATE_GROUP('RG_NEW'); \
           c1 := ADD_GROUP_COLUMN(g, 'LABEL', CHAR_COLUMN, 20); \
           ADD_GROUP_COLUMN(g, 'QTY', NUMBER_COLUMN); \
           ADD_GROUP_ROW(g, END_OF_GROUP); ADD_GROUP_ROW(g, END_OF_GROUP); \
           SET_GROUP_CHAR_CELL('RG_NEW.LABEL', 1, 'first'); SET_GROUP_NUMBER_CELL('RG_NEW.QTY', 1, 7); \
           SET_GROUP_CHAR_CELL('RG_NEW.LABEL', 2, 'second'); \
           ADD_GROUP_ROW(g, 1); SET_GROUP_CHAR_CELL('RG_NEW.LABEL', 1, 'zeroth'); \
           :GLOBAL.CNT := GET_GROUP_ROW_COUNT(g); \
           :GLOBAL.L2 := GET_GROUP_CHAR_CELL('RG_NEW.LABEL', 2); \
           :GLOBAL.Q2 := GET_GROUP_NUMBER_CELL('RG_NEW.QTY', 2); \
           x := GET_GROUP_NUMBER_CELL('RG_NEW.LABEL', 1); IF FORM_FAILURE THEN :GLOBAL.TYPE := 'mismatch'; END IF; \
           DELETE_GROUP_ROW(g, 1); :GLOBAL.CNT2 := GET_GROUP_ROW_COUNT(g); \
           DELETE_GROUP_ROW(g, ALL_ROWS); :GLOBAL.CNT3 := GET_GROUP_ROW_COUNT(g); \
           DELETE_GROUP(g); IF ID_NULL(FIND_GROUP('RG_NEW')) THEN :GLOBAL.DEL := 'deleted'; END IF; \
         END;",
    );
    assert!(matches!(st, ExecutionStatus::Success(_)) || matches!(st, ExecutionStatus::UnhandledException(_)), "{st:?}");
    assert_eq!(global(&s, "CNT"), "3", "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "L2"), "first");
    assert_eq!(global(&s, "Q2"), "7");
    assert_eq!(global(&s, "TYPE"), "mismatch");
    assert_eq!(global(&s, "CNT2"), "2");
    assert_eq!(global(&s, "CNT3"), "0");
    assert_eq!(global(&s, "DEL"), "deleted");
}

#[test]
fn record_group_queries_populate_and_report_status() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE g RecordGroup; n NUMBER; BEGIN \
           g := CREATE_GROUP_FROM_QUERY('RG_Q', 'SELECT item_name, amount FROM test_items WHERE amount > :GLOBAL.MIN ORDER BY item_id'); \
           :GLOBAL.MIN := '20'; \
           n := POPULATE_GROUP(g); :GLOBAL.S1 := n; :GLOBAL.C1 := GET_GROUP_ROW_COUNT(g); \
           :GLOBAL.FIRST := GET_GROUP_CHAR_CELL('RG_Q.ITEM_NAME', 1); \
           n := POPULATE_GROUP_WITH_QUERY(g, 'SELECT item_name, amount FROM test_items WHERE amount > 1000'); \
           :GLOBAL.S2 := n; :GLOBAL.C2 := GET_GROUP_ROW_COUNT(g); \
           DELETE_GROUP('RG_STATIC'); IF FORM_FAILURE THEN :GLOBAL.DESIGN := 'protected'; END IF; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "S1"), "0");
    assert_eq!(global(&s, "C1"), "3", "amounts 99.5, 25, 150");
    assert_eq!(global(&s, "FIRST"), "Gadget B", "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "S2"), "1403", "no rows");
    assert_eq!(global(&s, "C2"), "0");
    assert_eq!(global(&s, "DESIGN"), "protected");
}

#[test]
fn design_time_record_groups_exist_at_startup() {
    let s = objects_session();
    assert_eq!(s.objects.record_groups["RG_STATIC"].rows.len(), 3, "static rows come from RecordGroupColumnRow");
    assert!(!s.objects.record_groups["RG_ITEMS"].populated, "query groups wait for a populate");
}

#[test]
fn timers_create_find_set_delete_and_fire() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE t TIMER; BEGIN :GLOBAL.TIMER_LOG := ''; \
           t := CREATE_TIMER('T_ONCE', 500, NO_REPEAT); CREATE_TIMER('T_REP', 200, REPEAT); \
           IF NOT ID_NULL(FIND_TIMER('T_ONCE')) THEN :GLOBAL.FOUND := 'yes'; END IF; \
           IF ID_NULL(FIND_TIMER('NOPE')) THEN :GLOBAL.NF := 'null'; END IF; \
           CREATE_TIMER('T_ONCE', 5); IF FORM_FAILURE THEN :GLOBAL.DUP := 'failed'; END IF; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?}", s.messages);
    assert_eq!(global(&s, "FOUND"), "yes");
    assert_eq!(global(&s, "NF"), "null");
    assert_eq!(global(&s, "DUP"), "failed");

    // Nothing is due yet.
    assert!(s.fire_due_timers(0).is_empty());
    assert!(s.next_timer_due_in_ms().unwrap() <= 200);

    s.objects.advance_clock(250);
    assert_eq!(s.fire_due_timers(0), vec!["T_REP"]);
    s.objects.advance_clock(250);
    let fired = s.fire_due_timers(0);
    assert_eq!(fired, vec!["T_REP", "T_ONCE"], "ordered by due time: T_REP was rescheduled to 450, T_ONCE is due at 500");
    assert_eq!(global(&s, "TIMER_LOG"), "T_REP;T_REP;T_ONCE;");

    // The one-shot timer stays defined but inactive; the repeating one keeps firing.
    s.objects.advance_clock(250);
    assert_eq!(s.fire_due_timers(0), vec!["T_REP"]);

    // SET_TIMER restarts with new settings; DELETE_TIMER removes.
    exec(&mut s, "BEGIN SET_TIMER('T_ONCE', 10, NO_REPEAT); SET_TIMER('T_REP', NO_CHANGE, NO_REPEAT); END;");
    s.objects.advance_clock(20);
    let fired = s.fire_due_timers(0);
    assert!(fired.contains(&"T_ONCE".to_string()), "{fired:?}");
    // T_REP became NO_REPEAT: it fires at most once more, then stays inactive.
    s.objects.advance_clock(1_000);
    s.fire_due_timers(0);
    s.objects.advance_clock(1_000);
    assert!(s.fire_due_timers(0).is_empty());
    exec(&mut s, "BEGIN DELETE_TIMER('T_REP'); DELETE_TIMER('T_ONCE'); END;");
    assert!(s.next_timer_due_in_ms().is_none());
    s.objects.advance_clock(10_000);
    assert!(s.fire_due_timers(0).is_empty());
    exec(&mut s, "BEGIN DELETE_TIMER('T_ONCE'); IF FORM_FAILURE THEN :GLOBAL.GONE := 'failed'; END IF; END;");
    assert_eq!(global(&s, "GONE"), "failed");
}

#[test]
fn startup_timer_with_short_delay_runs_after_startup() {
    let mut s = objects_session();
    s.start().unwrap();
    assert_eq!(global(&s, "TIMER_LOG"), "", "timer is created but the clock has not reached it");
    let fired = s.fire_due_timers(100);
    assert_eq!(fired, vec!["T_STARTUP"]);
    assert_eq!(global(&s, "TIMER_LOG"), "T_STARTUP;");
    assert!(s.fire_due_timers(100).is_empty(), "NO_REPEAT fires once");
}

#[test]
fn timers_do_not_fire_inside_a_running_trigger() {
    let mut s = objects_session();
    exec(&mut s, "BEGIN CREATE_TIMER('T0', 0, NO_REPEAT); END;");
    // fire_due_timers refuses while a trigger is running; outside it fires.
    assert_eq!(s.fire_due_timers(0), vec!["T0"]);
}

/// Handler that answers LOV requests with `choice` and records what it was shown.
fn lov_handler(shown: Arc<Mutex<Vec<(String, Vec<Vec<serde_json::Value>>, Option<String>)>>>, choice: Option<usize>) -> oracle_forms_runtime::InteractionHandler {
    Box::new(move |req: &InteractionRequest| match req {
        InteractionRequest::ShowLov { lov_name, rows, filter, .. } => {
            shown.lock().unwrap().push((lov_name.clone(), rows.clone(), filter.clone()));
            InteractionResponse::LovSelection(choice)
        }
        _ => InteractionResponse::Acknowledge,
    })
}

#[test]
fn show_lov_runs_query_applies_column_mappings_and_returns_true() {
    let mut s = objects_session();
    let shown = Arc::new(Mutex::new(Vec::new()));
    s.set_interaction_handler(lov_handler(shown.clone(), Some(1)));
    exec(&mut s, "BEGIN IF SHOW_LOV('LOV_ITEMS') THEN :GLOBAL.R := 'picked'; END IF; END;");
    assert_eq!(global(&s, "R"), "picked", "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(s.get_bind_value("CUSTOMERS.ITEM_NAME").to_string_lossy(), "Gadget B");
    assert_eq!(s.get_bind_value("CUSTOMERS.PRICE"), OracleValue::number_from_str("99.5"));
    let shown = shown.lock().unwrap();
    assert_eq!(shown[0].0, "LOV_ITEMS");
    assert_eq!(shown[0].1.len(), 5, "the record group query returned all five rows");
}

#[test]
fn show_lov_cancel_returns_false_and_leaves_items_alone() {
    let mut s = objects_session();
    s.set_interaction_handler(lov_handler(Arc::new(Mutex::new(Vec::new())), None));
    exec(&mut s, "BEGIN IF NOT SHOW_LOV('LOV_ITEMS') THEN :GLOBAL.R := 'cancelled'; END IF; END;");
    assert_eq!(global(&s, "R"), "cancelled");
    assert!(s.get_bind_value("CUSTOMERS.ITEM_NAME").is_null());
}

#[test]
fn show_lov_without_a_user_is_cancelled_and_missing_lov_fails() {
    let mut s = objects_session();
    exec(&mut s, "BEGIN IF NOT SHOW_LOV('LOV_CUST') THEN :GLOBAL.R := 'cancelled'; END IF; END;");
    assert_eq!(global(&s, "R"), "cancelled");
    assert!(s.trace_log.iter().any(|t| t.starts_with("LOV_AUTO_CANCELLED")));
    exec(&mut s, "BEGIN IF NOT SHOW_LOV('NO_SUCH_LOV') THEN :GLOBAL.M := 'x'; END IF; END;");
    assert!(s.last_builtin_failed || s.messages.iter().any(|m| m.contains("NO_SUCH_LOV")), "{:?}", s.messages);
}

#[test]
fn list_values_uses_the_current_items_lov_and_restricts_by_typed_text() {
    let mut s = objects_session();
    s.start().unwrap();
    let shown = Arc::new(Mutex::new(Vec::new()));
    s.set_interaction_handler(lov_handler(shown.clone(), Some(0)));
    s.go_item("CUSTOMERS.CUST_NAME").unwrap();
    s.set_item_value("CUSTOMERS.CUST_NAME", serde_json::json!("Ap")).unwrap();
    s.do_key("LIST_VALUES").unwrap();
    {
        let shown = shown.lock().unwrap();
        assert_eq!(shown[0].1.len(), 1, "RESTRICT: only values starting with the typed text: {:?}", shown[0]);
        assert_eq!(shown[0].2.as_deref(), Some("Ap"));
    }
    assert_eq!(s.get_bind_value("CUSTOMERS.CUST_NAME").to_string_lossy(), "Apex Ltd");
    assert_eq!(s.get_bind_value("CUSTOMERS.CITY").to_string_lossy(), "Vancouver");

    exec(&mut s, "BEGIN LIST_VALUES(NO_RESTRICT); END;");
    assert_eq!(shown.lock().unwrap()[1].1.len(), 3, "NO_RESTRICT lists everything");
}

#[test]
fn list_values_on_an_item_without_a_lov_fails_visibly() {
    let mut s = objects_session();
    s.start().unwrap();
    s.go_item("CUSTOMERS.CITY").unwrap();
    assert!(s.do_key("LIST_VALUES").is_err());
}

#[test]
fn key_listval_trigger_overrides_the_default() {
    let mut s = objects_session();
    s.module.triggers.push(trigger("KEY-LISTVAL", None, None, "BEGIN :GLOBAL.CUSTOM := 'ran'; END;"));
    s.start().unwrap();
    s.go_item("CUSTOMERS.CUST_NAME").unwrap();
    s.do_key("LIST_VALUES").unwrap();
    assert_eq!(global(&s, "CUSTOM"), "ran");
}

#[test]
fn validate_from_list_accepts_exact_completes_unique_prefix_and_rejects_unknown() {
    let mut s = objects_session();
    s.start().unwrap();
    s.go_item("CUSTOMERS.CUST_NAME").unwrap();

    s.set_item_value("CUSTOMERS.CUST_NAME", serde_json::json!("Birch Co")).unwrap();
    oracle_forms_runtime::lov::validate_from_list(&mut s, "CUSTOMERS", "CUST_NAME").unwrap();
    assert_eq!(s.get_bind_value("CUSTOMERS.CITY").to_string_lossy(), "Kelowna", "exact match fills the other return item");

    s.set_item_value("CUSTOMERS.CUST_NAME", serde_json::json!("Acm")).unwrap();
    oracle_forms_runtime::lov::validate_from_list(&mut s, "CUSTOMERS", "CUST_NAME").unwrap();
    assert_eq!(s.get_bind_value("CUSTOMERS.CUST_NAME").to_string_lossy(), "Acme Corp", "a unique prefix completes");

    s.set_item_value("CUSTOMERS.CUST_NAME", serde_json::json!("Zzz")).unwrap();
    let err = oracle_forms_runtime::lov::validate_from_list(&mut s, "CUSTOMERS", "CUST_NAME").unwrap_err();
    assert!(err.contains("FRM-40212") && err.contains("CUST_NAME"), "{err}");
}

#[test]
fn validate_from_list_with_several_matches_shows_the_lov() {
    let mut s = objects_session();
    s.start().unwrap();
    let shown = Arc::new(Mutex::new(Vec::new()));
    s.set_interaction_handler(lov_handler(shown.clone(), Some(1)));
    s.set_item_value("CUSTOMERS.CUST_NAME", serde_json::json!("A")).unwrap();
    oracle_forms_runtime::lov::validate_from_list(&mut s, "CUSTOMERS", "CUST_NAME").unwrap();
    assert_eq!(shown.lock().unwrap()[0].1.len(), 2, "Acme and Apex both start with A");
    assert_eq!(s.get_bind_value("CUSTOMERS.CUST_NAME").to_string_lossy(), "Apex Ltd");
}

#[test]
fn button_trigger_can_use_show_lov_result() {
    let mut s = objects_session();
    s.start().unwrap();
    s.set_interaction_handler(lov_handler(Arc::new(Mutex::new(Vec::new())), Some(3)));
    assert_eq!(s.press_button("CUSTOMERS.BTN_PICK"), TriggerResult::Success, "{:?}", s.messages);
    assert_eq!(global(&s, "PICKED"), "YES");
    assert_eq!(s.get_bind_value("CUSTOMERS.ITEM_NAME").to_string_lossy(), "Doohickey D");
}

#[test]
fn lov_overrides_change_the_group() {
    let mut s = objects_session();
    s.set_interaction_handler(lov_handler(Arc::new(Mutex::new(Vec::new())), Some(0)));
    exec(&mut s, "BEGIN SET_LOV_PROPERTY('LOV_CUST', GROUP_NAME, 'RG_STATIC'); :GLOBAL.G := GET_LOV_PROPERTY('LOV_CUST', GROUP_NAME); END;");
    assert_eq!(global(&s, "G"), "RG_STATIC");
}

// ---------------------------------------------------------------------------
// Navigation built-ins (the stack itself is exercised through the server)
// ---------------------------------------------------------------------------

#[test]
fn form_names_lose_paths_and_extensions() {
    use oracle_forms_runtime::nav::form_module_name;
    assert_eq!(form_module_name("orders.fmx"), "orders");
    assert_eq!(form_module_name("/apps/forms/ORDERS.FMB"), "ORDERS");
    assert_eq!(form_module_name("C:\\apps\\orders"), "orders");
    assert_eq!(form_module_name("a.b"), "a.b", "unknown extensions stay");
}

#[test]
fn open_new_go_close_queue_requests_with_parsed_modes_and_parameters() {
    use oracle_forms_runtime::nav::NavRequest;
    let mut s = objects_session();
    exec(
        &mut s,
        "DECLARE pl ParamList; BEGIN pl := CREATE_PARAMETER_LIST('NAVPL'); ADD_PARAMETER(pl, 'k', TEXT_PARAMETER, 'v'); \
           OPEN_FORM('dir/other.fmx', NO_ACTIVATE, SESSION, pl); NEW_FORM('third', FULL_ROLLBACK, QUERY_ONLY); \
           GO_FORM('other'); CLOSE_FORM('other'); END;",
    );
    let q = std::mem::take(&mut s.nav_queue);
    assert_eq!(q.len(), 4, "{q:?} {:?}", s.messages);
    match &q[0] {
        NavRequest::Open(o) => {
            assert_eq!(o.form, "other");
            assert!(!o.activate && o.new_session);
            assert_eq!(o.params, vec![("K".to_string(), "v".to_string())]);
        }
        other => panic!("{other:?}"),
    }
    match &q[1] {
        NavRequest::New(n) => assert!(n.query_only && n.rollback == oracle_forms_runtime::Rollback::FullRollback),
        other => panic!("{other:?}"),
    }
    assert_eq!(q[2], NavRequest::GoForm("other".into()));
    assert_eq!(q[3], NavRequest::CloseForm("other".into()));
}

#[test]
fn call_form_without_a_host_is_reported_and_data_parameters_are_refused() {
    let mut s = objects_session();
    let unit = oracle_forms_plsql::Parser::new("BEGIN CALL_FORM('x'); :GLOBAL.AFTER := 'ran'; END;").parse_unit().unwrap();
    let err = Interpreter::for_trigger(&s, "T").execute_unit(&unit, &mut s).unwrap_err();
    assert!(err.to_string().contains("CALL_FORM"), "no host: CALL_FORM is reported, not ignored: {err}");

    let mut s = objects_session();
    exec(&mut s, "DECLARE pl ParamList; BEGIN pl := CREATE_PARAMETER_LIST('P'); ADD_PARAMETER(pl, 'd', DATA_PARAMETER, 'RG_STATIC'); OPEN_FORM('x', ACTIVATE, NO_SESSION, pl); IF FORM_FAILURE THEN :GLOBAL.R := 'refused'; END IF; END;");
    assert_eq!(global(&s, "R"), "refused");
    assert!(s.nav_queue.is_empty());
}

#[test]
fn form_introspection_reports_the_stack() {
    let mut s = objects_session();
    s.calling_form = Some("CALLER".into());
    s.open_forms = vec!["CONF20".into(), "OTHER".into()];
    exec(
        &mut s,
        "BEGIN :GLOBAL.CALLING := GET_APPLICATION_PROPERTY(CALLING_FORM); :GLOBAL.CUR := :SYSTEM.CURRENT_FORM; \
           :GLOBAL.FN := GET_FORM_PROPERTY(CURRENT_FORM, FORM_NAME); :GLOBAL.FB := GET_FORM_PROPERTY(CURRENT_FORM, FIRST_BLOCK); \
           IF NOT ID_NULL(FIND_FORM('other')) THEN :GLOBAL.F1 := 'found'; END IF; IF ID_NULL(FIND_FORM('nope')) THEN :GLOBAL.F2 := 'null'; END IF; END;",
    );
    assert_eq!(global(&s, "CALLING"), "CALLER");
    assert_eq!(global(&s, "CUR"), "CONF20");
    assert_eq!(global(&s, "FN"), "CONF20");
    assert_eq!(global(&s, "FB"), "CUSTOMERS");
    assert_eq!(global(&s, "F1"), "found");
    assert_eq!(global(&s, "F2"), "null");
}

#[test]
fn exit_form_modes_control_the_commit_and_the_prompt() {
    let mut s = objects_session();
    // A control-only change never prompts; NO_COMMIT / NO_VALIDATE exit immediately.
    exec(&mut s, "BEGIN EXIT_FORM(NO_COMMIT); END;");
    assert!(s.exit_requested);
    let mut s = objects_session();
    exec(&mut s, "BEGIN EXIT_FORM(NO_VALIDATE); END;");
    assert!(s.exit_requested);
    let mut s = objects_session();
    exec(&mut s, "BEGIN EXIT_FORM; END;");
    assert!(s.exit_requested, "nothing to save: ASK_COMMIT exits without asking");
}
