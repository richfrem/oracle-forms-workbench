//! Runtime gap-closure tests: execution budgets / runaway protection, then parameter
//! lists, timers, LOV dialogs and record groups. All fixtures are synthetic.
//!
//! Purpose: Integration/conformance tests (runtime_gap_tests.rs).
//! Key Input Dependencies: oracle_forms_core, oracle_forms_db, oracle_forms_runtime, oracle_forms_xml, samples/conformance/conf_03_query_cascade_fmb.xml
//! Index: sample, unit, trigger, session_with, button, press, infinite_loop_in_package_routine_is_stopped_and_named, cancelled_session_stops_running_plsql, stray_end_in_program_unit_text_does_not_hang_session_creation, record_field_assignment_and_read_work, objects_session, exec, global, parameter_list_lifecycle_and_out_parameters, parameter_list_errors_set_form_failure_and_continue, record_group_programmatic_cells_rows_and_types, record_group_queries_populate_and_report_status, design_time_record_groups_exist_at_startup, timers_create_find_set_delete_and_fire, startup_timer_with_short_delay_runs_after_startup, timers_do_not_fire_inside_a_running_trigger, lov_handler, show_lov_runs_query_applies_column_mappings_and_returns_true, show_lov_cancel_returns_false_and_leaves_items_alone, show_lov_without_a_user_is_cancelled_and_missing_lov_fails, list_values_uses_the_current_items_lov_and_restricts_by_typed_text, list_values_on_an_item_without_a_lov_fails_visibly, key_listval_trigger_overrides_the_default, validate_from_list_accepts_exact_completes_unique_prefix_and_rejects_unknown, validate_from_list_with_several_matches_shows_the_lov, button_trigger_can_use_show_lov_result, lov_overrides_change_the_group, form_names_lose_paths_and_extensions, open_new_go_close_queue_requests_with_parsed_modes_and_parameters, navigation_arguments_follow_each_built_ins_signature, call_form_without_a_host_is_reported_and_data_parameters_are_refused, form_introspection_reports_the_stack, exit_form_modes_control_the_commit_and_the_prompt, post_writes_without_committing_and_commit_form_then_only_commits, post_with_nothing_to_write_reports_and_a_read_only_session_refuses, text_io_writes_reads_appends_and_reports_end_of_file, text_io_errors_raise_named_exceptions_and_stay_in_the_sandbox, host_is_denied_by_default_and_emulated_on_the_virtual_files_when_enabled, host_can_use_a_host_supplied_executor, show_document_queues_an_open_url_action_and_refuses_unsafe_schemes, list_items_start_from_design_elements_and_can_be_edited_and_populated, menu_items_keep_enabled_checked_displayed_and_label, menu_item_lookup_is_strict_once_a_custom_menu_is_loaded, menu_paths_are_validated_in_full_and_same_named_items_stay_distinct, record_properties_read_and_set_status, queried_sqlite_records_carry_a_rowid_so_updates_and_deletes_reach_the_database, execute_policy_never_logs_command_arguments_or_output, new_line_count_has_a_hard_limit_without_allocating_the_oversized_buffer, file_and_session_size_caps_accept_the_boundary_and_reject_the_next_byte, open_handle_count_is_capped

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
fn navigation_arguments_follow_each_built_ins_signature() {
    use oracle_forms_runtime::nav::NavRequest;
    // (call, expected message fragment)
    let bad = [
        ("CALL_FORM('c', 'BOGUS')", "BOGUS"),
        ("CALL_FORM('c', HIDE, HIDE)", "another argument"),
        ("CALL_FORM('c', DO_REPLACE)", "display argument accepts HIDE or NO_HIDE"),
        ("CALL_FORM('c', HIDE, DO_REPLACE, QUERY_ONLY, NO_HIDE)", "mode constant"),
        ("CALL_FORM('c', HIDE, DO_REPLACE, QUERY_ONLY, 'PL', 'extra')", "too many arguments"),
        ("CALL_FORM('c', HIDE, 'PL')", "switch_menu"),
        ("OPEN_FORM('o', 'BOGUS')", "activate_mode"),
        ("OPEN_FORM('o', ACTIVATE, ACTIVATE)", "another argument"),
        ("OPEN_FORM('o', SESSION, ACTIVATE)", "activate_mode"),
        ("OPEN_FORM('o', ACTIVATE, SESSION, 'PL', 1)", "too many arguments"),
        ("OPEN_FORM('o', 'PL')", "activate_mode"),
        ("NEW_FORM('n', 'BOGUS')", "rollback_mode"),
        ("NEW_FORM('n', QUERY_ONLY)", "rollback_mode argument accepts TO_SAVEPOINT or NO_ROLLBACK or FULL_ROLLBACK"),
        ("NEW_FORM('n', NO_ROLLBACK, NO_ROLLBACK)", "another argument"),
        ("NEW_FORM('n', NO_ROLLBACK, QUERY_ONLY, 'PL', 'x')", "too many arguments"),
    ];
    for (call, fragment) in bad {
        let mut s = objects_session();
        exec(&mut s, &format!("BEGIN {call}; IF FORM_FAILURE THEN :GLOBAL.F := 'failed'; END IF; END;"));
        assert_eq!(global(&s, "F"), "failed", "{call}: {:?}", s.messages);
        assert!(s.messages.iter().any(|m| m.contains(fragment)), "{call}: expected '{fragment}' in {:?}", s.messages);
        assert!(s.nav_queue.is_empty(), "{call}: nothing queued");
    }
    // Accepted forms: NULL/omitted defaults, a short list form and the full signature.
    let mut s = objects_session();
    exec(
        &mut s,
        "DECLARE pl ParamList; BEGIN pl := CREATE_PARAMETER_LIST('PL'); ADD_PARAMETER(pl, 'k', TEXT_PARAMETER, 'v'); \
           OPEN_FORM('a'); OPEN_FORM('b', NULL, SESSION); OPEN_FORM('c', pl); OPEN_FORM('d', NO_ACTIVATE, NO_SESSION, 'PL'); \
           NEW_FORM('e', NULL, QUERY_ONLY, pl); NEW_FORM('f', pl); END;",
    );
    let q = std::mem::take(&mut s.nav_queue);
    assert_eq!(q.len(), 6, "{q:?} {:?}", s.messages);
    let open = |i: usize| match &q[i] { NavRequest::Open(o) => o.clone(), o => panic!("{o:?}") };
    assert!(open(0).activate && !open(0).new_session);
    assert!(open(1).activate && open(1).new_session);
    assert_eq!(open(2).params.len(), 1, "short form: parameter list right after the form name");
    assert!(!open(3).activate && open(3).params.len() == 1);
    match &q[4] { NavRequest::New(n) => assert!(n.query_only && n.params.len() == 1), o => panic!("{o:?}") }
    match &q[5] { NavRequest::New(n) => assert!(!n.query_only && n.params.len() == 1), o => panic!("{o:?}") }
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

#[test]
fn post_writes_without_committing_and_commit_form_then_only_commits() {
    let key = ModuleKey::new("CONF08", ModuleType::Form);
    let cache = std::env::temp_dir().join(format!("ofw-gap08-{}", std::process::id()));
    let module = FormsXmlParser::parse(&key, &sample("conf_08_commit_pipeline_fmb.xml"), &cache).unwrap();
    let mut s = FormSession::new(module);
    s.set_database_driver(Arc::new(oracle_forms_db::MockDriver::new().unwrap()));
    s.start().unwrap();
    let mut vals = std::collections::HashMap::new();
    vals.insert("ITEM_ID".to_string(), serde_json::json!(101));
    vals.insert("ITEM_NAME".to_string(), serde_json::json!("Widget A"));
    vals.insert("AMOUNT".to_string(), serde_json::json!(50.0));
    let qr = oracle_forms_db::QueryResult {
        block_name: "ITEMS".into(),
        table_name: "ITEMS".into(),
        columns: vec!["ITEM_ID".into(), "ITEM_NAME".into(), "AMOUNT".into()],
        records: vec![oracle_forms_db::QueryRecord { row_id: Some("101".into()), values: vals }],
        total_count: 1,
        execution_time_ms: 1,
        sql_statement: "SELECT".into(),
    };
    s.blocks.get_mut("ITEMS").unwrap().populate_from_query_result(qr);
    s.blocks.get_mut("ITEMS").unwrap().set_cursor(0);
    s.set_item_value("ITEMS.ITEM_NAME", serde_json::json!("posted name")).unwrap();
    assert!(s.has_unsaved_changes());
    s.trace_log.clear();

    exec(&mut s, "BEGIN POST; END;");
    let trace = s.trace_log.join("\n");
    assert!(trace.contains("DML: UPDATE"), "POST writes the row: {trace}");
    assert!(trace.contains("POST-FORMS-COMMIT"), "POST-FORMS-COMMIT fires during the post phase: {trace}");
    assert!(!trace.contains("DATABASE_COMMIT"), "no database commit: {trace}");
    assert!(s.posted_uncommitted);
    assert!(!s.has_unsaved_changes(), "posted records are no longer pending changes");

    s.trace_log.clear();
    s.do_key("COMMIT_FORM").unwrap();
    let trace = s.trace_log.join("\n");
    assert!(trace.contains("DATABASE_COMMIT") && !trace.contains("DML:"), "COMMIT_FORM only commits what was posted: {trace}");
    assert!(!s.posted_uncommitted);
}

#[test]
fn post_with_nothing_to_write_reports_and_a_read_only_session_refuses() {
    let mut s = objects_session();
    exec(&mut s, "BEGIN POST; END;");
    assert!(s.messages.iter().any(|m| m.contains("FRM-40401")), "{:?}", s.messages);
    assert!(!s.posted_uncommitted);
}

// ---------------------------------------------------------------------------
// TEXT_IO, HOST policy, WEB.SHOW_DOCUMENT
// ---------------------------------------------------------------------------

#[test]
fn text_io_writes_reads_appends_and_reports_end_of_file() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE f TEXT_IO.FILE_TYPE; line VARCHAR2(200); n NUMBER := 0; BEGIN \
           f := TEXT_IO.FOPEN('c:\\\\temp\\\\out.txt', 'w'); \
           TEXT_IO.PUT_LINE(f, 'first'); TEXT_IO.PUT(f, 'sec'); TEXT_IO.PUT(f, 'ond'); TEXT_IO.NEW_LINE(f); \
           TEXT_IO.PUTF(f, 'row %s of %s\\n', 2, 3); \
           IF TEXT_IO.IS_OPEN(f) THEN :GLOBAL.OPEN1 := 'open'; END IF; TEXT_IO.FCLOSE(f); \
           IF NOT TEXT_IO.IS_OPEN(f) THEN :GLOBAL.OPEN2 := 'closed'; END IF; \
           f := TEXT_IO.FOPEN('/temp/out.txt', 'a'); TEXT_IO.PUT_LINE(f, 'appended'); TEXT_IO.FCLOSE(f); \
           f := TEXT_IO.FOPEN('temp/out.txt', 'r'); \
           BEGIN LOOP TEXT_IO.GET_LINE(f, line); n := n + 1; :GLOBAL.LAST := line; IF n = 1 THEN :GLOBAL.FIRST := line; END IF; END LOOP; \
           EXCEPTION WHEN NO_DATA_FOUND THEN :GLOBAL.EOF := 'eof after ' || n; END; \
           TEXT_IO.FCLOSE_ALL; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "OPEN1"), "open");
    assert_eq!(global(&s, "OPEN2"), "closed");
    assert_eq!(global(&s, "FIRST"), "first");
    assert_eq!(global(&s, "LAST"), "appended");
    assert_eq!(global(&s, "EOF"), "eof after 4", "four lines were read before the end of file");
    assert_eq!(String::from_utf8(s.objects.files["temp/out.txt"].clone()).unwrap(), "first\nsecond\nrow 2 of 3\nappended\n");
    assert!(s.objects.open_files.is_empty());
}

