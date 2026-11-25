# 14: Modern IDE Dual-Engine & Skill Integration Plan

Implements [vision.md](vision.md) across `crates/oracle-forms-*`, `ui/`, and `plugins/oracle-forms/.apm/skills/`.

---

## Workstream 1: Web App Dual-Engine Controls & APIs (`crates/oracle-forms-server` + `ui/`)

### 1.1 Dual-Engine Export (`Rust` vs. `Oracle 14c`)
- **Backend (`crates/oracle-forms-server/src/routes.rs`, `oracle_preview.rs`)**:
  - Keep `GET/POST /api/forms/export-xml` (or existing export route) for instant Rust `FormsXmlWriter` / `.pld` / `report_to_xml` export.
  - Ensure `convert_to_xml` (`POST /api/forms/convert` or equivalent) is clearly exposed as the **Oracle 14c (`Forms2XML` / `rwconverter`)** engine option, returning readiness errors cleanly if `forms-server-14c` / Podman is offline.
  - Add `POST /api/forms/export-binary` (wrapping `RosBinaryWriter::write_module` / `write_rdf_report` for offline `ROS.60050` export, returning a downloadable `.fmb`/`.mmb`/`.olb`/`.rdf` or staging summary).
- **Frontend (`ui/index.html`, `ui/app.js`, `ui/preview.js`)**:
  - Add an explicit **Engine selector** (`Rust Native (Offline)` vs. `Oracle 14c (Container)`) in the Export and Build/Preview dialogs—making the capability boundary honest (`summary` vs. authoritative `Forms2XML`, and native `FormSession` vs. `XML2Forms` → `frmcmp_batch` → FSAL).
  - Surface a **Parity Diff** shortcut in Form Diff to compare the active binary-opened `ModuleIndex` against its sibling `*_fmb.xml` (or `Forms2XML` output).

### 1.2 Web Previewer & Designer Fidelity (`ui/app.js`, `crates/oracle-forms-binary/src/ros.rs`)
- **XML Attribute Alignment (`crates/oracle-forms-xml/src/writer.rs`)**:
  - Suppress default `Width="800" Height="600"` and default `RecordsDisplayCount` / `RealUnit` attributes when they match Forms defaults, reducing `attr_diffs` in `compare_artifacts.py xml-vs-xml`.
- **Renderer Fidelity (`ui/app.js`)**:
  - Render `GraphicInfo.points` (`<Point>`) for polygons/polylines on the SVG canvas.
  - Render `ItemInfo.list_elements` (`<ListItemElement>`) in List Items during both Designer preview and native Runtime sessions.
  - Continue `UI-REVIEW-BACKLOG.md` items: coordinate scaling (`CoordinateInfo`), prompt attachment offsets, tab page overlays, and `Alert`/`LOV` modal fidelity.

---

## Workstream 2: Agent Skills Alignment (`plugins/oracle-forms/.apm/skills/`)

### 2.1 `converting-oracle-artifacts`
- Add a reference `references/rust-native-binary-conversion.md` and link it from `converting-oracle-artifacts/SKILL.md`:
  - Document when to use **Rust `oracle-forms-cli export-xml`** (instant, zero-container `ROS.60050` `.fmb`/`.mmb`/`.olb`/`.pll`/`.rdf` inspection & summary export) vs. **Oracle 14c `convert_fmb.py` / `build_dependency_manifest.py`** (authoritative `Forms2XML` before Stage 2/3 builds).
  - Document `experiments/fmb-re-test/compare_artifacts.py` (`xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml`) for verifying parity offline.

### 2.2 `compiling-and-running-oracle-forms`
- Add a reference `references/rust-vs-14c-build-and-preview.md` and link it from `compiling-and-running-oracle-forms/SKILL.md`:
  - Document the two-loop workflow:
    1. **Inner loop**: Author/edit XML or `.pld` → inspect/run natively in Rust (`cargo run -p oracle-forms-server` at `http://127.0.0.1:7070`, or `oracle-forms-cli query` / `export-binary`).
    2. **Outer loop**: Validate against `forms-14c.xsd` (`validate_forms_xml.py`) → Stage 2 `XML2Forms` → Stage 3 `frmcmp_batch` → `launch_fsal.py` in `forms-server-14c`.
  - Explicitly note that `oracle-forms-cli export-binary` (`RosBinaryWriter`) is for offline `ROS.60050` round-trip testing and does not replace Stage 2 `XML2Forms` for `frmcmp_batch` yet.

---

## Workstream 3: Verification Gates
1. **Rust & UI Tests**: `cargo check --workspace && cargo test --workspace` and `node --test tests/ui_*.test.cjs` pass with 0 warnings/failures.
2. **Skill Audit**: Run `python3 ~/.claude/skills/authoring-skills/scripts/audit_skill.py plugins/oracle-forms --all --mode source` after updating skill references.
3. **Round-Trip Parity**: Re-run `python3 experiments/fmb-re-test/run_roundtrip.py` and verify Stage A & Stage B parity metrics do not regress.
