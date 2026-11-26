# Acceptance Criteria: `compiling-and-running-oracle-forms`

## 1. Schema Validation (Stage 1)

- **[PASSED]**: `validate_forms_xml.py` validates `.xml` files against `assets/forms-14c.xsd` (`14c`) or `assets/reports.dtd` before `XML2Forms` conversion and exits non-zero on schema errors.
- **[FAILED]**: Converts invalid XML without running schema validation first or points to missing legacy `docs/forms-14c.xsd` paths.

## 2. Compile & Launch Boundaries (Stage 3 & FSAL)

- **[PASSED]**: `compile_form.py`, `run_form.py` (`--no-launch` for Compile Only vs. full FSAL launch), and `launch_fsal.py` stage dependencies in `OFW_WORK_DIR`, deploy to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`, keep passwords out of logs/docs, and preserve `samples/` intact.
- **[FAILED]**: Runs `Build & Compile (14c)` / `Save & launch` on `samples/` as a test harness, executes `compile_from_manifest.py` interactively with passwords in chat, or claims compilation proves behavioral parity.
