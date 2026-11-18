#!/usr/bin/env python3
"""
test_parity.py (CLI)
=====================================

Purpose:
    Live runtime verification harness for Oracle Forms modules running on:
      1. Oracle Fusion Middleware 14c Forms Server (WebLogic 14.1.2 + FSAL)
      2. Native Rust Oracle Forms Workbench (oracle-forms-server + oracle-thin)

    Stages and compiles outside the repository, launches FSAL, probes the Forms servlet,
    runs a native database-query smoke check, and guides interactive session comparison.

Usage Examples:
    python3 plugins/oracle-forms/.apm/skills/testing-forms-parity/scripts/test_parity.py samples/ofw_demo_employees_fmb.xml
    python3 plugins/oracle-forms/.apm/skills/testing-forms-parity/scripts/test_parity.py samples/ofw_sales_fmb.xml
    python3 plugins/oracle-forms/.apm/skills/testing-forms-parity/scripts/test_parity.py samples/test_db_fmb.xml --block BLOCK5

Layer: Tooling / Skill Script (testing-forms-parity)

Key Inputs:
    - OFW_ORACLE_HOME, OFW_WORK_DIR, OFW_FORMS_PATH_EXTRA, and database credentials from env/.env
    - Read-only source module (.xml or .fmb); optional sample SQL is used only from samples/

Key Functions:
    - resolve_oracle_home(repo_root: Path) -> Path
    - work_dir(repo_root: Path) -> Path
    - compile_for_oracle(input_file: Path, repo_root: Path) -> Path
    - test_rust_cli_query(xml_path: Path, block: Optional[str], repo_root: Path) -> Tuple[bool, str]
    - main() -> int
"""

import sys
import os
import argparse
import subprocess
import urllib.request
import urllib.error
import urllib.parse
from pathlib import Path
from typing import Any, Dict, Optional, Tuple


def _read_dotenv() -> Dict[str, str]:
    """Read KEY=VALUE lines from the nearest .env at or above this script's folder (git-ignored)."""
    here = Path(__file__).resolve()
    for folder in [here.parent, *here.parents]:
        candidate = folder / ".env"
        if candidate.is_file():
            values = {}
            for raw in candidate.read_text(encoding="utf-8").splitlines():
                line = raw.strip()
                if line and not line.startswith("#") and "=" in line:
                    key, value = line.split("=", 1)
                    values[key.strip()] = value.strip().strip('"').strip("'")
            return values
    return {}


_DOTENV = _read_dotenv()


def _env(name: str, default: str = "") -> str:
    """Environment variable first, then the .env file, then the default."""
    return os.environ.get(name) or _DOTENV.get(name, default)


def _default_userid() -> str:
    """DB_USER/DB_PASSWORD@DB_SERVICE, from the environment or .env."""
    return f"{_env('DB_USER')}/{_env('DB_PASSWORD')}@{_env('DB_SERVICE', 'FREEPDB1')}"

def resolve_workspace_root() -> Path:
    return Path(__file__).resolve().parents[6]


def resolve_oracle_home(repo_root: Path) -> Path:
    """Resolve the configured Oracle Home, then use the repository default."""
    value = _env("OFW_ORACLE_HOME").strip()
    return Path(os.path.expanduser(value)).resolve() if value else (repo_root / "oracle-reference-stack" / "oracle_home_14c").resolve()


def _is_within(path: Path, parent: Path) -> bool:
    """Return whether a resolved path is equal to or below its resolved parent."""
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def work_dir(repo_root: Path) -> Path:
    """Resolve an external work directory and reject repository-local output paths."""
    value = _env("OFW_WORK_DIR").strip()
    folder = Path(os.path.expanduser(value)) if value else resolve_oracle_home(repo_root) / "ofw-work"
    resolved = folder.resolve()
    if _is_within(resolved, repo_root):
        raise RuntimeError(f"OFW_WORK_DIR must resolve outside the repository: {resolved}")
    return resolved


def check_container(name: str) -> bool:
    """Return whether a named Podman container is running."""
    try:
        res = subprocess.run(
            ["podman", "ps", "--filter", f"name={name}", "--format", "{{.Names}}"],
            capture_output=True,
            text=True,
            check=True,
        )
        return name in res.stdout
    except (OSError, subprocess.CalledProcessError):
        return False


