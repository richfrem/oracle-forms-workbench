# Batch dependency-ordered conversion

Project-neutral pipeline (Stages 0 to 2) that turns a folder of exported Forms XML and libraries into binaries, in dependency order, outside the repository. Stage 3 (compile) is in the `compiling-and-running-oracle-forms` skill: the Batch manifest compilation reference. Dependency background: `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`.

## 3-Stage Lifecycle Definition

Visual flow and component architecture: `compiling-and-running-oracle-forms/references/batch-pipeline-stages.mmd`.

Across `build-manifest.json`, `build_dependency_manifest.py`, and `compile_from_manifest.py`:

1. **Stage 1: Decompiled (Text Source):** `*.xml` (`_fmb.xml`, `_mmb.xml`, `_olb.xml`) and `*.pld`.
2. **Stage 2: Built (Oracle 14c Native Source):** `.olb`, `.pll`, `.mmb`, `.fmb` — built in topological dependency order (least dependencies -> most dependencies) inside the 14c container with **no database logon (`logon=no`)**:
   - `.xml` -> `.olb` / `.mmb` / `.fmb` via `XML2Forms` (with `FORMS_PATH=/work:/oracle_home_14c/forms`).
   - `.pld` -> `.pll` via `frmcmp_batch module=<name>.pld output_file=<name>.pll module_type=library parse=yes logon=no batch=yes` (prefer `.pld` over existing `.pll` when `.pld` is present so every `.pll` is freshly built in 14c).
3. **Stage 3: Compiled (Runtime Executables):** `.plx`, `.mmx`, `.fmx` — compiled in the same dependency order via `compile_from_manifest.py` with user interactive database login and published to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (`.olb` is build-only and has no `.olx`).

## Before you start: start `forms-server-14c` once and keep it running

Run everything from the Ubuntu WSL shell (Windows) or the normal shell (macOS), from the repo root. **Start `forms-server-14c` once, before Stage 1, and leave it running until Stage 3 has finished.** Do not stop and restart it between phases.

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
python3 start_testbed.py --status   # read-only: is forms-server-14c Up?
python3 start_testbed.py            # starts the database and forms-server-14c; waits for WebLogic
```

How each phase uses Podman (one container, one lifecycle):

| Phase | Where it runs | Behavior |
|---|---|---|
| Stage 1 (analyze, decompile) | `podman exec -u oracle forms-server-14c` | Decompiles run in the running container when the folder is under the Oracle Home mount |
| Stage 2 (build 14c binaries) | `podman exec -u oracle forms-server-14c` | XML2Forms and `frmcmp_batch` run in the running container |
| Stage 3 (compile) | `podman exec -u oracle forms-server-14c` | `sqlplus` and `frmcmp_batch`; **required**: stops at once, with the container's real state, if it is not running |

**Fallback only:** if `forms-server-14c` is not running (checked per file), Stages 1 and 2 use a throwaway `podman run --rm ... oracle-forms-14c` container for that file; each output line names which one ran. Stage 3 has no fallback. Start the container once, before Stage 1, so the whole run uses it. Single files use `compile_form.py`, which has the same container check and the same user/alias/password prompt as Stage 3.

`start_testbed.py` also starts the local `oracle23ai-lite` database. Stage 3 does not use it: it connects only to the database alias you type. That alias must exist in the container's `tnsnames.ora` (`<Oracle Home>/user_projects/domains/base_domain/config/fmwconfig`), and the container must be able to reach that database over the network. Details: `setting-up-oracle-testbed/references/starting-the-stack.md`.

## Interactive guided workflow (agent protocol)

The agent runs these gates in order in WSL2, shows a status table at each, and asks the next question before stopping (see `.agent/rules/document-every-step.md`, "Proactive guided execution"). Everything generated goes under `OFW_WORK_DIR`.

| Gate | Agent does | Agent asks |
|---|---|---|
| 0 Intake | Ask for primary source folder(s), secondary fallback folder(s) for missing `.fmb`/`.pll`/`.mmb`/`.olb`, and the external `OFW_WORK_DIR` (default `<OFW_ORACLE_HOME>/ofw-work`). Copy into `<OFW_WORK_DIR>/source/` (originals untouched). | Duplicates: default prefer primary XML, fill gaps from secondary, exclude `.fmx`/`.plx`/`.err` and backup copies. Then: copy and run Stage 1? |
| 1 Analysis | `--analyze-only`; table of modules by type, ordered count, `unresolvedDependencies`, true `cycles`; list scratch or duplicate files. | Clean up, supply missing files, or `--allow-missing`? Then: run Stage 2? `--normalize` for hand-authored XML? |
| 2 Conversion | Stage 2 in order; converted vs failed counts. | Hand off to Stage 3: database alias, and publish to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`? |

## Stage 0: gather sources

