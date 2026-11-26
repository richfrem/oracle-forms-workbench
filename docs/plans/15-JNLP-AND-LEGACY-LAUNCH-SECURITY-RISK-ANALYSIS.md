# 15: JNLP & Legacy Oracle Forms Launch Security Risk Analysis

> **Status:** Proposed security analysis and hardening track (complements [Plan 11](11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md) and [Plan 12](12-ROLE-IMPERSONATION-AND-FORM-LAUNCHER-SPEC.md)).

---

## 1. Context & Repository Audit Finding (No `.jnlp` Example in Repo)

When launching Oracle Forms 14c desktop clients locally or against a remote legacy development environment, two client vectors appear across this repository's documentation and plans:

1. **FSAL (`frmsal.jar`)**: Used by [`launch_fsal.py`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py) (`config=standaloneapp` on Java 17/21, default `--interactive` logon prompt).
2. **Java Web Start / `.jnlp`**: Described in [Plan 11](11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md), [Plan 12](12-ROLE-IMPERSONATION-AND-FORM-LAUNCHER-SPEC.md), [Plan 06](06-IMPLEMENTATION-ROADMAP-AND-MILESTONES.md), and [Plan 10](10-NEXT-PHASE-SESSION-PROMPT.md) for legacy apps requiring `<component-desc/>` custom client JAR extensions, `webstart=enabled`, `allowStaticHTML=true`, `otherparams`, and `envFile`.

> [!NOTE]
> **Repository Status — Sanitized Generic `.jnlp` & `formsweb.cfg` Templates Added (`S15.1`):**
> An initial audit confirmed **zero `.jnlp` files or templates** existed in tracked repository files or Git history. A sanitized, generic localhost `.jnlp` example ([`sample_local.jnlp`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/sample_local.jnlp)) and security-annotated `formsweb.cfg` template ([`formsweb.cfg.template`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/formsweb.cfg.template)) have now been added under `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/` and documented in [`jnlp-and-formsweb-launch.md`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/references/jnlp-and-formsweb-launch.md), with all project-specific identifiers stripped per `.agent/rules/cross-project-isolation.md`.

---

## 2. Why JNLP / Web Start Is a Weak Security Vector

Even when used only against a development or test Oracle Forms 14c server, JNLP introduces meaningful architectural weaknesses:

| Risk ID | Risk Category | Technical Mechanism | Dev / Staging Impact |
|---|---|---|---|
| **JNLP-R1** | **Unrestricted Thick-Client Execution (`<all-permissions/>`)** | `.jnlp` XML dynamically directs the workstation launcher (`javaws` / OpenWebStart) to download remote JARs and execute them with `<all-permissions/>` (required by WebUtil and most enterprise Forms JAR extensions), bypassing JVM sandbox isolation. | Compromised server config, extension JAR tampering, or unpinned `<component-desc/>` JARs yield direct code execution on the developer/QA workstation. |
| **JNLP-R2** | **EOL Runtime & Trust Relaxation** | Oracle removed Java Web Start after JDK 8. Users rely either on unsupported Java 8 runtimes or third-party launchers (OpenWebStart), frequently accompanied by self-signed JAR certs or weakened certificate revocation/mixed-code checks. | Workstation JVM exposure to known client CVEs and weak certificate verification hygiene. |
| **JNLP-R3** | **Cleartext Transport, Argument Tampering & Disk Caching** | Downloaded `.jnlp` descriptors are plain XML cached in browser `Downloads` or Web Start cache directories. `codebase`, `serverURL`, `otherparams`, `envFile`, and query parameters can be inspected, modified, and replayed—especially over plain HTTP (`:7001`). | Exposed internal hostnames, session hints (`ifsessid`), environment/config names, or database credentials if `userid=` is ever passed via URL/JNLP parameters. |
| **JNLP-R4** | **Permissive `formsweb.cfg` Override Surface** | Enabling `webstart=enabled`, `webstartRequest=true`, and `allowStaticHTML=true` without strict `restrictedURLparams` allows callers to inject `form=`, `otherparams=`, `envFile=`, or alternative config sections via launch URLs. | Lateral movement between modules or environment profiles (`FORMS_PATH`, database aliases) beyond the intended entry form. |
| **JNLP-R5** | **Session / Role State Passed Over Client Arguments** | Legacy entry-to-target form flows that pass identity, role profile hints, or `:GLOBAL.*` initialization flags via JNLP launch parameters expose authorization inputs to client-side manipulation before DB role activation (`Plan 11` / `Plan 12`). | Broken assumptions about client-asserted identity unless all authorization stays enforced inside DB grants/PL/SQL hooks. |

