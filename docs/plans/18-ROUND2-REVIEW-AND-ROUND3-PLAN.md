# 18: Round 2 review (independent reviewer) and round 3 plan

Round 2 self-review gaps are in [16](16-RUNTIME-GAP-CLOSURE.md) and [17](17-UNSUPPORTED-BUILTINS-AND-FORM-LAUNCH-ANALYSIS.md).
An independent reviewer (the `codex` CLI agent, persona "Astra", run through the `cli-agent` skill with
the isolated flag on a bundle of the plan documents and the new modules) produced the findings
below. They are recorded verbatim as review input; each item is a claim to verify with a test before
changing code, not a verified defect. Round 3 implements the tasks that can be done with synthetic
tests (status is tracked at the end of this file).

## A. Defects in the supplied code

1. **Navigation arguments are parsed without respecting each built-in’s signature.** In `nav.rs`, `mode_words` treats the first non-mode text argument after the form name as a parameter-list name, wherever it appears. For example, `CALL_FORM('CHILD', 'BOGUS')` is reported as a missing parameter list instead of an invalid mode. Conversely, arguments in invalid positions can still be accepted. Parse each built-in’s positional arguments and allowed modes explicitly; add tests for invalid modes, duplicate modes, and misplaced parameter lists.

2. **`SESSION` can silently fail to create a separate database connection.** In `form_app.rs`, `new_connection` returns the shared driver if connection setup fails. An `OPEN_FORM(..., SESSION)` therefore proceeds with shared-session behavior while claiming to have opened a separate session. Propagate the connection error to the opening form, or explicitly report the fallback; test with a driver whose connection creation fails.

3. **`OPEN_FORM` accepts a form whose startup failed.** `open_form` stores the session, adds a startup error message, and may activate it even when `start_session` returns an error. The caller receives no direct failure result. Define and implement the failure path: test a child whose startup trigger fails and verify whether it is removed, left open, or reported according to the chosen behavior.

4. **`GET_LINE` does not enforce read/write mode symmetrically with `PUT`.** `textio.rs` checks that a handle is readable before reading, but it does not validate the output argument or enforce a maximum line size. `PUTF` also converts unmatched `%s` placeholders to empty strings and ignores extra arguments. Validate arguments and documented format limits; test malformed calls and boundary-sized lines.

5. **`NEW_LINE` can request an unbounded allocation.** `textio.rs` repeats a newline `n` times after casting a positive `i64` to `usize`, without a cap. A very large value can exhaust memory. Reject counts above a documented limit and test the largest accepted value and the first rejected value.

6. **`SET_MENU_ITEM_PROPERTY` accepts menu names by item name alone.** In `ui_builtins.rs`, `menu_key` checks whether *any* menu contains the final item name, then accepts the supplied full key. With two menus, `OTHER_MENU.SAVE` can be accepted because `SAVE` exists in the attached menu. Resolve and validate the full menu path; test same-named items in different menus.

7. **`SET_RECORD_PROPERTY` does not validate `VISUAL_ATTRIBUTE`.** In `ui_builtins.rs`, any value is stored, even when the named visual attribute does not exist. Resolve the attribute against the form’s metadata or report a visible Forms-style failure; test known and unknown attributes.

## B. Behaviour that differs from Forms 14c

1. **`CALL_FORM` flags are recorded but do not control the menu runtime.** `form_app.rs` carries `hide_caller` and `replace_menu` into state, while `nav.rs` documents them as presentation flags. Until the menu runtime applies them, `HIDE` and `DO_REPLACE` do not provide their Forms behavior. Add a menu-stack test that verifies the caller’s menu is hidden or replaced during the call and restored on return.

2. **`NEW_FORM` does not implement its rollback modes.** In `form_app.rs`, only `FULL_ROLLBACK` calls `clear_form`; `TO_SAVEPOINT` and `NO_ROLLBACK` are effectively identical. It also replaces the form without asking about unsaved changes. Implement rollback/savepoint behavior and the appropriate unsaved-change flow, with synthetic tests for each mode and dirty buffers.

3. **`CLOSE_FORM` closes an opened form without handling its unsaved changes.** `apply_nav` calls `close_form` directly. Add the Forms-compatible commit or prompt behavior and tests for dirty and clean opened forms.

4. **Timers on suspended forms do not run.** `tick_all` skips every form that is not `runnable`, and `runnable` excludes callers suspended by `CALL_FORM`. Verify the Forms 14c behavior and implement the matching timer lifecycle; test a caller timer while a called form is active.

5. **The one-shot timer remains defined after firing.** `timers.rs` clears its due time but retains the timer. The bundle explicitly marks this as unverified. Check the Forms behavior and make `FIND_TIMER`, `SET_TIMER`, and deletion behave accordingly, with a synthetic test for firing and restarting a one-shot timer.

