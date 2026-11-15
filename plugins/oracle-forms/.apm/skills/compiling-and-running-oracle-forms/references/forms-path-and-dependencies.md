# `FORMS_PATH`, External Dependencies, and Target Databases

> **Update:** `compile_form.py` and `run_form.py` no longer compile inside the repository or in `temp/`. They stage the module and its attachments in `<work folder>/stage/` (`OFW_WORK_DIR`, default `<Oracle Home>/ofw-work`, seen in the container as `/oracle_home_14c/ofw-work`), compile there, and publish `.fmx`/`.mmx`/`.plx` to `<Oracle Home>/forms/`. The `/work/temp/...` paths below describe the earlier layout.

Use this guide when compiling or running a form against a development database where the form depends on other `.fmb`/`.fmx` forms, `.mmb`/`.mmx` menus, `.pll`/`.plx` PL/SQL libraries, or `.olb` object libraries.

> **Platform note:** On **Windows**, run all `podman` and `python3` commands inside the **Ubuntu WSL2 shell** (not PowerShell). On **macOS**, run them from your normal terminal inside the Podman machine setup.

## 1. How container paths map to your machine

In `oracle-reference-stack/compose.yaml`, `forms-server-14c` mounts two host folders:

| Container path | Host / WSL path | Purpose |
|---|---|---|
| `/work` | Repository root | Your checked-out repo (`samples/`, `temp/`, etc.) |
| `/oracle_home_14c` | `$OFW_ORACLE_HOME` from `.env` (default `oracle-reference-stack/oracle_home_14c`; on Windows WSL typically `~/oracle_home_14c`) | Persistent Oracle Home, WebLogic domain, `forms/`, and `tnsnames.ora` |

## 2. Where `FORMS_PATH` is set (compile time vs. runtime)

`FORMS_PATH` is configured in **two** places:

1. **Compile time (`frmcmp_batch` via `compile_form.py` and `run_form.py`)**
   Both scripts export:
   ```sh
   FORMS_PATH=/oracle_home_14c/forms:/work/samples:<directory-of-form-being-compiled>
   ```
   So during compilation, `frmcmp_batch` searches:
   - `/oracle_home_14c/forms` (`$OFW_ORACLE_HOME/forms/`)
   - `/work/samples` (`samples/`)
   - The staging/source folder of the form itself (for example `/work/temp/oracle-preview/<job-id>`).

2. **Runtime (WebLogic `frmweb` / FSAL)**
   Read from the Forms domain `default.env`:
   - Container path: `/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env`
   - Host/WSL path: `$OFW_ORACLE_HOME/user_projects/domains/base_domain/config/fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env`
   - Verified values:
     ```ini
     TNS_ADMIN=/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig
     FORMS_PATH=/oracle_home_14c/forms
     FORMS_MODULE_PATH=$FORMS_PATH
     ```

Because `samples/` and `temp/` are **not** in `default.env` by default, any `.fmx`, `.mmx`, or `.plx` needed at **runtime** must either be placed in `/oracle_home_14c/forms` or its directory must be added to `FORMS_PATH` in `default.env`.

## 3. Where to place dependencies

### Option A: Set `OFW_FORMS_PATH_EXTRA` in `.env` (recommended for an external folder like `/mnt/c/forms`)

Add `OFW_FORMS_PATH_EXTRA` in `.env` (see `example.env`; colon-separated if multiple folders):

```ini
OFW_FORMS_PATH_EXTRA=/mnt/c/forms
```

How `compile_form.py` and `run_form.py` handle it:
- If the folder is inside the repo (for example `temp/deps`), its `/work/...` path is appended directly to `FORMS_PATH`.
- If the folder is outside the repo (for example `/mnt/c/forms` on Windows WSL or `/Users/<you>/forms` on macOS), `.fmb`, `.fmx`, `.mmb`, `.mmx`, `.pll`, `.plx`, and `.olb` files are synced into `temp/forms_extra/` (git-ignored, mounted as `/work/temp/forms_extra`) and `/work/temp/forms_extra` is appended to `FORMS_PATH` — no container recreation required.
- Both `oracle-reference-stack/apply_forms14c_config.sh` (during setup) and `compile_form.py` / `run_form.py` (when `OFW_FORMS_PATH_EXTRA` is set and `forms-server-14c` is running) idempotently append `/work/temp/forms_extra` (or the `/work/...` path) to `FORMS_PATH=` in `default.env` for **runtime (`frmweb` / FSAL)** as well. (If you change `default.env` while `forms-server-14c` is already running, restart `forms-server-14c` if `frmweb` needs to pick up the new `default.env`.)

### Option B: Copy dependencies into `$OFW_ORACLE_HOME/forms/` (works immediately for both compile and runtime)

