# Testing Both Engines & Parity Verification

Detail behind the `testing-forms-parity` skill: how to verify offline `ROS.60050` binary/XML parity, compile and target local or non-local dev databases in Oracle 14c, and test the same form in both Oracle Fusion Middleware 14c and the native Rust workbench.

## Contents

- [Comparison matrix](#comparison-matrix)
- [Offline binary and XML parity (zero containers)](#offline-binary-and-xml-parity-zero-containers)
- [Engine A: Fusion Middleware 14c (local and dev DB)](#engine-a-fusion-middleware-14c-local-and-dev-db)
- [Engine B: Native Rust workbench (browser & CLI)](#engine-b-native-rust-workbench-browser--cli)

## Comparison matrix

| Capability | Oracle Fusion Middleware 14c | Native Rust Oracle Forms Workbench |
| :--- | :--- | :--- |
| **Runtime Architecture** | Java EE / WebLogic AdminServer 14.1.2 | Standalone native Rust binary (`oracle-forms-server`) |
| **Client Requirement** | JRE 17/21 + Forms Standalone Launcher (`frmsal.jar`) | Zero-install web browser (`http://127.0.0.1:7070`) |
| **Binary / Source Handling** | `XML2Forms` + `frmcmp_batch` → `.fmx`/`.mmx`/`.plx` | Contiguous `ROS.60050` reader (`ros.rs`, `ros_props.rs`, `0x0505` B-tree PL/SQL), `RawXmlNode` XML, `RosBinaryWriter` (offline round-trip) |
| **Startup / Query Latency** | ~3 to 5 seconds (JVM + applet init) | 1 to 10 ms direct async TCP queries |
| **Database Connectivity** | Oracle Net / OCI (`TNS_ADMIN` inside container) | Pure-Rust `oracle-thin` (async Tokio TCP wire protocol) + SQLite / Mock |
| **Dependency Footprint** | ~5 GB container / Oracle Home | Single native compiled binary |

## Offline binary and XML parity (zero containers)

Before launching containers, verify native Rust `ROS.60050` binary parsing and XML serialization parity:

1. **Rust binary conformance tests:** `cargo test -p oracle-forms-binary` (exercises `ros_layout_tests.rs` and `ros_roundtrip_tests.rs` across sample `.fmb`, `.mmb`, `.olb`, `.pll`, and `.rdf` artifacts).
2. **Stage A & B round-trip comparator:** `python3 experiments/fmb-re-test/run_roundtrip.py` runs `oracle-forms-cli export-xml` (Stage A) and `export-binary` (Stage B) across staged binaries and diffs against `samples/*` via `compare_artifacts.py` (`xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml`).
3. **Browser Form Diff:** Open the **Form Diff** tab in `http://127.0.0.1:7070` to compare a baseline form against modified versions or uploaded snapshots.

## Engine A: Fusion Middleware 14c (local and dev DB)

Needs the testbed running (`python3 start_testbed.py --status`) and Java 17 or 21 (WSL OpenJDK 21 on Windows).

1. **How to compile forms and target a database:**
   - Follow `compiling-and-running-oracle-forms` (`rust-vs-14c-build-and-preview.md` and `forms-path-and-dependencies.md`).
   - **Local testbed (`FREEPDB1`):** Use Web UI **Build & Compile (14c)** (default `Target database = FREEPDB1`), `run_form.py <xml_or_fmb> --compile-from-env --interactive`, or `compile_form.py <fmb> --use-local-env`.
   - **Non-local dev database:** Enter a container-resolvable TNS alias or `host:port/service` in the Web UI **Target database** field, or run `compile_form.py <file> -u <user>@<target_db>` / `compile_from_manifest.py --build-dir <external-dir> --userid <user>@<target_db>` after verifying `TNS_ADMIN` and `sqlplus -L` inside `forms-server-14c`.
   - Compiled outputs publish to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (never `<OFW_ORACLE_HOME>/forms/`).
2. **Launch FSAL:**
   - Use `compiling-and-running-oracle-forms/scripts/launch_fsal.py <form>.fmx --interactive` or Web UI **Run / Preview → Launch in 14c**.
   - **Logon:** `DB_USER` / `DB_PASSWORD` @ `localhost:1521/FREEPDB1` for the local testbed (container `tnsnames.ora` uses `oracle-db-23ai:1521/FREEPDB1`), or your dev DB credentials when targeting a dev database. Never use a container's numeric IP address.

## Engine B: Native Rust workbench (browser & CLI)

1. Start the server: `cargo run -p oracle-forms-server` (or `python3 start_testbed.py`).
2. Open `http://127.0.0.1:7070` and select a `*_fmb.xml` or binary `.fmb`/`.mmb`/`.olb`/`.pll`/`.rdf` artifact.
3. Inspect SVG layout (polygons, tabs, `CompoundText`, `ListItemElement`), **PL/SQL Studio** (including `.mmb` `MenuItem` code), and **Reports Engine**.
4. Click **Run / Preview → Preview in Rust** for a live `FormSession` against `oracle-thin`, SQLite, or Mock, or run `cargo run -p oracle-forms-cli -- query <file> --backend oracle-thin` from the terminal.
