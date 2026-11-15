# Batch dependency-ordered conversion

Project-neutral pipeline (Stages 0 to 2) that turns a folder of exported Forms XML and libraries into binaries, in dependency order, outside the repository. Stage 3 (compile) is in the `compiling-and-running-oracle-forms` skill: the Batch manifest compilation reference. Dependency background: `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.

## Stage 0: gather sources

Put the current exports (`*_fmb.xml`, `*_mmb.xml`, `*_olb.xml`, `.pll`, optionally `.pld` and binaries) in one folder. That folder is **read-only** input: the script never writes to it. Run the script without `--source-dir` in a terminal and it asks where that folder is; the sources are then copied to `<build-dir>/stage/`. The build folder must be outside the repository (default `<OFW_WORK_DIR>/build`) and must be inside the Oracle Home mount if Stage 3 will run against it (the default `OFW_WORK_DIR` is).

## Stage 1: analyze

```bash
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/build_dependency_manifest.py \
  --source-dir <source-folder> --build-dir <external-folder> --analyze-only
```

Reads XML (`AttachedLibrary`, `FormModule/@MenuModule`, `ParentFilename`; an `.olb` parent counts as an object library, anything else as a parent module) and `.pld` (`.attach library`). Binary modules with no XML use the name-matching heuristic (may over-include). Kahn's algorithm orders providers before consumers; ties break by case-insensitive name, and file-name case is preserved. Output: `<build-dir>/build-manifest.json`, validated by `assets/build-manifest.schema.json` (starting point: `assets/build-manifest.template.json`).

It fails closed (exit 2, manifest still written for diagnosis) on a **cycle**, or on a **missing provider** unless `--allow-missing`. A `.pld` with no `.pll` is reported as unresolved: PLD to PLL has no command-line route on 14c.

## Stage 2: convert

Without `--analyze-only` the script copies sources to `<build-dir>/stage/` and runs XML2Forms in a throwaway converter container, one module at a time in manifest order, updating `status` (`converted` or `failed`). `.pll` and binary inputs are staged as they are. `--normalize` first applies the Oracle-compatibility copy used by Preview (`prepare_oracle_preview_xml`), for hand-written XML only.

## Not verified

Unit-tested with synthetic fixtures only (order, cycles, missing providers, external-only build dir). No XML2Forms run through this script has been verified yet; the single-module commands in `test_conversions.py` are the verified ones.

## Conversion details

XML2Forms runs with `FORMS_PATH=/work:<oracle home>/forms` so staged providers resolve. Subclassing parents are read from both `ParentFilename` and `ParentModule`. After conversion the binary is located case-insensitively in `stage/` and the manifest's `binaryFile`/`executableFile` are updated if Oracle wrote a different case.

