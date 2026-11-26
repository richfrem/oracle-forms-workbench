# AGENTS.md

## Overview

**Oracle Forms Workbench** is a native Rust development workbench for inspecting, reverse-engineering, authoring, modifying, comparing, and running Oracle Forms & Reports artifacts (`.fmb`, `.mmb`, `.olb`, Forms2XML exports, `.pld`, `.pll`, `.rdf`, `.rpt`). The Rust inner loop edits XML/PLD, validates XML against `forms-14c.xsd`, runs native PL/SQL/DB sessions, and builds offline `ROS.60050` binaries (`export-binary`) for round-trip tests; optional Oracle Fusion Middleware 14c builds Stage 2 source binaries offline (`XML2Forms`), compiles DB-connected Stage 3 executables (`.fmx`/`.mmx`/`.plx`), and launches FSAL. Preserve this full development loop rather than treating the project as read-only.

---

## Architecture & Layout

Read [architecture.md](architecture.md) before any setup, run, Oracle tooling, or Rust task. Two separate apps exist: **App 1** is the Rust workbench (`crates/oracle-forms-*`, `ui/`, `samples/`, port `7070`, zero JVM or Oracle runtime requirement); **App 2** is the optional Podman stack (`oracle-reference-stack/Containerfile.14c`, `compose.yaml` with `oracle23ai-lite` + `forms-server-14c`, and skills under `plugins/oracle-forms/.apm/skills/`). App 1 invokes App 2 only for **Compile Only (14c)** (`run_form.py --no-launch`), **Build & Compile (14c)** (`run_form.py`), and **Launch in 14c** (`launch_fsal.py`). On macOS, Rust runs natively and Podman runs in a VM with Rosetta; on Windows, **all** tooling (Podman in the `Ubuntu` WSL2 distro—never `podman.exe` or `podman-machine-default`—Python, Java 21 for FSAL, and Rust) MUST run inside Ubuntu WSL2, with `OFW_ORACLE_HOME` and `OFW_WORK_DIR` outside this checkout on Ubuntu's disk (never `/mnt/c`; native Windows Visual Studio builds are unsupported).

---

## Core Commands

```bash
python3 start_testbed.py [--no-forms|--status|--stop-rust|--stop-forms]  # stack lifecycle (needs user for sudo)
cargo run -p oracle-forms-server                                         # native Rust UI on http://127.0.0.1:7070
cargo check && cargo test                                                # required Rust gate (0 warnings)
node --test tests/ui_*.test.cjs && python3 tests/test_reference_pipeline.py  # UI + pipeline test suites
cargo run -p oracle-forms-cli -- inspect samples/orders_fmb.xml          # CLI: inspect | query | dump-sql | export-binary
```

On Windows WSL2, launch Rust with `. "$HOME/.cargo/env"; export XDG_RUNTIME_DIR=/run/user/$(id -u) CARGO_TARGET_DIR=$HOME/ofw-target`. Check status with `start_testbed.py --status` first; if WSL reboots, let the user enter `sudo` to recreate `/run/user/<uid>`. Run Oracle utilities (`frmcmp_batch`, `rwconverter`) inside throwaway `podman run --rm` containers from `oracle-forms-14c:latest`, and provision, copy, or verify `OFW_ORACLE_HOME` (and verify the `forms-server-14c` `/oracle_home_14c` mount) only via `oracle-reference-stack/provision_oracle_home.sh`.

---

## Non-Negotiable Rules & Invariants