6. **`LIST_VALUES` and LOV property behavior are incomplete.** `lov.rs` uses the picker’s find box for filtering, while `FILTER_BEFORE_DISPLAY` does not trigger a separate Forms-style prompt; `SHOW_LOV` also ignores its positioning arguments. Implement the expected filter and positioning behavior or report each unsupported case. Test restricted and unrestricted lookup, the property, and supplied coordinates.

7. **Form navigation does not model all Forms module lifecycles.** The bundle says opening an already-open form is refused, and `RUN_PRODUCT` is unsupported. Add coverage for repeated instances and `RUN_PRODUCT(FORMS...)`, then implement the Forms 14c lifecycle that those tests establish.

## C. Missing features needed for production forms

- **Menu runtime and rendering:** apply `menuItems` changes, submenu and item roles, and `CALL_FORM` menu replacement.
- **Database and transaction behavior:** SQLite query `ROWID` support for update/delete; real `NEW_FORM` savepoint semantics; and explicit failure handling for `SESSION` connection setup.
- **Record groups:** selection built-ins, `GET_GROUP_RECORD_NUMBER`, `UNSET_GROUP_SELECTION`, query population for groups with unpopulated query columns, and resolving PL/SQL locals in group queries.
- **Text and client integration:** UI/API upload and download for virtual files, file-size limits, `CLIENT_TEXT_IO`/WebUtil, OLE, and DDE.
- **Common runtime calls:** `RUN_PRODUCT`, `RUN_REPORT_OBJECT`, `EXEC_SQL`, `FORMS_DDL`, `LOGON`/`LOGOUT`, `GET_FILE_NAME`, `EDIT_TEXTITEM`, `PRINT`, and required `DBMS_*` packages.
- **Rendering and state APIs:** render `SET_WINDOW_PROPERTY`; expose state/tick operations for a specified form; exercise the tab strip, module switch, LOV dialog, and timer polling in a browser.
- **Diagnostics and compatibility:** carry source lines into statements for budget errors; verify exception names, FRM messages, prompts, timer behavior, and startup timing against Forms 14c.

## D. Security and robustness risks

- **Unbounded virtual-file operations:** `TEXT_IO` writes and `HOST` emulation have no per-file or per-session size cap. `NEW_LINE` can amplify this into a memory-exhaustion input. Add byte and operation limits.
- **Host execution depends on the server callback’s safety.** `host.rs` passes the complete command string to `HostExec`. The supplied code does not show how the callback tokenizes or enforces the allowlist. Test that shell metacharacters, arguments, and command chaining cannot bypass the allowlist, and keep execution disabled by default.
- **Sensitive data may enter trace logs.** `HOST` logs the raw command and command output; commands can contain credentials or returned data. Redact or avoid logging command contents and output, and test that secrets are absent from session state.
- **Timeouts do not terminate runaway parser or loader work.** The bundle says an overrun thread is abandoned, not killed. Repeated hostile inputs could continue consuming CPU and memory. Bound parser work internally and test that cancellation stops parsing and loading.
- **UI/API file access is not yet present.** When added, validate names, payload size, and access per session; never map client paths to real host paths.

## Prioritised round-3 task list

1. **Enforce bounded `TEXT_IO` operations**  
   **Files:** `crates/oracle-forms-runtime/src/textio.rs`, `crates/oracle-forms-runtime/src/host.rs`, `crates/oracle-forms-runtime/tests/runtime_gap_tests.rs`  
   **Acceptance tests:** a synthetic `NEW_LINE` at the maximum accepted count succeeds; the next count fails without allocation; writes exceeding the per-session cap fail; existing path traversal tests still pass.  
   **Size:** S

2. **Make navigation argument parsing signature-aware**  
   **Files:** `crates/oracle-forms-runtime/src/nav.rs`, `crates/oracle-forms-runtime/tests/runtime_gap_tests.rs`  
   **Acceptance tests:** valid modes in valid positions work for `CALL_FORM`, `OPEN_FORM`, and `NEW_FORM`; invalid, duplicate, and misplaced modes fail visibly; parameter lists are accepted only in their defined argument position.  
   **Size:** M

3. **Make navigation startup and session failures explicit**  
   **Files:** `crates/oracle-forms-server/src/form_app.rs`, `crates/oracle-forms-server/src/runtime_sessions.rs` (if needed), `crates/oracle-forms-server/src/runtime_sessions` tests  
   **Acceptance tests:** a synthetic child startup failure has a deterministic open/close outcome and visible caller error; a failing `SESSION` connection does not silently use the shared connection.  
   **Size:** M