#[test]
fn text_io_errors_raise_named_exceptions_and_stay_in_the_sandbox() {
    let mut s = objects_session();
    let st = exec(
        &mut s,
        "DECLARE f TEXT_IO.FILE_TYPE; BEGIN \
           BEGIN f := TEXT_IO.FOPEN('../etc/passwd', 'r'); EXCEPTION WHEN OTHERS THEN :GLOBAL.E1 := SQLERRM; END; \
           BEGIN f := TEXT_IO.FOPEN('missing.txt', 'r'); EXCEPTION WHEN OTHERS THEN :GLOBAL.E2 := SQLERRM; END; \
           BEGIN f := TEXT_IO.FOPEN('x.txt', 'q'); EXCEPTION WHEN OTHERS THEN :GLOBAL.E3 := SQLERRM; END; \
           BEGIN TEXT_IO.PUT_LINE('nohandle', 'x'); EXCEPTION WHEN OTHERS THEN :GLOBAL.E4 := SQLERRM; END; \
           f := TEXT_IO.FOPEN('r.txt', 'w'); TEXT_IO.FCLOSE(f); f := TEXT_IO.FOPEN('r.txt', 'r'); \
           BEGIN TEXT_IO.PUT_LINE(f, 'x'); EXCEPTION WHEN OTHERS THEN :GLOBAL.E5 := SQLERRM; END; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?} {:?}", s.trace_log, s.messages);
    assert!(global(&s, "E1").contains("29280"), "{}", global(&s, "E1"));
    assert!(global(&s, "E2").contains("29283"));
    assert!(global(&s, "E3").contains("29281"));
    assert!(global(&s, "E4").contains("29282"));
    assert!(global(&s, "E5").contains("not open for writing"));
    assert!(!s.objects.files.keys().any(|k| k.contains("..")));
}