Because `/oracle_home_14c/forms` is already on both compile-time `FORMS_PATH` and runtime `default.env`, anything placed there is visible to `frmcmp_batch`, `compile_form.py`, `run_form.py`, and FSAL (`frmweb`) without editing config files.

- **Important on Windows WSL (rootless Podman permissions):** `~/oracle_home_14c` is owned by container UID `1000` (`oracle`) via Podman user-namespace mapping. Copying files directly with `cp` in WSL can leave them unreadable by `oracle` inside `forms-server-14c`. Use `podman cp` instead (from the Ubuntu WSL shell on Windows, or your terminal on macOS):

  ```bash
  podman cp /path/to/my_lib.pll forms-server-14c:/oracle_home_14c/forms/
  podman cp /path/to/my_lib.plx forms-server-14c:/oracle_home_14c/forms/
  podman cp /path/to/my_menu.mmx forms-server-14c:/oracle_home_14c/forms/
  ```

- If you also need to compile a `.pll` -> `.plx` or `.mmb` -> `.mmx` inside the container, copy the source `.pll` / `.mmb` into `temp/` (git-ignored) and compile it with `compile_form.py`:

  ```bash
  python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py temp/deps/my_lib.pll --type library
  python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py temp/deps/my_menu.mmb --type menu
  ```

### Option C: Keep dependencies in `temp/deps/` (git-ignored) and extend `FORMS_PATH` in `default.env`

If you have many external `.fmx`, `.mmx`, `.plx`, `.pll`, or `.olb` files and want to keep them in a separate git-ignored folder in the repo (never commit proprietary binaries to `samples/`):

1. Put them in `temp/deps/` (mapped to `/work/temp/deps` inside the container).
2. Add `/work/temp/deps` to `FORMS_PATH` in `default.env` inside the container (restart `forms-server-14c` or WebLogic if `frmweb` needs to pick up the updated `default.env`):

   ```bash
   # Inspect current FORMS_PATH in default.env
   podman exec forms-server-14c grep -n '^FORMS_PATH=' \
     /oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env
   ```

   Update the `FORMS_PATH=` line to:

   ```ini
   FORMS_PATH=/oracle_home_14c/forms:/work/temp/deps
   ```

3. For **compile time** (`run_form.py` / `compile_form.py`), `frmcmp_batch` currently hardcodes `FORMS_PATH=/oracle_home_14c/forms:/work/samples:<form_dir>` — so either copy/symlink the compile-time `.pll`/`.olb`/`.mmb` files into `/oracle_home_14c/forms/` (Option A), or run `compile_form.py` from `temp/deps/`.

## 4. Case sensitivity on Linux containers

`forms-server-14c` runs Oracle Linux (case-sensitive filesystem, and on Windows WSL `~/oracle_home_14c` is on ext4, also case-sensitive).

If a form attaches `MY_MENU.mmx` or `MY_LIB.plx` in uppercase while the file on disk is lowercase `my_menu.mmx`, Forms fails with `FRM-10221: Cannot read file ...` or `PDE-PLI018`. Match the exact filename case or add a symlink inside `/oracle_home_14c/forms/`:

```bash
podman exec -u oracle forms-server-14c ln -sf my_menu.mmx /oracle_home_14c/forms/MY_MENU.mmx
```

## 5. Connecting to another dev database (`tnsnames.ora` vs. Easy Connect)

- **Easy Connect (`host:port/service`)**: In **Preview in Oracle -> Target database** (and in the FSAL runtime logon dialog's Database field), enter `db.example.com:1521/ORCLPDB1`. No `tnsnames.ora` edit is required if the database listener accepts `SERVICE_NAME`.
- **TNS alias (required when the target DB uses `SID=...` or a custom descriptor)**:
  Add the alias to `$OFW_ORACLE_HOME/user_projects/domains/base_domain/config/fmwconfig/tnsnames.ora` (inside the container: `/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/tnsnames.ora`), keeping ownership as `oracle` (UID `1000`):

  ```bash
  podman exec -u oracle forms-server-14c cat \
    /oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/tnsnames.ora
  ```

  Then enter that alias name (for example `DEV_DB`) in **Target database** and in the FSAL runtime logon prompt. The container must also be able to reach the database host and port over the network (for example, corporate VPN active).

## Verification status

- Verified in WSL: `TNS_ADMIN`, `FORMS_PATH=/oracle_home_14c/forms`, and `FORMS_MODULE_PATH=$FORMS_PATH` in `default.env`, plus compile-time `FORMS_PATH` in `compile_form.py` and `run_form.py`.
- Not verified: `temp/deps/` runtime loading via an extended `default.env` `FORMS_PATH`, or compiling external `.pll`/`.olb` dependencies against a remote dev database.
