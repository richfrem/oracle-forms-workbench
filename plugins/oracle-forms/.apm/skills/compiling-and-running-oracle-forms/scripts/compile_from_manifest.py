#!/usr/bin/env python3
"""
compile_from_manifest.py (CLI)
=====================================

Purpose:
    Stage 3 of the batch pipeline. Reads <build-dir>/build-manifest.json (written by the converting skill's
    build_dependency_manifest.py), asks the user to log in to the target database, runs a container-side
    sqlplus logon check, then compiles every module with frmcmp_batch in manifest (dependency) order and
    publishes the results to <Oracle Home>/forms/.

Layer: Tooling / Skill Script (compiling-and-running-oracle-forms)

Usage Examples:
    python3 compile_from_manifest.py --build-dir <external-folder>
    python3 compile_from_manifest.py --manifest <external-folder>/build-manifest.json --userid <user>@<db>
    python3 compile_from_manifest.py --build-dir <external-folder> --offline --no-publish

Input Dependencies:
    - build-manifest.json with modules already staged (and XML converted) in <build-dir>/stage
    - Running container forms-server-14c; <build-dir> inside the Oracle Home mount (default OFW_WORK_DIR is)
    - Credentials: --userid, DB_USER/DB_PASSWORD/DB_SERVICE from .env, or an interactive prompt (never on the command line)

Output:
    .plx / .mmx / .fmx beside each staged module, published to <Oracle Home>/forms/ unless --no-publish;
    per-module status and diagnostics written back to the manifest.

Key Functions:
    - resolve_userid(userid, offline) -> Optional[str]
    - preflight(userid: str) -> Tuple[bool, str]
    - compile_module(module: Dict, build_dir: Path, userid: Optional[str]) -> Tuple[bool, str]
    - run_manifest(manifest_path: Path, userid: Optional[str], no_publish: bool) -> int
    - main() -> int
"""

import argparse
import getpass
import json
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

import compile_form as cf

MODULE_TYPE = {"PLL": "library", "MMB": "menu", "FMB": "form"}
CONTAINER = "forms-server-14c"
TNS_ADMIN = "/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig"


def resolve_userid(userid: Optional[str], offline: bool) -> Optional[str]:
    """Logon string for the compiler, or None when offline. Prompts (input/getpass) for what is missing so a
    password never has to be typed on the command line or stored in a file."""
    if offline:
        return None
    if userid and "/" in userid:
        return userid
    if userid:
        user, _, target = userid.partition("@")
    elif cf._env("DB_USER") and cf._env("DB_PASSWORD"):
        return cf._default_userid()
    else:
        user = input("Database user: ").strip()
        target = input(f"Connect alias [{cf._env('DB_SERVICE', 'FREEPDB1')}]: ").strip() or cf._env("DB_SERVICE", "FREEPDB1")
    password = getpass.getpass(f"Password for {user}@{target}: ")
    return f"{user}/{password}@{target}"


def _redact(text: str, userid: Optional[str]) -> str:
    """Remove the logon string (raw and quoted forms) from any output."""
    for secret in {userid, cf.format_userid(userid)} if userid else set():
        text = text.replace(secret, "<redacted>")
    if userid and "/" in userid:
        text = text.replace(userid.split("/", 1)[1].rsplit("@", 1)[0], "<redacted>")
    return text


