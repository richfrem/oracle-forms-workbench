# Bringing up legacy forms that inherit from other modules

Use this when a form from an older Oracle Forms release must be extracted, compared, compiled and run in the 14c container, especially when it subclasses objects from other forms or libraries. Evidence is from real runs in WSL; items marked "not verified" were not run. Platform notes are in `forms-path-and-dependencies.md`.

## 1. Rule of thumb

Compile the original `.fmb` with all of its dependencies in one folder. Use XML only to read, compare or deliberately edit a form. Do not convert an inheriting form through XML and expect an equal form back.

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

## 4. Build with dependencies in the same folder

The compiler resolves inherited objects, attached libraries, menus and object libraries by searching the current folder, then `FORMS_PATH`.

| Dependency | Needed at compile time | Needed at run time |
|---|---|---|
| Parent forms the module subclasses from (`.fmb`) | Yes, the `.fmb` itself | No |
| Attached libraries (`.pll`) | Yes | Compiled `.plx` in the run folder |
| Menus (`.mmb`) | Yes | Compiled `.mmx` |
| Object libraries (`.olb`) | Yes | No |
| Forms it opens or calls (`call_form`, `open_form`) | No | Compiled `.fmx`, named exactly as called |

Symptoms of a missing parent form: `FRM-18108: Failed to load the following objects` (it lists the source module and object), then many follow-on `bad bind variable '...'` errors. `compile_form.py` stages only `.pll`, `.olb` and `.mmb` files (`_FORMS_DEP_EXTS`), so parent `.fmb` files must be copied into `<Oracle Home>/ofw-work/stage/` by hand until staging is extended. Proposed, not implemented: stage a module's parent `.fmb` files on demand from its declared parents, recursively, lower-cased, failing clearly if one is missing.

Compile order: libraries, then menus, then forms. Use the database account that can see the objects the PL/SQL references.

## 4a. Why XML round trips fail for inheriting forms

- XML2Forms can crash with `Cannot invoke "java.lang.Integer.intValue()" because "<local1>" is null` when an `ObjectGroup` lists children that exist only in a parent module (inherited children have no object in the child's XML). Removing those `ObjectGroup` elements lets it convert, but inherited content is then lost.
- A form built this way can open blank, and its binary is much smaller than the original (82 KB against 192 KB in one case).
- A 12c-style export has no `AttachedLibrary` lines; add them before compiling.
- XML2Forms writes `<xml-name>.fmb` into `OUTPUTDIR`.

Hand edits via XML are acceptable only for forms with no inheritance, or for throwaway diagnostic copies kept in a scratch folder.

## 5. Run, and pass form parameters

```sh
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py <form>.fmx -u USER@alias [--param NAME=VALUE]
```

- A form must be compiled and its `.fmx` copied to `<Oracle Home>/forms/` (a compile publishes it unless `--no-publish`; with `--no-publish` copy with `XDG_RUNTIME_DIR=/run/user/$(id -u) podman unshare cp <fmx> <Oracle Home>/forms/`).
- A form that calls another form by name (`call_form('x')`) needs `x.fmx` in the run folder under exactly that name; `FRM-40010 Cannot read form x` otherwise.
- `--param` appends `&NAME=VALUE` to the launch URL (before `userid`). Whether an extra URL parameter reaches the running form was **not verified**; the form's module parameter default is the reliable path.
- Logon strings use an Oracle Net alias that must exist in `tnsnames.ora`; the login form's database dropdown can hard-code production aliases that the container lacks (use `-u USER@alias`).

## 6. Diagnosing behaviour inside a form (scratch copy only)

1. Export the module to XML (section 2) into a scratch folder; never edit the original or the source folder.
2. Add alerts that show the session (`sys_context('USERENV', ...)`, `user`, `session_roles`) and the key variables; run diagnostic queries through a record group (`create_group_from_query`, `populate_group`, `Get_Group_Char_Cell('GROUP.COLUMN', row)`; there is no `Get_Group_Column`).
3. Convert with XML2Forms (remove dangling `ObjectGroup`s, add `AttachedLibrary` lines if needed), compile with the right account and run.
4. Program-unit text in XML: newline `&amp;#10;`, `<` `&lt;`, `&` `&amp;`, tab `&#x9;`.
5. A query that fails inside a silent-query helper (message level 5) shows as zero rows with no error; reproduce it in SQL*Plus as the same user and roles.

Remove the debug copy and its `.fmx` from `<Oracle Home>/forms/` when done.

## 7. Known runtime messages that are setup, not code

| Message | Cause |
|---|---|
| `FRM-40010 Cannot read form x` | `x.fmx` not in the run folder under that name |
| `WebUtil Error: ... CApiFunctions bean not found` | The WebUtil client jar is not in the launch configuration's archive list (`frmwebutil.jar` and `jacob.jar`, signed); the `standaloneapp` config has none. Not fixed; needs a configuration change in the Oracle Home |
| Blank message window | Built from an XML round trip; compile the original `.fmb` |
| `ORA-01001` raised in a trigger's error path | Original code closing a cursor it never opened; a source bug, not a setup problem |

Verified: after copying the parent `.fmb` files into the stage folder, a menu form that had failed with `FRM-18108` compiled successfully and published its `.fmx`. Not verified: that this alone makes every inheriting form compile; the WebUtil fix; `--param` delivery.
