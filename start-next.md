# Start here next

Read [AGENTS.md](AGENTS.md) and every file in [.agent/rules/](.agent/rules/) first. After every step below, update the documentation before the next step (change log: `plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/windows-wsl-change-log.md`), script anything done by hand, and report only what was verified. Run everything inside WSL2. Do not remove or replace the owner's containers or images, and say what will happen before any image rebuild.

## Current open work

1. **Track K golden traces**: build deterministic trace capture from Oracle Forms 14c and compare it with native runtime sessions. Full golden-trace parity is not verified.
2. **Multi-module dependency staging**: extend single-form parent staging into complete dependency discovery and ordered batch conversion/compilation.
3. **External database validation**: verify Forms-container connectivity, `TNS_ADMIN` resolution, and compiler-account object/role visibility. No external database was tested for this status update.
4. **UI parity**: continue font persistence, theme/zoom/DPI comparison, tab/overlay behavior, and runtime verification. Compilation alone does not establish parity.

The runtime session REST API at `/api/sessions` is implemented. The current standard workbench URL is `http://127.0.0.1:7070`. For detailed status and the copy-paste continuation prompt, see [docs/plans/10-NEXT-PHASE-SESSION-PROMPT.md](docs/plans/10-NEXT-PHASE-SESSION-PROMPT.md).

## Proposed post-G design

Phase H is a proposed direction, not implemented work: establish equivalent login, role, context,
and global-state initialization before comparing legacy form behavior across the two runtimes.
The [dual-runtime session and parity harness](docs/plans/11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md)
defines its milestones; the [test-user and form-launcher specification](docs/plans/12-ROLE-IMPERSONATION-AND-FORM-LAUNCHER-SPEC.md)
describes the synthetic utility form. Neither document is evidence of current runtime support or
of verified parity.

## Historical setup notes (2026-11-14; not the active backlog)

The following notes describe an earlier environment state. Recheck current settings and references
before acting; do not assume any previously reported container, path, credential, or test status
still applies.

- **Oracle Home on Ubuntu's disk (`~/oracle_home_14c`) — DONE:** `OFW_ORACLE_HOME=/home/<you>/oracle_home_14c` is set in `.env`, `forms-server-14c` is recreated on `~/oracle_home_14c`, `python3 start_testbed.py` booted WebLogic + Forms servlet in **45s** (down from 15–44 minutes on `/mnt/c`), `launch_fsal.py ofw_demo_employees.fmx` started without error (with `OFW_EMP_MENU.mmx -> ofw_emp_menu.mmx` symlink, also scripted in `apply_forms14c_config.sh`), and `health_check.py` passed (**All systems operational!**).
- **Cleaned up:** deleted the old `oracle-reference-stack/oracle_home_14c/` folder and removed the one-off `restore_oracle_home_attributes.sh` script. Kept `apply_forms14c_config.sh` and `provision_oracle_home.sh`.
- **Preview in Oracle & `OFW_FORMS_PATH_EXTRA` (2026-11-14):**
  - **Done:** Non-sample forms load from `OFW_FORMS_PATH_EXTRA` without touching `samples/`, and saves/conversions for non-sample forms write to `OFW_FORMS_PATH_EXTRA` (or `temp/saved/` if unset; `forms` path dependencies are synced to `temp/forms_extra/` and appended to `FORMS_PATH`). `run_form.py` runs a `sqlplus -L` logon diagnostic when `frmcmp_batch` fails without `PL/SQL ERROR` / `ORA-`.
  - **Two verified blockers on external `.fmb` Preview in Oracle:**
    1. **`frmcmp_batch` silent logon failure despite `sqlplus -L` exit `0`:** `sqlplus -L -S <userid>` connected OK (`exit 0`) to `<db-host>:1521/<service>`, yet `frmcmp_batch ... userid=<userid> compile_all=yes` printed only the Oracle banner + `Form not created` and exited `0` (verified with an invalid `userid=` test that `frmcmp_batch` suppresses logon `ORA-`/`FRM-` errors and exits `0`). Likely cause: `frmcmp_batch`'s `userid=` parser rejecting special characters in the password (needing `user/"pass"@...` quoting) or OCI/Easy Connect differences vs `sqlplus`.
    2. **Lossy `(summary)` XML omits `<AttachedLibrary>` and truncates PL/SQL:** Opening a binary `.fmb` with the workbench's native reader exports `(summary)` XML (`summary_xml` in `service.rs`), which omits `<AttachedLibrary>` tags (even when the `.pll`/`.olb` dependencies are present in `OFW_FORMS_PATH_EXTRA`) and truncates PL/SQL (`PL/SQL ERROR 103` `Encountered the symbol "end-of-file"` when tested with `frmcmp_batch ... logon=no`).
  - **Next:**
    1. Fix `frmcmp_batch` `userid=` quoting/logon in `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/run_form.py` (and `compile_form.py`) so `frmcmp_batch` connects whenever `sqlplus -L` connects.
    2. For binary `.fmb` files opened from `OFW_FORMS_PATH_EXTRA` (or uploaded), use Oracle's `frmf2xml` (`convert_fmb.py` / container `frmf2xml`) for full XML export (or compile the source `.fmb` directly when unmodified) instead of compiling a regenerated `.fmb` from lossy `(summary)` XML.

