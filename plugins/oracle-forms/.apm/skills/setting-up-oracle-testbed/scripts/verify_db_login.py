#!/usr/bin/env python3
"""verify_db_login.py — Verify Oracle 23ai Free database logins from .env using sqlplus.

Purpose:
    Tests that DB_USER/DB_PASSWORD and SYSTEM/ORACLE_PWD from .env (or env)
    authenticate against the running oracle23ai-lite container via sqlplus.
    Never prints password values.

Key Input Dependencies:
    - .env at repository root (DB_USER, DB_PASSWORD, DB_SERVICE, ORACLE_PWD)
    - Running Podman container 'oracle23ai-lite' with sqlplus

Usage Examples:
    python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/verify_db_login.py
    python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/verify_db_login.py --container oracle23ai-lite

Key Functions:
    - resolve_repo_root() -> Path
    - load_dotenv(env_path: Path) -> Dict[str, str]
    - check_sqlplus_login(container: str, user: str, password: str, service: str, as_sysdba: bool) -> Tuple[bool, str]
    - main(argv: Optional[List[str]]) -> int
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple


# Locate the repository root from this script's path inside plugins/oracle-forms/.
def resolve_repo_root() -> Path:
    """Return the repository root directory containing .env."""
    return Path(__file__).resolve().parents[6]


# Parse KEY=VALUE lines from .env without invoking a shell (safe for special chars).
def load_dotenv(env_path: Path) -> Dict[str, str]:
    """Read key-value pairs from a .env file, stripping optional quotes."""
    values: Dict[str, str] = {}
    if not env_path.is_file():
        return values
    for raw in env_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, val = line.split("=", 1)
        cleaned = val.strip()
        if len(cleaned) >= 2 and cleaned[0] == cleaned[-1] and cleaned[0] in ('"', "'"):
            cleaned = cleaned[1:-1]
        values[key.strip()] = cleaned
    return values


# Resolve a setting from process environment first, then .env, then default.
def get_setting(dotenv: Dict[str, str], key: str, default: str = "") -> str:
    """Return the configured value for key without logging secrets."""
    return os.environ.get(key) or dotenv.get(key, default)


# Run a minimal SELECT USER FROM DUAL query inside the database container via sqlplus.
def check_sqlplus_login(
    container: str, user: str, password: str, service: str, as_sysdba: bool = False
) -> Tuple[bool, str]:
    """Verify a single Oracle database login via sqlplus and return (ok, detail)."""
    if not user or not password:
        return False, "missing username or password in .env"
    escaped_pwd = password.replace('"', '""')
    connect_str = f'{user}/"{escaped_pwd}"@localhost:1521/{service}'
    if as_sysdba:
        connect_str += " AS SYSDBA"
    sql_input = "SET HEADING OFF FEEDBACK OFF PAGESIZE 0\nSELECT user FROM dual;\nEXIT\n"
    proc = subprocess.run(
        ["podman", "exec", "-i", container, "sqlplus", "-L", "-S", connect_str],
        input=sql_input,
        text=True,
        capture_output=True,
    )
    output_lines = [ln.strip() for ln in proc.stdout.splitlines() if ln.strip()]
    last_line = output_lines[-1] if output_lines else proc.stderr.strip()
    if proc.returncode == 0 and output_lines:
        return True, f"authenticated as {output_lines[-1]}"
    return False, last_line or f"sqlplus exited with code {proc.returncode}"


# Parse CLI flags for verify_db_login.py.
def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    """Parse command-line options for container name."""
    parser = argparse.ArgumentParser(description="Verify Oracle 23ai logins from .env via sqlplus.")
    parser.add_argument("--container", default="oracle23ai-lite", help="Database container name (default: oracle23ai-lite)")
    return parser.parse_args(argv)


# Entry point: verify DB_USER, SYSTEM, and SYS logins against the running database.
def main(argv: Optional[List[str]] = None) -> int:
    """Verify configured Oracle database logins and return 0 if all pass."""
    args = parse_args(argv)
    dotenv = load_dotenv(resolve_repo_root() / ".env")
    service = get_setting(dotenv, "DB_SERVICE", "FREEPDB1")
    db_user = get_setting(dotenv, "DB_USER", "forms_user")
    db_pwd = get_setting(dotenv, "DB_PASSWORD")
    oracle_pwd = get_setting(dotenv, "ORACLE_PWD")

    checks = [
        (f"{db_user}@{service}", db_user, db_pwd, False),
        (f"system@{service}", "system", oracle_pwd, False),
        (f"sys@{service} AS SYSDBA", "sys", oracle_pwd, True),
    ]
    all_ok = True
    for label, user, pwd, sysdba in checks:
        ok, detail = check_sqlplus_login(args.container, user, pwd, service, as_sysdba=sysdba)
        status = "OK  " if ok else "FAIL"
        print(f"{status} {label}: {detail}")
        if not ok:
            all_ok = False
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