Put the current exports (`*_fmb.xml`, `*_mmb.xml`, `*_olb.xml`, `.pll`, optionally `.pld` and binaries) in one folder. That folder is **read-only** input: the script never writes to it. Run the script without `--source-dir` in a terminal and it asks where that folder is; the sources are then copied to `<build-dir>/stage/`. The build folder must be outside the repository (default `<OFW_WORK_DIR>/build`) and must be inside the Oracle Home mount if Stage 3 will run against it (the default `OFW_WORK_DIR` is).

## Stage 1: analyze

```bash
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/build_dependency_manifest.py \
  --source-dir <source-folder> --build-dir <external-folder> --analyze-only
```

Reads XML (`AttachedLibrary`, `FormModule/@MenuModule`, `ParentFilename`, `ParentModule`; an `.olb` parent counts as an object library, anything else as a parent module; a reference to the module's own file name or internal `Name` is ignored, and parent forms resolve by internal module name too) and `.pld` (`.attach library`). Binary modules with no XML use the name-matching heuristic (may over-include). Kahn's algorithm orders providers before consumers; ties break by case-insensitive name, and file-name case is preserved. Output: `<build-dir>/build-manifest.json`, the one JSON for the whole pipeline (its shape is `assets/build-manifest.template.json`; there is no separate schema file).

**Undeclared library use (inferred attachments).** Older exports (for example 12c `Forms2XML`) can omit `<AttachedLibrary>` lines that the binary has, so a form or menu calls a library package that its XML never declares and Stage 3 fails with `PL/SQL ERROR 201 identifier 'PKG.PROC' must be declared`. Stage 1 therefore reads the top-level packages, procedures and functions of every `.pld` (units that start in column 1), and the identifiers in each form's and menu's PL/SQL attributes (`*Text`, `*Code`; strings, comments and package members after a dot are ignored; units the module defines itself shadow library units). A library whose unit is used but not attached is added to the module's `attachedLibraries` (so ordering, the dependency closure and case symlinks include it) and recorded as `inferredAttachedLibraries` (`library`, `uses`). Stage 2 writes the matching `<AttachedLibrary Name="LIB" LibraryLocation="lib" LibrarySource="File"/>` into the **staged copy** only, after any leading `Coordinate`/`Alert` elements; the source folder is never changed. A unit name defined by more than one library is not inferred; it is listed in `ambiguousLibraryUnits` for a person to attach. Object libraries are never given inferred attachments (they are not compiled; the forms that use their objects attach the library). `--no-infer-libraries` turns this off. Only static calls are seen; a call built at run time is not.

**One form and its dependencies (`--target`).** `--target <module>` (repeatable; a module name, file name or `_fmb.xml` name, a form preferred when several types share the name) keeps only that module and everything it depends on, followed recursively: attached libraries (declared and inferred), the libraries those attach (`.attach library` in `.pld`), object libraries, parent modules and menus. Stage 2 then stages and converts only those modules, so one form can be built from a large source folder. Forms reached only by `CALL_FORM`/`OPEN_FORM` are run-time calls, not build dependencies, and are not included; target them too when needed.

**Original binaries first (`--prefer-binaries`).** When a module has both an XML export and its original `.fmb`/`.mmb`/`.olb` in the source folder, the XML wins by default. With `--prefer-binaries` the original binary is staged and compiled as is (no `XML2Forms` round trip, which loses inherited content, inherited canvas colours and object groups), while its dependency list still comes from the XML export (exact, unlike the binary name heuristic). Library inference still runs on them for ordering (an older XML export can omit the attachment the binary has, which would otherwise put a form ahead of its library), but the staged binary is never edited. Use it whenever the originals are available; it is the documented fix for forms that inherit.

**Waves.** Each module gets a `wave`: wave 1 holds modules with no dependency in the manifest, wave N those whose dependencies are all in earlier waves. Modules in one wave do not depend on each other. Stage 1 prints the waves; Stage 3 compiles wave by wave.

```bash
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/build_dependency_manifest.py \
  --source-dir <source-folder> --build-dir <external-folder> --target <form> --analyze-only
```

Each unresolved dependency lists `suggestions` (present modules with a shared prefix or close spelling): a form can reference an old file name while the current file is named slightly differently. `needsReview` has one entry per module with a missing dependency, plus `mentionedBy` (other source files whose text names it, a hint whether it is still used; calls built from variables or stored in a database are not seen). Nothing is changed automatically; a person decides to substitute, detach or skip.

## Decompile binary-only modules (optional gate)

**Decompile only what the server export does not already cover.** An export from the Forms server is the authority: its XML (forms, menus, object libraries) and its library text (PL/SQL libraries are often exported as `.txt`, not `.pld`) are used as they are and are never re-decompiled. Before running anything:

1. Rename export library text (`.txt` holding PL/SQL, usually starting with `.attach library` or `PACKAGE`/`PROCEDURE`) to `.pld`. The scanner only recognizes `.pld`; an unrenamed `.txt` makes its library look binary-only and gets decompiled for nothing.
2. List the modules that have a binary but no XML and no `.pld`. That list, and only that list, is what gets decompiled. Show it and wait for approval; if the user says "use what is already exported", the list is exactly the gap.
3. Keep **one** folder as the full text set (every XML and `.pld`, exported and decompiled alike), normally `<build-dir>/decompiled/out/`. Copy the exported files into it next to the decompiled ones, so the user finds the whole set in one place. Record in the manifest which are exported and which were decompiled.
4. Never clear, move or rename folders without saying so first, and report counts only after verifying them.
5. **Validate after decompile.** Add `--validate-xml` (alone or with `--decompile-binaries`): each module's XML (its source XML, else its decompiled XML) is checked against the 14c schema with `validate_forms_xml.py`, and the result is written into the module's `validated` record in the manifest. Fix or exclude failures before Stage 2. `.pld` text has no schema check.

```bash
python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/build_dependency_manifest.py \
  --source-dir <source-folder> --build-dir <external-folder> --decompile-binaries
```

For every module with no XML (`requiresConversion: false`: libraries, or forms/menus/object libraries that exist only as binaries) it copies the binary to `<build-dir>/decompiled/in/`, decompiles it (`.pll` to `.pld`; others to XML) in the running `forms-server-14c` (throwaway container only as a fallback) into `<build-dir>/decompiled/out/`, and writes the result straight into each module's `decompiled` record (`status` done or failed, `output`) in `build-manifest.json`, saved after every module. **There is one JSON for the whole pipeline:** each stage writes only its own record on each module (`decompiled`, `validated`, `status` for convert and compile), and re-running Stage 1 carries the `decompiled` and `validated` records over. The manifest therefore shows which modules are fully available as text. Review the decompiled output, then copy it into the source folder and re-run Stage 1. The source folder is never written to.

It fails closed (exit 2, manifest still written for diagnosis) on a **cycle**, or on a **missing provider** unless `--allow-missing`. Cycles are the real strongly connected loops only (Tarjan), not everything downstream of them. When ordering stalls, heuristic edges of binary-only modules that sit inside a loop are dropped with a note on stderr (name-matching can over-include, for example versioned copies of one library "attaching" each other); XML-derived edges are never dropped. When a `.pld` file exists in the sources, it is treated as authoritative and built into a `.pll` binary during Stage 2 without database logon.

## Stage 2: convert

`--convert-only` runs Stage 2 alone against the existing `build-manifest.json`: no Stage 1 re-run, no re-staging, and no fail-closed check on unresolved providers (their dependents simply fail to convert or compile later). Without `--analyze-only` the script copies sources to `<build-dir>/stage/` and converts modules in manifest (topological) order, updating `status` (`converted` or `failed`). For `.xml` files (`OLB`, `MMB`, `FMB`), it runs `XML2Forms`. For `.pld` text libraries, it runs `frmcmp_batch module=<name>.pld output_file=<name>.pll module_type=library parse=yes logon=no batch=yes`. Raw `.pll` binaries (without `.pld`) are staged as they are. `--normalize` applies the Oracle-compatibility copy used by Preview (`prepare_oracle_preview_xml`) for 12c version headers or hand-written XML.

## Not verified

Unit-tested with synthetic fixtures (order, cycles, missing providers, external-only build dir, inferred attachments, `--target` closure, waves). Stages 1 and 2 have been run end to end on a 14-module external chain (object libraries, five libraries, two menus, forms with parent modules) in `forms-server-14c`: all 14 converted. Library inference, run on that chain's unpatched sources, found the same three missing attachments that the compiler errors had shown (two forms and one menu). Not verified: inference across a whole corpus (false positives from identifiers that only look like library units), and the `--target` path in Stage 2 on a real folder.

## Conversion details

**Inherited canvases turn black.** A canvas that subclasses an object-library canvas and has no `BackColor` of its own is stored without a colour by `XML2Forms` on headless Linux and renders black after Stage 3 (named colours such as `gray` are also unreliable). Add an explicit `BackColor` to such canvases in the staged copies. Check before Stage 2 with `grep -o -E '(Back|Fore)Color="[^"]*"' <source>/*.xml | sort | uniq -c`. Fix and recipe: `converting-oracle-artifacts/references/conversion-notes.md` ("Named Color Palette Loss"); prefer the original binaries when they exist.

XML2Forms runs with `FORMS_PATH=/work:<oracle home>/forms` so staged providers resolve. Subclassing parents are read from both `ParentFilename` and `ParentModule`. After conversion the binary is located case-insensitively in `stage/` and the manifest's `binaryFile`/`executableFile` are updated if Oracle wrote a different case.

