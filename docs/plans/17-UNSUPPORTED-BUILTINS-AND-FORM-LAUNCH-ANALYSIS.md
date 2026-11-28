# 17: Unsupported built-ins and form launch paths: analysis and implementation approaches

Status: analysis only (nothing here is implemented yet unless stated). Companion to
[16](16-RUNTIME-GAP-CLOSURE.md). Goal stays 100% functional alignment: each item below is
work to build, with the browser/server equivalent of what Forms 14c did on the client or
the application server. "Verify" lines say how to reverse engineer exact behavior; all
verification needs the optional 14c stack (compile a synthetic probe form with
`frmcmp_batch`, run it, record results into globals/messages/tables) and is therefore
not done yet. Public Oracle documentation and decompiling the jars shipped in the Oracle
Home (license permitting) are the other sources.

## A. Form launch paths (critical)

Forms has many ways to start or switch forms. The runtime must support all of them and they
must share one model.

### A.1 Entry points (outside any form)
1. Workbench UI: Run on the open module (exists). 
2. API/CLI: `POST /api/sessions {moduleName, backend, ...}`. Needed additions: startup
   parameters (`parameters: {NAME: value}` becoming `:PARAMETER.NAME`), the
   `formsweb.cfg`-style launch parameters already modelled by the FSAL launcher
   (`otherparams`, `userid`), and a `menu` override.
3. Deep link: `/?form=<module>&p_NAME=value` opens the UI and starts the session.
4. Role-impersonation launcher (plan 12): choose a user/role, then start a session with
   the matching global/parameter seed.
5. Headless `oracle-forms-cli run <module>` for tests (drives events from a script).

### A.2 In-form built-ins
| Built-in | Semantics in Forms | Runtime model |
|---|---|---|
| `CALL_FORM(form [, display_mode HIDE\|NO_HIDE] [, switch_menu DO_REPLACE\|NO_REPLACE] [, query_mode NO_QUERY_ONLY\|QUERY_ONLY] [, paramlist])` | Starts the form on top of the caller; the caller is suspended (modal), same database session/transaction; returns when the called form exits (`EXIT_FORM`). `HIDE` hides caller windows; `DO_REPLACE` swaps the caller's menu for the callee's; `QUERY_ONLY` forbids insert/update/delete in the callee. | Push on a call stack; caller blocked until pop; `FORM_SUCCESS` reflects load success. |
| `OPEN_FORM(form [, activate_mode ACTIVATE\|NO_ACTIVATE] [, session_mode NO_SESSION\|SESSION] [, paramlist])` | Opens a peer form (modeless); the opener keeps running. `SESSION` gives the new form its own database session (own transaction); `NO_SESSION` shares it. | Add to the set of open forms; `SESSION` creates a second driver connection. |
| `NEW_FORM(form [, rollback_mode TO_SAVEPOINT\|NO_ROLLBACK\|FULL_ROLLBACK] [, query_mode] [, paramlist])` | Exits the current form and starts another in its place (after commit/rollback handling); the replacement runs where the old one was (root of its call chain). | Replace the top frame; apply rollback mode first. |
| `GO_FORM(form)` | Moves input focus to another already-open form. | Switch active form. |
| `CLOSE_FORM(form)` | Closes a form opened with `OPEN_FORM`. | Remove from the open set. |
| `EXIT_FORM([commit_mode ASK_COMMIT\|DO_COMMIT\|NO_COMMIT\|NO_VALIDATE])` | In a called form returns to the caller; in an opened form closes it; in the root form ends the application. | Pop/close, with the commit modes. |
| `RUN_PRODUCT(FORMS, ...)` (legacy) | Can start a Forms module in older releases. | Map to `NEW_FORM`/`OPEN_FORM` equivalent only if a form uses it. |
| Menu items (PL/SQL command type) | Menus invoke the built-ins above; `DO_REPLACE` switches the menu module. | Use the same stack API; menu runtime is gap C.7. |
| Parameters | Every start can carry a parameter list (`DEFAULT` list name or an id) or the form's own `:PARAMETER.*` defaults. Called forms read `:PARAMETER.X`; text parameters only. | Copy list entries into the child session. |
| Introspection | `GET_APPLICATION_PROPERTY(CALLING_FORM, CURRENT_FORM, CURRENT_FORM_NAME)`, `:SYSTEM.CURRENT_FORM`, `GET_FORM_PROPERTY(...)`, `FIND_FORM`. | Answer from the stack. |

