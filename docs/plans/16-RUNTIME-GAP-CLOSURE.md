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

## Shared design: session objects and interactions

- Object ids are the object's name (as for alerts and items); `NULL` means "not found", so
  `ID_NULL(FIND_x(...))` works. A failed built-in sets `FORM_FAILURE`, records a message and
  lets the trigger continue (Forms semantics).
- State lives in `FormSession::objects` (`RuntimeObjects`): parameter lists, record groups
  (design-time groups are loaded at session start; static groups are already populated),
  timers, LOV property overrides and the session clock.
- Built-ins with OUT parameters (`GET_PARAMETER_ATTR`) are declared in
  `builtins::out_arg_start`; the interpreter evaluates only the IN arguments, then assigns
  the produced values (`RuntimeObjects::pending_out`) to the OUT targets.
- Interactions that need the user (`SHOW_ALERT`, `SHOW_LOV`) go through the session's
  interaction handler. The server pauses the session thread, returns the question as the
  HTTP response (`alert` or `lov` in the state), and the follow-up request
  (`/alert` or `/lov`) carries the answer back. While a question is open other events get 409.
- The model now keeps LOV column mappings with their return items, LOV title/auto-refresh,
  record group type, column types and static rows, and the item `ValidateFromList` flag
  (`LovInfo`, `RecordGroupInfo`, `ItemInfo`); the XML summary writer emits them again.

## 1. Parameter lists (done)

`CREATE_PARAMETER_LIST` (NULL name creates an unnamed list), `GET_PARAMETER_LIST`,
`DESTROY_PARAMETER_LIST`, `ADD_PARAMETER`, `DELETE_PARAMETER`, `GET_PARAMETER_ATTR`,
`SET_PARAMETER_ATTR`, `ID_NULL` on list ids. `TEXT_PARAMETER` holds text, `DATA_PARAMETER`
holds the name of an existing record group (checked on add/set). Keys are case-insensitive
and unique; duplicate creates/adds and unknown lists/keys fail visibly.
Remaining: the lists are not yet consumed by `CALL_FORM` / `OPEN_FORM` / `NEW_FORM` /
`RUN_PRODUCT` (multi-form navigation is the next item, see the design note below).

## 2. Timers (done)

`CREATE_TIMER(name, ms [, REPEAT|NO_REPEAT])`, `DELETE_TIMER`, `SET_TIMER(name, ms|NO_CHANGE,
REPEAT|NO_REPEAT|NO_CHANGE)` (restarts the countdown), `FIND_TIMER`,
`GET_APPLICATION_PROPERTY(TIMER_NAME)`, form-level `WHEN-TIMER-EXPIRED`.

Event model: the runtime has no thread; the host calls `FormSession::fire_due_timers`.
The server calls it (a) after startup with a 100 ms look-ahead, so the "run after the form
opens" idiom (a few-millisecond timer created in `WHEN-NEW-FORM-INSTANCE`, including `0`)
fires as part of the startup response; (b) after every browser event; (c) on
`POST /api/sessions/{id}/tick`. State carries `nextTimerMs`; the UI polls `tick` after that
delay (clamped to 50 ms - 5 s) while no alert/LOV is open. Timers never fire inside a running
trigger. Repeating timers reschedule `interval` after firing (no catch-up burst).
Assumption, not verified against 14c: a non-repeating timer stays defined (inactive) after it
fires. Limits: server-push (SSE) is not used; polling is the transport.

## 3. LOV dialogs (done)

