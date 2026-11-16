# `FORMS_PATH`, External Dependencies, and Target Databases

Use this guide when compiling or running Forms modules that depend on other forms, menus, PL/SQL libraries, or object libraries, including when connecting the Forms container to a non-local database.

## External sources and staging

Treat every directory in `OFW_FORMS_PATH_EXTRA` and every other external project directory as read-only. Never copy external artifacts into the repository, including `samples/`, `temp/`, `context/`, `tests/`, or `oracle-reference-stack/`; never write compilation outputs or diagnostics there. See [cross-project-isolation.md](../../../../../../.agent/rules/cross-project-isolation.md). For a whole project, use the manifest-driven batch pipeline ([batch-manifest-compilation.md](batch-manifest-compilation.md)) instead of hand-staging.

Set `OFW_WORK_DIR` outside the repository and somewhere the Forms container can access. The supported arrangement is under the configured Oracle Home, which is mounted as `/oracle_home_14c`; the scripts use `<OFW_WORK_DIR>/stage/` for working files. The scripts reject a work directory that resolves inside the repository.

Set `OFW_FORMS_PATH_EXTRA` to one or more read-only source directories (colon-separated in WSL/Linux and macOS). `compile_form.py` and `run_form.py` stage the input and required companion sources into the work folder:

- Attached `.pll`, `.mmb`, and `.olb` source files are staged with the module.
- Parent `.fmb` candidates are found recursively in the input directory and extra source directories and copied with their original filename case. XML references use `ParentFilename` and `ParentModule`; binary FMB references are matched heuristically against candidate filenames, so a candidate may be staged if its name also appears elsewhere in the binary.
- Source files are read, not modified. The scripts do not copy all unrelated FMB files.

Compile-time `FORMS_PATH` orders the dedicated app output folder (`<OFW_ORACLE_HOME>/forms_apps/<app_name>/`), the staged folder (`stage/`), the module folder, and `/oracle_home_14c/forms` (last, for Oracle product libraries such as `webutil.pll`). This makes custom `.pll`/`.olb` dependencies resolve before Oracle system defaults. XML2Forms parent resolution itself has not been verified; keep the original binaries and check conversion results.

Compiled `.fmx`, `.mmx`, and `.plx` outputs are published to `<OFW_ORACLE_HOME>/forms_apps/<app_name>/` (default app: `default`, or `--app-name` / `OFW_APP_NAME`) unless publication is explicitly disabled (`--no-publish`). Keep `/oracle_home_14c/forms` strictly for Oracle product binaries (`webutil.*`, `frmsal.jar`, etc.). The Forms server's runtime `FORMS_PATH` is read from:

`<OFW_ORACLE_HOME>/user_projects/domains/base_domain/config/fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env`

The container `FORMS_PATH` places `<OFW_ORACLE_HOME>/forms_apps/<app_name>` and `<OFW_WORK_DIR>/stage` ahead of `/oracle_home_14c/forms`. Do not add repository or external source directories to this runtime setting; publish runtime executables into the dedicated app directory instead.

## Check database reachability from the Forms container

A successful host-side connection does not prove that `forms-server-14c` can resolve or reach the database. Check from the container that runs Forms, using the same target connect identifier and `TNS_ADMIN` used by the Forms configuration:

```sh
podman exec forms-server-14c grep '^TNS_ADMIN=' \
  /oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env
podman exec forms-server-14c /bin/sh -c 'test -r /oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/tnsnames.ora'
podman exec -it forms-server-14c /oracle_home_14c/bin/sqlplus -L -S /nolog
```

At the SQL*Plus prompt, connect using the target alias or Easy Connect identifier, then run `SELECT 1 FROM DUAL;`. Supply the password only at a protected prompt; do not put credentials in command lines, shell history, documentation, or logs. If available, `getent hosts <database-host>` can help distinguish DNS failure from listener or service failure. A successful SQL*Plus login checks connectivity, not whether the compiler account can resolve every application object.

For TNS aliases, verify that the alias exists in the `tnsnames.ora` file under the `TNS_ADMIN` directory visible inside the container. For Easy Connect, verify container DNS/routing to the host and listener port. VPN or host network access does not automatically imply container network access.

## Check compiler-account object visibility

Run object checks using the same account and target database that `frmcmp_batch` will use. A successful login alone does not establish that referenced tables, views, packages, synonyms, or required grants are available to the compiler.

```sql
SELECT owner, object_name, object_type
FROM all_objects
WHERE object_name = UPPER('<OBJECT_NAME>');

SELECT role
FROM session_roles
ORDER BY role;
```

Where applicable, inspect the account's object privileges as well (for example, `ALL_TAB_PRIVS`). Confirm that dependencies resolve under the compiler account and that required privileges are granted in a way usable by compilation; enabled roles are useful evidence but are not a substitute for required direct grants. `compile_form.py` quotes special characters in the `userid` password field; never paste real credentials into docs or test fixtures.

## Runtime parameters and entry-form flows

Both `launch_fsal.py` and `run_form.py` accept repeatable `--param NAME=VALUE` options. Each name and value is URL-encoded for the servlet request. For example, use synthetic names and values while testing:

```sh
python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/run_form.py <form>.fmb \
  --param P_MODE=review --param P_LABEL="North & South"
```

Do not treat individual `--param` query parameters as interchangeable with the Forms servlet's `otherparams` argument string. When reproducing an application URL that uses `otherparams`, encode that argument according to the URL contract and verify what the target form receives; URL construction alone does not prove runtime delivery.

Test both direct launch and the intended entry-form/navigation flow. Entry forms may initialize globals, parameters, user/session state, or database context that a dependent form does not establish when launched alone.

## Verification status

- Parent FMB staging and repository-local `OFW_WORK_DIR` rejection have synthetic unit tests.
- Live XML2Forms parent resolution, container-to-development-database reachability, compiler-account object visibility, and runtime delivery of supplied parameters require a running Forms/database environment and have not been verified in this guide.
