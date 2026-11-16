# Batch manifest compilation (Stage 3)

Compiles what `converting-oracle-artifacts/scripts/build_dependency_manifest.py` staged and built in Stage 2 (see that skill's Batch dependency-ordered conversion reference), in the manifest's dependency order.

## 3-Stage Lifecycle Reference

Visual flow and component architecture: [batch-pipeline-stages.mmd](batch-pipeline-stages.mmd).

Across `build-manifest.json`, `build_dependency_manifest.py`, and `compile_from_manifest.py`:

1. **Stage 1: Decompiled (Text Source):** `*.xml` (`_fmb.xml`, `_mmb.xml`, `_olb.xml`) and `*.pld`.
2. **Stage 2: Built (Oracle 14c Native Source):** `.olb`, `.pll`, `.mmb`, `.fmb` — built in topological dependency order (least dependencies -> most dependencies) inside the 14c converter container with **no database logon (`logon=no`)**:
   - `.xml` -> `.olb` / `.mmb` / `.fmb` via `XML2Forms` (with `FORMS_PATH=/work:/oracle_home_14c/forms`).
   - `.pld` -> `.pll` via `frmcmp_batch module=<name>.pld output_file=<name>.pll module_type=library parse=yes logon=no batch=yes` (prefer `.pld` over existing `.pll` when `.pld` is present so every `.pll` is freshly built in 14c).
3. **Stage 3: Compiled (Runtime Executables):** `.plx`, `.mmx`, `.fmx` — compiled in tier (`OLB` → `PLL` → `MMB` → `FMB`) and dependency order via `compile_from_manifest.py` with user interactive database login and written straight into `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (outside `$ORACLE_HOME/forms`, which stays reserved for Oracle product binaries).

```bash
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_from_manifest.py \
  --build-dir <external-folder> [--app-name <app>] [--output-dir <dir>] [--types PLL] [--no-deps] [--skip-compiled] [--userid <user>@<db>] [--offline] [--no-publish] [--skip-preflight] [--use-local-env]
```

## Before you run Stage 3

Start `forms-server-14c` once, before Stage 1, and keep it running until Stage 3 has finished (`python3 start_testbed.py --status` shows whether it is Up; `python3 start_testbed.py` starts it). Stage 3 requires it. Stages 1 and 2 also run in it (`podman exec -u oracle forms-server-14c`), using a throwaway `oracle-forms-14c` container only as a fallback when it is not running; see `converting-oracle-artifacts/references/batch-dependency-conversion.md`, "Before you start".

## Interactive guided workflow (agent protocol)

**Gate 3, authenticated compile. The agent never runs `compile_from_manifest.py`, not even against `.env` or the local test database.** Stage 3 is for the user to run interactively in their own terminal against the target project's development database. The agent's work ends after Stage 2 (the 14c `.olb`/`.pll`/`.mmb`/`.fmb` binaries in `build/stage/`): it confirms the Stage 2 status in `build-manifest.json`, prints the exact command below for the user to run, and stops. The user is asked for nothing beforehand; the script itself prompts for the user, the alias and the password, and the password is never pasted in chat or stored. After the run it summarizes compiled, skipped and failed modules from `build-manifest.json`, diagnoses `PL/SQL ERROR 201` (object, grant or synonym not visible to the account) and `FRM-18108` (parent module missing from `FORMS_PATH`), and asks which compiled form to launch with `launch_fsal.py`.

## Login

The user logs in directly against the target database. The script always asks at the terminal (`input()` for user and connect alias, `getpass` for the password); the alias has no default. The repo's git-ignored `.env` holds local testbed credentials, so `DB_USER`/`DB_PASSWORD`/`DB_SERVICE` are used only when `--use-local-env` is passed explicitly. The password is never put on the command line or in a file, and is removed from printed output and stored diagnostics. `--userid user@db` still prompts for the password.

## Preflight

Before any prompt, the script checks that `forms-server-14c` is running (`podman inspect`, state must be `running`); if not it exits 2 with the actual state and asks you to start the stack, and no login is attempted. This check also applies with `--offline`, because compiling happens in the container.

Unless `--skip-preflight` or `--offline`, `sqlplus -L -S` runs inside `forms-server-14c` (`TNS_ADMIN` set, the logon sent on stdin) and counts `session_roles` and `all_objects` for the account. Failure stops the run (exit 2). A successful logon is **not** proof that the account sees every object the libraries use; a compile error `PL/SQL ERROR 201 ... must be declared` means it does not.

## Compile

For each module in tier (`PLL` → `MMB` → `FMB`) and manifest order: before compiling, `ensure_manifest_case_symlinks()` creates exact-case symlinks in `<build-dir>/stage/` and `<app_output_dir>` for any mixed-case or uppercase dependency names recorded in `build-manifest.json` (`parentModules`, `attachedLibraries`, `menus`, `objectLibraries`). `.olb` is skipped (provider only); `.pll` compiles with `module_type=library`, `.mmb` with `menu`, `.fmb` with `form` through `compile_form.build_compiler_command`, with `FORMS_PATH` = `<app_output_dir>:<build-dir>/stage:/oracle_home_14c/forms`. A module whose typed provider (`(artifactType, name)`) failed or was skipped is marked `skipped` (so a failed `FMB` never blocks dependents of a same-named `PLL`). Each result and diagnostics are written back to the same `build-manifest.json` (one JSON for the whole pipeline) as the module's `compiled` record (`done`, `failed` or `skipped`) plus `status`, saved atomically after every module. The other stages' records (`decompiled`, `validated`, `converted`) are left untouched.

## Publish

Compiled `.plx`, `.mmx`, `.fmx` are written by `frmcmp_batch` (`output_file=`) straight into `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (and registered ahead of `/oracle_home_14c/forms` in `default.env` `FORMS_PATH`); there is no separate copy step, and a compile overwrites a same-named file there at once. `/oracle_home_14c/forms` remains strictly for Oracle product binaries (`webutil.*`, `frmsal.jar`, etc.). With `--no-publish` they are written beside the module in `<build-dir>/stage/` instead. A failed compile does not write its output, but any older file of the same name in the app folder stays. `<build-dir>` and `--output-dir` must be under the Oracle Home or repo mount so the container can see them; otherwise the script says so.

## Not verified

Unit-tested with mocks only (order, credential prompt, skip-on-failure). No compile through this script against a database has been verified.

## Publishing and early checks

Publishing a `.mmx` or `.plx` also creates a case-variant symlink in `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` when the other case is absent, complementing the `requiredSymlinks` identified in Stage 2 and created by `ensure_manifest_case_symlinks()` ([linux-case-sensitivity-and-symlinks.md](linux-case-sensitivity-and-symlinks.md)). A `--build-dir` not visible to the container is rejected before any credential prompt; use a folder under `OFW_WORK_DIR`.

