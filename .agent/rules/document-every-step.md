---
trigger: always_on
description: Where and how to document every experiment, pivot, error, and fix in this repo. Capture all generalized learnings in skills, skill references, executable scripts, and the change log immediately, without leaking external project details.
---

# Document every step, pivot, and experiment, as it happens, in the right place

The point of this repository's setup and tooling work is a written, executable, reproducible record that other people (and future agent sessions) can follow on a clean clone, on macOS or on Windows (Ubuntu WSL2)[cite: 4]. **Every experiment—whether it works, fails, or forces a pivot—must be distilled into generalized updates to skills, skill references, executable scripts (`.py`/`.sh`/Rust), and the change log immediately[cite: 4].** It is never a final cleanup task[cite: 4].

## Where things are documented (one source of truth)

**How to do things** (set up, install, run, convert, compile, test, troubleshoot) lives **once**, in the relevant skill's `SKILL.md`, `scripts/`, and `references/` under `plugins/oracle-forms/.apm/skills/`[cite: 4]. Never keep a second copy of a how-to in `docs/` or at the repo root; duplicates drift[cite: 4]. The ordered list from a fresh clone is `setting-up-oracle-testbed/references/clean-clone-checklist.md`[cite: 4].

| Topic | Where (One Source of Truth) |
|---|---|
| Ordered steps from a fresh clone, both platforms | `setting-up-oracle-testbed/references/clean-clone-checklist.md`[cite: 4] |
| macOS host (Podman machine, Rosetta, XQuartz) | `setting-up-oracle-testbed/references/podman-setup-guide.md`[cite: 4] |
| Windows host (WSL2, Ubuntu, Podman in Ubuntu, agent in WSL, fixes) | `setting-up-oracle-testbed/references/windows-podman-setup.md` (steps), `references/windows-podman-status-log.md` (status log, prerequisites, troubleshooting log)[cite: 4] |
| Installing Forms 14c, the domain, the configuration script, starting the server | `setting-up-oracle-testbed/references/installing-forms-14c.md`[cite: 4] |
| The database container and seed schema | `setting-up-oracle-testbed/references/database-23ai.md`[cite: 4] |
| Installing and running the Rust app | `setting-up-oracle-testbed/references/rust-workbench-setup.md`; architecture: `references/native-rust-workbench-architecture.md` and `.mmd`[cite: 4] |
| Where the Oracle Home lives, moving it, verifying a copy | `setting-up-oracle-testbed/references/oracle-home-location.md`[cite: 4] |
| Every file in `oracle-reference-stack/`, `compose.yaml` | `setting-up-oracle-testbed/references/oracle-reference-stack-map.md` (the folder's `README.md` only points to it)[cite: 4] |
| Starting, restarting, and confirming the stack (`start_testbed.py`) | `setting-up-oracle-testbed/references/starting-the-stack.md`[cite: 4] |
| Testing the servers, FSAL, compiling, Reports | `setting-up-oracle-testbed/references/forms-and-reports-testing-guide.md`, `references/reports-conversion.md`[cite: 4] |
| Forms Builder GUI (`frmbld`) and live forms with FSAL | `setting-up-oracle-testbed/references/forms-builder-gui.md`[cite: 4] |
| Symptom, cause, fix | `setting-up-oracle-testbed/references/troubleshooting.md` (Windows-only fixes: `windows-podman-setup.md` section 10)[cite: 4] |
| The 12c legacy setup | `setting-up-oracle-testbed/references/legacy-12c.md`[cite: 4] |
| Module dependency waves & compile order | `setting-up-oracle-testbed/references/module-dependencies-and-compile-order.md`[cite: 3, 4] |
| Conversions, validation, compiling, running, parity, feature audits | The `SKILL.md`, `scripts/*.py`, and `references/` of `converting-oracle-artifacts`, `compiling-and-running-oracle-forms`, `testing-forms-parity`, and `comparing-forms-features`[cite: 4] |
| **History: every attempt, change, error, fix, pivot, open item** | `setting-up-oracle-testbed/references/windows-wsl-change-log.md` (one table row each; this is where "what we tried and what happened" goes)[cite: 4] |
| Open work for the next session | `start-next.md` (repo root)[cite: 4] |
| Agent rules and orientation | `AGENTS.md`, `CLAUDE.md`, `.agent/rules/`; where things run and the repo map: `architecture.md`[cite: 4] |
| Running the Rust app day to day, CLI | `HOW-TO-RUN.md` (repo root)[cite: 4] |
| Dependencies: Rust toolchain and crates (root) / Oracle stack, installers, downloads (skill) | `DEPENDENCIES.md` (root) / `setting-up-oracle-testbed/references/dependencies.md`[cite: 4] |
| Design, roadmap, status of features, UI backlog | `docs/plans/`[cite: 4] |

## How to write it (Pattern Distillation & Reproducibility)

1. **Distill external experiments into generalized patterns (Cross-Project Isolation):** Follow the read-only inspection and Pattern Distillation Protocol in [`.agent/rules/cross-project-isolation.md`](cross-project-isolation.md). When an insight, bug, or workaround comes from testing an external project, strip all external project names, file paths, table/schema identifiers, TNS aliases, and logs before writing anything into this repository (including `temp/`, `context/`, `samples/`, `scripts/`, `references/`, and change logs). Record only the generalized Oracle Forms/Reports mechanic and use generic placeholders (`<external-forms-dir>`, `DEPT`, `EMP`, `ORDERS`).
2. **Encode working solutions in scripts and skills, not just prose:** If an experiment reveals a new flag, quoting rule, dependency wave order, or health check, update the owning `.py` / `.sh` / Rust code **and** the owning `SKILL.md` / `references/` page so future runs do it automatically.
3. **Write what to do in references; write what happened in the change log:** A reference page states the verified procedure: exact commands, why they are needed, expected output, and any unverified caveats[cite: 4]. The story of what was tried, what failed, and why we pivoted goes in `windows-wsl-change-log.md` and `troubleshooting.md`[cite: 4].
4. **Cover both platforms:** For anything that differs between macOS and Windows, state both or link the platform-specific guide[cite: 4]. On Windows, all commands run inside the Ubuntu WSL2 shell—never PowerShell or a native Windows Rust build[cite: 4].
5. **No machine-specific or sensitive values:** Never commit usernames, home paths, passwords, or container IPs; use `<you>`, `$VARIABLES` from the git-ignored `.env`, and container hostnames such as `oracle-db-23ai`[cite: 4].
6. **Link, do not repeat; keep indexes current:** Link to the owning page with relative links[cite: 4]. When adding a new reference page, add it to the owning skill's `SKILL.md` index table[cite: 4]. After moving or deleting a file, verify zero broken links with `grep -rn "<old name>"` (excluding `oracle_home_14c/`, `target/`, `temp/`, and `.git/`)[cite: 4].
7. **Status on every claim:** Mark what was actually executed and verified versus what is "not verified"[cite: 4]. Remove stale "not yet verified" notes as soon as you verify them[cite: 4].

## Rules

1. **Document immediately—do not batch:** After every experiment, code change, error, fix, or configuration pivot, update the change log, owning skill/reference, and scripts **before** moving to the next step[cite: 4]. Never defer documentation as "pending"[cite: 4].
2. **Three-part capture for every discovery:**
   - **History:** Add a row to `windows-wsl-change-log.md` recording what was tried, what succeeded or failed, and why[cite: 4].
   - **Guidance:** Update the owning `SKILL.md` and `references/` page (including `troubleshooting.md` for errors/gotchas) so someone on a clean clone avoids the pitfall[cite: 4].
   - **Automation:** Update or add `.py` / `.sh` scripts (and `example.env` if new settings were introduced) so hand work is automated[cite: 4].
3. **Record failed attempts and pivots explicitly:** Failed commands, silent exits (e.g., `frmcmp_batch` exiting `0` without output), and abandoned approaches are high-value knowledge—document the exact symptom, root cause, and pivot in `windows-wsl-change-log.md` and `troubleshooting.md`[cite: 4].
4. **Script hand work:** A configuration or build step done by hand is incomplete until codified in a checked-in script or template[cite: 4].
5. **Be honest about verification status:** Claim "verified" or "done" only for commands you ran and confirmed in the current environment[cite: 4].
6. **Moving or removing a document:** Preserve unique content in its new home first (`diff`/`cmp`), use `git mv`/`git rm`, fix all links, update `SKILL.md`, and log the move in `windows-wsl-change-log.md`[cite: 4].

## Working with the repo owner

- Answer yes/no questions with yes or no first, then explain[cite: 4].
- When the owner has already specified what to do, execute it directly without offering extra options or asking for confirmation[cite: 4].
- **Proactive guided execution.** When running a multi-step skill workflow (for example batch conversion and compilation), never leave the owner guessing what comes next. After every completed step, give a short status summary (counts, failures, anything unresolved) and immediately ask a direct authorization question for the next step, including any choices it needs, preferably with the interactive question tool. Follow the skill's "Interactive guided workflow" gates. Authorization gates cover steps that start containers, convert, compile or delete; read-only analysis the owner already asked for does not need a second confirmation. Never ask for passwords in chat: give the owner the exact command so they type the password at the `getpass` prompt.
- Work one step at a time when requested; keep status updates concise and factual[cite: 4].
- Before any destructive or image-level action (removing/recreating containers, rebuilding images, deleting files), state what will happen first[cite: 4]. Never remove or replace the owner's containers or images on your own initiative[cite: 4].
- Never commit, push, or open a pull request until the owner says the work is done[cite: 4].
- Run all Oracle tooling, builds, and tests inside WSL2 on Windows, never natively on Windows[cite: 4].

## Check before finishing a task

- **Isolation check:** Have all external project names, paths, table/schema identifiers, and raw logs been stripped so only generalized patterns remain (including inside `temp/`, `context/`, and `samples/`)?
- **Change log & references:** Is every attempt, failure, pivot, and fix recorded in `windows-wsl-change-log.md` **and** in the owning skill's `SKILL.md` / `references/`?[cite: 4]
- **Executable automation:** Was any manual workaround or new capability codified into the relevant `.py` / `.sh` script?[cite: 4]
- **Skill audit & links:** If any skill under `plugins/oracle-forms/` was modified, did you re-run `python3 ~/.claude/skills/authoring-skills/scripts/audit_skill.py plugins/oracle-forms --all --mode source` (and `--mode installed` if installed) and check relative links?[cite: 4]
- **Verification honesty:** Do all status claims match what was actually run and confirmed?[cite: 4]