**Historical checklist (not the active backlog):**
1. **Password rotation (verified for WebLogic & `DB_USER`):** `WLS_ADMIN_USER`/`WLS_ADMIN_PASSWORD` (`boot.properties` + `DefaultAuthenticatorInit.ldift` reset, `forms-server-14c` booted to `RUNNING` in **36s**, WebLogic REST auth `200 OK`) and `DB_USER`/`DB_PASSWORD` (`sqlplus` `exit=0`) are verified. `SYSTEM` login with `ORACLE_PWD` returned `exit=1` (not yet checked). Still to check tracked files with `git grep`.

**Step 0, every session:** read `architecture.md`, then in the WSL shell run `python3 start_testbed.py --status` (read-only). If it says "Podman runtime directory missing", ask the user to run `python3 start_testbed.py --no-forms` (or the full command) in the Ubuntu shell: it recreates `/run/user/<uid>` and prompts for the WSL sudo password, which an agent cannot enter. Throwaway `podman run --rm` containers (conversions, schema extraction) need that directory but not the running `forms-server-14c`.

**Schema work (2026-11-14):** Oracle's Forms 14.1.2 schema is `forms-14c.xsd`, in the `assets/` of `compiling-and-running-oracle-forms` and `converting-oracle-artifacts` (identical; none in `docs/`). Built and documented: `validate_forms_xml.py` (`xml-validation.md`) and `extract_forms_schema.py` (`forms-xml-schema.md`). Still to do: run `extract_forms_schema.py` once (14c has no `getformsxmlschema.sh`; the generator is the `SchemaGenerator` class, its arguments are unverified, and Podman must be up); wire the 12c schema the owner pasted (`assets/12c/forms.xsd` in the compiling skill, copy it as a `12c` version in both skills' `assets/`) and compare it with 14c.

State (2026-11-14): conversion paths from `plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/test_conversions.py` (Windows 11, everything in WSL2, Forms 14.1.2 image):

| Type | Path | Status |
|---|---|---|
| Forms | FMB to XML, XML to FMB, compile FMB to FMX | PASS |
| Menus | MMB to XML, XML to MMB, compile MMB to MMX | PASS |
| Libraries | PLL to PLD | PASS |
| Libraries | PLD to PLL | BLOCKED: `frmcmp_batch` writes no `.pll` from `.pld` text; `Forms2XML` rejects libraries. Unresolved |
| OLB | OLB to XML, XML to OLB | PASS |
| Reports | XML to RDF, RDF to XML | The owner reported `2 PASS` after the image gained `libnsl2`; the agent saw only that last line. Confirm (below) |

Goal: every path for all five types works. One path is blocked (PLD to PLL); the report paths need confirming.

## 1. Reports: confirm XML to RDF and RDF to XML

No rebuild is needed: the current `localhost/forms-converter-14c:latest` already has `libnsl2` (`rpm -q libnsl2` printed `libnsl2-1.2.0-2.20180605git4a062cf.el8.x86_64`; someone rebuilt it, not the agent). The old image is tagged `backup-20261114` and still backs the running `forms-server-14c`. To do:
1. Run `python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/test_conversions.py --only reports` and read the table (not just the last line). `apply_forms14c_config.sh` step 5b (Reports `prefs.ora` and printer files) was added by someone else and is the likely reason it passes; find out and record it.
2. Run `convert_rpt.py` end to end on a real `.rdf` (the one the test produced), including the `.rdf` to XML direction, and record the result in the change log and `references/reports-conversion.md`.
3. Do not remove or recreate `forms-server-14c` or `oracle23ai-lite` without the owner's approval (settings saved in the git-ignored `temp/image-backup/forms-server-14c.inspect.json`).

