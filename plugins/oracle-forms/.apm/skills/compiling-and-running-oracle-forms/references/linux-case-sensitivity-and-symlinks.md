# Linux Case Sensitivity and Uppercase Symlinks

When migrating or running Oracle Forms modules authored on Windows in the Linux `forms-server-14c` container, filename casing matters for both compilation (`frmcmp_batch`) and runtime (`WLS_FORMS` / `frmweb`).

## Why this happens in Oracle Forms on Linux

1. **Internal reference casing is baked into `.fmb`, `.pll`, and `.mmb` binaries:**
   When a developer attaches a library (`.pll`), subclasses an object library (`.olb`), or calls a menu or form (`CALL_FORM('MYFORM')`, `OPEN_FORM('myform')`) in Windows Forms Builder, Oracle stores that string verbatim inside the module's internal metadata. On Windows, NTFS ignores case so `MYLIB`, `MyLib`, and `mylib` all open `mylib.plx`.
2. **Oracle Forms on Linux does not normalize case:**
   Inside `forms-server-14c` (Oracle Linux on `ext4`), `frmcmp_batch` and Forms Runtime pass the stored name straight to the Linux filesystem. If a module records an attached library as `MYLIB` or a menu as `MY_MENU`, Linux looks literally for `MYLIB.plx` or `MY_MENU.mmx` and fails with `PDE-PLI018: Could not find library` or `FRM-40010: Cannot read form` when only `mylib.plx` or `my_menu.mmx` exists.

## How the workbench solves it with symlinks

Both `compile_from_manifest.py` and `apply_forms14c_config.sh` create case-variant symlinks so Linux resolves references regardless of casing:

- **App output symlinks (`add_case_variant()`):** After publishing each compiled `.plx`, `.mmx`, or `.fmx` into `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`, `compile_from_manifest.py` creates an uppercase symlink (`MYLIB.plx -> mylib.plx`), and `apply_forms14c_config.sh` creates `OFW_EMP_MENU.mmx -> ofw_emp_menu.mmx` in `forms_apps/default/`.
- **Mixed-case dependency symlinks (`ensure_manifest_case_symlinks()`):** Legacy forms often store **Title-Case / mixed-case** parent `.fmb` or `.pll` references (for example `ParentForm.fmb` or `MyLib.pll`) while the staged files on disk are lowercase (`parentform.fmb`, `mylib.pll`). Without a matching mixed-case symlink in `stage/`, `frmcmp_batch` fails with **`FRM-18108: Failed to load the following objects`** on subclassed object groups, alerts, canvases, and items. Before compiling, `compile_from_manifest.py` scans `build-manifest.json` (`parentModules`, `attachedLibraries`, `menus`, `objectLibraries`) and creates exact-case symlinks in `stage/` and `forms_apps/<app_name>/`.
- **Zero extra disk space & no legacy source edits:** `MyLib.plx -> mylib.plx` and `ParentForm.fmb -> parentform.fmb` are relative pointers to the single real file on disk, so legacy `.fmb`/`.pll`/`.mmb` sources never need to be re-saved just to normalize casing.
- **Both lowercase, uppercase, and recorded mixed-case resolve:** `CALL_FORM('myform')`, `CALL_FORM('MYFORM')`, and subclassing `ParentForm.fmb` all resolve cleanly at compile time and runtime.

> [!NOTE]
> If you compile a single form directly with `compile_form.py` (bypassing `compile_from_manifest.py`) and hit `FRM-18108` on a mixed-case `Source Module: ParentForm.fmb`, create the matching symlink in `stage/` (`ln -s parentform.fmb ParentForm.fmb`) or run Stage 3 once so `ensure_manifest_case_symlinks()` populates `stage/`. If `PL/SQL ERROR 201` appears on database packages/tables, compile with a database login (`-u` / interactive prompt) rather than `--offline`.

## Verifying symlinks in `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`

1. **Confirm symlinks point to real binaries (not duplicate files):**

   ```bash
   ls -la <OFW_ORACLE_HOME>/forms_apps/<app_name>/
   ```

   Expected entries look like `MYLIB.plx -> mylib.plx`.

2. **Check for broken symlinks (for example `WEBUTIL.plx`):**

   ```bash
   find <OFW_ORACLE_HOME>/forms_apps/<app_name>/ -xtype l
   ```

   If `WEBUTIL.plx` is a broken symlink because `webutil.plx` lives in `/oracle_home_14c/forms/webutil.plx` rather than `<app_name>/`, either point it to `/oracle_home_14c/forms/webutil.plx` (inside the container) or remove the broken symlink if your forms attach lowercase `webutil`.
