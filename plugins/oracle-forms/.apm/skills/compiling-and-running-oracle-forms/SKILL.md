---
name: compiling-and-running-oracle-forms
description: "Validates Forms, menu, library, and report XML against Oracle's schema, compiles Oracle Forms (.fmb -> .fmx) headlessly via frmcmp_batch, and runs forms live on macOS/Windows desktops via Forms Standalone Launcher (FSAL). Use when checking hand-written form XML before converting it, compiling forms, testing database SQL*Net binding, or running forms live."
---

# Compiling And Running Oracle Forms

Validate XML, compile in the Forms 14c container (`forms-server-14c`), and run with FSAL. Needs the `setting-up-oracle-testbed` containers up and `.env` present. On Windows run from the Ubuntu WSL shell. End-to-end diagram: [references/compiling-and-running-oracle-forms-flow.mmd](references/compiling-and-running-oracle-forms-flow.mmd).

## Contents

- [Constraints](#constraints)
- [Quick start](#quick-start)
- [Workflow](#workflow)
- [Scripts and assets](#scripts-and-assets)
- [Interactive guided workflow (agent protocol)](#interactive-guided-workflow-agent-protocol)
- [Dependencies, account and compile order](#dependencies-account-and-compile-order)
- [Verification](#verification)

## Constraints

1. Validate XML **before** XML2Forms/compile ([references/xml-validation.md](references/xml-validation.md)).
2. Use database-connected compile for SQL-referencing PL/SQL (`--offline` can fail on DB objects). Never put passwords in commands or docs.
3. FSAL needs Java 17 or 21 (WSL OpenJDK 21 works; Windows Java 26 fails with `java/applet/AppletStub`).
4. Never test via **Build & Compile (14c)** in the Rust app (it rewrites the form's XML); use `launch_fsal.py` or staged copies in `OFW_WORK_DIR`.
5. Schema/compile/launch success is a pipeline check, not visual or behavioral parity.
6. A black canvas with invisible prompts after compiling from XML is a conversion problem, not look-and-feel: inherited canvas colours were lost by `XML2Forms`. Fix in the staged XML and reconvert (`setting-up-oracle-testbed/references/troubleshooting.md`); `lookAndFeel`/`colorScheme` do not help.

## Quick start

```bash
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/validate_forms_xml.py samples/ofw_demo_employees_fmb.xml
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py samples/test_db.fmb
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py ofw_demo_employees.fmx --interactive
```

## Workflow

1. **Validate** XML: `validate_forms_xml.py <files>` (`--version 14c|auto|<name>` or `--schema FILE`).
2. **Build Stage 2** (`XML2Forms`, see `converting-oracle-artifacts`), then **Compile Stage 3** against local `FREEPDB1` or a dev DB (`compile_form.py <file> -u user@target_db` or `compile_from_manifest.py`).
3. **Run** via `run_form.py <file>` (convert + compile + FSAL; `--compile-from-env --interactive`, `--offline`, `--param NAME=VALUE`) or `launch_fsal.py <form>.fmx --interactive`.
4. Confirm servlet first: `python3 start_testbed.py --status` (port 7001).

## Scripts and assets

| File | Purpose |
|---|---|
| `scripts/validate_forms_xml.py`, `scripts/ValidateXml.java` | XSD/DTD check via xmllint, lxml, or container Java (`--version`, `--schema`, `--list-versions`) |
| `scripts/compile_form.py` | Headless `frmcmp_batch` compile-only (`.fmb`, `.mmb`, `.pll`; `-u user@db`, `--use-local-env`, `--offline`) |
| `scripts/run_form.py` | Stage, XML2Forms, `frmcmp_batch`, publish, and launch FSAL (`--no-launch`, `--param NAME=VALUE`, `--app-name`, `--config`; `--source-dir DIR` builds the form with its recursive dependencies in waves) |
| `scripts/launch_fsal.py` | Launch deployed `.fmx` via FSAL (`--param NAME=VALUE`, `--interactive`) |
| `scripts/compile_from_manifest.py` | Batch Stage 3 wave by wave, incremental (unchanged modules skipped; `--rebuild-all`), `sqlplus -L` preflight, publishes to `forms_apps/<app_name>/` (**user runs in terminal, never the agent**) |
| `references/rust-vs-14c-build-and-preview.md`, `references/compiling-and-running-oracle-forms-flow.mmd` | Full local-to-dev-DB dev flow: Rust inner loop, Web App `Target database` field, compile-only gap, `RosBinaryWriter` vs `XML2Forms` |
| `references/batch-manifest-compilation.md`, `references/batch-pipeline-stages.mmd` | Stage 0–3 manifest compile protocol and visual pipeline |
| `references/forms-path-and-dependencies.md`, `references/linux-case-sensitivity-and-symlinks.md` | Staging external sources, container `TNS_ADMIN`/`sqlplus` checks, case symlinks, `forms_apps/<app_name>/` |
| `references/inheriting-forms-and-xml-roundtrip.md`, `references/xml-validation.md` | Inherited parent `.fmb` rules, XSD rules, 14c XML fixups |
| `references/jnlp-and-formsweb-launch.md`, `assets/sample_local.jnlp`, `assets/formsweb.cfg.template` | Sanitized generic `.jnlp` & security-annotated `formsweb.cfg` templates (FSAL vs. Web Start) |
| `assets/forms-14c.xsd`, `assets/reports.dtd`, `assets/12c/forms.xsd` | 14.1.2 XSD & Reports DTD (12c XSD dropped, `--version 12c` not yet wired) |

## Interactive guided workflow (agent protocol)

Gates 0–2 live in `converting-oracle-artifacts`. **Gate 3, compile:** ask for target `user@db`, `sqlplus` preflight, and `--app-name`; print the exact `compile_from_manifest.py` command so the user enters the password at `getpass` (never in chat); summarize compiled/skipped/failed and offer `launch_fsal.py`. Details: [references/batch-manifest-compilation.md](references/batch-manifest-compilation.md).

## Dependencies, account and compile order

- Compile account visibility (`all_objects`, `session_roles`, direct grants) decides whether SQL resolves (`PL/SQL ERROR 201`).
- Compile order: providers (`OLB`/`PLL` without attachments) → dependent `PLL` → `MMB` → `FMB`, staged in `OFW_WORK_DIR/stage/` (never in-repo) and published to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` ahead of `/oracle_home_14c/forms`. Details: `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.

## Verification

Verified in WSL: `validate_forms_xml.py` (`xmllint`), `compile_form.py`, `launch_fsal.py`; verified on macOS: `22/22` Stage 1, `18/18` Stage 2, `17/17` Stage 3 sample builds. **Not verified:** `java` validator backend, `--version 12c`, live `compile_from_manifest.py` DB run. Criteria: [acceptance-criteria.md](references/acceptance-criteria.md); evals: `evals/evals.json`.