## 2. Libraries: PLD to PLL

Facts: `frmcmp_batch module=lib.pld module_type=library ...` (tried with `script=no`, `compile_all=yes`, and both) prints nothing and writes no `.pll`. `Forms2XML` says `File lib.pll must be a FormModule, ObjectLibrary or MenuModule`, so there is no XML route. The `.pld` that `frmcmp_batch script=yes` exports is plain PL/SQL source text (the same kind of file as `samples/utils.pld`).

Not yet tried:
- Forms Builder (`frmbld`, works through WSLg; command in `plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/forms-builder-gui.md`): create or open a library, paste the program units from the `.pld`, save as `.pll`. If this works, script it.
- Search Oracle's 14c documentation and `forms/templates` for any batch library-import utility.
- `frmcmp_batch` options not yet tried (read its `help=yes` output).
If nothing works, document that PLD to PLL needs Forms Builder (or is unsupported), update the test script's wording, and keep it as a known limit.

## 3. Other open items

- **Move the Oracle Home to Ubuntu's disk** (owner's decision, 2026-11-14, scripted, not yet run): follow `plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/oracle-home-location.md`: dry run, `--from-copy`, `--verify --against` (fingerprint IDENTICAL and the timed walks), set `OFW_ORACLE_HOME`, recreate `forms-server-14c` with the new mount (owner approves; settings in `temp/image-backup/forms-server-14c.inspect.json`), re-run `python3 start_testbed.py` and record the Forms "took" time against the 15 to 44 minutes before. Then `--backup`. Also update `testing-forms-parity/scripts/test_parity.py`, which still hard-codes the repo Oracle Home path in two places, and look for host paths inside the WebLogic domain files before recreating the container.
- **`apply_forms14c_config.sh` step 5b** (Oracle Reports `prefs.ora` and printer files for `rwconverter`) was added by someone other than the agent; find out who and what they verified, then re-run `test_conversions.py --only reports` against the new `latest` image (it has `libnsl2`).

- **Run `oracle-reference-stack/apply_forms14c_config.sh` on a clean copy of the Oracle Home** (not the live one) and compare with the working setup. It was written from the verified manual steps and has never been run end to end.
- **XML Source tab:** restart the app to see the new full-XML default, and fix the bug where selecting one item shows two lines (selecting `EMP.FIRST_NAME` showed lines 23 to 24, which include `EMP_ID`; the start line looks one too early; look in `crates/oracle-forms-xml` for where line numbers are recorded).
- **Oracle download steps:** `windows-podman-setup.md` section 4 and `references/dependencies.md` 2.4b list part numbers, file names, sizes, and destinations, but not the click-by-click eDelivery steps. The owner did the downloads and needs to supply them.
- **Not recorded originally:** the exact Ubuntu Java package name (`openjdk-21-jre` is a guess; WSL runs OpenJDK 21.0.9).
- **Rust 1.100 prints 8 manifest lint warnings** (redundant `homepage`, unused `tracing`) while AGENTS.md says zero warnings. Needs the owner's decision.
- **Not tested:** XLIFF, `.rpt`, `.rex` conversions; MMB/OLB conversions on macOS; whether the `--user root` container rule is needed inside WSL.
- Stale local branches `docs/windows-podman-setup` and `feat/windows-wsl-all-in-wsl` still exist; ask before deleting.
- **Run Reports on Fusion Middleware (like Forms):** Investigate running Oracle Reports (`.rdf` / `_report.xml`) on the Fusion Middleware 14c server (Reports Server / `rwservlet`) from the workbench, similar to how Forms launches via FSAL.
- **Skills audit (2026-11-14):** all five skills PASS the `authoring-skills` audit in source and installed mode (0 warnings). Re-run it after changing any skill: `python3 ~/.claude/skills/authoring-skills/scripts/audit_skill.py plugins/oracle-forms --all --mode source` (and `--mode installed`).
- **`oracle-reference-stack/compose.yaml`** has a literal database password value (now read from .env), an obsolete `version: '3.8'`, and a macOS-only `DISPLAY`; documented in `plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/oracle-reference-stack-map.md`, not fixed.
- **`launch_fsal.py`** now builds its default logon from `DB_USER` and `DB_PASSWORD` in `.env` (changed on the owner's instruction; check the macOS flow once). `compile_form.py` already reads the logon from `.env`.
- **`run_form.py` has no `--auto-login` flag** (an older `HOW-TO-RUN.md` said so; fixed there).
