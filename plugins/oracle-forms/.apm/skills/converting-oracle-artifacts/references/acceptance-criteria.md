# Acceptance Criteria: `converting-oracle-artifacts`

## 1. Rust Offline vs. Oracle 14c Conversion Routing

- **[PASSED]**: Routes fast offline inspection/XML export to `oracle-forms-cli` / Web UI `Export XML (Rust)` (`references/rust-native-binary-conversion.md`) and authoritative 14c conversions (`convert_fmb.py`, `convert_pll.py`, `convert_rpt.py`, Web UI `Convert via Oracle (14c)`) through `converter_runtime.py` inside the Forms 14c container.
- **[FAILED]**: Treats offline `RosBinaryWriter` (`export-binary`) output as Stage 2 `frmcmp_batch` input or invokes unconfigured `bin/*.sh` template wrappers directly.

## 2. Isolation & Batch Dependency Gates

- **[PASSED]**: `build_dependency_manifest.py` keeps external inputs read-only, avoids re-decompiling existing XML/PLD sources, stages outputs solely in external `OFW_WORK_DIR`, and `test_conversions.py` reports verified pass/fail status without overclaiming unverified paths.
- **[FAILED]**: Copies external artifacts or outputs into `samples/` or skips Stage 0–2 interactive gates.
