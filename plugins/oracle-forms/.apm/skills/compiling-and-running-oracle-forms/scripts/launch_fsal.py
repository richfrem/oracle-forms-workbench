#!/usr/bin/env python3
"""
launch_fsal.py (CLI)
=====================================

Purpose:
    Cross-platform desktop runner for Oracle Forms applications using
    Forms Standalone Launcher (FSAL). Automatically handles URL generation,
    default port mapping, password encoding, and runtime launching without
    requiring browser applets or X11 forwarding.

Layer: Tooling / Skill Script (compiling-and-running-oracle-forms)

Usage Examples:
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py test_db.fmx
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py simple_module.fmx --interactive
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py test_db.fmx --port 7001

Supported Object Types:
    - .fmx (Compiled binary executable form module)

CLI Arguments:
    form                  : Name or path of the form to run (e.g., test_db.fmx).
    -p, --port <int>      : WebLogic AdminServer port (default: 7001).
    -u, --userid <str>    : Database logon string (default: DB_USER/DB_PASSWORD from .env, service FREEPDB1; if unset, the client prompts).
    --interactive         : Omit credentials and show the native GUI Logon dialog.
    --param NAME=VALUE    : Form parameter passed on the launch URL (repeatable).
    --frmsal-jar <path>   : Custom path to frmsal.jar (optional).

Input Files:
    - oracle-reference-stack/oracle_home_14c/forms/java/frmsal.jar (Official FSAL client)
    - WebLogic Forms Server active on http://localhost:<port>/forms/frmservlet

Output:
    - Native desktop application window running Oracle Forms on macOS or Windows

Key Functions:
    - resolve_workspace_root() -> Path
    - build_fsal_url(form_name: str, port: int, userid: str, interactive: bool, params: Optional[List[str]]) -> str
    - launch_fsal_process(jar_path: Path, url: str) -> int
    - parse_cli_args(argv: List[str]) -> argparse.Namespace
    - main() -> int

Script Dependencies:
    - Python 3.8+ (standard library: sys, os, argparse, subprocess, urllib.parse, pathlib, typing)
    - Java 17+ or Java 25 JRE installed on host OS

Consumed by:
    - run_form.py (final desktop launch step)
    - Developers running standalone Forms sessions on desktop
"""

import sys
import os
import argparse
import subprocess
import urllib.parse
from pathlib import Path
from typing import List, Optional


# Resolves the absolute path to the repository root directory.
def resolve_workspace_root() -> Path:
    """Locate the top-level repository root directory.

    Returns:
        Path pointing to the repository root.
    """
    return Path(__file__).resolve().parents[6]


def resolve_oracle_home(repo_root: Path) -> Path:
    """Resolve Oracle Home from OFW_ORACLE_HOME (env or .env), or the repo default."""
    val = os.environ.get("OFW_ORACLE_HOME", "").strip()
    if not val:
        dotenv = repo_root / ".env"
        if dotenv.is_file():
            for raw in dotenv.read_text(encoding="utf-8").splitlines():
                line = raw.strip()
                if line.startswith("OFW_ORACLE_HOME="):
                    val = line.split("=", 1)[1].strip().strip('"').strip("'")
                    break
    return Path(val) if val else repo_root / "oracle-reference-stack/oracle_home_14c"


# Constructs the complete Forms Listener Servlet URL with safe parameter encoding.
def default_userid() -> str:
    """Logon string built from DB_USER and DB_PASSWORD in the environment (load .env first).
    No password is written in this file. Returns "none" when they are not set, so the FSAL client asks for the logon."""
    user = os.environ.get("DB_USER", "")
    password = os.environ.get("DB_PASSWORD", "")
    return f"{user}/{password}@FREEPDB1" if user and password else "none"


