# Vision: Modern Agentic Oracle Forms 14c Workbench

## Purpose
Evolve **Oracle Forms Workbench** (`oracle-forms-server` + `ui/` + `crates/oracle-forms-*` + `plugins/oracle-forms/.apm/skills/`) into a modern, Git-native, LLM-assisted development environment for Oracle Forms & Reports 14c that replaces day-to-day reliance on the legacy Oracle Forms Builder (`frmbld`) GUI while keeping Oracle's 14c toolchain (`XML2Forms`, `frmcmp_batch`, WebLogic/FSAL) as the authoritative production build and runtime target.

---

## Grounded Architecture: Dual-Engine, Two-Loop Model

The workbench already separates **App 1 (Native Rust Workbench)** from **App 2 (Optional Oracle 14c Reference Stack)**:

1. **Git-Native Text Source of Truth (`*_fmb.xml`, `*_mmb.xml`, `*_olb.xml`, `*_report.xml`, `.pld`)**:
   - Developers and AI coding agents create, inspect, and refactor Forms, Menus, Object Libraries, PL/SQL Libraries, and Reports as version-controlled XML and `.pld` text.
   - Structural edits in the browser designer update `samples/<form>_fmb.xml` atomically in place (`patchedOriginal` preserves unknown XML nodes; `summary` regenerates from `ModuleIndex`).

2. **Fast Zero-Container Inner Loop (Pure Rust Engine — `crates/oracle-forms-*`)**:
   - **Native Binary Reader (`oracle-forms-binary`)**: Parses `ROS.60050` `.fmb`, `.mmb`, `.olb`, `.pll`, and `.rdf` files into `ModuleIndex` / `ReportDefinition` with zero JVM, Oracle Client, or Podman overhead—using the contiguous `0x00010000` virtual stream, `parent_vptr` + `0x0c` wrapper resolution, `ListItemElement` (`0x22`), `Point` (`0x52`), compact 8 KB `.mmb`/`.olb` support, and `0x0000004f 0x0084` PL/SQL blob decoding.
   - **Native Designer, PL/SQL Studio & Runtime (`oracle-forms-server`, `oracle-forms-runtime`, `ui/`)**: Renders canvases, blocks, items, tabs, graphics, and triggers in the browser (`http://127.0.0.1:7070`), runs native `FormSession`s against Oracle Thin / SQLite / Mock, and evaluates/renders Reports to HTML5.
   - **Native Export (`export-xml` / `export-binary`)**: Exports `ModuleIndex` / `ReportDefinition` to summary XML, `.pld` text, or `ROS.60050` binaries (`RosBinaryWriter`) for offline inspection and round-trip testing. *(Honest boundary: `RosBinaryWriter` emits valid `ROS.60050` object/blob pages for native Rust round-trips, but does not yet emit Oracle's `0x3000..0x8000` internal dictionary tables for `frmcmp_batch`.)*

3. **Authoritative Oracle 14c Outer Loop (`oracle-reference-stack/` + `.apm/skills/`)**:
   - **Stage 1 (`Forms2XML` / `convert_*.py`)**: Authoritative binary → XML/PLD decompilation when complete Oracle attribute fidelity is required.
   - **Stage 2 (`XML2Forms` / `build_dependency_manifest.py`)**: Builds topological dependency-ordered `.olb`, `.pll`, `.mmb`, `.fmb` binaries (`logon=no`).
   - **Stage 3 (`frmcmp_batch` / `compile_from_manifest.py` / `run_form.py` / `launch_fsal.py`)**: Compiles against a live Oracle database into `.plx`, `.mmx`, `.fmx` and launches via FSAL on WebLogic (`forms-server-14c`).
   - **Why Stage 3 stays Oracle-hosted**: `.fmx` is a platform-specific little-endian runtime image containing compiled PL/SQL p-code and database-validated SQL plans; keeping `frmcmp_batch` in the outer loop guarantees 100% Fusion Middleware compatibility.

---

## Target Developer & Agent Experience

- **Dual-Engine Controls in the Web IDE**:
  - **Export XML / PLD**: Choose **Rust (Instant, Offline)** or **Oracle 14c (`Forms2XML`, Authoritative)**.
  - **Build / Compile**: Choose **Rust (`ROS.60050` Binary)** for offline round-trip checks, **14c Stage 2 (`XML2Forms`)**, or **14c Stage 3 (`frmcmp_batch` → FSAL)** for live middleware execution.
  - **Parity Diff**: Compare native Rust-decompiled XML against `Forms2XML` output directly in Form Diff.
- **Skill-Driven LLM Authoring (`converting-oracle-artifacts`, `compiling-and-running-oracle-forms`)**:
  - Skills explicitly route between **Rust fast-path** (instant inspect, edit, preview, offline parity) and **Oracle 14c path** (authoritative `Forms2XML`, `XML2Forms`, DB-connected `frmcmp_batch`, FSAL launch), so an agent or developer can author forms end-to-end without opening `frmbld`.
