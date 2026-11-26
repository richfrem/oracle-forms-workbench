# Acceptance Criteria: `testing-forms-parity`

## 1. Offline Binary & XML Parity

- **[PASSED]**: `cargo test -p oracle-forms-binary` and `experiments/fmb-re-test/run_roundtrip.py` / `compare_artifacts.py` execute cleanly without containers or JVM, validating contiguous `ROS.60050` decoding and round-trip parity.
- **[FAILED]**: Requires Oracle 14c containers for native Rust binary/XML unit tests or equates structural match with runtime UI/trigger equivalence.

## 2. Live Runtime & Database Parity

- **[PASSED]**: Compiles target modules against `FREEPDB1` or non-local dev DBs via `compiling-and-running-oracle-forms`, verifies FSAL servlet readiness (`7001`), runs `oracle-forms-cli query --backend oracle-thin` / Web UI `Preview in Rust` / `Form Diff`, and documents any unverified script status (`test_parity.py`) honestly.
- **[FAILED]**: Hardcodes credentials or container IPs, or claims Track K trace parity where unverified.
