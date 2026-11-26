---
name: converting-oracle-artifacts
description: "Converts Oracle Forms (.fmb, .mmb, .olb to and from XML), PL/SQL libraries (.pll to .pld), and Reports (.rdf and XML) with Oracle's own tools in the Forms 14c container image, and extracts the Forms XML schema from the install. Use when extracting form ASTs, decompiling binary forms, converting XML back to binary forms or menus, exporting library source, inspecting reports, or checking which conversion paths work."
---

# Converting Oracle Artifacts

Runs Oracle's conversion tools inside the Forms 14c container (`oracle-forms-14c:latest` / `forms-server-14c`, Oracle Home at `/oracle_home_14c`) on macOS, Linux, and Windows (run everything in WSL2). Flow diagram: [references/converting-oracle-artifacts-flow.mmd](references/converting-oracle-artifacts-flow.mmd).

## Contents

- [Constraints](#constraints) • [Quick start](#quick-start) • [Workflow](#workflow) • [Interactive guided workflow (agent protocol)](#interactive-guided-workflow-agent-protocol) • [Scripts and references](#scripts-and-references) • [Verified conversion paths](#verified-conversion-paths) • [Verification](#verification)

## Constraints

1. Needs the stack from `setting-up-oracle-testbed`: check with `python3 start_testbed.py --status` (repo root) and `convert_fmb.py --check`.
2. Keep test outputs and manifests in `OFW_WORK_DIR` (default `<Oracle Home>/ofw-work`), never in `samples/` or the repo.
3. In 14c, `bin/*.sh` scripts are templates; the scripts call the Java classes and binaries (`rwconverter`, `frmcmp_batch`) directly ([references/conversion-notes.md](references/conversion-notes.md)).
4. Keep attached `.pll`, `.olb`, and `.mmb` files beside inputs; the scripts stage dependencies automatically (`setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`).
5. Validate XML before `XML2Forms` (`validate_forms_xml.py` in `compiling-and-running-oracle-forms`). Only claim PASS when `test_conversions.py` or a recorded run verifies it.

## Quick start

```bash
python3 start_testbed.py --status
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/convert_fmb.py --check
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/test_conversions.py
```

## Workflow

1. **Fast offline inspect/export (Rust):** Use `oracle-forms-cli export-xml` or Web UI `Export XML (Rust)` ([references/rust-native-binary-conversion.md](references/rust-native-binary-conversion.md)).
2. **Authoritative 14c decompile:** `convert_fmb.py` (`.fmb`/`.mmb`/`.olb` → `*_xml`), `convert_pll.py` (`.pll` → `.pld`), `convert_rpt.py` (`.rdf` → `_rdf.xml`), or Web UI **Convert via Oracle (14c)** (`/api/artifacts/convert-to-xml`).
3. **XML/PLD to 14c binaries (Stage 2):** `XML2Forms` / `build_dependency_manifest.py --source-dir <folder> --build-dir <external>`, then hand off to `compiling-and-running-oracle-forms` (`compile_form.py` / `compile_from_manifest.py`) or Web UI **Build & Compile (14c)**.
4. **Schema check:** [references/forms-xml-schema.md](references/forms-xml-schema.md) and `extract_forms_schema.py`.

## Interactive guided workflow (agent protocol)

At each gate, summarize counts in a table and ask one direct question; all outputs go under `OFW_WORK_DIR`. **Gate 0 (intake):** confirm read-only primary XML/PLD and secondary binary folders, external `OFW_WORK_DIR`, duplicate policy, and permission to copy to `<OFW_WORK_DIR>/source/`. **Gate 1 (analysis):** run `--analyze-only`, report module counts, `unresolvedDependencies`, `cycles`, and duplicates; confirm cleanup or `--allow-missing` and `--normalize`. **Decompile rule:** never re-decompile existing server XML or `.txt`/`.pld` exports—rename library `.txt` to `.pld`, decompile only binary-only gaps (`--decompile-binaries` / `--validate-xml`), and keep one merged `<build-dir>/decompiled/out/` folder. **Gate 2 (Stage 2):** convert in dependency order, report converted/failed, and hand off to Stage 3 in `compiling-and-running-oracle-forms` ([references/batch-dependency-conversion.md](references/batch-dependency-conversion.md)).

## Scripts and references

| File | Purpose |
|---|---|
| `scripts/converter_runtime.py` | Shared 14c/12c image, `OFW_ORACLE_HOME`, Podman wrapper, `--check` |
| `scripts/convert_fmb.py`, `convert_pll.py`, `convert_rpt.py` | Forms/menus/OLBs, PL/SQL libraries, and Reports (`rwconverter`) conversions |
| `scripts/build_dependency_manifest.py` | Stage 0–2 dependency analyzer, decompiler, and `XML2Forms` batch builder (`build-manifest.json`) |
| `scripts/test_conversions.py`, `extract_forms_schema.py` | Conversion matrix runner (`--only`); 14c `SchemaGenerator` extractor (`--usage` unverified) |
| `references/converting-oracle-artifacts-flow.mmd` | Flowchart: Rust offline conversion vs. Oracle 14c Stage 1–2 pipeline |
| `references/rust-native-binary-conversion.md` | Rust `ROS.60050` (`export-xml`/`export-binary`) vs. Oracle 14c `Forms2XML` & Web UI routes |
| `references/batch-dependency-conversion.md`, `conversion-notes.md`, `forms-xml-schema.md` | Batch Stage 0–2 protocol, Windows/WSL limits, XSD provenance |
| `assets/forms-14c.xsd`, `assets/build-manifest.template.json` | Forms 14.1.2 schema and manifest template |

## Verified conversion paths

| Type | Path | Status |
|---|---|---|
| Forms / Menus / OLB | Binary ↔ XML (14c XML), compile to `.fmx`/`.mmx` | PASS (`18/18` Stage 2, `17/17` Stage 3 samples on macOS; WSL verified) |
| Libraries | `.pll` → `.pld` | PASS when attached libraries sit in same folder |
| Libraries | `.pld` → `.pll` | `SKILL.md` baseline `BLOCKED` vs `parse=yes` batch note (open reconciliation, Plan 13 D2) |
| Reports | `.rdf` → `_rdf.xml` (`convert_rpt.py`) / `XML` → `.rdf` | `.rdf` → `_rdf.xml` verified (`#51`, `29,083`B); `XML` → `.rdf` (`12 PASS` WSL run) |

## Verification

Run `test_conversions.py` after converter or image changes and report its table. Criteria: [acceptance-criteria.md](references/acceptance-criteria.md); evals: `evals/evals.json`.
