#!/usr/bin/env python3
"""
compile_from_manifest.py (CLI)
=====================================

Purpose:
    Stage 3 of the batch pipeline. Reads <build-dir>/build-manifest.json (written by the converting skill's
    build_dependency_manifest.py), asks the user to log in to the target database, runs a container-side
    sqlplus logon check, then compiles every module with frmcmp_batch in tier and dependency order and
    writes the compiled results straight into <OFW_ORACLE_HOME>/forms_apps/<app_name>/ (outside $ORACLE_HOME/forms).

Layer: Tooling / Skill Script (compiling-and-running-oracle-forms)

Usage Examples:
    python3 compile_from_manifest.py --build-dir <external-folder> --app-name my_app
    python3 compile_from_manifest.py --build-dir <external-folder> --types PLL --no-deps
    python3 compile_from_manifest.py --build-dir <external-folder> --types PLL --skip-compiled
    python3 compile_from_manifest.py --manifest <external-folder>/build-manifest.json --userid <user>@<db>
    python3 compile_from_manifest.py --build-dir <external-folder> --offline --no-publish

Input Dependencies:
    - build-manifest.json with modules already staged (and XML converted) in <build-dir>/stage
    - Running container forms-server-14c; <build-dir> inside the Oracle Home mount (default OFW_WORK_DIR is)
    - Credentials: --userid (user@db), or an interactive prompt for user, alias and password. The repo .env holds
      local testbed credentials and is used only with --use-local-env (never on the command line)

Output:
    .plx / .mmx / .fmx written straight into <OFW_ORACLE_HOME>/forms_apps/<app_name>/ (and registered in default.env),
    or beside each staged module with --no-publish; per-module status and diagnostics written back to the manifest.

Key Functions:
    - resolve_userid(userid, offline) -> Optional[str]
    - container_running() -> Tuple[bool, str]
    - preflight(userid: str) -> Tuple[bool, str]
    - resolve_app_dir(manifest: Dict, build_dir: Path, app_name: Optional[str], output_dir: Optional[str]) -> Path
    - compile_module(module: Dict, build_dir: Path, userid: Optional[str], publish: bool, app_dir: Optional[Path] = None) -> Tuple[bool, str]
    - run_manifest(manifest_path: Path, userid: Optional[str], no_publish: bool, ...) -> int
    - main() -> int
"""

import argparse
import fnmatch
import getpass
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

import compile_form as cf

MODULE_TYPE = {"PLL": "library", "MMB": "menu", "FMB": "form"}
TYPE_TIER = {"OLB": 0, "PLL": 1, "MMB": 2, "FMB": 3}
CONTAINER = cf.SERVER_CONTAINER
TNS_ADMIN = "/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig"

# One implementation of the logon prompt and the container check, shared with compile_form.py.
resolve_userid = cf.resolve_userid
container_running = cf.container_running


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


def resolve_app_dir(manifest: Dict, build_dir: Path, app_name: Optional[str] = None,
                    output_dir: Optional[str] = None) -> Path:
    """Resolve the dedicated app output folder under <OFW_WORK_DIR>/apps/<app_name>/ (or explicit --output-dir)."""
    repo = cf.resolve_workspace_root()
    if output_dir and output_dir.strip():
        return cf.app_output_dir(repo, output_dir=output_dir)
    derived = app_name or manifest.get("appName")
    if not derived:
        src_raw = manifest.get("sourceDir", "")
        src_name = Path(src_raw).name if src_raw else ""
        if src_name and src_name not in ("source", "build", "stage", "ofw-work"):
            derived = src_name
        elif build_dir.name not in ("build", "stage", "ofw-work"):
            derived = build_dir.name
    return cf.app_output_dir(repo, app_name=derived)


def _forms_command(module_path: str, mod_type: str, logon: str, stage_path: str, output_file: str = "",
                   app_output_path: str = "") -> str:
    """frmcmp_batch script for one staged module; FORMS_PATH orders app output -> stage -> /oracle_home_14c/forms."""
    return cf.build_compiler_command(module_path, mod_type, logon, stage_path, output_file, app_output_path)


