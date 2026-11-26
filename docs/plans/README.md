# Plans and status

This directory separates active tracking from completed design specifications. None of the active
documents below are 100% complete yet, so none were moved to `completed/`.

## Active tracking

- [Vision: Modern Agentic Oracle Forms 14c Workbench](vision.md) — dual-engine, two-loop model (Rust inner loop + Oracle 14c outer loop) and current capability boundaries.
- [14: Modern IDE dual-engine & skill integration plan](14-MODERN-IDE-DUAL-ENGINE-PLAN.md) — mostly delivered across `oracle-forms-binary`, `oracle-forms-xml`, `oracle-forms-server`, `ui/`, and skills; Form Diff parity shortcut, `preview.js` list items, and XML attribute alignment remain open.
- [13: Save safety and macOS regression follow-up](13-SAVE-SAFETY-AND-MACOS-REGRESSION-FOLLOWUP.md) — M2 resolved (`36/36` Python tests); S1–S3, M1, D2–D4, and R1/R4/R5 remain open.
- [Implementation roadmap and milestones](06-IMPLEMENTATION-ROADMAP-AND-MILESTONES.md) — Phases A–G and Track T done; ROS.60050/XML/dual-engine parity updates, Track K, and Phase H active/proposed.
- [Conformance testing](09-CONFORMANCE-TESTING.md) — Track K golden-trace design (open) plus current regression & `ROS.60050` round-trip test status.
- [Next-phase session prompt](10-NEXT-PHASE-SESSION-PROMPT.md) — current session continuation status and prompt.
- [UI review backlog](UI-REVIEW-BACKLOG.md) — source-inspected UI gaps and parity follow-ups.

## Proposed post-G design

- [11: Dual-runtime legacy application parity and session harness](11-DUAL-RUNTIME-DEV-PARITY-AND-SESSION-HARNESS.md)
- [12: Test-user sign-in, role impersonation, and form launcher](12-ROLE-IMPERSONATION-AND-FORM-LAUNCHER-SPEC.md)

## Completed specifications

The completed phase and architecture documents are archived in [completed/](completed/README.md).
They record the original design and are not the current source of implementation status. Use
[architecture.md](../../architecture.md), the active roadmap, and the Oracle skill references for
current behavior and verified setup procedures.
