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

## 5. Multi-form navigation (done)

Implemented as designed in [17](17-UNSUPPORTED-BUILTINS-AND-FORM-LAUNCH-ANALYSIS.md) A.3 option 1:
one session thread owns the application's form stack (`crates/oracle-forms-server/src/form_app.rs`);
the runtime crate (`nav.rs`) only parses the built-ins and calls host callbacks, so it has no
file-system or thread code.

- `CALL_FORM(form [, HIDE|NO_HIDE] [, DO_REPLACE|NO_REPLACE] [, QUERY_ONLY|NO_QUERY_ONLY] [, paramlist])`
  is a real nested call: the caller's trigger blocks in the navigation handler, which loads the
  module (host `FormLoader` callback), starts it with the parameter list consumed into
  `:PARAMETER.*`, serves browser commands for the called form, and returns when it exits, so the
  caller continues after `CALL_FORM`. Calls nest. A suspended caller (and any form behind a
  modal called form) refuses events with a clear error. `QUERY_ONLY` starts the form read-only.
  `HIDE` and `DO_REPLACE` are reported per form in the state (`hidden`, `replaceMenu`); the menu
  runtime that would act on `DO_REPLACE` is not built yet.
- `OPEN_FORM(form [, ACTIVATE|NO_ACTIVATE] [, SESSION|NO_SESSION] [, paramlist])` queues the
  open; it is applied when the opening trigger ends (the opener keeps running). `SESSION` gives
  the form its own database connection (shared for in-memory SQLite, where a second connection
  would be an unrelated empty database, and for Mock). Opening an already open form is refused
  with a message.
- `NEW_FORM(form [, TO_SAVEPOINT|NO_ROLLBACK|FULL_ROLLBACK] [, QUERY_ONLY] [, paramlist])` replaces
  the issuing form in place (keeps its place in a call chain and its `calling form`);
  `FULL_ROLLBACK` discards the form's buffered changes first. Savepoint semantics beyond that
  are not modelled (changes are written only by commit).
- `GO_FORM`, `CLOSE_FORM` (peers; called forms end with `EXIT_FORM`), `FIND_FORM`, `EXIT_FORM`
  with `ASK_COMMIT` (asks "save changes?" Yes/No/Cancel through the alert channel when a
  database block has changes), `DO_COMMIT`, `NO_COMMIT`, `NO_VALIDATE`. Exit of a called form
  returns to the caller, of an opened form closes it, of the root ends the application.
- `:GLOBAL` values are shared across the application (copied in before and out after a form
  runs; `:PARAMETER.*` stay per form). Introspection: `GET_APPLICATION_PROPERTY(CALLING_FORM)`,
  `:SYSTEM.CURRENT_FORM`, `GET_FORM_PROPERTY(FORM_NAME|FIRST_BLOCK|LAST_BLOCK|CURRENT_BLOCK|CALLING_FORM)`.
- API: `POST /api/sessions` accepts `parameters: {NAME: value}`; every event may carry `formId`
  (default: the active form), alert/LOV answers may carry `formId` (409 if it is not the form
  that asked), the new event `activateForm {formId}` is the tab click, and every state carries
  `formId`, `activeForm`, `sourceFile` and `forms: [{id, name, kind, active, modal, suspended,
  hidden, replaceMenu}]`.
- UI: a tab strip appears when more than one form is open; the preview switches to the
  module of the active form (`/api/open-sample/<sourceFile>`) without ending the session.
- Tests: `crates/oracle-forms-server/src/runtime_sessions.rs` (call/return, parameters, globals,
  nesting, missing form, QUERY_ONLY/HIDE, open/activate/go/close/find, NO_ACTIVATE/SESSION,
  NEW_FORM, exit modes and prompt, wrong `formId`), runtime unit tests, `tests/ui_runtime.test.cjs`,
  and the synthetic forms `samples/conformance/conf_21_nav_*_fmb.xml`.
Remaining: `RUN_PRODUCT(FORMS...)`, savepoint rollback semantics, menu replacement,
per-form timers continue to run only for the forms that are not suspended, and the
exact `FORM_SUCCESS`/message texts for failed loads are unverified against 14c.

## 6. POST, WEB.SHOW_DOCUMENT, TEXT_IO, HOST (done)

- `POST`: `FormSession::post()` runs the commit sequence (validation, `PRE-COMMIT`, the DML
  triggers and statements, `POST-FORMS-COMMIT`) inside the savepoint but issues no database
  `COMMIT`; posted records become query records; `posted_uncommitted` makes the next
  `COMMIT_FORM` only commit (and fire `POST-DATABASE-COMMIT`). Needs write opt-in like commit.
  Finding while testing: queries on the SQLite backend return no `ROWID`, so updates and deletes
  of queried records fail there with "changed record has no ROWID" (existing limitation, not
  changed here; the POST test uses the Mock driver).
- `WEB.SHOW_DOCUMENT(url [, target])`: queues an `openUrl` action (`actions` in the state); only
  `http`, `https` and workbench-relative URLs pass (server and UI both check); the UI opens
  the window and shows a banner if the browser blocks it.
- `TEXT_IO` (`textio.rs`): `FOPEN`, `FCLOSE`, `FCLOSE_ALL`, `IS_OPEN`, `GET_LINE` (OUT), `PUT`,
  `PUT_LINE`, `NEW_LINE`, `PUTF`, `FFLUSH` on a per-session in-memory virtual file system
  (`sandboxFiles` lists the files in the state). Client-style paths (`c:\dir\f.txt`) map into
  the sandbox; `..` is refused. Errors raise `INVALID_PATH` / `INVALID_MODE` / `INVALID_FILEHANDLE`
  / `INVALID_OPERATION`, end of file raises `NO_DATA_FOUND` (names unverified against 14c).
  New: built-ins can raise PL/SQL exceptions (`BuiltinResult::Raise`) and qualified type
  names (`TEXT_IO.FILE_TYPE`) parse. Remaining: files cannot yet be uploaded to or downloaded
  from the sandbox through the UI/API; `CLIENT_TEXT_IO` (client PC) is part of the WebUtil work.
- `HOST` policy (`host.rs`, `host_exec.rs`, env `OFW_HOST_POLICY`): `deny` (default, visible
  failure), `emulate` (`echo [>|>>]`, `touch`, `type`/`cat`, `del`/`rm`, `copy`/`cp`,
  `move`/`mv`, `mkdir`, `dir`/`ls`, chained with `&&`, `&`, `;`; unknown commands named in the
  error), or `allow:prog1,prog2` (real execution without a shell, 10 s timeout, output capped).