def compile_module(module: Dict, build_dir: Path, userid: Optional[str], publish: bool = True,
                   app_dir: Optional[Path] = None) -> Tuple[bool, str]:
    """Compile one staged module in the container. With publish the compiled file is written straight into
    the dedicated app folder (<OFW_WORK_DIR>/apps/<app_name>/); otherwise it stays beside the module in stage/.
    Returns (ok, redacted diagnostics)."""
    repo = cf.resolve_workspace_root()
    binary = build_dir / module["binaryFile"]
    if not binary.is_file():
        return False, f"{module['binaryFile']} is missing; run the conversion stage first."
    exe_name = Path(module["executableFile"]).name
    dest_dir = app_dir or cf.app_output_dir(repo)
    try:
        module_path = cf.container_path(binary, repo)
        stage_path = cf.container_path(build_dir / "stage", repo)
        app_cpath = cf.container_path(dest_dir, repo) if publish else ""
    except RuntimeError as err:
        return False, str(err)
    output_file = f"{app_cpath}/{exe_name}" if publish else ""
    logon = "logon=no" if userid is None else "userid=" + shlex.quote(cf.format_userid(userid))
    cmd = _forms_command(module_path, MODULE_TYPE[module["artifactType"]], logon, stage_path, output_file, app_cpath)
    res = _container_run(cmd)
    err_file = binary.with_suffix(".err")
    detail = res.stdout + (err_file.read_text(errors="replace") if err_file.is_file() else "")
    produced = (dest_dir / exe_name) if publish else build_dir / module["executableFile"]
    return produced.is_file() and res.returncode == 0, _redact(detail, userid).strip()


def add_case_variant(exe_name: str, dest_cpath: str) -> None:
    """Add a case-variant symlink in the app output folder for .mmx/.plx (Linux is case-sensitive, so a lookup in
    the other case would otherwise fail with FRM-10221 / PDE-PLI018)."""
    if Path(exe_name).suffix.lower() not in (".mmx", ".plx"):
        return
    variant = exe_name.lower() if exe_name != exe_name.lower() else Path(exe_name).stem.upper() + Path(exe_name).suffix.lower()
    script = f"cd {shlex.quote(dest_cpath)} && [ ! -e {shlex.quote(variant)} ] && ln -s {shlex.quote(exe_name)} {shlex.quote(variant)}; true"
    subprocess.run(cf.podman_cmd(["exec", CONTAINER, "/bin/sh", "-c", script]), check=True)


def require_oracle_home_build(build_dir: Path) -> None:
    """Fail early (before any credential prompt) unless the build folder is visible to the container."""
    repo = cf.resolve_workspace_root()
    try:
        cf.container_path(build_dir, repo)
    except RuntimeError as err:
        raise RuntimeError(f"--build-dir {build_dir} is not inside OFW_ORACLE_HOME ({cf.oracle_home_host(repo)}), "
                           f"so forms-server-14c cannot see it. Use a folder under OFW_WORK_DIR. ({err})") from err


_DEP_TYPE = {
    "objectLibraries": "OLB",
    "attachedLibraries": "PLL",
    "menus": "MMB",
    "parentModules": "FMB",
}


def _blocked_by(module: Dict, failed: Set[Tuple[str, str]], stage_index: Optional[Dict[Tuple[str, str], str]] = None) -> List[str]:
    """Names of failed or skipped providers this module depends on (matched by artifact type and name).

    A failed parent FMB only blocks child FMBs when its .fmb binary is absent from stage/;
    reference/template forms often cannot compile standalone .fmx files yet still supply
    subclassed objects from their .fmb binary at compile time.
    """
    deps = module["dependencies"]
    blocked = [
        n
        for key, dep_type in _DEP_TYPE.items()
        for n in deps.get(key, [])
        if (dep_type, n.casefold()) in failed
        and not (key == "parentModules" and stage_index is not None and (n.casefold(), ".fmb") in stage_index)
    ]
    return sorted(blocked)


def _index_casefold(directory: Optional[Path]) -> Dict[Tuple[str, str], str]:
    """Map (stem.casefold(), suffix.lower()) -> actual filename in directory."""
    index: Dict[Tuple[str, str], str] = {}
    if directory and directory.is_dir():
        for entry in directory.iterdir():
            index.setdefault((entry.stem.casefold(), entry.suffix.lower()), entry.name)
    return index