def seed_database_if_present(sql_path: Path) -> bool:
    """Seed only a repository-owned sample schema; never execute external sidecar SQL."""
    if not sql_path.exists():
        return True
    print(f"[*] Seeding database using {sql_path.name}...")
    try:
        cmd = [
            "podman", "exec", "-i", "oracle23ai-lite",
            "sqlplus", "-s", f"{_env('DB_USER')}/{_env('DB_PASSWORD')}@localhost:1521/{_env('DB_SERVICE', 'FREEPDB1')}"
        ]
        with open(sql_path, "rb") as f:
            proc = subprocess.run(cmd, stdin=f, capture_output=True, text=True, check=True)
        print(f"    Schema seeded successfully.")
        return True
    except subprocess.CalledProcessError as e:
        print(f"[!] Warning: database seeding had output:\n{e.stderr or e.stdout}")
        return False


def _load_form_tools(repo_root: Path) -> Tuple[Any, Any]:
    """Load the shared compile and run helpers that own staging and publication."""
    scripts = repo_root / "plugins" / "oracle-forms" / ".apm" / "skills" / "compiling-and-running-oracle-forms" / "scripts"
    if str(scripts) not in sys.path:
        sys.path.insert(0, str(scripts))
    import compile_form
    import run_form
    return compile_form, run_form


def compile_for_oracle(input_file: Path, repo_root: Path) -> Path:
    """Stage an input outside the repository, then use shared conversion and compile helpers."""
    compile_form, run_form = _load_form_tools(repo_root)
    work_dir(repo_root)
    staged = compile_form.stage_module(input_file, repo_root)
    if staged.suffix.lower() == ".xml":
        fmb_file = run_form.convert_xml_to_fmb(staged, repo_root)
    elif staged.suffix.lower() == ".fmb":
        fmb_file = staged
    else:
        raise RuntimeError("Parity verification accepts an XML or FMB source module")
    return run_form.compile_fmb_to_fmx(fmb_file, repo_root, _default_userid(), offline=False)


def probe_weblogic_servlet(fmx_name: str, port: int = 7001) -> bool:
    query = urllib.parse.urlencode({"config": "standaloneapp", "form": fmx_name})
    url = f"http://localhost:{port}/forms/frmservlet?{query}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "OFW-Parity-Probe"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status == 200
    except Exception as e:
        return False


