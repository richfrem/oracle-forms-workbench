# oracle-forms Plugin

Generated via Agent Scaffolder.

## Purpose
Oracle Forms & Reports 14c automation, XML/XSD validation, dependency-ordered manifest builds, headless compilation targeting local or dev DBs, FSAL launching, and Rust `ROS.60050` dual-engine parity.

## Plugin Components
List the self-contained skills, sub-agents, and hooks provided by this plugin here. Do NOT list them in `.claude-plugin/plugin.json` as APM and Claude Code auto-discover primitives under `.apm/`.

### Skills
- **`setting-up-oracle-testbed`**: Podman container orchestration (macOS & Windows WSL2), Oracle Home management, health checking, `start_testbed.py` guidance, and DB 23ai Free seeding.
- **`converting-oracle-artifacts`**: Authoritative 14c conversion (`Forms2XML`, `XML2Forms`, `convert_pll.py`, `convert_rpt.py` `rwconverter`) for `.fmb`/`.mmb`/`.olb`/`.pll`/`.rdf`, Stage 0–2 dependency-ordered manifest builds (`build_dependency_manifest.py`), XSD extraction, and Rust `ROS.60050` offline conversion guidance.
- **`compiling-and-running-oracle-forms`**: Forms 14c XSD/DTD validation (`validate_forms_xml.py`), headless compilation (`compile_form.py`, `compile_from_manifest.py`) targeting local `FREEPDB1` or non-local dev DBs (`forms_apps/<app_name>/`), and FSAL launch (`run_form.py`, `launch_fsal.py`).
- **`testing-forms-parity`**: Offline `ROS.60050` binary/XML parity (`cargo test -p oracle-forms-binary`, `run_roundtrip.py`, `compare_artifacts.py`) and live Oracle 14c vs. native Rust query/runtime parity (`test_parity.py`).
- **`comparing-forms-features`**: Side-by-side feature comparison, compatibility scoring, built-in subprogram auditing, and latency benchmarking between Oracle Fusion Middleware 14c and the native pure-Rust Oracle Forms Workbench.

### Sub-Agents

### Hooks

## Directory Structure

```text
oracle-forms/
├── .apm/
│   └── skills/
├── .claude-plugin/
│   └── plugin.json
├── apm.yml
└── README.md
```