- **Mandatory Rules**: ALWAYS read all files in `.agent/rules/` (including [cross-project-isolation.md](.agent/rules/cross-project-isolation.md) and [document-every-step.md](.agent/rules/document-every-step.md)) first; if instructions conflict, follow the rule, tell the user, answer yes/no first, and confirm rule and change-log compliance at start and end.
- **Document Every Step**: MUST log every change, error, and fix immediately in the change log and owning reference; keep how-to docs once in testbed references, script hand work, and mark anything unverified; see [document-every-step.md](.agent/rules/document-every-step.md).
- **Cross-Project Isolation**: NEVER place external files or outputs in this repo (`samples/`, `temp/`, `context/`); keep `OFW_FORMS_PATH_EXTRA` read-only and direct external outputs to an external `OFW_WORK_DIR`; see [cross-project-isolation.md](.agent/rules/cross-project-isolation.md) and [project-file-inspection-authorization.md](.agent/rules/project-file-inspection-authorization.md).
- **Coding Conventions**: MUST make edits directly in place, follow language headers and style rules, and run `cargo check` and `cargo test` with zero warnings after every modification; see [coding-conventions.md](.agent/rules/coding-conventions.md).
- **Zero Runtime Coupling**: NEVER add JVM, Node.js, or Oracle runtime dependencies to `crates/`; native parsing, PL/SQL, sessions, and Thin/SQLite/Mock DB paths MUST work standalone, leaving Oracle compilation and FSAL strictly optional.
- **Container & Operational Discipline**: NEVER start, stop, remove, or rebuild containers/images without approval; follow documented steps first, reproduce exact blockers (e.g. `GET /api/forms/oracle-preview/readiness`), keep shared macOS code intact, and give concise unblock steps.
- **Save Semantics & Test Discipline**: ALWAYS distinguish Save Form (updates `samples/<form>_fmb.xml` in place; NEVER create `_edited_N`/`_copy`/`_v2` copies, excepting app lossy-summary fallback) from Save Record, and NEVER click Build & Compile (14c) / Save & launch on `samples/` to test.
- **Credentials, Paths & Honest Claims**: NEVER hardcode machine paths or persist passwords (use git-ignored `.env`), keep native sessions read-only without DML opt-in, validate XML against `forms-14c.xsd`, and NEVER overstate parity or build/compile equivalence.

---

## On-Demand Skills & Reference Index

- **Setup & Testbed**: `setting-up-oracle-testbed` (`SKILL.md`, `references/index.md`) — start at [`clean-clone-checklist.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/clean-clone-checklist.md), [`starting-the-stack.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/starting-the-stack.md), [`oracle-home-location.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/oracle-home-location.md), [`windows-podman-setup.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/windows-podman-setup.md), [`windows-podman-status-log.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/windows-podman-status-log.md), and [`windows-wsl-change-log.md`](plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/references/windows-wsl-change-log.md).
- **Conversion, Build & Parity Skills** (under `plugins/oracle-forms/.apm/skills/`): `converting-oracle-artifacts` (FMB/MMB/OLB/PLL/RDF, `test_conversions.py`, schema extraction), `compiling-and-running-oracle-forms` (`validate_forms_xml.py`, `compile_form.py`, `run_form.py` / `--no-launch`, `launch_fsal.py`, `forms-14c.xsd`), `testing-forms-parity`, and `comparing-forms-features`; read each `SKILL.md` and [plugins/oracle-forms/README.md](plugins/oracle-forms/README.md) before using.
- **Run, Config & Architecture**: Read [README.md](README.md) for the end-to-end workflow, [HOW-TO-RUN.md](HOW-TO-RUN.md) for server/CLI/manual launch steps, [architecture.md](architecture.md) for crate/API boundaries, and [DEPENDENCIES.md](DEPENDENCIES.md) for Rust toolchain prerequisites.
- **Plans & Parity Testing**: Read [`vision.md`](docs/plans/vision.md), [`14-MODERN-IDE-DUAL-ENGINE-PLAN.md`](docs/plans/14-MODERN-IDE-DUAL-ENGINE-PLAN.md), [`06-IMPLEMENTATION-ROADMAP-AND-MILESTONES.md`](docs/plans/06-IMPLEMENTATION-ROADMAP-AND-MILESTONES.md), and [`10-NEXT-PHASE-SESSION-PROMPT.md`](docs/plans/10-NEXT-PHASE-SESSION-PROMPT.md) before extending features; run offline binary parity with `experiments/fmb-re-test/run_roundtrip.py`.

---

## Pending Windows Handoff (Remove Once Completed)

- **Collect & sanitize Windows `.jnlp` example (`S15.1`):** When the next session starts on Windows, **ask the user for their local `.jnlp` copy** (kept outside Git on Windows). Strip any project names, internal URLs, schemas, or secrets per `.agent/rules/cross-project-isolation.md`, place the generalized example + security-annotated `formsweb.cfg` template in `plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/assets/` (and link it from `references/` and [`15-JNLP-AND-LEGACY-LAUNCH-SECURITY-RISK-ANALYSIS.md`](docs/plans/15-JNLP-AND-LEGACY-LAUNCH-SECURITY-RISK-ANALYSIS.md)), log it in `windows-wsl-change-log.md`, and **remove this section from `AGENTS.md` afterward**.