def ensure_manifest_case_symlinks(manifest: Dict, stage_dir: Path, app_dir: Optional[Path] = None) -> List[str]:
    """Create exact-case symlinks in stage/ and app_dir for mixed/uppercase dependency names on Linux."""
    stage_idx = _index_casefold(stage_dir)
    app_idx = _index_casefold(app_dir)
    created: List[str] = []

    def _link(directory: Optional[Path], index: Dict[Tuple[str, str], str], name: str, ext: str) -> None:
        if not directory:
            return
        target = index.get((name.casefold(), ext))
        want = name + ext
        if not target or want == target:
            return
        dst = directory / want
        if not dst.exists() and not dst.is_symlink():
            try:
                dst.symlink_to(target)
                created.append(f"{directory.name}/{want} -> {target}")
            except OSError:
                pass

    for mod in manifest.get("modules", []):
        deps = mod.get("dependencies") or {}
        for n in deps.get("parentModules", []):
            _link(stage_dir, stage_idx, n, ".fmb")
        for n in deps.get("attachedLibraries", []):
            _link(stage_dir, stage_idx, n, ".pll")
            _link(app_dir, app_idx, n, ".plx")
        for n in deps.get("menus", []):
            _link(stage_dir, stage_idx, n, ".mmb")
            _link(app_dir, app_idx, n, ".mmx")
        for n in deps.get("objectLibraries", []):
            _link(stage_dir, stage_idx, n, ".olb")
    return created


def _save_manifest(manifest_path: Path, manifest: Dict) -> None:
    """Write the manifest atomically (temp file + os.replace) so an interrupted run never leaves half a JSON file."""
    temp = manifest_path.with_name(manifest_path.name + ".tmp")
    temp.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    os.replace(temp, manifest_path)


def _has_dependencies(module: Dict) -> bool:
    """True when the module declares any upstream OLB, PLL, MMB, or parent FMB dependency."""
    deps = module.get("dependencies") or {}
    return any(deps.get(k) for k in ("objectLibraries", "attachedLibraries", "menus", "parentModules"))


def _parse_types(raw: Optional[str]) -> Optional[Set[str]]:
    """Parse comma-separated artifact types (PLL, MMB, FMB) for --types filtering."""
    if not raw:
        return None
    selected = {part.strip().upper() for part in raw.split(",") if part.strip()}
    invalid = selected - set(MODULE_TYPE)
    if invalid:
        raise ValueError(f"Unsupported --types value(s): {', '.join(sorted(invalid))}. Choose from PLL, MMB, FMB.")
    return selected


def _parse_modules(raw: Optional[str]) -> Optional[List[str]]:
    """Parse comma-separated module name patterns (supports * wildcards, case-insensitive)."""
    if not raw:
        return None
    patterns = [part.strip().casefold() for part in raw.split(",") if part.strip()]
    return patterns or None


def _matches_modules(module: Dict, patterns: Optional[List[str]]) -> bool:
    """True when no --modules filter was given or module name matches any pattern."""
    if not patterns:
        return True
    name = module["name"].casefold()
    return any(fnmatch.fnmatchcase(name, pat) for pat in patterns)


def _is_already_compiled(module: Dict, build_dir: Path, publish: bool, app_dir: Optional[Path] = None) -> bool:
    """True when the module is marked compiled and its target executable exists on disk."""
    if module.get("status") != "compiled" or not module.get("executableFile"):
        return False
    exe_name = Path(module["executableFile"]).name
    if publish:
        dest_dir = app_dir or cf.app_output_dir(cf.resolve_workspace_root())
        target = dest_dir / exe_name
    else:
        target = build_dir / module["executableFile"]
    return target.is_file()


