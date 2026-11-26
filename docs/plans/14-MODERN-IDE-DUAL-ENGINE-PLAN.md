# 14: Modern IDE Dual-Engine & Skill Integration Plan

Implements [vision.md](vision.md) across `crates/oracle-forms-*`, `ui/`, and `plugins/oracle-forms/.apm/skills/`.

> **Status:** Mostly implemented (Workstreams 1.1 export/build/menu controls, 1.2 SVG `<Point>`/`CompoundText`/Designer `<ListItemElement>`, 2.1, 2.2, and 3 verified); Form Diff parity shortcut, `preview.js` live `<ListItemElement>` dropdown population, `writer.rs` default attribute alignment, and `UI-REVIEW-BACKLOG.md` items remain open.

---

## Empirical Binary Parity Foundation (`experiments/fmb-re-test`)

An offline reverse-engineering experiment (`experiments/fmb-re-test/`, gitignored) validated the `ROS.60050` binary framing against `libfrmjapi.so.0` (`d2fi2aot`→`d2foct` 43 object types, `d2fi2apt`→`d2fpct` 665 property IDs) and Stage 1–3 reference artifacts:
- Every `0xffffffd4` object page uses a `0x10`-byte header skip into one contiguous `0x00010000`-based virtual byte stream (fixing multi-page `vptr` shift and truncated names).
- `parent_vptr` + `0x0c` wrapper (`pid 0x001f`) resolution reconstructs the true object hierarchy; `0x72000001`/`0x0505` B-tree rows keyed by `col0 == 0x01d0` supply PL/SQL text blobs (`0x01d0` MenuItem, `0x01d1` Trigger, `0x01d3` ProgramUnit).
- Stage 3 `.fmx` binaries are little-endian platform-specific p-code/SQL runtime images with no PL/SQL source text, confirming that native Rust should target `.fmb`/`.mmb`/`.olb`/`.pll`/`.rdf` parsing and native execution while `frmcmp_batch` remains the Stage 3 compiler.

These findings were integrated into `crates/oracle-forms-binary` (`ros.rs`, `ros_props.rs`, `scanner.rs`, `writer.rs`, `rdf.rs`) and `crates/oracle-forms-xml` (`reader.rs`, `writer.rs`, `RawXmlNode`). Together with the dual-engine IDE controls and XSD preservation work, they substantially raise Oracle 14c compatibility and parity—moving the Rust workbench toward serving as a standalone modern replacement for the unmaintained Oracle Forms Builder (`frmbld`) GUI for authoring, inspecting, editing, and native testing, while retaining headless `XML2Forms`/`frmcmp_batch`/FSAL for final `.fmx` compilation and runtime certification.

---

## Workstream 1: Web App Dual-Engine Controls & APIs (`crates/oracle-forms-server` + `ui/`)

### 1.1 Dual-Engine Export (`Rust` vs. `Oracle 14c`)
- **Backend (`crates/oracle-forms-server/src/routes.rs`, `service.rs`, `oracle_preview.rs`)**:
  - **[DONE]** `POST /api/forms/export-xml`: instant Rust `FormsXmlWriter` / `.pld` (for `.pll` libraries) / `_rdf.xml` (for `.rdf`/report modules).
  - **[DONE]** `POST /api/artifacts/convert-to-xml`: **Oracle 14c (`Forms2XML` / `convert_pll.py` / `convert_rpt.py`)** engine option, returning `payload` (`idx.to_payload()`), `outputFile`, `outputPath`, `outputContent`, and `log`, or readiness/runtime errors cleanly if `forms-server-14c` / Podman is unavailable.
  - **[DONE — Forms/Menus/OLB]** `POST /api/forms/export-binary`: wraps `RosBinaryWriter::write_module` for offline `ROS.60050` export (`.fmb`/`.mmb`/`.olb`). *(Note: `write_rdf_report` exists in `oracle-forms-binary` and CLI, but `/api/forms/export-binary` currently dispatches `RosBinaryWriter::write_module` only.)*
- **Frontend (`ui/index.html`, `ui/app.js`, `ui/styles.css`)**:
  - **[DONE]** Grouped context bar menus (`Export / Convert`: `Export XML (Rust)` / `Export PLD (Rust)` + `Convert via Oracle (14c)`; `Build`: `Build Binary (Rust)` + `Compile Only (14c)` + `Build & Compile (14c)`; `Run / Preview`: `Preview in Rust` + `Launch in 14c`) with browser download and XML Source / PL/SQL Studio refresh on conversion.
  - **[DONE — Forms `Build & Compile (14c)` & `Compile Only (14c)`]** Target database field (`#oracleCompileTarget` / `validate_compile_target`) lets users target `FREEPDB1` or any container-resolvable TNS alias / `host:port/service`, and `Compile Only (14c)` (`#oracleCompileOnlyBtn` / `#compileOnlyOraclePreviewBtn` / `compileOnly: true` → `run_form.py --no-launch`) compiles and deploys `.fmx` in 14c without opening FSAL.
  - **[OPEN — App Batch / `.mmb` / `.pll` in Web UI]** Extend Web UI 14c compile-only to `.mmb`/`.pll` modules and multi-module app manifest builds (currently handled via CLI `compile_form.py` / `compile_from_manifest.py`).
  - **[OPEN]** Surface a one-click **Parity Diff** shortcut in Form Diff to compare the active binary-opened `ModuleIndex` against its sibling `*_fmb.xml` (or `Forms2XML` output).

