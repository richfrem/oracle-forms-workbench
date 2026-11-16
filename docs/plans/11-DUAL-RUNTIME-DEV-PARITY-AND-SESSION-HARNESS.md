# 11: Dual-Runtime Legacy Application Parity & Dynamic Session Harness

> **Status:** Proposed post-G architecture. Track 1's dependency-ordered batch pipeline and live JNLP/WebStart entry-to-main-form launch have been exercised in `forms-server-14c`; Track 2 (native Rust session pinning and multi-form transitions) and cross-engine parity remain unimplemented.

## 1. Vision and goal

Enable a developer or QA analyst to connect either the local Podman Forms 14c server
(`forms-server-14c` with FSAL) or the native Rust workbench (`oracle-forms-server` with
`oracle-thin`) to a shared development database (`<dev-db>`), then exercise legacy 12c
application flows with comparable data, trigger, and security behavior.

The two engines make independent connections to the same database. A meaningful comparison
must establish equivalent users, enabled roles, application context, startup state, input
parameters, and data. A successful connection, compile, or isolated form launch is not evidence
of behavioral parity. The application's login and navigation sequence is part of the test.

## 2. Why entry-form initialization matters

Some legacy applications establish authorization and session state during sign-in rather than
when a target form is opened. Their startup flow may activate granted roles using
`SET ROLE ... IDENTIFIED BY ...` or `DBMS_SESSION.SET_ROLE`, invoke custom PL/SQL logon
packages, initialize application contexts consumed through `SYS_CONTEXT`, and set
`:GLOBAL.*` values. Directly launching a protected form can therefore fail or behave
differently even when the database credentials are valid.

Role activation is not arbitrary user impersonation: the database must grant the role, and
switching users may require a new authenticated connection or a database-approved proxy
mechanism. The harness must report the effective session state rather than assume that a
requested identity or role was applied.

## 3. Session Initialization Contract

Both tracks must make their initialization steps visible and repeatable. A comparison record
should capture the target module, test-user profile identifier, requested and effective roles,
application context, parameter values, and initialization outcome without recording secrets.

### Track 1: Oracle Forms 14c container

1. Resolve the configured `<dev-db>` connection from inside `forms-server-14c`; verify
   `TNS_ADMIN` or Easy Connect resolution in that same container environment.
2. Discover and stage the required `.olb`, `.pll`, `.mmb`, and parent `.fmb` dependencies,
   convert or compile them in dependency order (with Linux case-sensitivity symlinks), and
   publish executable modules to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (keeping
   `<OFW_ORACLE_HOME>/forms/` clean for Oracle 14c product binaries).
3. Start the application's login or entry form (for example via WebStart/JNLP with `otherparams`,
   `envFile`, and a `<component-desc/>` extension JNLP for custom client JARs) and run its
   established authentication, role-provisioning, context, and global-initialization sequence
   before navigating to the target form.
4. Record effective database identity, enabled roles, and relevant context using approved,
   non-secret diagnostics. A successful compile or servlet response alone is not a pass.

### Track 2: Native Rust runtime

1. Create a runtime session connected to the same `<dev-db>` and test-user profile.
2. Run the application's startup triggers, including `WHEN-NEW-FORM-INSTANCE`, and invoke
   approved stored PL/SQL role-activation or context-initialization hooks using the session's
   database connection.
3. Pin each runtime session's database work to one persistent `OracleThinDriver` connection
   (or equivalent connection lease) so enabled roles, temporary-table state, and package
   state survive queries and DML within that session.
4. Preserve `:GLOBAL.*` and `:PARAMETER.*` values through multi-form transitions, including
   `CALL_FORM`, `OPEN_FORM`, and `NEW_FORM`.
5. Expose effective session diagnostics without exposing passwords, tokens, or other secrets.

The persistent-connection behavior and multi-form state contract are future requirements;
they are not claims about the current connection-pool or runtime implementation.

## 4. Phase H / post-G milestones

### H.1 — Multi-form session stack

Implement `CALL_FORM`, `OPEN_FORM`, `NEW_FORM`, and `EXIT_FORM` in
`oracle-forms-runtime` and `oracle-forms-server`. Maintain shared `:GLOBAL.*` state,
`PARAMLIST` values, return behavior, and active-form stack switching. Define transitions and
failure behavior before wiring the launcher to them.

**Acceptance:** deterministic tests cover nesting, return, replacement, parameter passing,
global-state visibility, and cleanup when a transition fails.

### H.2 — Session role and context diagnostics

Expose approved database session state (`USER`, `SESSION_ROLES`, and selected
`SYS_CONTEXT('USERENV', ...)` values) in the runtime session inspector and
`oracle-forms-cli`. Clearly distinguish requested state from state returned by the database.

**Acceptance:** diagnostics work for the active session connection, redact secrets, and fail
visibly when the database does not permit a requested inspection.

### H.3 — Pluggable legacy authentication and role hook

Define a generic, allowlisted hook configuration outside the repository, such as in
`OFW_WORK_DIR` or ignored environment configuration. It may identify approved stored PL/SQL
entry points and non-secret parameters; credentials remain supplied through the established
secret configuration mechanism. Do not commit project-specific login logic or arbitrary SQL.

**Acceptance:** an operator can configure, invoke, and diagnose a test hook without changing
tracked source files, and invalid or unauthorized hooks fail closed.

### H.4 — Dual-runtime parity verification

Provide a side-by-side checklist for Forms 14c FSAL and native Rust using the same development
user profile, effective role set, application context, entry flow, and target form. Compare
record data, trigger outcomes, navigation, and relevant runtime state; record differences
instead of treating compile or startup success as parity.

**Acceptance:** each comparison records its preconditions, evidence, and unresolved differences,
and does not claim coverage beyond the exercised scenario.

## 5. Security and scope

This plan is limited to authorized local development and test environments. Database grants,
proxy authentication, application-context policies, and test-user provisioning remain owned by
the database/application administrators. Never bypass production authorization, persist test
passwords in tracked files, or include external project configuration, schema details, or logs
in repository documentation.
