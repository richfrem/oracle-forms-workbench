# External Repository Integration & Testbed Isolation

This reference describes how **Oracle Forms Workbench** (`richfrem/oracle-forms-workbench`) operates alongside external legacy application repositories (such as `bcgov-c/jag-JUSTIN-Oracle-Forms-Analysis`), how to start all services, and how to maintain strict workspace isolation so that proprietary application assets never pollute the open-source workbench.

---

## 1. Two-Repository Topology

The tooling and application analysis repositories reside as sibling directories under the developer's local source tree:

```text
C:\Users\RICHFREM\source\repos\
├── oracle-forms-workbench\              (GitHub: richfrem/oracle-forms-workbench)
│   ├── crates/                          (App 1: Native Rust Workbench source, port 7070)
│   ├── oracle-reference-stack/          (App 2: Oracle 14c & DB containers)
│   ├── plugins/oracle-forms/            (Automation skills & scripts)
│   └── start_testbed.py                 (Master services startup script)
│
└── jag-JUSTIN-Oracle-Forms-Analysis\    (GitHub: bcgov-c/jag-JUSTIN-Oracle-Forms-Analysis)
    ├── legacy-system/                   (Application source XMLs, FMBs, PLLs, MMBs)
    │   └── analysis-outputs/
    │       ├── legacy-authentication-authorization/ (Auth, roles, logon flow analysis)
    │       └── oracle-forms-14c-testbed/            (14c rebuild playbook & batch results)
    └── sandbox/                         (Modernization prototype: .NET API + React UI)
```

In the Windows WSL2 Ubuntu environment:
- Workbench: `/mnt/c/Users/RICHFREM/source/repos/oracle-forms-workbench`
- External Application: `/mnt/c/Users/RICHFREM/source/repos/jag-JUSTIN-Oracle-Forms-Analysis`

---

## 2. Starting All Services (`start_testbed.py`)

Always run `start_testbed.py` from the **Ubuntu WSL2 shell** at the root of `oracle-forms-workbench`:

```bash
cd /mnt/c/Users/RICHFREM/source/repos/oracle-forms-workbench
python3 start_testbed.py
```

### What `start_testbed.py` Does:
1. **Podman Runtime (`/run/user/<uid>`):** Validates the tmpfs directory. After a Windows reboot or `wsl --shutdown`, it prompts for the WSL `sudo` password to recreate the directory (operator must run this).
2. **Oracle Database 23ai Free (`oracle23ai-lite`):** Starts the container on port `1521` and waits for `DATABASE IS READY TO USE`.
3. **Oracle Forms 14.1.2 (`forms-server-14c`):** Starts WebLogic and Forms servlet on port `7001` and waits for `Server state changed to RUNNING`.
   - *Performance Note:* If the Oracle Home is hosted on the Windows 9P filesystem (`/mnt/c/`), WebLogic boot takes 15–45 minutes. When installed on the native ext4 disk (`/home/<user>/`), boot takes ~2 minutes.
4. **Rust Workbench Server (`oracle-forms-server`):** Compiles and runs in the background on port `7070`.
5. **Readiness Probe:** Queries `GET http://127.0.0.1:7070/api/forms/oracle-preview/readiness` to verify end-to-end communication.

### Useful Command Flags:

| Command | Description |
|---|---|
| `python3 start_testbed.py` | Starts Database (1521), Forms 14c (7001), and Rust Workbench (7070) |
| `python3 start_testbed.py --status` | Non-destructive check of running containers, ports, and health |
| `python3 start_testbed.py --no-forms` | Fast mode: Starts Database + Rust app only (skips WebLogic) |
| `python3 start_testbed.py --restart-rust` | Rebuilds and restarts the Rust server without touching containers |
| `python3 start_testbed.py --stop-forms` | Graceful 60-second shutdown of `forms-server-14c` |

---

## 3. Workspace Isolation Boundaries (`cross-project-isolation.md`)

To uphold clean repository boundaries:

### A. External Repositories Are Read-Only
- All files in `jag-JUSTIN-Oracle-Forms-Analysis` are treated as **read-only source inputs**.
- Neither `oracle-forms-server` nor conversion/compilation scripts will ever edit, write into, or create scratch files within `legacy-system/source/`.

### B. Isolated External Working Directory (`OFW_WORK_DIR`)
- In `.env`, configure `OFW_WORK_DIR` pointing to an external workspace on the Linux filesystem:
  ```ini
  OFW_WORK_DIR=/home/richfrem/oracle_home_14c/ofw-work
  OFW_FORMS_PATH_EXTRA=../jag-JUSTIN-Oracle-Forms-Analysis/legacy-system/source
  ```
- All intermediate `Forms2XML` outputs, `XML2Forms` staging binaries, dependency resolution manifests (`build-manifest.json`), and compiler logs are strictly contained in `OFW_WORK_DIR`.
- Zero temporary or generated binaries enter either `oracle-forms-workbench` or `jag-JUSTIN-Oracle-Forms-Analysis`.

### C. Dedicated Application Runtime Directory (`forms_apps/<app>/`)
- Compiled `.plx`, `.mmx`, and `.fmx` runtime binaries do not overwrite the stock Oracle 14c product directory (`/oracle_home_14c/forms/`).
- Instead, they deploy to a dedicated runtime folder:
  ```text
  /oracle_home_14c/forms_apps/justin/
  ```
- This directory is prepended to `FORMS_PATH` in `formsweb.cfg` `[justin]` and `justin.env` ahead of the product folder:
  ```ini
  FORMS_PATH=/oracle_home_14c/forms_apps/justin:/oracle_home_14c/forms
  ```

---

## 4. Key File Pointers for AI Agents

### For Agents Operating in `oracle-forms-workbench`:
- **External Integration Hub:** [`docs/external-integration/README.md`](../../../../../../docs/external-integration/README.md)
- **Startup Orchestration:** `start_testbed.py`
- **Batch Dependency Manifest Generator:** `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/build_dependency_manifest.py`
  - Scans external source directories, generates dependency DAG, and detects Linux case-sensitivity symlinks (`requiredSymlinks`).
- **Batch Compiler:** `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_from_manifest.py`
  - Prompts once for DB credentials in-memory via `getpass` and compiles modules in dependency order.
- **Reference Stack Definition:** `oracle-reference-stack/compose.yaml`

### For Agents Operating in `jag-JUSTIN-Oracle-Forms-Analysis`:
- **14c Rebuild Playbook:** `legacy-system/analysis-outputs/oracle-forms-14c-testbed/justin-14c-rebuild-playbook.md`
  - Step-by-step documentation for custom client JARs (`justin_ext.jnlp`), AdminServer `formsweb.cfg` settings, `DEVJ` connection, `caution.bmp`, and DB-authenticated compilation.
- **Service & Isolation Guide:** `legacy-system/analysis-outputs/oracle-forms-14c-testbed/starting-the-stack-and-isolation.md`
- **Batch Compilation Ledger:** `legacy-system/analysis-outputs/oracle-forms-14c-testbed/batch-compilation-findings.md`
  - 244/258 (`94.6%`) FMB compile history and Category 1/2/3 patch records.
- **Desktop Launcher:** `legacy-system/analysis-outputs/oracle-forms-14c-testbed/justin_local.jnlp`
- **Domain Authentication & Roles:** `legacy-system/analysis-outputs/legacy-authentication-authorization/`
