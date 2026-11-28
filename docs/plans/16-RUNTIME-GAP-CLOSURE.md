# 16: Runtime gap closure (execution budgets, parameter lists, timers, LOV dialogs, record groups)

Goal: 100% functional alignment of the native runtime with Oracle Forms 14c. Every missing
feature is work to build, not a limit. Generic tooling only: all fixtures are synthetic
(`crates/oracle-forms-runtime/tests/runtime_gap_tests.rs`, parser regressions, server unit
tests). Per-feature sections below record design, status, and what remains. Status words
mean "executed and verified in this repository" unless marked not verified.

## P0. Startup hang and runaway protection (done)

Symptom: with libraries attached, `POST /api/sessions` never returned for most large forms;
the server climbed to ~1 GB and a constant CPU core.

Root cause (verified with a stack sample of the hung process): not the interpreter. The hang
was in `Parser::parse_all_units` while building the program registry. Two parser defects
combined:

1. A dotted assignment target (`rec.field := expr;`, `pkg.var := expr;`) was parsed as a
   procedure call and failed at `:=`.
2. Statements inside `IF` / `LOOP` / `WHILE` / `FOR` bodies were parsed with `?`, so one bad
   statement aborted the whole enclosing construct and left its `END IF;` / `END LOOP;` /
   `END;` tokens at top level. `parse_unit` then returned an empty statement list without
   consuming the stray `END`, and `parse_all_units` pushed empty units forever (unbounded
   memory growth, 100% CPU).

Fixes:

- Parser: dotted assignment targets are assignments; nested statements recover individually
  (`parse_statement_recovering` yields an `Opaque` statement carrying the error, which is
  reported when executed); `parse_all_units` has a progress guard that skips and reports
  stray top-level tokens, so no input can make it loop.
- Interpreter: assignment to a field of a declared record variable creates the field.
- Execution budget (defence in depth): the existing step limit now reports the trigger, the
  program-unit call chain, the kind of statement running and the step count
  (`StepBudgetExceeded(String)`); a 20 s per-trigger wall-clock limit
  (`TimeBudgetExceeded`) and a host cancellation flag (`FormSession::cancel`,
  `Cancelled`) are sampled every 256 steps. `FormSession::command_deadline` lets the host
  bound a whole command.
- Server: each session already runs on its own thread; request handlers now await the
  session thread with a timeout (90 s startup, 60 s per command). On overrun the session is
  cancelled and removed (HTTP 504 / a startup error) so one runaway session cannot block
  other sessions or requests.

Remaining: statements carry no source line (the AST has no positions), so budget messages
name the statement kind, not a line number. Adding line numbers needs a `line` on
`Statement`; tracked here as follow-up. A runaway inside the parser or XML loader itself is
bounded only by the request timeout (the thread is abandoned, not killed).

Regression tests: `stray_top_level_end_terminates_and_is_reported`,
`dotted_assignment_targets_parse_as_assignments`, `bad_statement_inside_if_does_not_orphan_end_if`
(plsql), `infinite_loop_in_package_routine_is_stopped_and_named`,
`cancelled_session_stops_running_plsql`, `stray_end_in_program_unit_text_does_not_hang_session_creation`,
`record_field_assignment_and_read_work` (runtime), `startup_overrun_returns_error_and_other_sessions_still_start` (server).
