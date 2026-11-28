# Rust Inner Loop vs. Oracle 14c Outer Loop (Local to Dev DB Flow)

This workbench replaces day-to-day reliance on Oracle Forms Builder (`frmbld`) by pairing a **zero-container Rust inner loop** with the **authoritative Oracle 14c outer loop**, from local authoring through non-local development database compilation.

## Contents

- [End-to-end development process flow](#end-to-end-development-process-flow)
- [Two-loop summary](#two-loop-summary)
- [Targeting local vs. non-local databases in 14c](#targeting-local-vs-non-local-databases-in-14c)
- [Important boundaries and current gaps](#important-boundaries-and-current-gaps)

## End-to-end development process flow

Diagram: [compiling-and-running-oracle-forms-flow.mmd](compiling-and-running-oracle-forms-flow.mmd).

1. **Author or modify XML / PLD:** Create or edit `*_fmb.xml` (UI Designer/Wizard, Save Form, or AI coding agent), `*_mmb.xml`, `*_olb.xml`, `*.pld`, or `*_report.xml`.
2. **Rust inner loop (offline, zero containers):**
   - Inspect structure, layout, and PL/SQL in the Rust web app (`http://127.0.0.1:7070`) or `oracle-forms-cli` (`inspect --json`, `dump-sql`, `diff`).
   - Run **Preview in Rust** (`FormSession` / Reports Engine) against SQLite, Mock, or `oracle-thin`, and optionally test offline `ROS.60050` round-trips via **Build Binary (Rust)** (`export-binary`).
   - Revise XML/PLD as needed before touching Fusion Middleware.
3. **Validate XML (Stage 1):** Run `validate_forms_xml.py` against `forms-14c.xsd` (or `reports.dtd`).
4. **Build 14c source binaries (Stage 2, `logon=no`):** Convert XML/PLD into 14c `.olb`/`.pll`/`.mmb`/`.fmb` via `XML2Forms` (`build_dependency_manifest.py` for multi-module projects, or `run_form.py` for a single form).
5. **Compile and preview against local testbed (`FREEPDB1`):** Use Web UI **Build & Compile (14c)** (default `Target database = FREEPDB1`) or `run_form.py --compile-from-env --interactive` to compile against `oracle23ai-lite` and verify live behavior in FSAL.
6. **Compile against a non-local dev database:** Retarget compilation to a shared dev DB server using either the Web UI **Target database** field or CLI `compile_form.py` / `compile_from_manifest.py` after verifying container reachability (`TNS_ADMIN` + `sqlplus -L`) and compiler-account object visibility (`forms-path-and-dependencies.md`).

## Two-loop summary

| Loop | Purpose | Tools / Buttons | Requirements |
|---|---|---|---|
| **Inner Loop (Rust Native)** | Author/edit XML or `.pld`, inspect layout/PLSQL, run native `FormSession`, test offline `ROS.60050` round-trips | Web UI: `Preview in Rust`, `Export XML (Rust)`, `Build Binary (Rust)`; CLI: `export-xml`, `export-binary`, `query`, `diff` | None (no Podman, JVM, or Oracle Home) |
| **Outer Loop (Oracle 14c)** | Schema-validate XML, build 14c binaries (Stage 2), compile against local or dev DB (Stage 3), launch FSAL | Web UI: `Compile Only (14c)`, `Build & Compile (14c)`, `Launch in 14c`; CLI: `validate_forms_xml.py`, `build_dependency_manifest.py`, `compile_form.py`, `compile_from_manifest.py`, `launch_fsal.py` | Podman (`forms-server-14c`, optional `oracle23ai-lite`), Java 17/21, Oracle Home |

## Targeting local vs. non-local databases in 14c

1. **In the Rust Web App (`Build & Compile (14c)` dialog):**
   - Enter **Database username**, **Database password**, and **Target database (TNS alias or host:port/service)** (defaults to `FREEPDB1`; accepts aliases from container `tnsnames.ora` or Easy Connect strings like `db.example.com:1521/ORCLPDB1`).
   - `oracle_preview.rs` validates the target string (`validate_compile_target`), stages a copy of the saved XML in `OFW_WORK_DIR/oracle-preview/<job>/`, and passes `OFW_PREVIEW_DB_LOGON=<user>/<pw>@<target>` via environment to `run_form.py --compile-from-env` (with `--no-launch` for **Compile Only (14c)** or `--interactive` for **Build & Compile (14c)**).
   - `frmcmp_batch` compiles inside `forms-server-14c` against that target DB, writes `.fmx` to `forms_apps/<app_name>/`, and opens FSAL (where runtime logon prompts separately).
   - **Forms with dependencies.** When the saved form declares dependencies (attached libraries, `ParentFilename` object libraries or parent forms, a non-default menu) and its folder holds at least one of them, the Web App passes `--source-dir <that folder>` to `run_form.py`, which builds the form with everything it depends on instead of the form alone: Stage 1 with `--target <form>` (recursive closure, inferred attachments, waves) and `--normalize`, Stage 2 into `OFW_WORK_DIR/dependency-builds/<form>/build/`, then `compile_from_manifest.py --compile-from-env` (incremental, wave by wave). A form whose folder holds none of its dependencies is built alone as before, with dependencies taken from the Forms path. A missing dependency stops the build before conversion with the module names (`UNRESOLVED: ...`) instead of an XML2Forms or compiler error. Not verified in a browser yet.
2. **In the CLI (compile-only, single module or dependency-ordered batch):**
   - Single module (`.fmb`, `.mmb`, `.pll`): `compile_form.py <file> -u <user>@<target_db>` (prompts for hidden password).
   - Multi-module corpus (`OLB` → `PLL` → `MMB` → `FMB`): `compile_from_manifest.py --build-dir <external-dir> --userid <user>@<target_db>` (runs `sqlplus -L` preflight inside `forms-server-14c` before compiling).

## Important boundaries and current gaps

1. **`Build Binary (Rust)` (`RosBinaryWriter`) vs. Stage 2 `XML2Forms`:**
   - `ROS.60050` (*Resource Object Store*) is Oracle's binary source-module container header shared by 6i through 14c `.fmb`/`.mmb`/`.olb`/`.pll` files (distinct from compiled Stage 3 `.fmx`/`.mmx`/`.plx` runtime files). `oracle-forms-cli export-binary` and `/api/forms/export-binary` emit `ROS.60050` binaries for **offline Rust round-trip testing**; they do not emit Oracle's `0x3000..0x8000` internal dictionary tables, so always run `XML2Forms` before `frmcmp_batch`.
2. **Why Stage 2 is offline (`logon=no`) while Stage 3 (`.fmx`/`.mmx`/`.plx`) requires a DB login:**
   - **Stage 2 (source binaries):** `XML2Forms` and `frmcmp_batch ... parse=yes logon=no` emit `ROS.60050` `.olb`/`.pll`/`.mmb`/`.fmb` source binaries offline with no database connection.
   - **Stage 3 (compiled executables):** As confirmed by inspecting `ofw_sales.fmx` (`experiments/fmb-re-test/README.md`), `.fmx` is Oracle's proprietary **little-endian** compiled runtime image containing compiled PL/SQL p-code and database-validated SQL descriptors (no PL/SQL source text). `frmcmp_batch` connects to the target database (`userid=<user>/<pw>@<target>`) to resolve tables, views, columns, packages, and grants (`logon=no` only succeeds on forms with zero database object references). That is why **Compile Only (14c)** and **Build & Compile (14c)** stay in `forms-server-14c`.
3. **Compile-only vs. Build & Launch in the Web UI:**
   - **Compile Only (14c)** (`Build → Compile Only (14c)` / `compileOnly: true` → `run_form.py --no-launch`) compiles and publishes form `.fmx` modules into `forms_apps/<app_name>/` without launching FSAL.
   - Multi-module batch compilation and `.mmb`/`.pll` compile-only still run via CLI (`compile_form.py` / `compile_from_manifest.py`).
