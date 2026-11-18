# 13: Save safety and macOS regression follow-up

Status: Open review findings; fixes not implemented. Reviewed on macOS at main revision `3310cf3` on 2026-11-17.

## Relationship to delivered work

Phases A–G and Track T remain delivered. The completed design specifications remain in
[completed/](completed/README.md); their archival status is not a claim of exhaustive parity.
Recent Windows work added direct ROS 60050 geometry extraction, inherited properties,
Tab/Stacked canvas composition, EWT styling, prompt alignment, radio controls, and Live UI
font/widget fixes. These improvements are preserved; this follow-up addresses separate save
safety and portability defects. Track K remains active, and plans 11/12 remain proposed.

## Open items and acceptance criteria

### S1 — High: prevent lossy summary overwrite

Evidence: `crates/oracle-forms-server/src/service.rs`, `save_form_file` obtains the export mode
but always calls `write_form_file`, which renames over `<stem>_fmb.xml`. It does not implement
the documented protection for an existing complete XML when only a summary is available.
Structural model edits can therefore replace unmodeled source content.

- Preserve in-place saving for supported original-XML patches.
- For a summary with an existing complete target, refuse replacement or use the explicitly
  documented summary fallback; never silently replace the complete definition.
- Add a regression that begins with unknown source properties, performs a structural edit,
  saves, and verifies the original complete XML is unchanged and the response identifies the outcome.

### S2 — High: keep external source directories read-only

Evidence: `resolve_save_destination` selects a matching or first `OFW_FORMS_PATH_EXTRA`
directory as its write destination. `write_form_file` can then replace an existing XML there,
including when the input was uploaded. Configured source paths must remain read-only.

- Save external/uploaded artifacts only to a validated external output/work directory.
- Verify that output is outside the repository and outside external source directories,
  including canonicalized paths and symlink aliases.
- Test matching source directories, upload fallback, filename collisions, and rejected output
  locations using synthetic temporary fixtures. Do not use or inspect external project artifacts.

### M1 — Medium: reconcile work-directory defaults and validation

Evidence: `compile_form.py::oracle_home_host` defaults to the in-repository Oracle Home;
`work_dir` defaults beneath that Home and then rejects paths inside the repository. Rust's
`oracle_preview.rs::work_dir` returns that same default without equivalent validation.
Existing installations using the documented default can reach inconsistent behavior.

- Define one consistent external work-directory default/policy across Rust and Python.
- Preserve existing Oracle Home installations; do not move Homes or recreate containers as
  part of this fix. Explain any required explicit output setting.
- Test unset settings, explicit external settings, rejected in-repository settings, and
  macOS canonical path aliases without launching Oracle tooling.

## Verification observed on this Mac

- `cargo check`: passed; `cargo test`: 134 passed, no failures.
- Frontend suite: 32 passed, no failures, including static-view parity regression coverage.
- `tests/test_reference_pipeline.py`: 34 of 36 passed. The offline compilation error test
  fails early on the rejected default work directory. The parent-chain staging test compares
  canonical `/private/var` output with an unresolved `/var` temporary path on macOS.
- Fix the test fixtures/path comparison as well as the configuration inconsistency; do not
  weaken the external-output boundary merely to make tests pass.
- No live Oracle 14c visual comparison, compilation, container restart, or deployment was
  performed during this review. Windows verification remains recorded in the owning change log.

## Completion gate

Close S1/S2/M1 only with targeted regression evidence and passing Rust, frontend, and Python
suites on macOS; retain or obtain Windows WSL validation for shared changes. Track K runtime
traces and live visual parity are separate gates in [09](09-CONFORMANCE-TESTING.md),
[11](11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md), and the UI backlog.

## Documentation reconciliation and additional findings

### D1 — Corrected on this branch: application deployment location

Older skill gate prompts, batch conversion guidance, inherited-form launch/cleanup guidance,
and the shared dependency reference sent application executables into Oracle's product `forms/`
directory. Updated those instructions to use `<OFW_ORACLE_HOME>/forms_apps/<app_name>/`
registered ahead of product files on runtime `FORMS_PATH`. The pipeline diagram now describes
direct output writes rather than a separate publication copy, and the running Forms container
with a fallback for build stages. No runtime configuration or files were moved.

Preserve the three-stage contract: a complete decompiled text set, dependency-ordered native
14c builds in one shared flat stage folder, then authenticated executable compilation from
that same workspace/manifest. Parent FMB, PLL, MMB and OLB providers and case symlinks must
remain discoverable together. Build staging and runtime deployment are different locations.
Original inherited binaries must remain available; XML round trips can lose inherited objects.
This deployment arrangement is the project's chosen convention; Oracle-vendor recommendation
has not been independently established.

### D2 — Open: reconcile PLD-to-PLL support and verification claims

Batch instructions describe rebuilding PLL from PLD using parse=yes, while conversion notes
and the conversion status table report no working command-line path. Determine which exact
command/configuration was verified and update the support claims together. Do not mark the
path supported merely because the batch script attempts it. Likewise reconcile stale batch
"not verified" statements with actual recorded runs, without upgrading mock tests to live evidence.