### A.3 Architecture options
- **Option 1: one session thread owning a stack/set of `FormSession`s (recommended).**
  Forms itself is single-threaded per runtime process, so one owner thread is faithful: it
  makes call-stack blocking trivial (the caller's trigger simply waits inside the nested
  `call_form`), keeps `:GLOBAL`, the driver and alert/LOV pausing in one place, and needs no
  cross-thread locks. A `FormInstance { id, FormSession, mode, parent }` list plus an
  `active` pointer lives in a new `Application` type in `oracle-forms-runtime`; a
  `ModuleLoader` callback (server provides: samples, `OFW_FORMS_PATH_EXTRA`, attached
  libraries and menus) resolves names, so the crate stays file-system free.
  `SESSION` mode asks the loader for a fresh driver.
- Option 2: one server session per form with a group id. Gives independent blocking but
  needs shared globals/connection state across threads and cross-session messaging; more
  complex and diverges from Forms' model. Not recommended except for `SESSION` forms'
  database connection, which Option 1 already covers.
- Option 3: separate browser windows per form. A UI choice, not a runtime one; compatible
  with Option 1 by tagging events with `formId`.
- **UI approaches** (pick per user preference; the protocol supports all): tab strip over
  the single preview (default), MDI-style floating windows (closest to Forms MDI), or one
  browser tab per form (state events carry `formId`; a `CALL_FORM` child is modal over the
  caller). Protocol: state gains `forms: [{id, name, active, modal}]` and `activeForm`;
  events/answers carry `formId`; alerts/LOVs belong to a form.
- Edge semantics to pin down by probe forms: what `FORM_SUCCESS` means after a failed
  load; whether `WHEN-NEW-FORM-INSTANCE` of the child fires before the call returns;
  transaction behavior at `NEW_FORM` with each rollback mode; `QUERY_ONLY` enforcement
  message; shared vs separate `:GLOBAL`; `:SYSTEM.FORM_STATUS` of the caller after return;
  `EXIT_FORM` inside `CALL_FORM` with pending changes (`ASK_COMMIT` alert text).
- Tests: synthetic parent/child pairs for each mode; parameter passing; globals;
  commit/rollback on exit and replace; menu replacement; missing module; recursion
  limit; timers and LOVs per form.

## B. Unsupported built-ins: what they do and how to provide the equivalent

Currently each of these stops the trigger with a visible "not supported" error.

### B.1 `POST`
Does: runs the commit pipeline up to and including the DML (validate, `PRE-INSERT` /
`PRE-UPDATE` / `PRE-DELETE`, the DML, `POST-INSERT` ...), but does not issue `COMMIT`;
status of posted records changes to "posted" (still uncommitted).
Approach: split `FormSession::commit` into `post` and `finish_commit`; hold a transaction
open on the driver between them (SQLite/Thin support this); `COMMIT_FORM` = post +
commit, so one code path. Verify: trace order and `:SYSTEM.*_STATUS` after `POST`.
Feasibility: high. Note: needs write opt-in like commit.