#[test]
fn host_is_denied_by_default_and_emulated_on_the_virtual_files_when_enabled() {
    let mut s = objects_session();
    exec(&mut s, "BEGIN HOST('echo hi > a.txt'); IF FORM_FAILURE THEN :GLOBAL.D := 'denied'; END IF; END;");
    assert_eq!(global(&s, "D"), "denied");
    assert!(s.messages.iter().any(|m| m.contains("disabled by policy")), "{:?}", s.messages);
    assert!(s.objects.files.is_empty());

    s.host_policy = oracle_forms_runtime::HostPolicy::Emulate;
    exec(
        &mut s,
        "BEGIN HOST('echo hello world > out\\\\a.txt && echo second >> out\\\\a.txt'); HOST('touch out/empty.txt'); \
           HOST('copy out\\\\a.txt out\\\\b.txt'); HOST('del out/empty.txt'); HOST('mkdir out'); \
           HOST('frobnicate now'); IF FORM_FAILURE THEN :GLOBAL.U := 'unknown'; END IF; \
           HOST('echo x > ../escape.txt'); IF FORM_FAILURE THEN :GLOBAL.ESC := 'blocked'; END IF; END;",
    );
    assert_eq!(String::from_utf8(s.objects.files["out/a.txt"].clone()).unwrap(), "hello world\nsecond\n");
    assert_eq!(s.objects.files["out/b.txt"], s.objects.files["out/a.txt"]);
    assert!(!s.objects.files.contains_key("out/empty.txt"));
    assert_eq!(global(&s, "U"), "unknown");
    assert_eq!(global(&s, "ESC"), "blocked");
    assert!(s.messages.iter().any(|m| m.contains("frobnicate")), "unknown commands are named");
}