### D3 — Open: remaining instruction/status drift

The older schema-validation reference says no 12c schema is included, although an asset exists
under assets/12c/forms.xsd; distinguish asset presence from version-selector integration.
Conversion notes still mention temp/convert-stage despite the external work-directory policy.
Architecture diagrams/status sections retain versioned-save descriptions and deleted test-file
landmarks. Reconcile these in a separate documentation pass without rewriting historical evidence.

### M2 — Open: macOS Python test canonicalization

The parent-chain staging test compares a resolved /private/var path with an unresolved /var
Oracle Home. Normalize both before checking ancestry, while preserving real containment checks.
The offline-error test also needs an explicit valid external work directory so it tests its
intended compiler diagnostic. Current review evidence is 34/36 Python tests passing.

### S3 — Open: concurrent save temporary-file collision

Source review shows every save of a given module uses the same .<name>.saving path.
Concurrent saves can truncate or rename one another's temporary file, causing failures or
incorrect content. Reproduce with synthetic concurrent requests; use unique temporary files
and define the desired conflict/serialization behavior before implementation.

## Scope and knowledge boundary

Major reviewed changes include WSL hosting, configurable Oracle Home/workspace, manifest-driven
dependency conversion/compilation, deployed-form launching, original-XML saving, inherited
property resolution, ROS binary geometry, EWT/widget styling, and tab/stacked canvas composition.
This is a focused review, not exhaustive certification of all changed files. Implementation
issues S1/S2/S3/M1/M2 and support/status issues D2/D3 remain open. Only D1 documentation is
corrected by this branch; live Oracle behavior has not been revalidated on macOS.

### D4 — Open: skill-reference link audit failures

The source-mode plugin audit run during this documentation update reports missing local links,
including start-next.md, a build-manifest template reference, and batch-manifest-compilation
references under the setup skill. Review and repair the relative links in their owning pages;
do not duplicate assets/how-to files to mask broken references. The source audit is not fully
passing; installed-mode audit was not run. Rust cargo check/test and whitespace checks passed
for this documentation change.

## Startup and platform review

The Windows changes document and exercise Ubuntu WSL2 with Podman in that distro. On Apple
Silicon the Rust server and launch tooling run on macOS, and Podman Machine runs the Linux
images with Rosetta for the amd64 Forms image. Both platforms use the same container-side
Oracle Home, flat dependency staging, manifest, and `forms_apps/<app_name>/` deployment
layout. WSL runtime-directory repair, WSLg, and Windows-drive workarounds belong to the
Windows setup. Existing Mac setup evidence predates some shared changes. Applicability to
that stack is expected, and live verification of the updated workflow remains open.

- **R1 — Restart scope.** `start_testbed.py:410` stops by process name across instances, and
  `HOW-TO-RUN.md:187` recommends force-killing processes found on port 7070. A restart can
  stop an unrelated checkout or process. Scope restart to the intended server and prefer
  graceful termination.
- **R2 — Startup configuration mismatch.** `start_testbed.py:458` probes hard-coded port
  7070 although the Rust server honors `PORT` and the startup script passes through its
  environment. A server on a custom port can start while the script reports failure. A
  different HTTP service returning 200 on port 7070 can also be accepted even if the
  Workbench readiness endpoint fails. Use the effective port and verify service identity.
- **R3 — WSL process detection.** `start_testbed.py:449` uses Windows `tasklist.exe` even
  when Rust and Cargo run as Linux processes inside WSL. This misses Linux startup in
  progress and can match an unrelated native Windows process. Use process detection for
  the platform actually hosting the server.
- **R4 — Documented Rust minimum.** `DEPENDENCIES.md:42` lists Rust and Cargo 1.75, but the
  checked-in `Cargo.lock` uses version 4. Establish the minimum against the locked workspace
  and record the tested toolchains for macOS and WSL.
- **R5 — Startup guidance drift.** `start_testbed.py:302` points to manual WSL `sudo`
  commands in HOW-TO-RUN section 6, which now links to the setup skill instead. The script's
  fixed 15–45 minute boot message also describes the older Windows-drive setup, while
  `architecture.md:65` records a substantially faster boot with Oracle Home on WSL's disk.
  Refer to the owning setup guide and report observed timings.

Source review covered `start_testbed.py`, `HOW-TO-RUN.md`, `DEPENDENCIES.md`, and
`architecture.md`. The launcher help command passed locally. Findings R1–R5 remain open.

Startup follow-up: R2 and R3 were addressed in `start_testbed.py`. The Mac launcher returned exit 0 with all services confirmed at `PORT=7075`, reusing running processes. Live WSL validation, a cold Mac start, and a full three-stage Mac batch remain outstanding.

The Mac now has a local ignored `.env` derived from `example.env`. The configured checkout-local Oracle Home cannot satisfy the external work-directory rule. The running Forms container records mount sources from an older checkout path that is absent on the Mac host. The recovery plan is to provision a verified copy of Oracle Home outside the checkout and recreate the Forms container with that mount. Set `OFW_WORK_DIR` beneath the external Home after the new mount is in place. Migration and live compilation remain pending.