def build_fsal_url(form_name: str, port: int, userid: str, interactive: bool, params: Optional[List[str]] = None) -> str:
    """Construct the HTTP URL for the standaloneapp configuration.

    Args:
        form_name: File name of the .fmx module.
        port: Listening port for WebLogic Forms server.
        userid: Database logon credential string.
        interactive: If True, omit credentials so Forms displays the GUI logon box.
        params: Optional NAME=VALUE form parameters appended to the URL.

    Returns:
        Formatted and URL-encoded query string.
    """
    query = [("config", "standaloneapp"), ("form", form_name)]
    for item in params or []:
        name, separator, value = item.partition("=")
        if not separator or not name:
            raise ValueError(f"Invalid form parameter {item!r}; expected NAME=VALUE")
        query.append((name, value))
    base_url = f"http://localhost:{port}/forms/frmservlet?{urllib.parse.urlencode(query, quote_via=urllib.parse.quote, safe='')}"
    if not interactive and userid and userid.lower() != "none":
        # Password '#' must be URL encoded as '%23'
        encoded_userid = urllib.parse.quote(userid, safe="@/:")
        return f"{base_url}&userid={encoded_userid}"
    return base_url


# Dispatches the Java process running frmsal.jar against the specified URL.
def launch_fsal_process(jar_path: Path, url: str) -> int:
    """Spawn the native FSAL desktop Java process.

    Args:
        jar_path: Path to frmsal.jar.
        url: Complete Forms Servlet connection URL.

    Returns:
        Exit code from the Java process.
    """
    cmd: List[str] = ["java", "-jar", str(jar_path), "-url", url]
    print(f"Launching Forms Standalone Launcher...")
    print(f"  JAR: {jar_path.name}")
    print(f"  URL: {url.split('&userid=')[0]}")
    try:
        return subprocess.call(cmd)
    except KeyboardInterrupt:
        print("\nFSAL window closed.")
        return 0


# Parses CLI arguments for launch_fsal.
def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Parse command-line flags and positional parameters.

    Args:
        argv: Command-line arguments.

    Returns:
        Parsed arguments namespace.
    """
    parser = argparse.ArgumentParser(description="Oracle Forms Standalone Launcher (FSAL) Runner")
    parser.add_argument("form", help="Name or path of the form to run (e.g., test_db.fmx or simple_module.fmx)")
    parser.add_argument("-p", "--port", default=7001, type=int, help="WebLogic AdminServer port (default: 7001)")
    parser.add_argument("-u", "--userid", default=default_userid(), help="Database logon string (default: DB_USER/DB_PASSWORD from .env; if unset, the FSAL client prompts)")
    parser.add_argument("--interactive", action="store_true", help="Omit credentials and show native GUI Logon dialog")
    parser.add_argument("--param", action="append", default=[], metavar="NAME=VALUE", help="Form parameter added to the launch URL (repeatable)")
    parser.add_argument("--frmsal-jar", default=None, help="Custom path to frmsal.jar (optional)")
    return parser.parse_args(argv)


# Main execution entry point for launch_fsal.py.
def main() -> int:
    """Validate environment, resolve launcher JAR, and run FSAL desktop client.

    Returns:
        0 on success, 1 on error.
    """
    args = parse_cli_args(sys.argv[1:])
    form_name = Path(args.form).name
    if not form_name.endswith(".fmx"):
        form_name += ".fmx"

    repo_root = resolve_workspace_root()
    scripts_dir = str(Path(__file__).resolve().parent)
    if scripts_dir not in sys.path:
        sys.path.insert(0, scripts_dir)
    from compile_form import resolve_extra_forms_path
    resolve_extra_forms_path(repo_root)
    if args.frmsal_jar:
        frmsal_path = Path(args.frmsal_jar).resolve()
    else:
        frmsal_path = resolve_oracle_home(repo_root) / "forms/java/frmsal.jar"

    if not frmsal_path.exists():
        print(f"Error: frmsal.jar not found at {frmsal_path}", file=sys.stderr)
        return 1

    try:
        url = build_fsal_url(form_name, args.port, args.userid, args.interactive, args.param)
    except ValueError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
    return launch_fsal_process(frmsal_path, url)


if __name__ == "__main__":
    sys.exit(main())