def run_manifest(
    manifest_path: Path,
    userid: Optional[str],
    no_publish: bool,
    types_filter: Optional[Set[str]] = None,
    no_deps_only: bool = False,
    skip_compiled: bool = False,
    app_name: Optional[str] = None,
    output_dir: Optional[str] = None,
    module_patterns: Optional[List[str]] = None,
) -> int:
    """Compile every selected module in tier and manifest order, skipping dependents of failures."""
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    build_dir = Path(manifest["buildDir"])
    repo = cf.resolve_workspace_root()
    app_dir: Optional[Path] = None
    app_cpath = ""
    if not no_publish:
        app_dir = cf.ensure_dir_writable(resolve_app_dir(manifest, build_dir, app_name, output_dir))
        app_cpath = cf.container_path(app_dir, repo)
        cf._ensure_runtime_forms_path([app_cpath])
        print(f"Publishing compiled binaries to: {app_dir} (container: {app_cpath})")
    failed: Set[Tuple[str, str]] = set()
    ensure_manifest_case_symlinks(manifest, build_dir / "stage", app_dir)
    ordered_modules = sorted(manifest["modules"], key=lambda m: TYPE_TIER.get(m.get("artifactType", ""), 9))
    selected = [
        m for m in ordered_modules
        if m["artifactType"] != "OLB"  # provider only: already in stage/
        and (types_filter is None or m["artifactType"] in types_filter)
        and not (no_deps_only and _has_dependencies(m))
        and _matches_modules(m, module_patterns)
    ]
    total = len(selected)
    compiled_count = 0
    skipped_count = 0
    for idx, module in enumerate(selected, 1):
        prefix = f"[{idx}/{total} | #{module['order']}]"
        if skip_compiled and _is_already_compiled(module, build_dir, not no_publish, app_dir):
            print(f"{prefix} skip {module['name']} (already compiled)")
            skipped_count += 1
            continue
        blocked = _blocked_by(module, failed, _index_casefold(build_dir / "stage"))
        if blocked:
            module["status"], module["diagnostics"] = "skipped", f"provider failed or skipped: {', '.join(blocked)}"
            module["compiled"] = {"status": "skipped", "diagnostics": module["diagnostics"]}
            failed.add((module["artifactType"], module["name"].casefold()))
            skipped_count += 1
            print(f"{prefix} skip {module['name']} ({module['diagnostics']})")
            _save_manifest(manifest_path, manifest)
            continue
        print(f"{prefix} compile {module['name']} ({MODULE_TYPE[module['artifactType']]})")
        ok, detail = compile_module(module, build_dir, userid, not no_publish, *([app_dir] if app_dir is not None else []))
        module["status"], module["diagnostics"] = ("compiled" if ok else "failed"), detail
        module["compiled"] = {"status": "done" if ok else "failed", "diagnostics": detail}
        if ok:
            compiled_count += 1
            if not no_publish and app_cpath:
                add_case_variant(Path(module["executableFile"]).name, app_cpath)
        else:
            failed.add((module["artifactType"], module["name"].casefold()))
            print(detail, file=sys.stderr)
        _save_manifest(manifest_path, manifest)
    print(f"Summary: {compiled_count} compiled, {skipped_count} skipped, {len(failed)} failed (of {total})")
    return 1 if failed else 0


def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Command-line options."""
    parser = argparse.ArgumentParser(description="Compile Oracle Forms modules in manifest order")
    parser.add_argument("--manifest", help="Path to build-manifest.json")
    parser.add_argument("--build-dir", help="Build folder that holds build-manifest.json")
    parser.add_argument("--app-name", default=None, help="Dedicated app directory name under <OFW_ORACLE_HOME>/forms_apps/<app_name>/")
    parser.add_argument("--output-dir", default=None, help="Explicit host output directory for compiled binaries (overrides --app-name)")
    parser.add_argument("--types", default=None, help="Comma-separated artifact types to compile (e.g. PLL or PLL,MMB)")
    parser.add_argument("--modules", default=None, help="Comma-separated module names or wildcards to compile (e.g. 'apple*,applj*,agt*')")
    parser.add_argument("--no-deps", action="store_true", help="Compile only modules with zero upstream dependencies")
    parser.add_argument("--skip-compiled", action="store_true", help="Skip modules already marked compiled whose output exists")
    parser.add_argument("-u", "--userid", help="user@db (password is prompted) or user/password@db")
    parser.add_argument("--offline", action="store_true", help="Compile with logon=no")
    parser.add_argument("--no-publish", action="store_true", help="Compile into stage/ instead of straight into <OFW_ORACLE_HOME>/forms_apps/<app_name>/")
    parser.add_argument("--skip-preflight", action="store_true", help="Skip the sqlplus logon check")
    parser.add_argument("--use-local-env", action="store_true", help="Use DB_USER/DB_PASSWORD/DB_SERVICE from the repo .env (local testbed) instead of prompting")
    return parser.parse_args(argv)


def main() -> int:
    """Resolve the manifest and credentials, preflight, then compile."""
    args = parse_cli_args(sys.argv[1:])
    try:
        types_filter = _parse_types(args.types)
    except ValueError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
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
    running, state = container_running()
    if not running:
        print(f"Error: {CONTAINER} is not running (state: {state or 'not found'}). Nothing was compiled and no login "
              "was attempted. Start the stack first: python3 start_testbed.py", file=sys.stderr)
        return 2
    userid = resolve_userid(args.userid, args.offline, args.use_local_env)
    if userid and not args.skip_preflight:
        ok, out = preflight(userid)
        print(out)
        if not ok:
            print("Preflight failed: the container could not log in and query. Fix the logon first.", file=sys.stderr)
            return 2
    return run_manifest(
        manifest_path, userid, args.no_publish, types_filter, args.no_deps, args.skip_compiled,
        args.app_name, args.output_dir, _parse_modules(args.modules),
    )


if __name__ == "__main__":
    sys.exit(main())
