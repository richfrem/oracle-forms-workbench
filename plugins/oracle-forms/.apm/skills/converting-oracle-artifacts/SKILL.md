---
name: converting-oracle-artifacts
description: "Converts Oracle Forms (.fmb, .mmb, .olb to and from XML), PL/SQL libraries (.pll to .pld), and Reports (.rdf and XML) with Oracle's own tools in the Forms 14c container image, and extracts the Forms XML schema from the install. Use when extracting form ASTs, decompiling binary forms, converting XML back to binary forms or menus, exporting library source, inspecting reports, or checking which conversion paths work."
---

# Converting Oracle Artifacts

Runs Oracle's own conversion tools inside the Forms 14c container image (`oracle-forms-14c:latest`, Oracle Home mounted at `/oracle_home_14c`). Works on macOS, Linux, WSL, and Windows PowerShell (through WSL). On Windows run everything inside WSL2.

## Contents

- [Constraints](#constraints)
- [Quick start](#quick-start)
- [Workflow](#workflow)
- [Interactive guided workflow (agent protocol)](#interactive-guided-workflow-agent-protocol)
- [Scripts](#scripts)
- [Verified conversion paths](#verified-conversion-paths)
- [Verification](#verification)

## Constraints

1. Needs the testbed image and Oracle Home from `setting-up-oracle-testbed`; check with `convert_fmb.py --check`.
2. Test on copies in the work folder (`OFW_WORK_DIR`, default `<Oracle Home>/ofw-work`), never in `samples/` or the repository.
3. In 14c the `bin/*.sh` wrappers are templates; the scripts call the Java classes and binaries directly. Details and limits: [references/conversion-notes.md](references/conversion-notes.md).
4. Keep every attached library, object library and menu in the input's folder; the scripts stage them automatically. Details: [references/conversion-notes.md](references/conversion-notes.md) and `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.
5. Validate hand-written XML before XML to binary (`validate_forms_xml.py`, `compiling-and-running-oracle-forms` skill). Do not call a path working unless `test_conversions.py` shows PASS.

## Quick start

From the repo root:

```bash
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/convert_fmb.py --check
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/test_conversions.py
```

## Workflow

1. **Binary to XML:** `convert_fmb.py form.fmb` (also `.mmb`, `.olb`; output `<name>_<ext>.xml`); `convert_pll.py lib.pll` (to `lib.pld`; prints FAILED when no file is written); `convert_rpt.py report.rdf` (to `report_rdf.xml`).
2. **XML to binary:** run XML2Forms in the container (`oracle.forms.util.xmltools.XML2Forms`); `test_conversions.py` shows the exact command per type.
3. **Check the schema** when a conversion rejects XML: `references/forms-xml-schema.md` (what it is, where it came from) and `extract_forms_schema.py` (re-extract and compare).
4. **Whole project:** `build_dependency_manifest.py --source-dir <folder> --build-dir <external>` analyzes, orders and converts every module, then compile with the other skill's `compile_from_manifest.py`.
5. **Test every path** with `test_conversions.py` (`--only forms|menus|libraries|olb|reports`, `--offline`) and report its table.

## Interactive guided workflow (agent protocol)

The agent drives the whole pipeline; the user never has to guess the next step. At each gate: summarize the result in a short table, then ask one direct question (use the interactive question tool) and wait. Run everything in WSL2 on Windows. All generated files (`build-manifest.json`, `stage/`) go only under `OFW_WORK_DIR`.

**Gate 0, intake (ask first):**
1. Primary read-only source folder(s) (XML exports) and any secondary fallback folder(s) for missing `.fmb`, `.pll`, `.mmb`, `.olb`.
2. The external `OFW_WORK_DIR` outside the repository (default `<OFW_ORACLE_HOME>/ofw-work`).
3. Duplicates across folders. Recommended default: prefer the primary XML exports, fill in missing binaries and libraries from the secondary folder, exclude compiled `.fmx`/`.plx`/`.err` and backup copies.
4. Ask: "Shall I copy the sources into `<OFW_WORK_DIR>/source/` (a copy; originals untouched) and run Stage 1 analysis?"

**Gate 1, analysis (`--analyze-only`):** run it, then show modules by type (`OLB`, `PLL`, `MMB`, `FMB`), the ordered count, `unresolvedDependencies` and true `cycles`. List unresolved items and scratch or duplicate files (for example `* - Copy.*`, test forms) and ask whether to clean up, supply the missing files, or continue with `--allow-missing`. Ask: "Shall I run Stage 2 (XML2Forms into `<OFW_WORK_DIR>/build/stage/`)? Is `--normalize` needed for hand-authored XML?"

**Decompile rule (read before any decompile):** the server export (XML plus library text, which is often `.txt`) is already converted; use it as is. Rename exported library `.txt` to `.pld` first, decompile **only** binaries with no export, and keep one folder (`<build-dir>/decompiled/out/`) with the full text set, exported and decompiled together. Do not decompile everything, and do not clear or move folders unannounced. Details: [references/batch-dependency-conversion.md](references/batch-dependency-conversion.md).

**Gate 2, conversion:** run Stage 2 in order, report converted and failed counts, then hand off to Stage 3 in the `compiling-and-running-oracle-forms` skill and ask for the target database alias and whether to publish compiled output to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`.

## Scripts

| Script | Purpose |
|---|---|
| `converter_runtime.py` | Shared: image name, Oracle Home path (`OFW_ORACLE_HOME`), Podman command for macOS, WSL, Windows, and the `--check` probe. `OFW_CONVERTER_IMAGE=forms-converter:latest` selects the 12c image |
| `convert_fmb.py`, `convert_pll.py`, `convert_rpt.py` | Forms/menus/object libraries, PL/SQL libraries, and Reports conversions |
| `test_conversions.py` | Runs every path for the five types and prints a PASS, FAIL, BLOCKED, SKIP table |
| `extract_forms_schema.py` | Runs Oracle's `SchemaGenerator` in a container, compares the result with `assets/forms-14c.xsd`, `--update` refreshes the copies. **Not verified:** the generator's arguments (`--usage`) |
| `build_dependency_manifest.py` | Batch: scans a read-only source folder (prompts for it if `--source-dir` is omitted), writes `<build-dir>/build-manifest.json` in dependency order, then stages and converts XML to binary. `--analyze-only`, `--allow-missing`, `--convert-only` (Stage 2 only, from the existing manifest), `--normalize`. Build dir must be outside the repo. [references/batch-dependency-conversion.md](references/batch-dependency-conversion.md) |
| `assets/build-manifest.template.json` | The one manifest shape for the whole pipeline (a worked example; each stage fills only its own record per module) |

## Verified conversion paths

Windows 11 in WSL2, 14.1.2 image, 2026-11-14: 9 PASS, 2 BLOCKED (PLD to PLL; XML to RDF, since fixed by the `libnsl2` image rebuild, owner reported 2 PASS), 1 SKIP. macOS not re-run.

| Type | Path | Status |
|---|---|---|
| Forms | FMB to XML, XML to FMB, compile FMB to FMX | PASS |
| Menus | MMB to XML, XML to MMB, compile MMB to MMX | PASS |
| Libraries | PLL to PLD | PASS, only when the attached libraries are in the same folder (otherwise silent: exit 0, no file) |
| Libraries | PLD to PLL | BLOCKED: no command-line route on 14c |
| OLB | OLB to XML, XML to OLB (14c-format XML) | PASS |
| Reports | XML to RDF, RDF to XML | Reported PASS by the owner; re-run to confirm |

## Verification

Run `test_conversions.py` after any change to the converter scripts or the image and report the table it prints. Eval cases: `evals/evals.json`.
