---
name: compiling-and-running-oracle-forms
description: "Validates Forms, menu, library, and report XML against Oracle's schema, compiles Oracle Forms (.fmb -> .fmx) headlessly via frmcmp_batch, and runs forms live on macOS/Windows desktops via Forms Standalone Launcher (FSAL). Use when checking hand-written form XML before converting it, compiling forms, testing database SQL*Net binding, or running forms live."
---

# Compiling And Running Oracle Forms

Validate the XML you write, compile it in the Forms 14c container, and open the form with FSAL. Needs the testbed from the `setting-up-oracle-testbed` skill running (containers up, `.env` present). On Windows run everything from the Ubuntu WSL shell.

## Contents

- [Constraints](#constraints)
- [Quick start](#quick-start)
- [Workflow](#workflow)
- [Scripts and assets](#scripts-and-assets)
- [Interactive guided workflow (agent protocol)](#interactive-guided-workflow-agent-protocol)
- [Verification](#verification)

## Constraints

1. Validate XML **before** converting it to a binary module: a schema check takes seconds, a failed XML2Forms or compile takes minutes ([references/xml-validation.md](references/xml-validation.md)).
2. Compile with database logon for PL/SQL that references schema objects; `--offline` (`logon=no`) can fail on unresolved objects. Credentials come from the git-ignored `.env`, never from the command line or docs.
3. FSAL needs Java 17 or 21 (WSL's OpenJDK 21 works; Windows Java 26 fails with `java/applet/AppletStub`).
4. Never test by clicking Save & launch in the Rust app (it rewrites the form's XML); use `launch_fsal.py`.
5. A schema pass means "schema-valid", not "will compile"; a compile or launch success is a pipeline result, not visual or behavioral parity.

## Quick start

From the repo root (paths below are relative to this skill's `scripts/` folder):

```bash
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/validate_forms_xml.py samples/ofw_demo_employees_fmb.xml
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py samples/test_db.fmb
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py ofw_demo_employees.fmx --interactive
```

## Workflow

1. **Validate** each XML (form, menu, object library, report): `validate_forms_xml.py <files>`. Pick the schema with `--version 14c|auto|<name>` or `--schema FILE`. Fix errors using the table in `references/xml-validation.md`.
2. **Convert** XML to a binary module with XML2Forms (see the `converting-oracle-artifacts` skill), then **compile** it: `compile_form.py <file.fmb>` (`--type menu`, `--offline`). It checks that `forms-server-14c` is running, then prompts for user, alias and a hidden password; `-u user@alias` pre-fills them and `--use-local-env` uses the git-ignored `.env` (local testbed only). Same behavior as `compile_from_manifest.py`.
3. **Run** it: `run_form.py <file>` does convert (if XML), compile (if FMB), and launch in one step (`--compile-from-env --interactive`, `--offline`), or `launch_fsal.py <form>.fmx --interactive` for an already compiled form.
4. Confirm the servlet first: `python3 start_testbed.py --status` (port 7001).

## Scripts and assets

| File | Purpose |
|---|---|
| `scripts/validate_forms_xml.py`, `scripts/ValidateXml.java` | Schema validation with xmllint, lxml, or Java in a container; `--version`, `--schema`, `--list-versions` |
| `scripts/compile_form.py` | `frmcmp_batch` in the Forms container with TNS binding and error parsing (form, menu, library) |
| `scripts/run_form.py` | Prepare, convert, compile, and launch FSAL in one step; repeat `--param NAME=VALUE` to pass URL-encoded Forms module parameters |
| `scripts/launch_fsal.py` | Open a compiled form through the Forms servlet with FSAL; repeat `--param NAME=VALUE` to add URL-encoded parameters |
| `scripts/compile_from_manifest.py` | Batch: compiles a `build-manifest.json` (from the converting skill) in tier (`OLB` → `PLL` → `MMB` → `FMB`) and dependency order; prompts for DB login (`input`/`getpass`), sqlplus preflight, compiles straight into `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (and registers it in `default.env` ahead of `/oracle_home_14c/forms`). Optional filters: `--app-name`, `--output-dir`, `--types PLL` (or `PLL,MMB`), `--no-deps`, `--skip-compiled`, `--offline`, `--no-publish` (compile into `stage/` instead), `--skip-preflight`, `--use-local-env` (local testbed `.env` login; off by default). **The agent never runs this script:** the user runs it in their own terminal (see the Stage 3 rule in the batch reference). |
| `references/batch-manifest-compilation.md` | Stage 3: start `forms-server-14c` once before Stage 1 and keep it running; interactive login, preflight, ordered compile, publish |
| `references/batch-pipeline-stages.mmd` | Visual 3-stage lifecycle flowchart: scripts, containers, ledger annotations, and outputs |
| `references/xml-validation.md` | Forms 14c XML Schema rules and common fixups |
| `references/forms-path-and-dependencies.md` | Where to place `.pll`/`.plx`/`.mmx`/`.olb`/`.fmx` dependencies on `FORMS_PATH` (WSL & macOS) and how to target another dev DB |
| `references/linux-case-sensitivity-and-symlinks.md` | Why Oracle Forms on Linux needs uppercase symlinks (`MYLIB.plx -> mylib.plx`), how `add_case_variant()` creates them, and how to check for broken symlinks |
| `references/inheriting-forms-and-xml-roundtrip.md` | Extract to XML, compare versions, compile with parent forms and libraries in one folder, run with form parameters, diagnose inside a form, known setup-only messages |
| `assets/forms-14c.xsd`, `assets/reports.dtd` | Oracle's Forms 14.1.2 XML Schema and the Reports DTD (script inputs, not for reading whole) |
| `assets/12c/forms.xsd` | The owner's 12c Forms schema, dropped here; **not yet wired** as a `12c` version, so `--version 12c` fails until it is |

## Interactive guided workflow (agent protocol)

The agent drives the gates; the user never guesses the next step. Gates 0 to 2 are in `converting-oracle-artifacts` (intake, analysis, conversion). **Gate 3, compile:** ask for the database `user@db`, whether to run the `sqlplus` preflight (recommended) and whether to publish to `<Oracle Home>/forms/`; give the exact `compile_from_manifest.py` command so the user types the password at the `getpass` prompt (never in chat); then summarize compiled, skipped and failed, diagnose and ask which form to launch with `launch_fsal.py`. Detail: [references/batch-manifest-compilation.md](references/batch-manifest-compilation.md).

## Dependencies, account and compile order

- **The database account decides whether a compile works.** A library that uses database objects by bare name compiles with an account that can see them and fails with `PL/SQL ERROR 201 ... must be declared` for an account that cannot, with the same file in the same folder. A successful logon is not proof of visibility. Check with `all_objects` and `session_roles` as that account.
- **Module type auto-detection:** `compile_form.py` infers `--type` (`library`, `menu`, `form`) from `.pll`, `.mmb`, or `.fmb` extensions automatically (pass `--type` to override). Compiling a library with `--type form` gives `FRM-10043`.
- **Order:** modules with no attachments first, then those that attach them, then menus and forms, all in one staged folder in `OFW_WORK_DIR` (outside the repository).
- **Compile folder is not run folder:** compiled `.plx`, `.mmx` and `.fmx` publish to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (which is prepended ahead of `/oracle_home_14c/forms` in `default.env`), keeping `/oracle_home_14c/forms` strictly for Oracle product binaries.
- **Cross-check in desktop Forms Builder** with the same account when a compile fails.
- **Forms that inherit from other forms:** compile the original `.fmb` with its parent `.fmb` files, libraries, menus and object libraries in the same folder; do not round-trip through XML ([references/inheriting-forms-and-xml-roundtrip.md](references/inheriting-forms-and-xml-roundtrip.md)).
- URL parameter construction is tested locally, but delivery into a running Forms module still requires live verification. Test both a direct launch and the application's intended entry/navigation flow when it initializes runtime state.

Full evidence and what is not verified: `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.

## Where files go

Nothing is staged, compiled or converted inside the repository. `OFW_WORK_DIR` (default `<Oracle Home>/ofw-work`, created on first use or with `compile_form.py --init-work-dir`) holds staged inputs under `stage/`, while compiled runtime outputs publish to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (default `default`; override with `--app-name`, `--output-dir`, or `OFW_APP_NAME`). `compile_form.py`, `run_form.py`, and `compile_from_manifest.py` register the app directory ahead of `/oracle_home_14c/forms` in `default.env`. Use `--no-publish` to keep outputs in `stage/`. Not yet verified end to end after this change (no compile run against a database).

## Verification

```bash
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/validate_forms_xml.py samples/*.xml    # 9 pass, 13 fail on 2026-11-14 (older 12c-style fixtures)
```

Verified in WSL: the validator with xmllint, `compile_form.py` (`test_db.fmb`, `ofw_sales.fmb`), `launch_fsal.py`. **Not verified:** the validator's `java` backend, and `--version 12c`. Eval cases: `evals/evals.json`.