`SHOW_LOV(lov)` (TRUE when a row was picked), `LIST_VALUES [(RESTRICT|NO_RESTRICT)]`, the
default `KEY-LISTVAL` (a user `KEY-LISTVAL` trigger overrides it), `FIND_LOV`,
`GET_LOV_PROPERTY` / `SET_LOV_PROPERTY` (`GROUP_NAME`, `TITLE`, `AUTO_REFRESH`,
`FILTER_BEFORE_DISPLAY`). The record group is (re)populated when it has a query and is not
populated yet or the LOV refreshes automatically (Forms default); rows go to the UI as
`lov: {lovName, title, columns, rows, filter}`; the answer `{selected: index|null}` applies
each column mapping to its return item. `RESTRICT` (default) pre-filters on the text typed
in the item. No user attached (batch runs): the LOV is auto-cancelled and traced.
Validate-from-list (`ValidateFromList`): after a typed value the runtime accepts an exact
match (and fills the other return items), completes a unique prefix, shows the list for
several matches, and otherwise raises `FRM-40212`.
UI: modal picker with a find box (`ui/app.js`, `ui/runtime.js`), `F9` and a toolbar button.
Remaining: `FILTER_BEFORE_DISPLAY` does not prompt for a separate find string (the picker's
find box is used); LOV column widths/positions are not rendered; LOV `X`/`Y` arguments are
ignored; the `ValidateFromList` check runs on the browser `SetItem` event, not on every
programmatic assignment.

## 4. Record groups (done)

`CREATE_GROUP`, `CREATE_GROUP_FROM_QUERY`, `POPULATE_GROUP`, `POPULATE_GROUP_WITH_QUERY`
(return `0`, `1403` for no rows, or the Oracle error number), `FIND_GROUP`, `FIND_COLUMN`,
`ADD_GROUP_COLUMN`, `ADD_GROUP_ROW` (row number or `END_OF_GROUP`), `GET/SET_GROUP_CHAR/
NUMBER/DATE_CELL` (column ids are `GROUP.COLUMN`; type checked), `GET_GROUP_ROW_COUNT`,
`DELETE_GROUP` (design-time groups are protected), `DELETE_GROUP_ROW` (row or `ALL_ROWS`).
Queries accept `:BLOCK.ITEM` / `:GLOBAL.X` binds. Remaining: selection built-ins
(`GET_GROUP_SELECTION*`, `SET_GROUP_SELECTION`, `RESET_GROUP_SELECTION`), `GET_GROUP_RECORD_NUMBER`,
`UNSET_GROUP_SELECTION`, `ADD_GROUP_ROW` into groups with unpopulated query columns,
and PL/SQL-local variables inside group queries (only form binds are resolved).

## 5. Multi-form navigation (design note, not implemented)

Goal: `CALL_FORM`, `OPEN_FORM`, `NEW_FORM` inside one session.
- `FormStack`: the session becomes a stack/set of `FormSession`s owned by one server session
  thread. `CALL_FORM` pushes a child and blocks the caller (modal; the parent is disabled
  until `EXIT_FORM`), `OPEN_FORM` adds a peer that can be activated (session mode
  `ACTIVATE`/`NO_ACTIVATE`), `NEW_FORM` replaces the current form (root of the stack).
- Module resolution: reuse the server's form lookup (`samples/`, `OFW_FORMS_PATH_EXTRA`) via a
  `ModuleLoader` callback on the session, so the runtime crate keeps no file-system knowledge.
- Shared state: `:GLOBAL.*` and the database connection/transaction are shared across the
  stack (as in Forms); item values, `:SYSTEM`, triggers, timers and parameter lists
  (except the list passed in) are per form. The parameter list argument is copied into the
  child's `:PARAMETER.*`; `DEFAULT` list means the one named `DEFAULT`.
- Protocol: state gets `forms: [{id, name, active}]` and `activeForm`; events carry an
  optional `formId`; `EXIT_FORM` in a child pops it and resumes the caller after `CALL_FORM`
  with `FORM_SUCCESS`; `EXIT_FORM` in the root ends the session; `QUIT_ON_ERROR`/`SYSTEM`
  semantics follow Forms.
- UI: a tab strip for opened forms, the active form is rendered by the existing preview.
- Tests: synthetic parent/child pair; `CALL_FORM` with parameter list, global passing, exit
  and return; `OPEN_FORM` peer switching; `NEW_FORM` replacement; error when the module is
  not found.
