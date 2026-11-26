# Vision: Modern Agentic Oracle Forms 14c Workbench

## Purpose
Evolve **Oracle Forms Workbench** (`oracle-forms-server` + `ui/` + `crates/oracle-forms-*` + `plugins/oracle-forms/.apm/skills/`) into a modern, Git-native, LLM-assisted development environment for Oracle Forms & Reports 14c—designed to serve as a full modern replacement for the legacy, unmaintained Oracle Forms Builder (`frmbld`) GUI for day-to-day authoring, inspection, binary/XML conversion, and native testing, while keeping Oracle's 14c headless toolchain (`XML2Forms`, `frmcmp_batch`, WebLogic/FSAL) as the authoritative production build and runtime target.

Because Oracle no longer actively modernizes `frmbld`, closing binary, XML, and UI parity gaps in the native Rust engine (guided by empirical reverse-engineering in `experiments/fmb-re-test/`) lets teams develop and refactor forms in Rust without needing the legacy `frmbld` Motif/X11 desktop builder.

---

## Grounded Architecture: Dual-Engine, Two-Loop Model

The workbench already separates **App 1 (Native Rust Workbench)** from **App 2 (Optional Oracle 14c Reference Stack)**:

1. **Git-Native Text Source of Truth (`*_fmb.xml`, `*_mmb.xml`, `*_olb.xml`, `*_report.xml`, `.pld`)**:
   - Developers and AI coding agents create, inspect, and refactor Forms, Menus, Object Libraries, PL/SQL Libraries, and Reports as version-controlled XML and `.pld` text.
   - Structural edits in the browser designer update `samples/<form>_fmb.xml` atomically in place (`patchedOriginal` preserves unknown XML nodes; `summary` regenerates from `ModuleIndex`).

2. **Fast Zero-Container Inner Loop (Pure Rust Engine — `crates/oracle-forms-*`)**:
   - **Native Binary Reader (`oracle-forms-binary`)**: Parses `ROS.60050` `.fmb`, `.mmb`, `.olb`, `.pll`, and `.rdf` files into `ModuleIndex` / `ReportDefinition` with zero JVM, Oracle Client, or Podman overhead—using the contiguous `0x00010000` virtual stream (`+0x10` header skip across all `0xffffffd4` pages), `ros_props.rs` static PID table, `0x72000001`/`0x0505` B-tree PL/SQL blob decoding (`0x01d0`/`0x01d1`/`0x01d3` including `MenuItem` code and `PropertyClass` triggers), `parent_vptr` + `0x0c` wrapper resolution, `CompoundText`/`TextSegment`, `ObjectGroup`, `ListItemElement` (`0x22`), `Point` (`0x52`), and compact 8 KB `.mmb`/`.olb` support.
   - **Schema-Preserving XML (`oracle-forms-xml`)**: Captures unmodeled XSD attributes and child elements (`RawXmlNode`, `extra_attrs`, `extra_children`) alongside `CompoundText`, `ObjectGroup`, `FormReportInfo`, and `MenuItemCode`.
   - **Native Designer, PL/SQL Studio & Runtime (`oracle-forms-server`, `oracle-forms-runtime`, `ui/`)**: Renders canvases, blocks, items, tabs, polygons (`<Point>`), `CompoundText`, and triggers in the browser (`http://127.0.0.1:7070`), shows `.mmb` `MenuItem` PL/SQL in PL/SQL Studio, runs native `FormSession`s against Oracle Thin / SQLite / Mock, and evaluates/renders Reports to HTML5.
   - **Native Export (`export-xml` / `export-binary`)**: Exports `ModuleIndex` / `ReportDefinition` to summary XML, `.pld` text (for `.pll` libraries), `_rdf.xml` (for `.rdf` reports), or `ROS.60050` binaries (`RosBinaryWriter` / `write_rdf_report`) for offline inspection and round-trip testing. *(Honest boundary: `RosBinaryWriter` emits valid `ROS.60050` object/blob pages for native Rust round-trips, but does not yet emit Oracle's `0x3000..0x8000` internal dictionary tables for `frmcmp_batch`.)*

3. **Authoritative Oracle 14c Outer Loop (`oracle-reference-stack/` + `.apm/skills/`)**:
   - **Stage 1 (`Forms2XML` / `rwconverter` / `convert_*.py`)**: Authoritative binary → XML/PLD decompilation (including 14c `convert_rpt.py` `.rdf` → `_rdf.xml` via `rwconverter` with `prefs.ora` and `DISPLAY`) when complete Oracle attribute fidelity is required.
   - **Stage 2 (`XML2Forms` / `build_dependency_manifest.py`)**: Builds topological dependency-ordered `.olb`, `.pll`, `.mmb`, `.fmb` binaries (`logon=no`).
   - **Stage 3 (`frmcmp_batch` / `compile_from_manifest.py` / `run_form.py` / `launch_fsal.py`)**: Compiles against a live Oracle database into `.plx`, `.mmx`, `.fmx` and launches via FSAL on WebLogic (`forms-server-14c`).
   - **Why Stage 3 stays Oracle-hosted**: `.fmx` is a platform-specific little-endian runtime image containing compiled PL/SQL p-code and database-validated SQL plans; keeping `frmcmp_batch` in the outer loop guarantees 100% Fusion Middleware compatibility.

---

## Target Developer & Agent Experience

- **Dual-Engine Controls in the Web IDE** *(delivered in grouped `Export / Convert`, `Build`, and `Run / Preview` menus)*:
  - **Export / Convert**: Choose **Export XML/PLD (Rust, Offline)** or **Convert via Oracle (14c `Forms2XML`/`rwconverter`)**.
  - **Build**: Choose **Build Binary (Rust `ROS.60050`)** for offline round-trip checks, or **Build & Compile (14c)** for Stage 2/3 Fusion Middleware compilation.
  - **Run / Preview**: Choose **Preview in Rust** (native `FormSession`) or **Launch in 14c** (FSAL).
  - **Parity Diff** *(remaining)*: One-click shortcut in Form Diff to compare native Rust-decompiled XML against `Forms2XML` output directly in the UI (currently supported via manual selection/upload in Form Diff or offline via `experiments/fmb-re-test/compare_artifacts.py`).
- **Skill-Driven LLM Authoring (`converting-oracle-artifacts`, `compiling-and-running-oracle-forms`)**:
  - Skills explicitly route between **Rust fast-path** (instant inspect, edit, preview, offline parity) and **Oracle 14c path** (authoritative `Forms2XML`, `XML2Forms`, DB-connected `frmcmp_batch`, FSAL launch), so an agent or developer can author forms end-to-end without opening `frmbld`.
