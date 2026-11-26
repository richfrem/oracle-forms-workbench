# Rust Native Binary Conversion vs. Oracle 14c

Use this guide to choose between the **Rust native engine** (instant, offline, zero containers) and the **Oracle 14c container engine** (`convert_fmb.py`, `convert_pll.py`, `convert_rpt.py`, `build_dependency_manifest.py`, or Web UI `Convert via Oracle (14c)`).

## Contents

- [When to use which engine](#when-to-use-which-engine)
- [Rust native capabilities and honest boundaries](#rust-native-capabilities-and-honest-boundaries)
- [Web UI vs. CLI/skill integration](#web-ui-vs-cliskill-integration)

## When to use which engine

| Need | Engine | Command / API | Notes |
|---|---|---|---|
| Instant inspect, preview, or summary XML/PLD/Report export offline | **Rust Native (`oracle-forms-cli` / Web UI)** | `cargo run -p oracle-forms-cli -- export-xml <file> -o <out>` or Web UI `Export XML (Rust)` / `Export PLD (Rust)` | Zero Podman/JVM overhead; decodes `ROS.60050` `.fmb`, `.mmb`, `.olb`, `.pll`, `.rdf` |
| Offline `ROS.60050` round-trip binary build | **Rust Native (`RosBinaryWriter` / `write_rdf_report`)** | `oracle-forms-cli export-binary` or `Build Binary (Rust)` (`/api/forms/export-binary`) | Round-trip testing only; lacks `0x3000..0x8000` tables needed by `frmcmp_batch` |
| Authoritative, lossless Oracle XML/PLD before Stage 2/3 builds | **Oracle 14c (`Forms2XML` / `rwconverter`)** | `convert_fmb.py`, `convert_pll.py`, `convert_rpt.py`, `build_dependency_manifest.py`, or Web UI `Convert via Oracle (14c)` | Required when preparing production XML for `XML2Forms` → `frmcmp_batch` |
| Offline parity check between binaries and Oracle XML | **Experiment Comparator** | `python3 experiments/fmb-re-test/compare_artifacts.py <mode> <left> <right>` | Modes: `xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml` |

## Rust native capabilities and honest boundaries

- **`.fmb` / `.mmb` / `.olb`**: `decode_ros60050_module` (`ros.rs` + `ros_props.rs`) reads contiguous `0x00010000` virtual object streams (`+0x10` header skip on every `0xffffffd4` page), `parent_vptr` + `0x0c` wrappers, `0x72000001`/`0x0505` B-tree PL/SQL blobs (`0x01d0` `MenuItem`, `0x01d1` Trigger, `0x01d3` ProgramUnit, `PropertyClass` triggers), `CompoundText`, `ObjectGroup`, `ListItemElement` (`0x22`), `Point` (`0x52`), and compact 8 KB `.mmb`/`.olb` modules.
- **`.pll` → `.pld`**: `decode_ros60050_pll` extracts program units from `ROS.60050` libraries; `export-xml` outputs `.pld` text.
- **`.rdf` → Report XML**: `parse_rdf_report` extracts report title, `Q_*` data source, `SELECT` SQL, `G_*` group, and `dataItem` columns.
- **XML preservation**: `oracle-forms-xml` preserves unmodeled XSD attributes/children via `RawXmlNode`, `extra_attrs`, and `extra_children`, though `FormsXmlWriter` still emits summary defaults for some binary-derived attributes; use `convert_fmb.py` / `Forms2XML` when authoritative 14c XML is required.

## Web UI vs. CLI/skill integration

- **Web UI (`Option 2`):** **Export / Convert → Convert via Oracle (14c)** calls `POST /api/artifacts/convert-to-xml` (`convert_to_xml` in `oracle_preview.rs`), dispatching `convert_fmb.py`, `convert_pll.py`, or `convert_rpt.py` and refreshing the active module in the browser.
- **CLI / Chat (`Option 1`):** Run `python3 start_testbed.py --status` first to confirm `forms-server-14c`, run `build_dependency_manifest.py` for Stage 0–2 dependency-ordered conversion, then hand off to `compiling-and-running-oracle-forms` (`compile_form.py` or `compile_from_manifest.py`) for Stage 3 compilation against local `FREEPDB1` or a dev DB.
