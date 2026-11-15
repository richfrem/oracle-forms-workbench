# 12: Test User Sign-In/Sign-Out, Role Impersonation & Dynamic Form Launcher

> **Status:** Proposed reusable test utility. No launcher artifact or runtime behavior is
> implemented or verified by this specification.

## 1. Purpose

Specify a synthetic utility form, `samples/ofw_session_launcher_fmb.xml`, its generated
`.fmb` artifact, and companion Rust UI controls for exercising authorized test users and
application roles in both Oracle Forms 14c FSAL and the native Rust workbench.

The launcher is a development/test harness, not an authentication product or a way to bypass
database authorization. It should make session setup, effective role state, and form navigation
observable while keeping engine-specific execution boundaries explicit.

## 2. Core capabilities

### Sign-in, sign-out, and user switching

- Authenticate to the configured development database with an explicitly selected test-user
  profile.
- Make the entry-form authentication and initialization sequence available before launching a
  target form.
- On sign-out, use Forms `LOGOUT` or reset/close the database session as appropriate, clear
  `:GLOBAL.*` and `:PARAMETER.*` state, discard cached blocks and form-stack state, and release
  session resources.
- Switching to a different database user requires re-authentication or an explicitly approved
  proxy-authentication flow; `SET ROLE` changes enabled roles, not the authenticated user.
  Keep the workbench server process running while replacing the application session.

### Test-user and role selection

- Select an approved test-user profile and one or more roles from configured choices, with
  manual entry limited by the administrator's allowlist policy.
- Support generic profiles such as standard user, read-only auditor, and elevated or
  superuser/admin-equivalent test role. Any elevated role is restricted to an isolated
  development/test database.
- Invoke configured role activation, proxy-authentication, or application-context hooks. A
  requested role or identity is not treated as effective until confirmed by database
  diagnostics.
- Display the effective `SESSION_ROLES` and selected session context on the launcher.

Role activation may use approved `ALTER SESSION`, `DBMS_SESSION.SET_ROLE`, `SET ROLE`, or a
custom PL/SQL context switcher. These are privileged operations: never expose arbitrary SQL
execution through a free-form UI field. Use named, parameterized hooks and fail closed if a
hook is absent, rejected, or unauthorized.

### Dynamic form selector

- Populate a selector or LOV from an explicitly configured forms directory
  (`OFW_FORMS_PATH_EXTRA`) or an approved session catalog.
- Treat discovered external form files as read-only. Stage conversions and compile outputs in
  the configured `OFW_WORK_DIR`; do not write external project artifacts into the repository.
- Launch a selected module with optional parameters through `CALL_FORM` or `OPEN_FORM`.
  Preserve the active database session, effective roles, approved application context, and
  `:GLOBAL.*` state; return to the launcher on `EXIT_FORM` where the chosen transition allows it.
- Show actionable errors for missing dependencies, invalid parameters, compilation failures,
  and rejected session initialization.

## 3. Dual-engine implementation design

### Oracle Forms 14c artifact

Author a standard, Forms 14c-schema-valid XML module at
`samples/ofw_session_launcher_fmb.xml`. The proposed form includes:

- Connection and test-user profile controls.
- Role and approved impersonation-hook controls.
- An active-session inspector for effective identity, `SESSION_ROLES`, and selected context.
- A form selector populated from configured sources or an approved catalog.
- `CALL_FORM` / `OPEN_FORM` actions with optional module parameters and an explicit return
  path.

Validate the XML against the repository's Forms 14c schema, convert and compile it through
`compile_form.py`, and run it through FSAL. Keep generated binaries in the designated build
output unless a reviewed fixture policy explicitly requires tracking them.

### Native Rust runtime support

The launcher depends on runtime behavior that must be implemented and tested before parity is
claimed:

- Dynamic list population, including `LIST_ITEMS` and `ADD_LIST_ELEMENT` where required.
- Multi-form transitions: `CALL_FORM`, `OPEN_FORM`, `NEW_FORM`, and `EXIT_FORM`.
- Parameter-list and `:GLOBAL.*` / `:PARAMETER.*` state propagation.
- Approved role/context calls on the session-pinned database connection.
- Accurate `GET_APPLICATION_PROPERTY` behavior for properties used by the launcher.

`FORMS_DDL` and arbitrary role-changing SQL must not become unrestricted capabilities. Prefer
named, allowlisted server-side hooks with validation and explicit development-only enablement.
Unsupported or partially implemented built-ins must be surfaced rather than silently treated as
successful.

## 4. Security guardrails

- Restrict user switching, elevated roles, and role/context hooks to local development/testbed
  use and authorized test accounts.
- Never store test passwords, tokens, or external database connection strings in source,
  samples, saved form XML, browser storage, reports, or logs.
- Keep secrets in the established ignored configuration or secret provider; redact them from
  diagnostics and errors.
- Do not infer authorization from a selected profile. Verify the resulting database identity,
  enabled roles, and application context.
- Require administrator-approved role and hook allowlists; do not allow arbitrary SQL or
  production impersonation.
- Ensure sign-out invalidates the old connection and clears all application/session state before
  another user is authenticated.

## 5. Verification outline

Test sign-in and sign-out, user reconnection, role activation and denial, context initialization,
dynamic list population, parameter passing, form return behavior, and state cleanup. Exercise
the same approved scenario in FSAL and the native runtime against the same development database.
Capture differences and failure states; a successful login or form launch alone is not a parity
result. See [11: Dual-Runtime Legacy Application Parity & Dynamic Session Harness](11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md)
for the broader session contract and Phase H milestones.
