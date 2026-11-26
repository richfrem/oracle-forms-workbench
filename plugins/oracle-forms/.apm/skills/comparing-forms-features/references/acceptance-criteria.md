# Acceptance Criteria: `comparing-forms-features`

## 1. Feature Comparison Execution

- **[PASSED]**: `compare_features.py` parses target `.xml` or `.fmb` modules via `oracle-forms-cli inspect --json`, classifies built-ins (`SUPPORTED`, `PARTIAL`, `UNSUPPORTED`), and outputs summary/JSON/Markdown scores per `references/architecture-matrix.md`.
- **[FAILED]**: Emits unparsed errors without building Rust CLI or silently reports heuristic feature scores as runtime parity.

## 2. Benchmark & Boundaries

- **[PASSED]**: `--benchmark` checks container readiness and `.env` credentials before querying Oracle 23ai, and distinguishes Form comparison (`references/testing-forms-reference.md`) from Report comparison (`references/testing-reports-reference.md`).
- **[FAILED]**: Hardcodes credentials or writes external artifacts into `samples/`.