def _container_run(script: str) -> subprocess.CompletedProcess:
    """Run a shell script inside the Forms container as the oracle user; the script goes in on stdin."""
    return subprocess.run(cf.podman_cmd(["exec", "-i", "-u", "oracle", CONTAINER, "/bin/sh", "-s"]),
                          input=script, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def preflight(userid: str) -> Tuple[bool, str]:
    """sqlplus -L -S logon from inside the container. The password reaches sqlplus on stdin, not on a command
    line. Reports the roles the account can see; success means a logon and a query worked."""
    env = (f"export ORACLE_HOME=/oracle_home_14c; export LD_LIBRARY_PATH=/oracle_home_14c/lib; "
           f"export TNS_ADMIN={TNS_ADMIN}; ")
    logon = cf.format_userid(userid)
    script = env + ("/oracle_home_14c/bin/sqlplus -L -S /nolog <<'SQL'\n"
                    f"connect {logon}\nset heading off feedback off\n"
                    "select 'ROLES=' || count(*) from session_roles;\nselect 'OBJECTS=' || count(*) from all_objects;\nexit\nSQL\n")
    out = _redact(_container_run(script).stdout, userid).strip()
    ok = "ROLES=" in out and "ORA-" not in out and "SP2-" not in out
    return ok, out


def _forms_command(module_path: str, mod_type: str, logon: str, stage_path: str) -> str:
    """frmcmp_batch script for one staged module; FORMS_PATH covers the stage folder and the Forms home."""
    return cf.build_compiler_command(module_path, mod_type, logon, stage_path)


def compile_module(module: Dict, build_dir: Path, userid: Optional[str]) -> Tuple[bool, str]:
    """Compile one staged module in the container. Returns (ok, redacted diagnostics)."""
    repo = cf.resolve_workspace_root()
    binary = build_dir / module["binaryFile"]
    if not binary.is_file():
        return False, f"{module['binaryFile']} is missing; run the conversion stage first."
    try:
        module_path = cf.container_path(binary, repo)
        stage_path = cf.container_path(build_dir / "stage", repo)
    except RuntimeError as err:
        return False, str(err)
    logon = "logon=no" if userid is None else "userid=" + shlex.quote(cf.format_userid(userid))
    cmd = _forms_command(module_path, MODULE_TYPE[module["artifactType"]], logon, stage_path)
    res = _container_run(cmd)
    err_file = binary.with_suffix(".err")
    detail = res.stdout + (err_file.read_text(errors="replace") if err_file.is_file() else "")
    produced = build_dir / module["executableFile"]
    return produced.is_file() and res.returncode == 0, _redact(detail, userid).strip()


def publish_outputs(compiled: Path, repo_root: Path) -> None:
    """Publish to <Oracle Home>/forms/ and add a case-variant symlink for .mmx/.plx (Linux is case-sensitive,
    so a lookup in the other case would otherwise fail with FRM-10221 / PDE-PLI018)."""
    cf.publish_outputs(compiled, repo_root)
    if compiled.suffix.lower() not in (".mmx", ".plx"):
        return
    name = compiled.name
    variant = name.lower() if name != name.lower() else compiled.stem.upper() + compiled.suffix.lower()
    home = f"{cf._CONTAINER_HOME}/forms"
    script = f"cd {home} && [ ! -e {shlex.quote(variant)} ] && ln -s {shlex.quote(name)} {shlex.quote(variant)}; true"
    subprocess.run(cf.podman_cmd(["exec", CONTAINER, "/bin/sh", "-c", script]), check=True)


def require_oracle_home_build(build_dir: Path) -> None:
    """Fail early (before any credential prompt) unless the build folder is visible to the container."""
    repo = cf.resolve_workspace_root()
    try:
        cf.container_path(build_dir, repo)
    except RuntimeError as err:
        raise RuntimeError(f"--build-dir {build_dir} is not inside OFW_ORACLE_HOME ({cf.oracle_home_host(repo)}), "
                           f"so forms-server-14c cannot see it. Use a folder under OFW_WORK_DIR. ({err})") from err


def _blocked_by(module: Dict, failed: Set[str]) -> List[str]:
    """Names of failed or skipped providers this module depends on."""
    deps = module["dependencies"]
    names = [n for key in ("objectLibraries", "attachedLibraries", "menus", "parentModules") for n in deps[key]]
    return sorted(n for n in names if n.casefold() in failed)


def run_manifest(manifest_path: Path, userid: Optional[str], no_publish: bool) -> int:
    """Compile every module in manifest order, skipping dependents of failures, and record status."""
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    build_dir = Path(manifest["buildDir"])
    failed: Set[str] = set()
    for module in manifest["modules"]:
        if module["artifactType"] == "OLB":
            continue  # provider only: already in stage/
        blocked = _blocked_by(module, failed)
        if blocked:
            module["status"], module["diagnostics"] = "skipped", f"provider failed or skipped: {', '.join(blocked)}"
            failed.add(module["name"].casefold())
            manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
            continue
        print(f"[{module['order']}] compile {module['name']} ({MODULE_TYPE[module['artifactType']]})")
        ok, detail = compile_module(module, build_dir, userid)
        module["status"], module["diagnostics"] = ("compiled" if ok else "failed"), detail
        if ok and not no_publish:
            publish_outputs(build_dir / module["executableFile"], cf.resolve_workspace_root())
        if not ok:
            failed.add(module["name"].casefold())
            print(detail, file=sys.stderr)
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return 1 if failed else 0


def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Command-line options."""
    parser = argparse.ArgumentParser(description="Compile Oracle Forms modules in manifest order")
    parser.add_argument("--manifest", help="Path to build-manifest.json")
    parser.add_argument("--build-dir", help="Build folder that holds build-manifest.json")
    parser.add_argument("-u", "--userid", help="user@db (password is prompted) or user/password@db")
    parser.add_argument("--offline", action="store_true", help="Compile with logon=no")
    parser.add_argument("--no-publish", action="store_true", help="Do not copy outputs to <Oracle Home>/forms")
    parser.add_argument("--skip-preflight", action="store_true", help="Skip the sqlplus logon check")
    return parser.parse_args(argv)


def main() -> int:
    """Resolve the manifest and credentials, preflight, then compile."""
    args = parse_cli_args(sys.argv[1:])
    raw = args.manifest or (str(Path(args.build_dir) / "build-manifest.json") if args.build_dir else "")
    manifest_path = Path(raw)
    if not raw or not manifest_path.is_file():
        print("Error: pass --manifest or --build-dir pointing at an existing build-manifest.json", file=sys.stderr)
        return 1
    try:
        require_oracle_home_build(Path(json.loads(manifest_path.read_text(encoding="utf-8"))["buildDir"]))
    except (RuntimeError, KeyError, ValueError) as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
    userid = resolve_userid(args.userid, args.offline)
    if userid and not args.skip_preflight:
        ok, out = preflight(userid)
        print(out)
        if not ok:
            print("Preflight failed: the container could not log in and query. Fix the logon first.", file=sys.stderr)
            return 2
    return run_manifest(manifest_path, userid, args.no_publish)


if __name__ == "__main__":
    sys.exit(main())