4. **Implement `NEW_FORM` rollback and dirty-form handling**  
   **Files:** `crates/oracle-forms-runtime/src/nav.rs`, `crates/oracle-forms-server/src/form_app.rs`, runtime navigation tests  
   **Acceptance tests:** `TO_SAVEPOINT`, `NO_ROLLBACK`, and `FULL_ROLLBACK` produce distinct documented buffer outcomes; dirty-form behavior is tested for each mode, including user cancel if a prompt applies.  
   **Size:** L

5. **Handle unsaved changes when closing opened forms**  
   **Files:** `crates/oracle-forms-server/src/form_app.rs`, `crates/oracle-forms-server/src/runtime_sessions.rs`, server navigation tests  
   **Acceptance tests:** closing clean, dirty, and cancelled forms produces the Forms-compatible result; called forms remain closed through `EXIT_FORM`, not `CLOSE_FORM`.  
   **Size:** M

6. **Validate menu paths and render menu state**  
   **Files:** `crates/oracle-forms-runtime/src/ui_builtins.rs`, `ui/app.js`, `ui/runtime.js`, UI tests  
   **Acceptance tests:** a property update applies only to the fully qualified menu item; enabled, displayed, checked, label, and icon changes appear in the web menu; same-named items in different menus remain distinct.  
   **Size:** L

7. **Verify and correct timer lifecycle across form navigation**  
   **Files:** `crates/oracle-forms-runtime/src/timers.rs`, `crates/oracle-forms-server/src/form_app.rs`, runtime/server tests  
   **Acceptance tests:** synthetic tests establish one-shot `FIND_TIMER`/restart behavior and whether suspended callers’ timers fire; implemented outcomes match the recorded Forms 14c behavior.  
   **Size:** M

8. **Add browser-level tests for runtime interactions**  
   **Files:** `tests/ui_runtime.test.cjs`, `ui/app.js`, `ui/runtime.js`  
   **Acceptance tests:** a browser test covers opening and switching tabs, module preview switching, LOV selection/cancel, and timer polling without relying only on pure helper tests.  
   **Size:** M

9. **Implement record-group selection built-ins**  
   **Files:** `crates/oracle-forms-runtime/src/record_groups.rs`, `crates/oracle-forms-runtime/src/objects.rs`, runtime gap tests  
   **Acceptance tests:** selection can be set, queried, reset, and unset on synthetic groups; row-number reporting is stable for selected and unselected records.  
   **Size:** M

10. **Add bounded virtual-file transfer APIs**  
    **Files:** `crates/oracle-forms-server/src/runtime_sessions.rs`, UI file-transfer code, server/UI tests  
    **Acceptance tests:** synthetic upload and download are isolated by session, reject traversal and oversized payloads, and preserve bytes across `TEXT_IO` reads and writes.  
    **Size:** L

## Round 3 status

Verification result: every claim implemented below was first reproduced or confirmed by a test
(A1 `CALL_FORM('x','BOGUS')` reported a missing list; A2 silent shared SESSION; A3 failed child left
open; A5 unbounded `NEW_LINE`; A6 `OTHER.SAVE` accepted; B2/B3 no unsaved-change handling). Details
in [16](16-RUNTIME-GAP-CLOSURE.md) section "Round 3". Commit hashes are on branch
`feat/runtime-gap-closure`.

| Task (round 3 list) | Plan item | Status | Commit |
|---|---|---|---|
| 1. Bound TEXT_IO / HOST, no HOST text in trace under Execute | D, list 1 | done | bd063a0 |
| 2. Signature-aware navigation arguments | A1, list 2 | done | 27398eb |
| 3. Explicit navigation startup and SESSION failures | A2, A3, list 3 | done | 6693279 |
| 4. NEW_FORM / CLOSE_FORM unsaved changes | B2, B3, list 4-5 | done (savepoint semantics not modelled; Yes path checks completion, not a cross-session read-back) | 1ca80d0 |
| 5. Menu runtime basics | A6, list 6 | partial: path validation done; UI mapping covers the standard default-menu names only, custom menus have no DOM, roles/submenus/`DO_REPLACE` replacement open, not exercised in a browser | c80ded9 |
| 6. Record group selection built-ins | list 9 | done (14c details of `GET_GROUP_RECORD_NUMBER` unverified) | b63b02f, 5cd01ef |
| 7. Virtual file transfer API | list 10 | partial: server API and tests done, no UI file list | a4a9a71 |
| 8. Timer lifecycle across navigation | B4, B5, list 7 | pinned only (behaviour unverified against 14c, unchanged) | 16373c8 |

Not attempted in round 3 (outside the task list): A4 `GET_LINE`/`PUTF` argument validation, A7
`VISUAL_ATTRIBUTE` validation, B1 menu stack for `HIDE`/`DO_REPLACE`, B6 LOV filter/position, B7
`RUN_PRODUCT` and repeated instances, list item 8 (browser-level tests), parser/loader cancellation
and host allowlist tokenizer tests (D).
