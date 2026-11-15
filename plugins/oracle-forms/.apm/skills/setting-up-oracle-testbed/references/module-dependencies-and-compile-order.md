# Module dependencies: convert and compile

What was learned running Oracle's Forms conversion and compile tools in the 14.1.2 container, and how to avoid the silent failures. Evidence level is stated for each point; anything not run is "not verified".

## Contents

- [Dependencies must sit beside the input (verified)](#dependencies-must-sit-beside-the-input-verified)
- [Which tool handles which file](#which-tool-handles-which-file)
- [Finding the dependencies](#finding-the-dependencies)
- [Compile order and why compiles fail (verified)](#compile-order-and-why-compiles-fail-verified)
- [Diagnosing a silent failure](#diagnosing-a-silent-failure)

## Dependencies must sit beside the input (verified)

`frmcmp_batch module=<lib>.pll module_type=library script=yes` (PLL to PLD) exits with code 0, prints nothing, and writes no file when the libraries that the `.pll` attaches are not in the same folder. With the attached libraries copied next to the input, the same unchanged command writes the `.pld`. The failure is silent, so a missing output file is the only sign.

What did **not** fix it (each tried, same silent result): `output_file=`, running as another user, a writable folder, running in the long-lived `forms-server-14c` container instead of a throwaway one, a database `userid=` (the exit code changed from 0 to 1, still no output), and `parse=yes`.

Also verified: a Forms2XML run (`.fmb`, `.mmb`, `.olb` to XML) works without dependencies present, but reports less. With attached object libraries absent it prints `ERROR Object group child <name> has no real object` for objects that live in them. Those errors are reported by the tool and the XML is still written; the object-library content is not merged into it. Whether supplying every missing object library removes all of those messages is not verified.

Wrong claim to ignore: Forms2XML does not take `.pll` input. PLL to text goes through `frmcmp_batch` only.

## Which tool handles which file

| Input | Tool | Output |
|---|---|---|
| `.fmb`, `.mmb`, `.olb` | Forms2XML Java class (`convert_fmb.py`) | `<name>_fmb.xml`, `_mmb.xml`, `_olb.xml` |
| `.pll` | `frmcmp_batch ... script=yes` (`convert_pll.py`) | `<name>.pld` |

`script=yes` and `parse=yes` are opposite directions (`script=yes` writes text; `parse=yes` reads text), so do not combine them to export.

## Finding the dependencies

Attachments are stored as plain names inside the binary module. `converter_runtime.py` (`find_dependencies`, `stage_for_conversion`) does this:

1. Scan the module for names that match a `.pll`, `.olb` or `.mmb` in the search folders (the input's folder plus `OFW_FORMS_PATH_EXTRA`).
2. Repeat for every dependency found.
3. If anything lives outside the input's folder, copy the module and its dependencies into `<work folder>/convert-stage/<name>/` (`OFW_WORK_DIR`, default `<Oracle Home>/ofw-work`; never inside the repository) and convert there. Never copy into `samples/` or into the source folder.
4. Run the conversion.

It is a heuristic: a name that only appears in code can pull in an extra file. Extra files in the staging folder are harmless. After a PLL export, the `.attach LIBRARY <name>` lines at the top of the `.pld` are the exact list and are the reliable source.

`convert_pll.py` also prints the container exit code and a FAILED line when no `.pld` appears, and has `--no-batch` (omits `batch=yes`) and `--userid-from-env` (adds `userid`, `parse=yes`, `output_file`; not verified to be needed).

Batch result: a folder of 24 libraries, 4 menus and 2 object libraries was converted one file at a time this way, with no failures.

## Compile order and why compiles fail (verified)

**The compiling database account decides the result, not the tool, the container or the folder.** The same library, in the same folder, compiled with one database account and failed with another, both in the container's `frmcmp_batch` and in a desktop Forms Builder (12.2) connected to the same database.

- A failing account gives `PL/SQL ERROR 201 ... identifier '<NAME>' must be declared` for every database object the library uses by bare name (packages, views, tables, and Oracle packages such as `DBMS_LOCK`), then `FRM-30312: Failed to compile the library`. No output file is written.
- A working account compiles the same libraries with `Compilation SUCCESSFUL`, with no application login step.
- The compiler resolves bare names in the connected account's session: its own schema, then private synonyms, then public synonyms, and the account must hold `EXECUTE`/`SELECT` on the target, directly or through roles that are enabled by default. A public synonym existing is not enough.
- A database logon alone (the Oracle Database banner prints) does not mean the account can see the objects. `compile_all=no` does not avoid the errors. Offline (`--offline`, `logon=no`) only works for libraries that use no database objects.
- To check an account before compiling, run as that account: `select owner, object_name, object_type from all_objects where object_name in (...)` and `select * from session_roles`, and compare with an account that compiles.

**Module type matters.** `compile_form.py` needs `--type library` (`.pll`), `--type menu` (`.mmb`) or `--type form` (`.fmb`; the default). A library compiled as a form fails with `FRM-10043: Cannot open file`.

**Order and folder.** Compile modules with no attachments first, then the ones that attach them, then menus and forms. Keep every attached library, object library and menu in one folder. `compile_form.py` does this itself: it copies the module and the `.pll`, `.olb` and `.mmb` files found beside it and in `OFW_FORMS_PATH_EXTRA` into `<work folder>/stage/` (the work folder is `OFW_WORK_DIR`, default `<Oracle Home>/ofw-work`, mounted in the container as `/oracle_home_14c/ofw-work`), compiles there, and then publishes the `.fmx`, `.mmx` or `.plx` to `<Oracle Home>/forms/`. Nothing is written in the repository. `--no-publish` skips the copy to `forms/`. Whether the attachments being present is required for compiling, as it is for PLL export, was not separated out; they were always present.

**Compile location is not runtime location.** Compiling happens in the work folder. At run time the Forms server loads compiled `.plx`, `.mmx` and `.fmx` files from the `FORMS_PATH` in the domain `default.env` (by default `<Oracle Home>/forms`, plus any extra folder configured through `OFW_FORMS_PATH_EXTRA`). `compile_form.py` copies them there when a compile succeeds. See `compiling-and-running-oracle-forms/references/forms-path-and-dependencies.md`.

**Cross-check.** If a compile fails, repeat it in a desktop Forms Builder with the same account (Program > Compile > All). Same errors in both means the account's visibility; different results means the container setup. A Builder-built `.plx` is the Builder's version and may not load in a different Forms version.

**Credentials.** Passwords with characters other than letters, digits, `_`, `#` or `$` must be wrapped in double quotes inside `userid=`. `compile_form.py` and `run_form.py` do this (`format_userid`). Prompt for the password instead of typing it in a command, and never put it in tracked files.

## Diagnosing a silent failure

1. Check the output file exists; do not trust exit code 0.
2. Put every attached module in the input's folder and retry.
3. Run the same command in a throwaway container and in the long-lived Forms container; if both behave alike the image is not the cause.
4. If a command is pasted into a terminal that wraps long lines, an `export` can be split so `LD_LIBRARY_PATH` is not exported; the error is then `error while loading shared libraries: libig.so.0`. Use one unbroken line.
5. `frmcmp_batch` prints nothing for a missing module too, so check the path first.