### B.2 `HOST`
Does: runs an operating-system command on the application server (not the user's PC) and
returns after it ends; optional `NO_SCREEN` / `NO_ACKNOWLEDGE`.
Approach: never run arbitrary commands by default. A `HostPolicy` per server: `Deny`
(default: `FORM_FAILURE`, message), `Emulate` (virtual shell over the per-session
sandbox directory supporting the common safe commands such as `echo ... > file`,
`touch`, `mkdir`, `rm`, `cp`, `type`/`cat`; unknown commands fail visibly), or
`Allow(list)` (exact-match allowlist run without a shell, with timeout and output cap).
The same sandbox directory backs `TEXT_IO`, so form code that writes a file and shells out
to check it works end to end. Verify: exit-code and `FORM_SUCCESS` behavior.
Feasibility: high for Emulate, policy-gated for real execution.

### B.3 `RUN_PRODUCT` (Reports, Graphics, Book)
Does: starts an Oracle product on a module: product, module, `SYNCHRONOUS` or
`ASYNCHRONOUS`, `BATCH` or `RUNTIME`, `FILESYSTEM` or `DB`, a parameter list (text
parameters, plus data parameters that pass a record group's query result), and a display.
In practice it is almost entirely used to run reports.
Approach: the workbench already has a report engine (`ReportDataEvaluator`,
`ReportRenderer`, `/api/reports/run`). Map the module to a report definition on the report
search path, copy text parameters into report parameters (`DESTYPE`, `DESFORMAT`,
`DESNAME`, `P_` parameters; honour `DESTYPE=SCREEN/PREVIEW` by returning the rendered HTML
to the UI as a panel/tab, `FILE` by writing into the session sandbox, `PRINTER` as PDF
download), feed `DATA_PARAMETER` record groups as the data source, and run asynchronous
requests as jobs. Failures (module not found, bad parameters) set `FORM_FAILURE` with
the Forms-style message. Verify against the 14c Reports tooling in the stack (`rwconverter`
is already used) for parameter handling and output differences. Feasibility: high where the
report definition exists in the workbench format; fidelity depends on the renderer.

### B.4 `RUN_REPORT_OBJECT` family (Reports Server integration)
Does: report objects are design-time module objects (`FIND_REPORT_OBJECT`,
`SET/GET_REPORT_OBJECT_PROPERTY`: file name, execution mode, communication mode,
destination type/format/name, server, other parameters, parameter list);
`RUN_REPORT_OBJECT` returns a job id; `REPORT_OBJECT_STATUS(job)` returns
`FINISHED/RUNNING/QUEUED/ERROR...`; `COPY_REPORT_OBJECT_OUTPUT(job, file)`;
`HOST` of output via `WEB.SHOW_DOCUMENT`.
Approach: add `ReportObject` to the model/reader (design-time object), a job table in
`RuntimeObjects`, executed on the same engine as B.3; statuses move
`QUEUED -> RUNNING -> FINISHED/ERROR`; output retrievable by an id-based URL
(`/api/sessions/{id}/reports/{job}`) which `WEB.SHOW_DOCUMENT` can open. Feasibility: high
once B.3 exists.

### B.5 `WEB.SHOW_DOCUMENT(url [, target])`
Does: asks the user's browser to open a URL (`_blank` default, `_self`, named window).
Approach: the built-in returns an interaction/side effect `OpenUrl{url, target}` in the
session state (a new `actions` array); the UI opens it. Security: only `http`/`https`
and workbench-relative report/output URLs; no `javascript:`/`data:`; popup-blocker friendly
because it is triggered by the user's last click (otherwise show a link banner).
Report URLs produced by form code (a report server base URL) map to workbench outputs when
they carry a known job id. Feasibility: high, small.

### B.6 `EXEC_SQL` package (`EXEC_SQL.OPEN_CONNECTION`, `OPEN_CURSOR`, `PARSE`,
`DEFINE_COLUMN`, `BIND_VARIABLE`, `EXECUTE`, `FETCH_ROWS`, `COLUMN_VALUE`, `VARIABLE_VALUE`,
`IS_OPEN`, `CLOSE_CURSOR`, `CLOSE_CONNECTION`, `LAST_ERROR_CODE/MESSAGE/POSITION`,
`MORE_RESULT_SETS`, `DEFAULT_CONNECTION`)
Does: DBMS_SQL-style dynamic SQL against the default or a secondary connection (even to
non-Oracle databases via ODBC).
Approach: session-owned connection and cursor tables over `DatabaseDriver`; secondary
connections create another driver from a stored config (`OPEN_CONNECTION(connect_string)`
resolved by an allowlist of named connections, since the workbench never stores passwords);
`COLUMN_VALUE` / `VARIABLE_VALUE` produce OUT values through the existing OUT-argument
mechanism (extend `out_arg_start` to a per-built-in spec); DML honours the read-only gate.
Verify: type mapping and error codes by probe form. Feasibility: medium (volume of API,
simple semantics).

### B.7 `FORMS_DDL(statement)`
Does: executes a dynamic SQL statement, DDL, or an anonymous PL/SQL block built at run
time; DDL implies a commit; errors are read with `DBMS_ERROR_CODE` / `DBMS_ERROR_TEXT`.
Approach: if the text starts with `BEGIN`/`DECLARE`, run it through the interpreter with
the caller's scope; otherwise execute through the driver: queries rejected, DML under the
write gate, DDL only with an explicit `allow_ddl` session option (default off, always
off for read-only sessions) with a clear refusal message; implicit commit for DDL;
set `FORM_SUCCESS`/`FORM_FAILURE` and the last error code/text; add `DBMS_ERROR_CODE/TEXT`.
Feasibility: high; SQLite cannot run Oracle-specific DDL (expected, reported).

### B.8 `TEXT_IO` (server-side file I/O)
Does: `FOPEN(path, 'r'|'w'|'a')`, `GET_LINE`, `PUT`, `PUT_LINE`, `NEW_LINE`, `FCLOSE`,
`FFLUSH`, `IS_OPEN`, `FCLOSE_ALL`; reads past end raise `NO_DATA_FOUND`.
Approach: per-session sandbox directory under the server work dir, paths resolved inside it
(absolute/`..` paths mapped or rejected, configurable), a `FILE_TYPE` handle table in
`RuntimeObjects`, and a switch to expose chosen host directories read-only. Share the
sandbox with `HOST`. Verify: exception types for bad paths/modes. Feasibility: high.

### B.9 WebUtil (`WEBUTIL_*`, `CLIENT_*`)
Does: a PL/SQL library plus a Java bean on the *user's PC*: client file access, file transfer,
host commands, system/browser info, OLE, native calls. The library talks to the bean through
the bean-area item built-ins `SET_CUSTOM_PROPERTY` / `GET_CUSTOM_PROPERTY` and the
`WHEN-CUSTOM-ITEM-EVENT` trigger (`:SYSTEM.CUSTOM_ITEM_EVENT`, `...PARAMETERS`).
Two ways to support it:
1. Shim (recommended first): intercept the public package calls (`CLIENT_TEXT_IO.*`,
   `CLIENT_HOST`, `CLIENT_GET_FILE_NAME`, `CLIENT_IMAGE`, `CLIENT_GET_SYSTEM_PROPERTY`,
   `WEBUTIL_FILE_TRANSFER.*`, `WEBUTIL_BROWSER.*`, `WEBUTIL_CLIENTINFO.*`,
   `WEBUTIL_SESSION.*`, `WEBUTIL_APP.*`) in the program registry before the attached library
   is consulted, and map each to a browser capability. Works whether or not the library is
   attached.
2. Bean emulation (fidelity, later): implement the bean-area built-ins, the custom-item-event
   trigger, and a JavaScript emulator of the WebUtil bean so the unmodified WebUtil library
   runs. Needed for forms that call the bean directly or depend on library internals.
Browser equivalents:
| WebUtil area | Browser/server equivalent |
|---|---|
| `CLIENT_GET_FILE_NAME`, `CLIENT_FILE`, file existence/list | File picker (File System Access API or `<input type=file>`); un-picked paths cannot be read: ask the user, remember the grant for the session |
| `CLIENT_TEXT_IO`, `CLIENT_IMAGE` | Read: user uploads/picks the file (interaction pause like a LOV); write: content is buffered and offered as a download on `FCLOSE` |
| `WEBUTIL_FILE_TRANSFER` (`CLIENT_TO_DB`, `DB_TO_CLIENT`, `URL_TO_CLIENT`, `AS_TO_CLIENT`, `CLIENT_TO_AS`) | HTTP upload/download endpoints with the same name mapping; `URL_TO_CLIENT` = download with a URL allowlist |
| `CLIENT_HOST` | Policy-gated like `HOST`, scoped to a client sandbox emulation, default deny |
| `CLIENT_GET_SYSTEM_PROPERTY`, `WEBUTIL_CLIENTINFO` | Configured/impersonated values (user name, OS, machine) plus browser user agent |
| `WEBUTIL_BROWSER` | `WEB.SHOW_DOCUMENT` path |
| `CLIENT_OLE2`, `WEBUTIL_OLE`, `WEBUTIL_C_API` | Windows COM/native only: not available in a browser; fail visibly with a clear message (see B.10) |
Verify: install the WebUtil library into a probe form, log its PL/SQL calls, and compare
the property/event traffic of the bean (decompile the WebUtil jars shipped with the 14c
install). Feasibility: shim high for files/transfer/info; bean emulation medium.

### B.10 `OLE2.*`, `DDE`, `D2KWUTIL`
Does: Windows automation on the client (Word/Excel via COM, DDE conversations, registry and
Windows file dialogs).
Approach: no browser equivalent exists. Keep the unsupported error, but make it precise
(which object/method), and offer scenario shims when real usage appears: an "export to
spreadsheet" OLE sequence can be recognised and replaced by server-side `.xlsx`/CSV
generation and a download; `D2KWUTIL.GET_FILE_NAME` maps to the file picker; registry reads
map to a configurable emulated store. Feasibility: low in general, case by case.

## C. Other unresolved built-ins seen when running real forms
| Built-in | Notes / approach |
|---|---|
| `SET_MENU_ITEM_PROPERTY`, `GET_MENU_ITEM_PROPERTY`, `FIND_MENU_ITEM` | Menu runtime: load the attached menu module into the session, keep enabled/checked/label per item, push changes to the UI menu bar; menu PL/SQL commands run through the same interpreter. Frequent in practice, feasible. |
| List items: `CLEAR_LIST`, `ADD_LIST_ELEMENT`, `DELETE_LIST_ELEMENT`, `POPULATE_LIST`, `GET_LIST_ELEMENT_*` | Runtime element table per list item (design-time elements already parsed); `POPULATE_LIST` from a record group. |
| `LOGON`, `LOGOUT` | Reconnect with new credentials: build a new driver from the session's backend config; prompts use the existing logon dialog interaction; passwords never stored. |
| `GET_FILE_NAME` | File picker interaction (like B.9). |
| `EDIT_TEXTITEM`, `SHOW_EDITOR` | Editor modal interaction (same pause mechanism). |
| `PRINT` | Browser print / PDF of the rendered form. |
| `SET_RECORD_PROPERTY` / `GET_RECORD_PROPERTY` | Per-record status/visual attribute; records already carry status. |
| `SET_CUSTOM_PROPERTY` / `GET_CUSTOM_PROPERTY`, `FIRE_POPUP` | Bean-area built-ins (B.9 option 2) and popup menus. |
| `DBMS_*` packages (`DBMS_OUTPUT`, `DBMS_SQL`, `DBMS_APPLICATION_INFO`, `DBMS_UTILITY`) | Database-resident on Oracle; for SQLite/Mock provide in-runtime emulations of the commonly used ones (output buffer shown in the trace panel). |
| `SET_WINDOW_PROPERTY` and other visual setters | Currently accepted and traced as "not rendered"; the UI should apply title/position/visibility to the preview. |
| `COPY_REGION`, `DUPLICATE_RECORD`, `SELECT_RECORDS`, `GET_MESSAGE`, `CALL_QUERY` | Small built-ins; add with the record/block API. |

## D. Suggested order
1. Multi-form `Application` (A.3 option 1) with `CALL_FORM`/`NEW_FORM`/`OPEN_FORM`/`GO_FORM`/
   `EXIT_FORM`/`CLOSE_FORM`, startup parameters, UI tab strip.
2. `POST`, `WEB.SHOW_DOCUMENT`, `TEXT_IO` + sandbox + `HOST` policy.
3. `RUN_PRODUCT` / report objects on the report engine; `FORMS_DDL`; `EXEC_SQL`.
4. Menu runtime, list item built-ins, `LOGON`/`LOGOUT`, `GET_FILE_NAME`, `EDIT_TEXTITEM`.
5. WebUtil shim, then bean emulation; OLE/DDE scenario shims on demand.
Each step: probe form against 14c where available, synthetic tests, plan-16/17 status
update, change log row.
