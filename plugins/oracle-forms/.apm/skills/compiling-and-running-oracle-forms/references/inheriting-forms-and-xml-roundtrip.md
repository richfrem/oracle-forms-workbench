# Bringing up legacy forms that inherit from other modules

Use this when a form from an older Oracle Forms release must be extracted, compared, compiled and run in the 14c container, especially when it subclasses objects from other forms or libraries. Evidence is from real runs in WSL; items marked "not verified" were not run. Platform notes are in `forms-path-and-dependencies.md`.

## 1. Rule of thumb

Keep the original `.fmb` and required dependencies available to the compiler, with a flat staging folder on `FORMS_PATH` as the simplest reproducible arrangement. This is a practical lookup strategy, not a universal same-folder requirement; the exact lookup precedence among embedded paths, the current working directory, and `FORMS_PATH` is tool-dependent and not fully verified. Use XML to read, compare, or deliberately edit a form, but do not assume an XML round-trip preserves inherited content.

## 2. Extract a binary module to XML (read-only inspection)

Run the Forms2XML class in the container from a scratch folder (host path under `<Oracle Home>/ofw-work/<scratch>`, container path `/oracle_home_14c/ofw-work/<scratch>`; give the container path, not the host path):

```sh
podman exec forms-server-14c /bin/sh -c 'cd <dir>; export ORACLE_HOME=/oracle_home_14c FORMS_API_TK_BYPASS=TRUE LD_LIBRARY_PATH=/oracle_home_14c/lib FORMS_PATH=<dir>:/oracle_home_14c/forms; /usr/lib/jvm/jdk-17.0.12/bin/java -Djava.library.path=/oracle_home_14c/lib -classpath /oracle_home_14c/jlib/frmjdapi.jar:/oracle_home_14c/jlib/frmxmltools.jar:/oracle_home_14c/oracle_common/modules/oracle.xdk/xmlparserv2.jar oracle.forms.util.xmltools.Forms2XML OVERWRITE=YES <dir>/<module>.fmb'
```

`convert_fmb.py` (skill `converting-oracle-artifacts`) does the same. The scratch folder must be writable by the container user (`chmod 777` folder, `666` files). `ERROR Object group child ... has no real object` printed for a form that inherits is expected: the children live in the parent module.

## 3. Compare two XML versions of a module

Strip carriage returns and diff:

```sh
diff <(tr -d '\r' < exported_from_binary.xml) <(tr -d '\r' < older_export.xml)
```

Expected harmless differences between an older release export and a 14c export of the same binary: the `<Module version="...">` header and `AttachedLibrary` lines that only the 14c export writes. Anything else is a real difference. Identical modules need no re-conversion.

## 4. Build with dependencies discoverable to the compiler

The observed workflow succeeds when inherited objects, attached libraries, menus, and object libraries are discoverable from the staged module folder or `FORMS_PATH`. A single flat staging folder is the least ambiguous setup, especially when embedded authoring paths refer to a different operating system or directory layout. Exact resolution order has not been exhaustively tested.

| Dependency | Needed at compile time | Needed at run time |
|---|---|---|
| Parent forms the module subclasses from (`.fmb`) | Yes, the `.fmb` itself | No |
| Attached libraries (`.pll`) | Yes | Compiled `.plx` in the run folder |
| Menus (`.mmb`) | Yes | Compiled `.mmx` |
| Object libraries (`.olb`) | Yes | No |
| Forms it opens or calls (`call_form`, `open_form`) | No | Compiled `.fmx`, named exactly as called |

Symptoms of a missing parent form: `FRM-18108: Failed to load the following objects` (it lists the source module and object), then many follow-on `bad bind variable '...'` errors. `compile_form.py` and `run_form.py` now stage referenced parent `.fmb` sources recursively from the input directory and `OFW_FORMS_PATH_EXTRA` into `<OFW_WORK_DIR>/stage/`, preserving the source filename case. XML parent references are read from `ParentFilename`/`ParentModule`; binary FMB references are matched against candidate filenames heuristically and may over-include a candidate whose stem appears elsewhere in the binary. Parent sources remain read-only.

Build a dependency graph from the module references and compile each provider before its consumers. Libraries often precede menus and forms, but do not force a fixed artifact-type order when actual references establish a different order. Use the database account that can see the objects the PL/SQL references. Object libraries are staged, not compiled as executables.