def test_rust_cli_query(xml_path: Path, block: Optional[str], repo_root: Path) -> Tuple[bool, str]:
    print(f"[*] Querying form via native Rust CLI (pure-Rust oracle-thin)...")
    cmd = [
        "cargo", "run", "-q", "-p", "oracle-forms-cli", "--",
        "query", str(xml_path.resolve()),
        "--backend", "oracle-thin",
    ]
    if block:
        cmd.extend(["--block", block])
    
    env = os.environ.copy()
    env["OFW_DB_USER"] = _env("DB_USER")
    env["OFW_DB_PASSWORD"] = _env("DB_PASSWORD")
    
    res = subprocess.run(cmd, cwd=repo_root, capture_output=True, text=True, env=env)
    if res.returncode == 0:
        lines = [line for line in res.stdout.splitlines() if "Records Fetched:" in line or "Execution Time:" in line]
        summary = " | ".join(lines) if lines else "Query succeeded"
        return True, summary
    else:
        return False, res.stderr or res.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description="Test parity between native Rust Oracle Forms Workbench and Fusion Middleware 14c")
    parser.add_argument("form_path", help="Path to input .xml or .fmb module file")
    parser.add_argument("--block", help="Specific block to query in Rust CLI")
    parser.add_argument("--port", type=int, default=7001, help="WebLogic AdminServer port")
    parser.add_argument("--skip-fsal", action="store_true", help="Do not launch desktop FSAL GUI")
    args = parser.parse_args()

    repo_root = resolve_workspace_root()
    input_file = (repo_root / args.form_path).resolve()
    if not input_file.exists():
        input_file = Path(args.form_path).resolve()
    if not input_file.exists():
        print(f"Error: Module file not found: {args.form_path}")
        return 1

    print("======================================================================")
    print("  Oracle Forms Workbench <-> Fusion Middleware 14c Parity Verifier")
    print("======================================================================")
    print(f"  Target Module: {input_file.name}")

    # 1. Check Containers
    db_ok = check_container("oracle23ai-lite")
    forms_name = _env("OFW_FORMS_CONTAINER", "forms-server-14c") or "forms-server-14c"
    fmw_ok = check_container(forms_name)
    print(f"  Oracle Database 23ai container : {'[UP]' if db_ok else '[DOWN]'}")
    print(f"  WebLogic Forms 14c container   : {'[UP]' if fmw_ok else '[DOWN]'}")
    if not db_ok or not fmw_ok:
        print("\n[!] Please ensure both containers are running:")
        print(f"    podman start oracle23ai-lite {forms_name}")
        return 1

    # 2. Seed matching SQL if available
    base_name = input_file.stem.replace("_fmb", "")
    alt_name = base_name.replace("ofw_", "ofw_demo_") + ".sql"
    sql_dir = repo_root / "oracle-reference-stack" / "sql"
    sql_candidate = next(
        (p for p in (
            input_file.parent / f"{base_name}.sql",
            input_file.parent / alt_name,
            sql_dir / f"{base_name}.sql",
            sql_dir / alt_name,
        ) if p.exists()),
        input_file.parent / f"{base_name}.sql",
    )

    if sql_candidate.exists():
        seed_database_if_present(sql_candidate, repo_root)

    # 3. XML -> FMB -> FMX compilation on Fusion Middleware 14c
    fmb_file = input_file
    if input_file.suffix.lower() == ".xml":
        converted = convert_xml_to_fmb(input_file, repo_root)
        if not converted:
            return 1
        fmb_file = converted

    fmx_file = compile_fmb_to_fmx(fmb_file, repo_root)
    if not fmx_file:
        return 1

    # 4. Probe Fusion Middleware Forms Listener Servlet
    servlet_ok = probe_weblogic_servlet(fmx_file.name, args.port)
    print(f"[*] Probing Fusion Middleware HTTP Servlet: {'[200 OK]' if servlet_ok else '[FAILED]'}")

    # 5. Test Native Rust Query Engine
    target_xml = input_file if input_file.suffix.lower() == ".xml" else input_file.with_name(f"{input_file.stem}_fmb.xml")
    rust_ok, rust_summary = False, "No matching XML for Rust CLI"
    if target_xml.exists():
        rust_ok, rust_summary = test_rust_cli_query(target_xml, args.block, repo_root)
        print(f"    Rust Engine Status: {'[SUCCESS]' if rust_ok else '[FAILED]'} ({rust_summary})")

    # 6. Parity Verification Summary & Launch Links
    fsal_url = f"http://localhost:{args.port}/forms/frmservlet?config=standaloneapp&form={fmx_file.name}"
    fsal_cmd = f"java -jar oracle-reference-stack/oracle_home_14c/forms/java/frmsal.jar -url \"{fsal_url}\""

    print("\n======================================================================")
    print("  PARITY VERIFICATION RESULTS")
    print("======================================================================")
    print(f"  [1] Fusion Middleware 14c Compilation : PASS ({fmx_file.name})")
    print(f"  [2] Fusion Middleware WebLogic Servlet : {'PASS (HTTP 200)' if servlet_ok else 'FAIL'}")
    print(f"  [3] Native Rust Engine Execution      : {'PASS' if rust_ok else 'FAIL'}")
    print("----------------------------------------------------------------------")
    print("  HOW TO TEST & COMPARE BOTH ENGINES:")
    print("----------------------------------------------------------------------")
    print("  A. Fusion Middleware 14c (Desktop FSAL):")
    print(f"     {fsal_cmd}")
    print(f"     Direct Servlet URL: {fsal_url}")
    print()
    print("  B. Native Rust Workbench (Browser):")
    print("     cargo run -p oracle-forms-server")
    print(f"     Open http://127.0.0.1:7070 -> Select '{target_xml.name if target_xml.exists() else input_file.name}' -> Click [▶ Run Form]")
    print("======================================================================\n")

    return 0 if (servlet_ok and rust_ok) else 1


if __name__ == "__main__":
    sys.exit(main())
