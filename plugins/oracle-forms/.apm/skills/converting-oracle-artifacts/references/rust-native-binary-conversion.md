# Rust Native Binary Conversion vs. Oracle 14c

Use this guide to choose between the **Rust native engine** (instant, offline, zero containers) and the **Oracle 14c container engine** (`convert_fmb.py` / `Forms2XML`).

## When to Use Which Engine

| Need | Engine | Command / API | Notes |
|---|---|---|---|
| Instant inspect, preview, or summary XML/PLD/Report export offline | **Rust Native (`oracle-forms-cli` / web UI)** | `cargo run -p oracle-forms-cli -- export-xml <file> -o <out>` or `💾 Export XML (Rust)` | Zero Podman/JVM overhead; decodes `ROS.60050` `.fmb`, `.mmb`, `.olb`, `.pll`, `.rdf` |
| Authoritative, lossless Oracle XML before Stage 2/3 builds | **Oracle 14c (`Forms2XML`)** | `convert_fmb.py`, `convert_pll.py`, `convert_rpt.py`, `build_dependency_manifest.py` | Required when preparing production XML for `XML2Forms` → `frmcmp_batch` |
| Offline parity check between binaries and Oracle XML | **Experiment Comparator** | `python3 experiments/fmb-re-test/compare_artifacts.py <mode> <left> <right>` | Modes: `xml-vs-xml`, `bin-vs-bin`, `bin-vs-xml` |

## Rust Native Capabilities & Honest Boundaries

- **`.fmb` / `.mmb` / `.olb`**: `decode_ros60050_module` reads contiguous `0x00010000` virtual object streams, `parent_vptr` + `0x0c` wrappers, `ListItemElement` (`0x22`), `Point` (`0x52`), compact 8 KB `.mmb`/`.olb` modules, and `0x0000004f 0x0084` PL/SQL blobs.
- **`.pll` → `.pld`**: `decode_ros60050_pll` extracts program units from `utils.pll`-style `ROS.60050` libraries; `oracle-forms-cli export-xml file.pll -o file.pld` writes `.pld` source text.
- **`.rdf` → Report XML**: `parse_rdf_report` extracts report title, `Q_*` data source, `SELECT` SQL, `G_*` group, and `dataItem` columns.
- **Boundary**: `FormsXmlWriter` emits a **summary** XML (some default/unmodeled Oracle attributes differ from `Forms2XML`); always use `convert_fmb.py` / `Forms2XML` for authoritative 14c source exports.