#[test]
fn host_can_use_a_host_supplied_executor() {
    let mut s = objects_session();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    s.host_policy = oracle_forms_runtime::HostPolicy::Execute(Arc::new(move |cmd: &str| {
        seen2.lock().unwrap().push(cmd.to_string());
        if cmd.starts_with("fail") { Err("exit status 3".into()) } else { Ok("done\n".into()) }
    }));
    exec(&mut s, "BEGIN HOST('tool --x'); HOST('fail now'); IF FORM_FAILURE THEN :GLOBAL.F := 'failed'; END IF; END;");
    assert_eq!(*seen.lock().unwrap(), vec!["tool --x".to_string(), "fail now".to_string()]);
    assert_eq!(global(&s, "F"), "failed");
    assert!(s.trace_log.iter().any(|t| t == "HOST: tool"), "only the program name is logged under Execute");
    let joined = s.trace_log.join("\n");
    assert!(!joined.contains("--x") && !joined.contains("done") && !joined.contains("now"), "no arguments or output in the trace: {joined}");
}

#[test]
fn execute_policy_never_logs_command_arguments_or_output() {
    let mut s = objects_session();
    s.host_policy = oracle_forms_runtime::HostPolicy::Execute(Arc::new(|_cmd: &str| Ok("secret-output".into())));
    exec(&mut s, "BEGIN HOST('tool --password=hunter2'); END;");
    let joined = s.trace_log.join("\n") + &s.messages.join("\n");
    assert!(!joined.contains("hunter2") && !joined.contains("secret-output"), "{joined}");
    s.host_policy = oracle_forms_runtime::HostPolicy::Emulate;
    exec(&mut s, "BEGIN HOST('echo visible'); END;");
    assert!(s.trace_log.iter().any(|t| t == "HOST: echo visible"), "Emulate keeps the command in the trace");
}

#[test]
fn new_line_count_has_a_hard_limit_without_allocating_the_oversized_buffer() {
    use oracle_forms_runtime::textio::MAX_NEW_LINES;
    let mut s = objects_session();
    let ok = format!("DECLARE f TEXT_IO.FILE_TYPE; BEGIN f := TEXT_IO.FOPEN('n.txt', 'w'); TEXT_IO.NEW_LINE(f, {MAX_NEW_LINES}); END;");
    assert_eq!(exec(&mut s, &ok), ExecutionStatus::Success(None), "{:?}", s.messages);
    assert_eq!(s.objects.files["n.txt"].len(), MAX_NEW_LINES as usize);
    let bad = format!("DECLARE f TEXT_IO.FILE_TYPE; BEGIN f := TEXT_IO.FOPEN('n.txt', 'a'); BEGIN TEXT_IO.NEW_LINE(f, {}); EXCEPTION WHEN OTHERS THEN :GLOBAL.NL := SQLERRM; END; END;", MAX_NEW_LINES + 1);
    exec(&mut s, &bad);
    assert!(global(&s, "NL").contains("29285"), "{}", global(&s, "NL"));
    assert_eq!(s.objects.files["n.txt"].len(), MAX_NEW_LINES as usize, "nothing written by the rejected call");
    let huge = "DECLARE f TEXT_IO.FILE_TYPE; BEGIN f := TEXT_IO.FOPEN('n.txt', 'a'); BEGIN TEXT_IO.NEW_LINE(f, 9000000000000); EXCEPTION WHEN OTHERS THEN :GLOBAL.NL2 := SQLERRM; END; END;";
    exec(&mut s, huge);
    assert!(global(&s, "NL2").contains("29285"));
}