When converting XML, stage referenced parent form and object-library sources before converting a dependent module as a precaution. Whether XML2Forms always resolves these references at conversion time is not verified; inspect conversion results and retain the original binaries.

## 4a. Why XML round trips fail for inheriting forms

- XML2Forms can crash with `Cannot invoke "java.lang.Integer.intValue()" because "<local1>" is null` when an `ObjectGroup` lists children that exist only in a parent module (inherited children have no object in the child's XML). Removing those `ObjectGroup` elements lets it convert, but inherited content is then lost.
- A form built this way can open blank, and its binary is much smaller than the original (82 KB against 192 KB in one case).
- Canvases that inherit their colour from an object library (no `BackColor` of their own) render black after `XML2Forms` on Linux. Use the original binaries, or add an explicit `BackColor` (for example `r75g75b75`) to each such canvas in the staged XML and replace named colours, converting the object library before the forms that subclass it (verified) (`converting-oracle-artifacts/references/conversion-notes.md`).
- A 12c-style export has no `AttachedLibrary` lines; add them before compiling. The manifest pipeline finds them for you: Stage 1 infers each library a form or menu calls without attaching it and Stage 2 adds the tag to the staged copy (`converting-oracle-artifacts/references/batch-dependency-conversion.md`, "Undeclared library use").
- XML2Forms writes `<xml-name>.fmb` into `OUTPUTDIR`.

Hand edits via XML are acceptable only for forms with no inheritance, or for throwaway diagnostic copies kept in a work folder outside the repository and external source tree.

## 5. Run, and pass form parameters

```sh
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py <form>.fmx -u USER@alias [--param NAME=VALUE]
```

- A form must be compiled with its `.fmx` in `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`, registered on runtime `FORMS_PATH`. Compilation publishes there unless `--no-publish`; that option retains outputs in staging. Keep application binaries out of the Oracle product `forms/` directory.
- A form that calls another form by name (`call_form('x')`) needs `x.fmx` in a runtime-visible directory under exactly that name; `FRM-40010 Cannot read form x` otherwise.
- `--param NAME=VALUE` adds a URL-encoded launch parameter. Verify that each supplied value reaches the target form; URL construction alone does not establish runtime delivery. Also test the normal entry form and navigation path when the application initializes globals, session state, or database context there.
- Logon strings use an Oracle Net alias that must exist in `tnsnames.ora`; the login form's database dropdown can hard-code production aliases that the container lacks (use `-u USER@alias`).

## 6. Diagnosing behaviour inside a form (scratch copy only)

1. Export the module to XML (section 2) into a scratch folder; never edit the original or the source folder.
2. Add alerts that show the session (`sys_context('USERENV', ...)`, `user`, `session_roles`) and the key variables; run diagnostic queries through a record group (`create_group_from_query`, `populate_group`, `Get_Group_Char_Cell('GROUP.COLUMN', row)`; there is no `Get_Group_Column`).
3. Convert with XML2Forms (remove dangling `ObjectGroup`s, add `AttachedLibrary` lines if needed), compile with the right account and run.
4. Program-unit text in XML: newline `&amp;#10;`, `<` `&lt;`, `&` `&amp;`, tab `&#x9;`.
5. A query that fails inside a silent-query helper (message level 5) shows as zero rows with no error; reproduce it in SQL*Plus as the same user and roles.

Remove diagnostic artifacts only with operator approval, from their external scratch folder and the dedicated application deployment directory.

## 7. Known runtime messages that are setup, not code

| Message | Cause |
|---|---|
| `FRM-40010 Cannot read form x` | `x.fmx` not in the run folder under that name |
| `WebUtil Error: ... CApiFunctions bean not found` | The WebUtil client jar is not in the launch configuration's archive list (`frmwebutil.jar` and `jacob.jar`, signed); the `standaloneapp` config has none. Not fixed; needs a configuration change in the Oracle Home |
| Blank message window | Built from an XML round trip; compile the original `.fmb` |
| `ORA-01001` raised in a trigger's error path | Original code closing a cursor it never opened; a source bug, not a setup problem |

Verified: after copying the parent `.fmb` files into the stage folder, a menu form that had failed with `FRM-18108` compiled successfully and published its `.fmx`. Not verified: that this alone makes every inheriting form compile; the WebUtil fix; `--param` delivery.
