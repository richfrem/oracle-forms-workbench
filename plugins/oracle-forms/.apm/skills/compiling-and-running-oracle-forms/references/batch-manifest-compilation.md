# Batch manifest compilation (Stage 3)

Compiles what `converting-oracle-artifacts/scripts/build_dependency_manifest.py` staged (see that skill's Batch dependency-ordered conversion reference), in the manifest's dependency order.

```bash
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_from_manifest.py \
  --build-dir <external-folder> [--userid <user>@<db>] [--offline] [--no-publish] [--skip-preflight]
```

## Login

The user logs in directly against the target database. Missing pieces are asked for at the terminal (`input()` for user and connect alias, `getpass` for the password); `DB_USER`/`DB_PASSWORD`/`DB_SERVICE` from the git-ignored `.env` are used when present. The password is never put on the command line or in a file, and is removed from printed output and stored diagnostics. `--userid user@db` still prompts for the password.

## Preflight

Unless `--skip-preflight` or `--offline`, `sqlplus -L -S` runs inside `forms-server-14c` (`TNS_ADMIN` set, the logon sent on stdin) and counts `session_roles` and `all_objects` for the account. Failure stops the run (exit 2). A successful logon is **not** proof that the account sees every object the libraries use; a compile error `PL/SQL ERROR 201 ... must be declared` means it does not.

## Compile

For each module in order: `.olb` is skipped (provider only); `.pll` compiles with `module_type=library`, `.mmb` with `menu`, `.fmb` with `form` through `compile_form.build_compiler_command`, with `FORMS_PATH` = `/oracle_home_14c/forms` plus the module's folder (`<build-dir>/stage`). A module whose provider failed or was skipped is marked `skipped`. Each result and diagnostics are written back to the manifest.

## Publish

Compiled `.plx`, `.mmx`, `.fmx` are copied to `<Oracle Home>/forms/` (the Forms server's `FORMS_PATH`) unless `--no-publish`. `<build-dir>` must be under the Oracle Home or repo mount so the container can see it; otherwise the script says so.

## Not verified

Unit-tested with mocks only (order, credential prompt, skip-on-failure). No compile through this script against a database has been verified.

## Publishing and early checks

Publishing a `.mmx` or `.plx` also creates a case-variant symlink in `<oracle home>/forms/` when the other case is absent (Linux case sensitivity: FRM-10221 / PDE-PLI018). A `--build-dir` not visible to the container is rejected before any credential prompt; use a folder under `OFW_WORK_DIR`.

