---
name: testing-forms-parity
description: "Tests and validates format, query, and runtime parity between Oracle Fusion Middleware 14c (XML2Forms + frmcmp_batch + FSAL) and the native pure-Rust Oracle Forms Workbench (oracle-forms-binary + oracle-forms-server + oracle-thin). Use when verifying binary/XML parity, form compilation, database execution, and runtime parity across both engines."
---

# Testing Oracle Forms Parity

Verifies parity across **Oracle Fusion Middleware 14c** (`XML2Forms` + `frmcmp_batch` + FSAL) and **Native Rust Workbench** (`ROS.60050` binary/XML parsing, `oracle-forms-server`, `oracle-thin`), covering both offline artifact parity and live database/runtime parity.

## Contents

- [Constraints](#constraints)
- [Quick start](#quick-start)
- [Workflow](#workflow)
- [Verification and evals](#verification-and-evals)

## Constraints

1. Live 14c checks need `oracle23ai-lite` and `forms-server-14c` up (`python3 start_testbed.py --status`), `.env` present, and Java 17 or 21 for FSAL. On Windows run from the Ubuntu WSL shell.
2. Offline `ROS.60050` binary/XML parity (`cargo test -p oracle-forms-binary`, `run_roundtrip.py`) requires zero containers or JVM.
3. A format or query pass is not visual or behavioral parity; Track K golden traces remain open.
4. `test_parity.py` honors `OFW_ORACLE_HOME` and `OFW_WORK_DIR` via `compiling-and-running-oracle-forms` helpers, though `main()` contains stale legacy function calls and has not been re-run after refactor.
5. Credentials come from `.env`; use `localhost:1521/FREEPDB1` from the host and `oracle-db-23ai:1521/FREEPDB1` (or `TNS_ADMIN`) inside containers—never a container IP.

## Quick start

```bash
# Offline ROS.60050 binary/XML parity (zero containers)
cargo test -p oracle-forms-binary
python3 experiments/fmb-re-test/run_roundtrip.py

# Live CLI query parity against Oracle 23ai (after compiling via compiling-and-running-oracle-forms)
OFW_DB_USER=$DB_USER OFW_DB_PASSWORD=$DB_PASSWORD cargo run -p oracle-forms-cli -- query samples/ofw_demo_employees_fmb.xml --block EMP --backend oracle-thin
```

## Workflow

1. **Offline Binary & XML Parity (Rust Inner Loop):** Run `cargo test -p oracle-forms-binary` and `experiments/fmb-re-test/run_roundtrip.py` / `compare_artifacts.py` (`xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml`) to verify contiguous `ROS.60050` decoding (`ros.rs`, `ros_props.rs`, `0x0505` B-tree PL/SQL) and `RosBinaryWriter` round-trips against `samples/*`.
2. **Compile & Target DB (14c Outer Loop):** Use the `compiling-and-running-oracle-forms` skill (`rust-vs-14c-build-and-preview.md`, `forms-path-and-dependencies.md`, `compile_form.py`, `compile_from_manifest.py`, or Web UI **Build & Compile (14c)** `Target database` field) to compile against local `FREEPDB1` or a non-local dev DB and publish into `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`.
3. **Probe 14c Servlet & Launch FSAL:** Verify HTTP 200 on `http://localhost:7001/forms/frmservlet?config=standaloneapp&form=<form>.fmx` and open via `launch_fsal.py <form>.fmx --interactive` (or Web UI **Launch in 14c**).
4. **Exercise Native Rust & Compare:** Run `oracle-forms-cli query --backend oracle-thin` or open the Web UI (`http://127.0.0.1:7070`) → **Preview in Rust** / **Form Diff**, then compare fetched records, layout, and trigger behavior ([references/testing-both-engines.md](references/testing-both-engines.md)).

## Verification and evals

Eval cases are in `evals/evals.json`; criteria: [acceptance-criteria.md](references/acceptance-criteria.md). Verified offline: `cargo test -p oracle-forms-binary` (`7` tests pass) and `run_roundtrip.py` across `20` staged binaries. Live `test_parity.py` script execution was not re-run in this update. For feature-level scoring see `comparing-forms-features`; for compiling and targeting databases see `compiling-and-running-oracle-forms`.
