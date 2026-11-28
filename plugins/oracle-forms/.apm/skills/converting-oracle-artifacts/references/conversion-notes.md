# Conversion notes: Windows and WSL, known limits

Details behind the converting skill's `SKILL.md`. Results are from `test_conversions.py` on Windows 11 with everything in WSL2, Forms 14.1.2 image, 2026-11-14. macOS was not re-run.

## Contents

- [Windows and WSL notes](#windows-and-wsl-notes)
- [Known limits](#known-limits)
- [Schema and validation](#schema-and-validation)

## Windows and WSL notes

- Run inside WSL (`wsl -d Ubuntu`) from the repo root. The scripts also work from Windows PowerShell; they call Podman through `wsl -d <distro>` (`OFW_WSL_DISTRO`, default `Ubuntu`).
- On Windows and WSL, `podman run` uses `--user root`. Rootless Podman maps the container's `oracle` user to a UID that cannot write on the Windows drive (`/mnt/c`); container root maps to your user.
- `convert_fmb.py` runs Forms2XML as a Java class with `-Djava.library.path=/oracle_home_14c/lib` and `FORMS_API_TK_BYPASS=TRUE`. In 14c, `bin/frmf2xml.sh` and the other `bin/*.sh` batch scripts are templates with `$$placeholder$$` values, not runnable files, so the scripts call the Java class and the binaries directly.
- `bin/rwconverter.sh` is also a template; call `bin/rwconverter` directly.
- Keep test files in the work folder (`OFW_WORK_DIR`, default `<Oracle Home>/ofw-work`), never in `samples/` or the repository.
- The Oracle Home location is a setting: `OFW_ORACLE_HOME` (environment, then `.env`) overrides the default `oracle-reference-stack/oracle_home_14c`; `converter_runtime.py` reads it.

## Known limits

- **PLD to PLL:** on 14c, `frmcmp_batch` prints nothing and writes no `.pll` when given a `.pld` text file, even a real Oracle-exported one. `Forms2XML` rejects libraries (`File ... must be a FormModule, ObjectLibrary or MenuModule`), so there is no XML route. No command-line path was found; Forms Builder was not tried. `samples/utils.pld` is plain PL/SQL text, the same kind of file `frmcmp_batch script=yes` exports.
- **XML to OLB:** the older 12c sample XML is rejected by 14c. 14c-format XML (from OLB to XML) converts back fine.
- **XML to FMB/OLB (Named Color Palette Loss):** Headless `XML2Forms` on Linux does not resolve abstract or system-named colors (e.g., `BackColor="gray"`). Because headless conversion lacks an active X11 colormap/display, unmapped named colors default to palette index `0` (`#000000` / pitch black) in the resulting binary `.fmb` or `.olb`. When compiled to `.fmx`, this causes canvases to render black at runtime. To avoid this, compile `.fmx` directly from the original source binaries, or use explicit palette colours in the XML before conversion. **Verified fix from XML (2026-11-27):** a canvas that subclasses an object-library canvas and has no `BackColor` of its own is stored without a colour by headless `XML2Forms` and renders black, even after the library itself carries a correct colour. Give every such canvas an explicit `BackColor` in the staged XML. Palette names are percentages (0-100 per channel, `rNNgNNbNN`; `r75g75b75` is the Windows light grey); 0-255 values such as `r212g208b200` are invalid. Recipe: (1) list colours: `grep -o -E '(Back|Fore)Color="[^"]*"' *_fmb.xml *_olb.xml | sort | uniq -c`, and canvases that inherit: `grep -o -E '<Canvas [^>]*ParentName="[^"]*"[^>]*>' <form>_fmb.xml`; (2) in the **staged copies** only (back them up), replace named colours (`gray` -> `r75g75b75`, `white` -> `r100g100b100`, `cyan` -> `r0g100b100`, `yellow` -> `r100g100b0`) and add `BackColor="r75g75b75"` (or the parent's colour) to each inheriting `<Canvas>` that has none; (3) convert the object library first, then the forms; (4) check the result before compiling by converting the new `.fmb` back to XML (`convert_fmb.py`): each canvas must show its `BackColor`; (5) recompile. Verified on a 14c login form against a remote development database: canvas grey, prompts visible. Not checked: other inherited colour properties (items, frames) and `grayNN` names, which rendered acceptably here.
- **Reports:** `bin/rwconverter` needs `libnsl.so.2` (the image has `libnsl2` since the rebuild). The owner reported `2 PASS` for the report paths; the agent saw only that last line, so re-run `test_conversions.py --only reports` to confirm. See `setting-up-oracle-testbed/references/reports-conversion.md`.
- **Not tested:** XLIFF, `.rpt`, and `.rex` conversions.

## Schema and validation

The Forms XML schema is `forms-14c.xsd` in this skill's `assets/` (see [forms-xml-schema.md](forms-xml-schema.md) for provenance and extraction with `extract_forms_schema.py`). Validate XML before converting it with the `compiling-and-running-oracle-forms` skill's `validate_forms_xml.py`.

## Dependencies and silent failures

PLL to PLD (`frmcmp_batch script=yes`) writes no file, prints nothing and exits 0 when the libraries the `.pll` attaches are not in the same folder. Copy them beside the input and the same command works. `convert_pll.py` and `convert_fmb.py` now find the attached `.pll`, `.olb` and `.mmb` files (name match inside the binary) and, when needed, convert in a git-ignored `temp/convert-stage/<name>/` folder. Forms2XML works without dependencies but prints `Object group child ... has no real object` for objects held in absent object libraries. Forms2XML does not accept `.pll`. Full notes and the evidence level for each claim: `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.