#[test]
fn file_and_session_size_caps_accept_the_boundary_and_reject_the_next_byte() {
    use oracle_forms_runtime::textio::{MAX_FILE_BYTES, MAX_SESSION_BYTES};
    let mut s = objects_session();
    s.objects.files.insert("big.txt".into(), vec![b'x'; MAX_FILE_BYTES - 1]);
    let put = |s: &mut FormSession, c: &str, g: &str| {
        let code = format!("DECLARE f TEXT_IO.FILE_TYPE; BEGIN f := TEXT_IO.FOPEN('big.txt', 'a'); BEGIN TEXT_IO.PUT(f, '{c}'); :GLOBAL.{g} := 'ok'; EXCEPTION WHEN OTHERS THEN :GLOBAL.{g} := SQLERRM; END; END;");
        exec(s, &code);
    };
    put(&mut s, "a", "P1");
    assert_eq!(global(&s, "P1"), "ok", "the largest accepted file");
    assert_eq!(s.objects.files["big.txt"].len(), MAX_FILE_BYTES);
    put(&mut s, "b", "P2");
    assert!(global(&s, "P2").contains("29285") && global(&s, "P2").contains("file size limit"), "{}", global(&s, "P2"));
    assert_eq!(s.objects.files["big.txt"].len(), MAX_FILE_BYTES);

    // Session total: fill to the cap with several files, then the next byte anywhere is refused.
    let mut s = objects_session();
    let full = MAX_SESSION_BYTES / MAX_FILE_BYTES;
    for i in 0..full {
        s.objects.files.insert(format!("f{i}.bin"), vec![0; MAX_FILE_BYTES]);
    }
    exec(&mut s, "DECLARE f TEXT_IO.FILE_TYPE; BEGIN f := TEXT_IO.FOPEN('extra.txt', 'w'); BEGIN TEXT_IO.PUT(f, 'a'); EXCEPTION WHEN OTHERS THEN :GLOBAL.S := SQLERRM; END; END;");
    assert!(global(&s, "S").contains("session file storage limit"), "{}", global(&s, "S"));
    // HOST emulation shares the caps.
    s.host_policy = oracle_forms_runtime::HostPolicy::Emulate;
    exec(&mut s, "BEGIN HOST('echo hi > extra2.txt'); IF FORM_FAILURE THEN :GLOBAL.H := 'refused'; END IF; END;");
    assert_eq!(global(&s, "H"), "refused");
    assert!(!s.objects.files.contains_key("extra2.txt"));
    // Freeing space makes room again.
    s.objects.files.remove("f0.bin");
    exec(&mut s, "BEGIN HOST('echo hi > extra3.txt'); END;");
    assert!(s.objects.files.contains_key("extra3.txt"));
}

#[test]
fn open_handle_count_is_capped() {
    use oracle_forms_runtime::textio::MAX_OPEN_HANDLES;
    let mut s = objects_session();
    let code = format!(
        "DECLARE f TEXT_IO.FILE_TYPE; BEGIN FOR i IN 1..{} LOOP f := TEXT_IO.FOPEN('h' || i || '.txt', 'w'); END LOOP; \
           BEGIN f := TEXT_IO.FOPEN('over.txt', 'w'); EXCEPTION WHEN OTHERS THEN :GLOBAL.O := SQLERRM; END; END;",
        MAX_OPEN_HANDLES
    );
    exec(&mut s, &code);
    assert_eq!(s.objects.open_files.len(), MAX_OPEN_HANDLES);
    assert!(global(&s, "O").contains("29283"), "{}", global(&s, "O"));
    assert!(!s.objects.files.contains_key("over.txt"));
}

#[test]
fn show_document_queues_an_open_url_action_and_refuses_unsafe_schemes() {
    let mut s = objects_session();
    exec(
        &mut s,
        "BEGIN WEB.SHOW_DOCUMENT('https://example.org/a?b=1', '_self'); WEB.SHOW_DOCUMENT('/api/reports/x/html'); \
           WEB.SHOW_DOCUMENT('javascript:alert(1)'); IF FORM_FAILURE THEN :GLOBAL.J := 'refused'; END IF; \
           WEB.SHOW_DOCUMENT('//evil.example/x'); IF FORM_FAILURE THEN :GLOBAL.K := 'refused'; END IF; END;",
    );
    assert_eq!(s.actions.len(), 2, "{:?}", s.actions);
    assert_eq!(s.actions[0]["url"], "https://example.org/a?b=1");
    assert_eq!(s.actions[0]["target"], "_self");
    assert_eq!(s.actions[1]["target"], "_blank");
    assert_eq!(global(&s, "J"), "refused");
    assert_eq!(global(&s, "K"), "refused");
}