### 1.2 Web Previewer & Designer Fidelity (`ui/app.js`, `ui/preview.js`, `crates/oracle-forms-xml/src/writer.rs`)
- **XML Attribute Alignment (`crates/oracle-forms-xml/src/writer.rs`)**:
  - **[DONE]** `RecordsDisplayCount` omitted when `<= 1`; `RawXmlNode` / `extra_attrs` / `extra_children` preserve unmodeled XSD attributes and child elements (`CompoundText`, `TextSegment`, `ObjectGroup`, `FormReportInfo`, `MenuItemCode`, `PropertyClass` triggers); `append_extra_attrs` deduplicates case-insensitively.
  - **[OPEN]** Suppress default `Canvas` `Width="800" Height="600"` and default `RealUnit` attributes when they match Forms defaults to further reduce `attr_diffs` in `compare_artifacts.py xml-vs-xml` (currently `4`–`45` attr diffs across form fixtures).
- **Renderer Fidelity (`ui/app.js`, `ui/preview.js`)**:
  - **[DONE]** Render `GraphicInfo.points` (`<Point>`) as SVG `<polygon>` and `CompoundText`/`TextSegment` text on the Designer SVG canvas; clear `oracleWindowTitle` to `<Module> — [No Canvas]` when `activeCanvas` is `null`.
  - **[PARTIAL]** Designer SVG (`ui/app.js`) falls back to `item.listElements[0].name` for List Items; `ui/preview.js` (Live UI / Runtime `<select>`) still renders only `curVal || 'Select...'` without populating `<option>` entries from `item.listElements`.
  - **[OPEN]** Continue [UI-REVIEW-BACKLOG.md](UI-REVIEW-BACKLOG.md) items: coordinate scaling (`CoordinateInfo`), prompt attachment offsets, tab page overlays, and `Alert`/`LOV` modal fidelity.

---

## Workstream 2: Agent Skills Alignment (`plugins/oracle-forms/.apm/skills/`)

### 2.1 `converting-oracle-artifacts` — **[DONE]**
- Added `references/rust-native-binary-conversion.md` and linked it from `converting-oracle-artifacts/SKILL.md`:
  - Documents **Rust `oracle-forms-cli export-xml`** vs. **Oracle 14c `convert_fmb.py` / `build_dependency_manifest.py`**.
  - Documents `experiments/fmb-re-test/compare_artifacts.py` (`xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml`).

### 2.2 `compiling-and-running-oracle-forms` — **[DONE]**
- Added `references/rust-vs-14c-build-and-preview.md` and linked it from `compiling-and-running-oracle-forms/SKILL.md`:
  - Documents the two-loop workflow (`Inner loop` in Rust vs. `Outer loop` in Oracle 14c) and explicitly notes `RosBinaryWriter` does not replace Stage 2 `XML2Forms`.

### 2.3 Planned Skill Gap: External Schema / DDL & PL/SQL Reconstruction (`[OPEN]`)
- Currently `setting-up-oracle-testbed/references/database-23ai.md` only seeds the built-in sample schema (`oracle-reference-stack/sql/`). Compiling external `.pll`/`.mmb`/`.fmb` modules in `forms-server-14c` or testing them with `OracleThinDriver` requires all referenced DB schemas, tables, views, indexes, sequences, synonyms, grants, and stored PL/SQL packages/procedures/functions to exist in `oracle23ai-lite` (`FREEPDB1`).
- **Planned skill** (e.g., `plugins/oracle-forms/.apm/skills/rebuilding-oracle-schemas/`): accept user-supplied external DDL/PL/SQL scripts from an external path, remap tablespaces/storage clauses for `FREEPDB1` (`FORMS_DATA`), execute objects in topological dependency order (`users/roles` → `sequences/tables/constraints/indexes` → `views/synonyms/grants` → `package specs/types/triggers` → `package bodies/procedures/functions` → `UTL_RECOMP` / `ALL_ERRORS` check), and keep all external SQL and logs strictly under `OFW_WORK_DIR` outside the repository per `.agent/rules/cross-project-isolation.md`.

---

## Workstream 3: Verification Gates

1. **Rust & UI Tests (`VERIFIED`)**: `cargo check --workspace && cargo test --workspace` (**137 passed, 0 failed, 0 warnings**) and `node --test tests/ui_*.test.cjs` (**32 passed, 0 failed**), plus `python3 tests/test_reference_pipeline.py` (**36 passed, 0 failed**).
2. **Skill Audit (`PARTIAL`)**: `python3 ~/.claude/skills/authoring-skills/scripts/audit_skill.py plugins/oracle-forms --all --mode source` — `converting-oracle-artifacts`, `compiling-and-running-oracle-forms`, `testing-forms-parity`, and `comparing-forms-features` **PASS** (the first two have `size.lean` 88/89 vs 80-line warnings); `setting-up-oracle-testbed` still **FAILs** on Plan 13 D4 links (`start-next.md`, `build-manifest.template.json`, `batch-manifest-compilation.md`).
3. **Round-Trip Parity (`VERIFIED BASELINE`)**: `python3 experiments/fmb-re-test/run_roundtrip.py` ran Stage A (`export-xml` vs `samples/*`) and Stage B (`export-binary` `bin-vs-bin`) across **20 staged binaries** (14 `.fmb`, 2 `.mmb`, 2 `.olb`, 1 `.pll`, 1 `.rdf`). Stage A hits `missing=0, extra=0` on 9 `.fmb` fixtures (`claims`, `customers`, `departments_employees`, `master_details`, `ofw_demo_employees`, `ofw_sales`, `orders`, `test_db` plus `simple_module` node count `306/306`); remaining diffs are `attr_diffs` (`4`–`45`), unnamed polygon `Point` pairing on `simple_module`, `.mmb`/`.olb`/`.rdf` structural gaps, and Stage B `prop_diffs` / `only_in_b` alignment.