### Localhost vs. Remote Dev Server Boundary

- **Localhost Podman Testbed (`127.0.0.1` / loopback only) with `--interactive`:** Risk is contained to the local single-user host when credentials are typed only at the native Logon prompt and services bind to loopback.
- **Non-Local Dev / Staging Network Endpoints:** Serving `.jnlp` over plain HTTP, caching launch XML containing operational endpoints/parameters, using self-signed JARs without CA trust, or running EOL Java 8 introduces active transport and workstation-execution risks.

---

## 3. Planned Analysis & Hardening Work Packages

### S15.1 — Author a Synthetic, Annotated `.jnlp` + `formsweb.cfg` Reference Example (Non-Project) — **Completed**
- Added sanitized generic templates [`sample_local.jnlp`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/sample_local.jnlp) and [`formsweb.cfg.template`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/formsweb.cfg.template) under `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/` using generic placeholders (`orders`, `orders_entry.fmx`, `app_ext.jnlp`) per `.agent/rules/cross-project-isolation.md`, paired with explicit **local-testbed-only** warnings and hardening annotations.
- Documented `formsweb.cfg` parameters (`webstart=enabled`, `restrictedURLparams`, `allowStaticHTML=false`, `envFile`, `FORMS_PATH` isolation) and JNLP vs. FSAL guidance in [`jnlp-and-formsweb-launch.md`](../../plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/references/jnlp-and-formsweb-launch.md).

### S15.2 — Client Vector Threat Audit: JNLP vs. FSAL (`frmsal.jar`) vs. Native Rust Web UI
- Evaluate where target 14c apps actually require WebStart/JNLP vs. **FSAL (`frmsal.jar`)** on patched Java 17/21 (including handling custom client JAR extensions without WebStart where supported).
- Document the migration path for removing thick-client Java execution altogether via the **Rust Web Workbench (`Plan 11 Track 2`)**, where browser TLS + server-side `OracleThinDriver` sessions eliminate workstation JAR downloads and JNLP descriptors.

### S15.3 — Concrete Hardening Controls Checklist for Non-Local 14c Servers
- Produce an operational security checklist covering:
  1. **Transport:** Require HTTPS (`https://` `codebase` and servlet endpoints) for any non-loopback 14c server; never transmit `.jnlp` descriptors or `frmservlet` traffic over HTTP across a network.
  2. **Credential Hygiene:** Ban `userid=` in URLs, JNLP descriptors, and `formsweb.cfg`; enforce interactive logon or SSO/OAM.
  3. **`formsweb.cfg` Lockdown:** Enforce `restrictedURLparams` (`envFile`, `userid`, `serverURL`, `otherparams` restrictions as appropriate), disable `allowStaticHTML=true` if unneeded, and segregate `[<app_name>]` sections cleanly.
  4. **Code Signing & Runtime Hygiene:** Sign custom JARs with an organization CA (never untrusted/expired self-signed certs or disabled revocation checks), separate custom JAR signing from Oracle's `frmall.jar` / `frmwebutil.jar`, and document cache-control / cleanup guidance for downloaded `.jnlp` artifacts.

### S15.4 — Audit Existing Repo Launchers (`launch_fsal.py` & `oracle_preview.rs`)
- Verify `launch_fsal.py`, `run_form.py`, and `crates/oracle-forms-server/src/oracle_preview.rs` never emit secrets into URLs/logs/staged descriptors, clearly distinguish loopback testbed defaults from remote dev connections, and reject malformed launch parameters (already partially covered by `parse_launch_params_validates_key_value_pairs` in `oracle_preview.rs`).

---

## 4. Acceptance Criteria

- [x] Explicit decision recorded on whether to commit a sanitized generic `.jnlp` example template (confirming zero external project material per `.agent/rules/cross-project-isolation.md`).
- [ ] Written risk matrix and hardening checklist covering JNLP (`R1`–`R5`), FSAL (`frmsal.jar`), and `formsweb.cfg` controls.
- [ ] Verified audit of `launch_fsal.py`, `run_form.py`, and `oracle_preview.rs` confirming zero secret leakage into URLs, CLI process arguments, or launch XML.
- [ ] Clear guidance mapping when legacy forms can stay on FSAL vs. when migrating execution to Plan 11 Track 2 (native Rust Web UI) removes the thick-client vector entirely.