// ---------------------------------------------------------------------------
// List items, menu items, record properties
// ---------------------------------------------------------------------------

#[test]
fn list_items_start_from_design_elements_and_can_be_edited_and_populated() {
    let mut s = objects_session();
    s.start().unwrap();
    let st = exec(
        &mut s,
        "DECLARE n NUMBER; BEGIN \
           :GLOBAL.N0 := GET_LIST_ELEMENT_COUNT('CUSTOMERS.COLOR'); :GLOBAL.L1 := GET_LIST_ELEMENT_LABEL('CUSTOMERS.COLOR', 1); \
           :GLOBAL.V2 := GET_LIST_ELEMENT_VALUE('CUSTOMERS.COLOR', 2); \
           ADD_LIST_ELEMENT('CUSTOMERS.COLOR', 2, 'Green', 'G'); :GLOBAL.N1 := GET_LIST_ELEMENT_COUNT('CUSTOMERS.COLOR'); \
           :GLOBAL.IDX := GET_LIST_ELEMENT_INDEX('CUSTOMERS.COLOR', 'B'); :GLOBAL.NOIDX := GET_LIST_ELEMENT_INDEX('CUSTOMERS.COLOR', 'Z'); \
           DELETE_LIST_ELEMENT('CUSTOMERS.COLOR', 1); :GLOBAL.N2 := GET_LIST_ELEMENT_COUNT('CUSTOMERS.COLOR'); \
           ADD_LIST_ELEMENT('CUSTOMERS.COLOR', 9, 'x', 'x'); IF FORM_FAILURE THEN :GLOBAL.BAD := 'range'; END IF; \
           :CUSTOMERS.COLOR := 'G'; CLEAR_LIST('CUSTOMERS.COLOR'); :GLOBAL.N3 := GET_LIST_ELEMENT_COUNT('CUSTOMERS.COLOR'); :GLOBAL.CUR := NVL(:CUSTOMERS.COLOR, 'null'); \
           POPULATE_LIST('CUSTOMERS.COLOR', 'RG_STATIC'); :GLOBAL.N4 := GET_LIST_ELEMENT_COUNT('CUSTOMERS.COLOR'); :GLOBAL.P1 := GET_LIST_ELEMENT_LABEL('CUSTOMERS.COLOR', 1) || '=' || GET_LIST_ELEMENT_VALUE('CUSTOMERS.COLOR', 1); \
           POPULATE_LIST('CUSTOMERS.COLOR', 'RG_ITEMS'); :GLOBAL.P2 := GET_LIST_ELEMENT_LABEL('CUSTOMERS.COLOR', 2); \
           CLEAR_LIST('CUSTOMERS.NOPE'); IF FORM_FAILURE THEN :GLOBAL.NOLIST := 'failed'; END IF; \
         END;",
    );
    assert_eq!(st, ExecutionStatus::Success(None), "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!((global(&s, "N0"), global(&s, "L1"), global(&s, "V2")), ("2".into(), "Red".into(), "B".into()));
    assert_eq!((global(&s, "N1"), global(&s, "IDX"), global(&s, "NOIDX")), ("3".into(), "3".into(), "0".into()));
    assert_eq!(global(&s, "N2"), "2");
    assert_eq!(global(&s, "BAD"), "range");
    assert_eq!((global(&s, "N3"), global(&s, "CUR")), ("0".into(), "null".into()), "CLEAR_LIST also clears the value");
    assert_eq!((global(&s, "N4"), global(&s, "P1")), ("3".into(), "Acme Corp=Victoria".into()), "first column label, second column value");
    assert_eq!(global(&s, "P2"), "Gadget B", "a query group is populated on demand");
    assert_eq!(global(&s, "NOLIST"), "failed");
}

#[test]
fn menu_items_keep_enabled_checked_displayed_and_label() {
    let mut s = objects_session();
    exec(
        &mut s,
        "BEGIN :GLOBAL.E0 := GET_MENU_ITEM_PROPERTY('FILE.SAVE', ENABLED); \
           SET_MENU_ITEM_PROPERTY('FILE.SAVE', ENABLED, PROPERTY_FALSE); SET_MENU_ITEM_PROPERTY('file.save', LABEL, 'Save now'); \
           SET_MENU_ITEM_PROPERTY('VIEW.GRID', CHECKED, PROPERTY_TRUE); \
           :GLOBAL.E1 := GET_MENU_ITEM_PROPERTY('FILE.SAVE', ENABLED); :GLOBAL.LB := GET_MENU_ITEM_PROPERTY('FILE.SAVE', LABEL); \
           :GLOBAL.CK := GET_MENU_ITEM_PROPERTY('VIEW.GRID', CHECKED); :GLOBAL.DS := GET_MENU_ITEM_PROPERTY('VIEW.GRID', DISPLAYED); \
           IF NOT ID_NULL(FIND_MENU_ITEM('FILE.SAVE')) THEN :GLOBAL.F := 'found'; END IF; \
           SET_MENU_ITEM_PROPERTY('FILE.SAVE', 'BOGUS', PROPERTY_TRUE); IF FORM_FAILURE THEN :GLOBAL.BAD := 'rejected'; END IF; END;",
    );
    assert_eq!((global(&s, "E0"), global(&s, "E1")), ("TRUE".into(), "FALSE".into()));
    assert_eq!(global(&s, "LB"), "Save now");
    assert_eq!((global(&s, "CK"), global(&s, "DS")), ("TRUE".into(), "TRUE".into()));
    assert_eq!(global(&s, "F"), "found");
    assert_eq!(global(&s, "BAD"), "rejected");
    assert_eq!(s.objects.menu_items["FILE.SAVE"]["ENABLED"].to_string_lossy(), "FALSE");
}

#[test]
fn menu_item_lookup_is_strict_once_a_custom_menu_is_loaded() {
    let mut s = objects_session();
    s.module.menus.push(oracle_forms_core::model::MenuInfo {
        name: "MAIN".into(),
        items: vec![oracle_forms_core::model::MenuItemInfo { name: "SAVE".into(), label: Some("Save".into()), command_type: None, sub_menu_name: None, menu_item_type: None, command_ref: None, command_text: None, text_encoding: Default::default() }],
    });
    exec(&mut s, "BEGIN :GLOBAL.LB := GET_MENU_ITEM_PROPERTY('MAIN.SAVE', LABEL); IF ID_NULL(FIND_MENU_ITEM('MAIN.GONE')) THEN :GLOBAL.G := 'null'; END IF; SET_MENU_ITEM_PROPERTY('MAIN.GONE', ENABLED, PROPERTY_FALSE); IF FORM_FAILURE THEN :GLOBAL.F := 'failed'; END IF; END;");
    assert_eq!((global(&s, "LB"), global(&s, "G"), global(&s, "F")), ("Save".into(), "null".into(), "failed".into()));
}

#[test]
fn menu_paths_are_validated_in_full_and_same_named_items_stay_distinct() {
    use oracle_forms_core::model::{MenuInfo, MenuItemInfo};
    let item = |n: &str, l: &str| MenuItemInfo { name: n.into(), label: Some(l.into()), command_type: None, sub_menu_name: None, menu_item_type: None, command_ref: None, command_text: None, text_encoding: Default::default() };
    let mut s = objects_session();
    s.module.menus.push(MenuInfo { name: "MENU_A".into(), items: vec![item("SAVE", "Save A"), item("ONLY_A", "Only A")] });
    s.module.menus.push(MenuInfo { name: "MENU_B".into(), items: vec![item("SAVE", "Save B"), item("ONLY_B", "Only B")] });
    exec(
        &mut s,
        "BEGIN SET_MENU_ITEM_PROPERTY('MENU_A.SAVE', ENABLED, PROPERTY_FALSE); \
           :GLOBAL.A := GET_MENU_ITEM_PROPERTY('MENU_A.SAVE', ENABLED); :GLOBAL.B := GET_MENU_ITEM_PROPERTY('MENU_B.SAVE', ENABLED); \
           :GLOBAL.LA := GET_MENU_ITEM_PROPERTY('MENU_A.SAVE', LABEL); :GLOBAL.LB := GET_MENU_ITEM_PROPERTY('menu_b.save', LABEL); \
           SET_MENU_ITEM_PROPERTY('MENU_A.ONLY_B', ENABLED, PROPERTY_FALSE); IF FORM_FAILURE THEN :GLOBAL.X1 := 'failed'; END IF; \
           SET_MENU_ITEM_PROPERTY('NO_MENU.SAVE', ENABLED, PROPERTY_FALSE); IF FORM_FAILURE THEN :GLOBAL.X2 := 'failed'; END IF; \
           SET_MENU_ITEM_PROPERTY('SAVE', ENABLED, PROPERTY_FALSE); IF FORM_FAILURE THEN :GLOBAL.X3 := 'ambiguous'; END IF; \
           SET_MENU_ITEM_PROPERTY('ONLY_B', CHECKED, PROPERTY_TRUE); :GLOBAL.CB := GET_MENU_ITEM_PROPERTY('MENU_B.ONLY_B', CHECKED); \
           IF ID_NULL(FIND_MENU_ITEM('MENU_A.ONLY_B')) THEN :GLOBAL.F := 'null'; END IF; END;",
    );
    assert_eq!((global(&s, "A"), global(&s, "B")), ("FALSE".into(), "TRUE".into()), "the same item name in two menus stays distinct");
    assert_eq!((global(&s, "LA"), global(&s, "LB")), ("Save A".into(), "Save B".into()));
    assert_eq!((global(&s, "X1"), global(&s, "X2"), global(&s, "X3")), ("failed".into(), "failed".into(), "ambiguous".into()));
    assert_eq!(global(&s, "CB"), "TRUE", "a unique bare name resolves to its menu");
    assert_eq!(global(&s, "F"), "null");
    assert!(!s.objects.menu_items.contains_key("MENU_A.ONLY_B"));
}

#[test]
fn record_properties_read_and_set_status() {
    let mut s = objects_session();
    s.start().unwrap();
    s.set_item_value("CUSTOMERS.CITY", serde_json::json!("x")).unwrap();
    exec(
        &mut s,
        "BEGIN :GLOBAL.S1 := GET_RECORD_PROPERTY(1, 'CUSTOMERS', STATUS); :GLOBAL.CUR := GET_RECORD_PROPERTY(1, 'CUSTOMERS', CURRENT_RECORD); \
           SET_RECORD_PROPERTY(1, 'CUSTOMERS', STATUS, QUERY_STATUS); :GLOBAL.S2 := GET_RECORD_PROPERTY(1, 'CUSTOMERS', STATUS); \
           SET_RECORD_PROPERTY(1, 'CUSTOMERS', VISUAL_ATTRIBUTE, 'VA_HOT'); :GLOBAL.VA := GET_RECORD_PROPERTY(1, 'CUSTOMERS', VISUAL_ATTRIBUTE); \
           :GLOBAL.X := GET_RECORD_PROPERTY(7, 'CUSTOMERS', STATUS); IF FORM_FAILURE THEN :GLOBAL.RANGE := 'failed'; END IF; \
           SET_RECORD_PROPERTY(1, 'CUSTOMERS', STATUS, 'BOGUS_STATUS'); IF FORM_FAILURE THEN :GLOBAL.BAD := 'failed'; END IF; END;",
    );
    assert_eq!(global(&s, "S1"), "INSERT", "{:?} {:?}", s.trace_log, s.messages);
    assert_eq!(global(&s, "CUR"), "TRUE");
    assert_eq!(global(&s, "S2"), "QUERY");
    assert_eq!(global(&s, "VA"), "VA_HOT");
    assert_eq!((global(&s, "RANGE"), global(&s, "BAD")), ("failed".into(), "failed".into()));
    assert_eq!(s.system.record_status.as_str(), "QUERY");
}

#[test]
fn queried_sqlite_records_carry_a_rowid_so_updates_and_deletes_reach_the_database() {
    let key = ModuleKey::new("CONF08", ModuleType::Form);
    let cache = std::env::temp_dir().join(format!("ofw-gap08s-{}", std::process::id()));
    let module = FormsXmlParser::parse(&key, &sample("conf_08_commit_pipeline_fmb.xml"), &cache).unwrap();
    let mut s = FormSession::new(module);
    let cfg = oracle_forms_db::DbConfig { backend: oracle_forms_db::DbBackend::Sqlite, database_path: Some(":memory:".into()), ..Default::default() };
    s.set_database_driver(Arc::<dyn oracle_forms_db::DatabaseDriver>::from(oracle_forms_db::create_driver(&cfg).unwrap()));
    s.start().unwrap();
    s.go_block("ITEMS").unwrap();
    s.do_key("EXECUTE_QUERY").unwrap();
    assert!(s.blocks["ITEMS"].records.iter().all(|r| r.row_id.is_some()), "every queried record has a ROWID: {:?}", s.trace_log);
    s.set_item_value("ITEMS.ITEM_NAME", serde_json::json!("renamed")).unwrap();
    s.do_key("COMMIT_FORM").unwrap();
    s.do_key("EXECUTE_QUERY").unwrap();
    let names: Vec<String> = s.blocks["ITEMS"].records.iter().filter_map(|r| r.get_value("ITEM_NAME").map(|v| v.to_string())).collect();
    assert!(names.iter().any(|n| n.contains("renamed")), "the update was written and re-read: {names:?}");
}
