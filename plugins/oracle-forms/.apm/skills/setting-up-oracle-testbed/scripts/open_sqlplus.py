#!/usr/bin/env python3
"""open_sqlplus.py — Launch an interactive SQL*Plus session inside oracle23ai-lite using .env.

Purpose:
    Reads database credentials from .env (safely handling special characters
    that break bash `source .env`) and opens an interactive `sqlplus` prompt
    inside the running database container. Never prints password values.

Key Input Dependencies:
    - .env at repository root (DB_USER, DB_PASSWORD, DB_SERVICE, ORACLE_PWD)
    - verify_db_login.py in the same scripts/ folder
    - Running Podman container 'oracle23ai-lite'

Usage Examples:
    python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/open_sqlplus.py
    python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/open_sqlplus.py --as system
    python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/open_sqlplus.py --as sys

After that

 SELECT table_name FROM user_tables ORDER BY table_name; 

Key Functions:
    - build_connect_string(dotenv: Dict[str, str], role: str) -> str
    - parse_args(argv: Optional[List[str]]) -> argparse.Namespace
    - main(argv: Optional[List[str]]) -> int
"""

import argparse
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_db_login import get_setting, load_dotenv, resolve_repo_root


# Build the Oracle SQL*Plus connect string for the requested role from .env.
def build_connect_string(dotenv: Dict[str, str], role: str) -> str:
    """Return a quoted SQL*Plus connect string for forms_user, system, or sys."""
    service = get_setting(dotenv, "DB_SERVICE", "FREEPDB1")
    if role == "sys":
        pwd = get_setting(dotenv, "ORACLE_PWD")
        if not pwd:
            raise ValueError("ORACLE_PWD is not set in .env")
        return f'sys/"{pwd.replace(chr(34), chr(34) * 2)}"@localhost:1521/{service} AS SYSDBA'
    if role == "system":
        pwd = get_setting(dotenv, "ORACLE_PWD")
        if not pwd:
            raise ValueError("ORACLE_PWD is not set in .env")
        return f'system/"{pwd.replace(chr(34), chr(34) * 2)}"@localhost:1521/{service}'
    user = get_setting(dotenv, "DB_USER", "forms_user")
    pwd = get_setting(dotenv, "DB_PASSWORD")
    if not user or not pwd:
        raise ValueError("DB_USER or DB_PASSWORD is not set in .env")
    return f'{user}/"{pwd.replace(chr(34), chr(34) * 2)}"@localhost:1521/{service}'


# Parse CLI flags for open_sqlplus.py.
def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    """Parse --as role and --container options."""
    parser = argparse.ArgumentParser(description="Open an interactive SQL*Plus session in oracle23ai-lite.")
    parser.add_argument(
        "--as",
        dest="role",
        choices=["forms_user", "system", "sys"],
        default="forms_user",
        help="Account to connect as (default: forms_user)",
    )
    parser.add_argument("--container", default="oracle23ai-lite", help="Database container name (default: oracle23ai-lite)")
    return parser.parse_args(argv)


# Entry point: exec into podman with an interactive TTY running sqlplus.
def main(argv: Optional[List[str]] = None) -> int:
    """Launch interactive sqlplus inside the database container."""
    args = parse_args(argv)
    dotenv = load_dotenv(resolve_repo_root() / ".env")
    try:
        connect_str = build_connect_string(dotenv, args.role)
    except ValueError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    proc = subprocess.run(["podman", "exec", "-it", args.container, "sqlplus", connect_str])
    return proc.returncode


if __name__ == "__main__":
    sys.exit(main())
