# External Repository Integration & Testbed Guide

This directory documents how **Oracle Forms Workbench** (`richfrem/oracle-forms-workbench`) operates in tandem with external enterprise application repositories, with specific reference to **`jag-JUSTIN-Oracle-Forms-Analysis`** (`bcgov-c/jag-JUSTIN-Oracle-Forms-Analysis`), how to start all local services, and how strict workspace isolation is preserved.

---

## 1. About the External Reference Repository (`jag-JUSTIN-Oracle-Forms-Analysis`)

The **JUSTIN** application is the mission-critical criminal justice administration and case tracking platform for the British Columbia Ministry of Attorney General. It serves court registries, Crown Counsel, judiciary, police services, and corrections across the province.

The repository `bcgov-c/jag-JUSTIN-Oracle-Forms-Analysis` serves as the primary analysis and modernization hub for JUSTIN:
- **Legacy Source Assets (`legacy-system/source/`):** Contains over 250 Oracle Forms (`.fmb`), 30+ PL/SQL libraries (`.pll`), object libraries (`.olb`), menus (`.mmb`), and corresponding Forms2XML exports.
- **Analysis Outputs (`legacy-system/analysis-outputs/`):**
  - `legacy-authentication-authorization/`: Documents legacy security, role propagation (`aglogon`, `APPL4`, `APPL6`), and login handshakes.
  - `oracle-forms-14c-testbed/`: Houses the 14c rebuild playbook, desktop WebStart JNLP launcher (`justin_local.jnlp`), batch compilation ledger (`244 / 258` compiled, `94.6%`), Category 1/3 patch records, and local stack isolation guide.
- **Modernization Sandbox (`sandbox/`):** An active prototype re-platforming JUSTIN workflows onto .NET 8 Web APIs (port 5200) and React 18/Vite UIs (port 3001).

---

## 2. Sibling Repository Architecture

Both repositories typically reside side-by-side in the developer's local source root:

```text
source/repos/
├── oracle-forms-workbench/           (GitHub: richfrem/oracle-forms-workbench)
│   ├── crates/                       (App 1: Native Rust Workbench source, port 7070)
│   ├── oracle-reference-stack/       (App 2: Oracle 14c & DB containers in Podman)
│   ├── plugins/oracle-forms/         (Batch compiler scripts & testbed automation)
│   ├── docs/external-integration/    (This guide)
│   └── start_testbed.py              (Master services startup script)
│
└── jag-JUSTIN-Oracle-Forms-Analysis/ (GitHub: bcgov-c/jag-JUSTIN-Oracle-Forms-Analysis)
    ├── legacy-system/                (Application source XMLs, FMBs, PLLs, MMBs)
    │   └── analysis-outputs/
    │       ├── legacy-authentication-authorization/ (Auth, roles, logon flow analysis)
    │       └── oracle-forms-14c-testbed/            (14c rebuild playbook & batch results)
    └── sandbox/                      (Modernization prototype: .NET API + React UI)
```

In the Windows WSL2 Ubuntu environment:
- Workbench: `/mnt/c/Users/RICHFREM/source/repos/oracle-forms-workbench`
- External Application: `/mnt/c/Users/RICHFREM/source/repos/jag-JUSTIN-Oracle-Forms-Analysis`

---

## 3. Starting All Services (`start_testbed.py`)

Run `start_testbed.py` from the **Ubuntu WSL2 shell** at the root of `oracle-forms-workbench`:

```bash
cd /mnt/c/Users/RICHFREM/source/repos/oracle-forms-workbench
python3 start_testbed.py
```

### Services Started:
1. **Oracle Database 23ai Free (`oracle23ai-lite`):** Starts on port `1521` (SID: `FREE`).
2. **Oracle Forms 14.1.2 WebLogic (`forms-server-14c`):** Starts on port `7001` (`/forms/frmservlet`).
3. **Rust Workbench Server (`oracle-forms-server`):** Compiles and runs in the background on port `7070` (`http://127.0.0.1:7070`).

### Common Commands:
- Full Stack: `python3 start_testbed.py`
- Health / Status: `python3 start_testbed.py --status`
- Fast Mode (No WebLogic boot): `python3 start_testbed.py --no-forms`
- Restart Rust Server: `python3 start_testbed.py --restart-rust`
- Stop Forms Server: `python3 start_testbed.py --stop-forms`

---

## 4. Workspace Isolation Rules (`cross-project-isolation.md`)

When connecting `oracle-forms-workbench` to external repositories like `jag-JUSTIN-Oracle-Forms-Analysis`:
1. **Read-Only External Inputs:** The external repository is strictly read-only. No intermediate or generated files are written into `legacy-system/source/`.
2. **External Staging Directory (`OFW_WORK_DIR`):** Set in `.env` to a staging folder on Linux ext4 (e.g. `/home/richfrem/oracle_home_14c/ofw-work`). All Forms2XML conversions, temporary FMBs, and compilation logs remain in `OFW_WORK_DIR`.
3. **Segregated Runtime Binaries:** Compiled executables (`.fmx`, `.plx`, `.mmx`) deploy to dedicated subdirectories (e.g., `/oracle_home_14c/forms_apps/justin/`), which are prepended to `FORMS_PATH` in `formsweb.cfg` ahead of the stock product directory.

---

## 5. Key Documentation Links

- Detailed Skill Reference: [external-repository-integration.md](../../plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/external-repository-integration.md)
- Batch Dependency Manifest Generator: `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/build_dependency_manifest.py`
- Batch Compiler: `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_from_manifest.py`
- Sibling Testbed Playbook: `../jag-JUSTIN-Oracle-Forms-Analysis/legacy-system/analysis-outputs/oracle-forms-14c-testbed/README.md`
