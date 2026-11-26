# Acceptance Criteria: `setting-up-oracle-testbed`

## 1. Platform & Environment Configuration

- **[PASSED]**: `clean-clone-checklist.md` and `create_env.py` keep `.env` git-ignored, place `OFW_ORACLE_HOME` and `OFW_WORK_DIR` outside the repo (and on native Ubuntu ext4 in WSL2), and keep `OFW_FORMS_PATH_EXTRA` read-only.
- **[FAILED]**: Recommends native Windows Visual Studio builds or `podman-machine-default`, commits `.env` credentials, or stores machine-specific `/Users/` or `/home/<real-user>/` paths in docs.

## 2. Stack Lifecycle & Change Capture

- **[PASSED]**: `start_testbed.py --status` and `health_check.py` verify container/servlet/DB health without removing or replacing user containers/images unprompted, and changes/errors/pivots are captured immediately in `references/windows-wsl-change-log.md`.
- **[FAILED]**: Modifies containers destructively without approval, leaves broken relative links in `references/`, or claims unrun setup steps as verified.
