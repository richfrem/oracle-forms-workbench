# Rust Inner Loop vs. Oracle 14c Outer Loop (Build, Preview & Run)

This workbench replaces day-to-day reliance on Oracle Forms Builder (`frmbld`) by pairing a **zero-container Rust inner loop** with the **authoritative Oracle 14c outer loop**.

## Two-Loop Workflow

| Loop | Purpose | Tools / Buttons | Requirements |
|---|---|---|---|
| **Inner Loop (Rust Native)** | Author/edit XML or `.pld`, inspect layout/PLSQL, run native `FormSession`, test offline `ROS.60050` round-trips | Web UI: `🦀 Preview in Rust`, `💾 Export XML (Rust)`, `🦀 Build Binary (Rust)` (`/api/forms/export-binary`); CLI: `export-xml`, `export-binary`, `query` | None (no Podman, JVM, or Oracle Home) |
| **Outer Loop (Oracle 14c)** | Schema-validate XML, build official 14c binaries (Stage 2), compile against live DB (Stage 3), and launch in FSAL | Web UI: `🔨 Build & Compile (14c)`, `🚀 Launch in 14c`; CLI: `validate_forms_xml.py`, `build_dependency_manifest.py`, `compile_from_manifest.py`, `launch_fsal.py` | Podman (`forms-server-14c`, `oracle23ai-lite`), Java 17/21, Oracle Home |

## Important Boundaries

1. **`🦀 Build Binary (Rust)` (`RosBinaryWriter`) vs. Stage 2 `XML2Forms`**:
   - `oracle-forms-cli export-binary` (and `/api/forms/export-binary`) emits `ROS.60050` `.fmb`/`.mmb`/`.olb`/`.rdf` binaries with contiguous object records, post-record string pools, and PL/SQL blobs for **offline Rust round-trip testing**.
   - It does **not** yet emit Oracle's `0x3000..0x8000` internal ROS dictionary tables, so always use `XML2Forms` (`build_dependency_manifest.py` / `run_form.py`) before `frmcmp_batch`.
2. **`.fmx` Compilation**:
   - `.fmx` is a platform-specific little-endian runtime image with compiled PL/SQL p-code bound against the target Oracle database. Stage 3 compilation always runs through `frmcmp_batch` (`compile_form.py` / `compile_from_manifest.py`).
