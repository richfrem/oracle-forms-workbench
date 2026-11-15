#!/usr/bin/env python3
"""
compile_form.py (CLI)
=====================================

Purpose:
    Headless batch compiler wrapper for Oracle Forms (.fmb), menus (.mmb),
    and libraries (.pll) using frmcmp_batch inside the Forms 14c container
    with automatic environment variable injection, TNS Net services binding,
    and structured error parsing.

Layer: Tooling / Skill Script (compiling-and-running-oracle-forms)

Usage Examples:
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py samples/test_db.fmb
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py samples/simple_module.fmb --offline
    python3 plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/compile_form.py samples/mainmenu.mmb --type menu

Supported Object Types:
    - .fmb (Oracle Forms binary form module) -> .fmx
    - .mmb (Oracle Forms binary menu module) -> .mmx
    - .pll (Oracle Forms binary PL/SQL library) -> .plx

CLI Arguments:
    module                : Path to input .fmb, .mmb, or .pll file.
    -t, --type <str>      : Module type (form, menu, library; default: form).
    --offline             : Compile offline without database logon (logon=no).
    -u, --userid <str>    : Database logon connection string (default: DB_USER/DB_PASSWORD/DB_SERVICE from .env).

Input Files:
    - Target source module (.fmb, .mmb, or .pll)
    - Container 'forms-server-14c' running with /oracle_home_14c/bin/frmcmp_batch
    - TNS configuration: /oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig/tnsnames.ora

Output:
    - Compiled file (.fmx, .mmx, or .plx) written in the work folder (OFW_WORK_DIR, default <Oracle Home>/ofw-work/stage)
      and copied to <Oracle Home>/forms/ unless --no-publish is given. Nothing is written inside the repository.

Key Functions:
    - resolve_workspace_root() -> Path
    - build_compiler_command(container_path: str, mod_type: str, logon_param: str) -> str
    - execute_compilation(cmd: str) -> int
    - parse_cli_args(argv: List[str]) -> argparse.Namespace
    - main() -> int

Script Dependencies:
    - Python 3.8+ (standard library: sys, os, argparse, subprocess, pathlib, typing)
    - Podman CLI available on host system
    - Running container: forms-server-14c

Consumed by:
    - run_form.py (compilation stage before launch)
    - Developers running batch compiles of legacy forms
"""

import sys
import os
import argparse
import platform
import shlex
import shutil
import subprocess
from pathlib import Path
from typing import List


# Resolves the absolute path to the repository root directory.
def _read_dotenv() -> dict:
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

def format_userid(userid: str) -> str:
    """Make a user/password@target logon safe for frmcmp_batch.

    Oracle needs a password with characters other than letters, digits, _, # and $ to be wrapped in double
    quotes (a bare '@', '/', '(' or ')' ends the field early). An already-quoted password is left alone.
    """
    if "/" not in userid or "@" not in userid:
        return userid
    user, rest = userid.split("/", 1)
    if rest.startswith('"'):
        return userid
    password, target = rest.rsplit("@", 1)
    if password and any(not (c.isalnum() or c in "_#$") for c in password) and '"' not in password:
        password = f'"{password}"'
    return f"{user}/{password}@{target}"


def resolve_workspace_root() -> Path:
    """Locate the top-level repository root directory.

    Returns:
        Path pointing to the repository root.
    """
    return Path(__file__).resolve().parents[6]


# Attached source modules that frmcmp_batch must find beside the module it compiles. Compiled .plx and .mmx
# files are not copied in: the ones built here are published to <Oracle Home>/forms, which is on FORMS_PATH.
_FORMS_DEP_EXTS = {".mmb", ".pll", ".olb"}
_CONTAINER_HOME = "/oracle_home_14c"


def oracle_home_host(repo_root: Path) -> Path:
    """Host folder of the Oracle Home (OFW_ORACLE_HOME, else the in-repo default)."""
    raw = _env("OFW_ORACLE_HOME").strip()
    return Path(os.path.expanduser(raw)) if raw else repo_root / "oracle-reference-stack" / "oracle_home_14c"


def work_dir(repo_root: Path) -> Path:
    """Folder for every staged, compiled and converted file: OFW_WORK_DIR, else <Oracle Home>/ofw-work.

    It is never inside the repository. The default sits inside the Oracle Home, which forms-server-14c already
    mounts at /oracle_home_14c, so no extra container mount is needed.
    """
    raw = _env("OFW_WORK_DIR").strip()
    return Path(os.path.expanduser(raw)) if raw else oracle_home_host(repo_root) / "ofw-work"


def ensure_work_dir(repo_root: Path) -> Path:
    """Create the work folder, writable by the host user and by the container, and return it.

    The Oracle Home belongs to the container user, which rootless Podman maps to a UID the host user cannot
    write as. On Linux and WSL the folder is therefore created once through `podman unshare`, owned by the host
    user and world-writable so the container can write into it as well.
    """
    folder = work_dir(repo_root)
    if folder.is_dir() and os.access(folder, os.W_OK):
        return folder
    try:
        folder.mkdir(parents=True, exist_ok=True)
        folder.chmod(0o777)
    except OSError:
        if platform.system() == "Windows":
            raise RuntimeError(f"Cannot create the work folder {folder}. Run this script from WSL.")
        script = f"mkdir -p {shlex.quote(str(folder))} && chown 0:0 {shlex.quote(str(folder))} && chmod 777 {shlex.quote(str(folder))}"
        res = subprocess.run(podman_cmd(["unshare", "sh", "-c", script]), stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"Cannot create the work folder {folder}:\n{res.stdout}")
    if not os.access(folder, os.W_OK):
        raise RuntimeError(f"The work folder {folder} is not writable by this user. Set OFW_WORK_DIR to a writable folder.")
    return folder


def container_path(path: Path, repo_root: Path) -> str:
    """Container path of a host file that is inside the repository (/work) or the Oracle Home (/oracle_home_14c)."""
    resolved = path.resolve()
    for base, mount in ((oracle_home_host(repo_root), _CONTAINER_HOME), (repo_root, "/work")):
        try:
            rel = resolved.relative_to(base.resolve())
        except ValueError:
            continue
        return mount if str(rel) == "." else f"{mount}/{rel.as_posix()}"
    raise RuntimeError(f"{resolved} is not under the repository or the Oracle Home, so forms-server-14c cannot see it (it mounts only the repository and the Oracle Home). Keep OFW_WORK_DIR inside the Oracle Home.")


def _sync_file(src: Path, dst: Path) -> None:
    """Copy src to dst unless an identical (same size and modification time) copy is already there."""
    if dst.is_file() and dst.stat().st_size == src.stat().st_size and int(dst.stat().st_mtime) == int(src.stat().st_mtime):
        return
    shutil.copy2(src, dst)


def extra_forms_dirs() -> List[Path]:
    """Existing host folders from OFW_FORMS_PATH_EXTRA."""
    raw = _env("OFW_FORMS_PATH_EXTRA").strip()
    found = []
    for entry in [part.strip() for part in raw.split(":") if part.strip()]:
        host_dir = Path(entry).expanduser()
        if host_dir.is_dir():
            found.append(host_dir.resolve())
        else:
            print(f"Warning: OFW_FORMS_PATH_EXTRA directory not found: {entry}", file=sys.stderr)
    return found


def stage_folder(repo_root: Path) -> Path:
    """The shared folder inside the work folder that holds modules and their attached libraries, menus and object libraries."""
    folder = ensure_work_dir(repo_root) / "stage"
    folder.mkdir(parents=True, exist_ok=True)
    return folder


def stage_module(module: Path, repo_root: Path) -> Path:
    """Copy a module, plus the attached modules found beside it and in OFW_FORMS_PATH_EXTRA, into the stage folder.

    Returns the staged copy of the module. A module already inside the work folder is used where it is.
    """
    module = module.resolve()
    folder = stage_folder(repo_root)
    try:
        module.relative_to(work_dir(repo_root).resolve())
        return module
    except ValueError:
        pass
    for source_dir in [module.parent, *extra_forms_dirs()]:
        for src in source_dir.iterdir():
            if src.is_file() and src.suffix.lower() in _FORMS_DEP_EXTS:
                _sync_file(src, folder / src.name)
    staged = folder / module.name
    shutil.copy2(module, staged)
    return staged


def publish_outputs(compiled: Path, repo_root: Path) -> None:
    """Copy a compiled .fmx, .mmx or .plx into the Oracle Home forms folder that the Forms server loads from."""
    cmd = f"cp {shlex.quote(container_path(compiled, repo_root))} {_CONTAINER_HOME}/forms/{compiled.name}"
    subprocess.run(podman_cmd(["exec", "forms-server-14c", "/bin/sh", "-c", cmd]), check=True)


def resolve_extra_forms_path(repo_root: Path) -> str:
    """Container FORMS_PATH entry for the stage folder (it holds the OFW_FORMS_PATH_EXTRA attachments)."""
    if not extra_forms_dirs():
        return ""
    folder = stage_folder(repo_root)
    for source_dir in extra_forms_dirs():
        for src in source_dir.iterdir():
            if src.is_file() and src.suffix.lower() in _FORMS_DEP_EXTS:
                _sync_file(src, folder / src.name)
    cpath = container_path(folder, repo_root)
    _ensure_runtime_forms_path([cpath])
    return cpath


_DEFAULT_ENV_CONTAINER = (
    "/oracle_home_14c/user_projects/domains/base_domain/config/"
    "fmwconfig/servers/AdminServer/applications/formsapp_14.1.2/config/default.env"
)


def _ensure_runtime_forms_path(extra_dirs: List[str]) -> None:
    """Append extra container paths to FORMS_PATH in default.env inside forms-server-14c (idempotent)."""
    check = subprocess.run(
        podman_cmd(["exec", "-u", "oracle", "forms-server-14c", "grep", "^FORMS_PATH=", _DEFAULT_ENV_CONTAINER]),
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )
    if check.returncode != 0 or "=" not in check.stdout:
        return
    current = check.stdout.splitlines()[0].split("=", 1)[1].strip()
    parts = [p for p in current.split(":") if p]
    missing = [d for d in extra_dirs if d not in parts]
    if not missing:
        return
    updated = ":".join(parts + missing)
    subprocess.run(
        podman_cmd([
            "exec", "-u", "oracle", "forms-server-14c",
            "sed", "-i", f"s|^FORMS_PATH=.*|FORMS_PATH={updated}|", _DEFAULT_ENV_CONTAINER,
        ]),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )


# Constructs the shell command executed inside the Forms 14c container for frmcmp_batch.
def build_compiler_command(container_path: str, mod_type: str, logon_param: str, extra_forms_path: str = "") -> str:
    """Build the environment exports and frmcmp_batch invocation string.

    Args:
        container_path: Path to module inside /work mount.
        mod_type: Module type (form, menu, library).
        logon_param: Database logon argument string.
        extra_forms_path: Optional colon-separated container paths from OFW_FORMS_PATH_EXTRA.

    Returns:
        Complete bash shell command string.
    """
    forms_path = f"/oracle_home_14c/forms:{container_path.rsplit('/', 1)[0]}"
    if extra_forms_path:
        forms_path = f"{forms_path}:{extra_forms_path}"
    return (
        # frmcmp_batch also re-saves a library into its working directory, so run in the module's own folder.
        f"cd {container_path.rsplit('/', 1)[0]}; "
        f"export ORACLE_HOME=/oracle_home_14c; "
        f"export LD_LIBRARY_PATH=/oracle_home_14c/lib; "
        f"export ORACLE_TERM=vt220; "
        f"export TERM=vt220; "
        f"export FORMS_PATH={forms_path}; "
        f"export TNS_ADMIN=/oracle_home_14c/user_projects/domains/base_domain/config/fmwconfig; "
        f"/oracle_home_14c/bin/frmcmp_batch "
        f"module={container_path} "
        f"module_type={mod_type} "
        f"{logon_param} "
        f"compile_all=yes"
    )


# Running inside WSL (Linux on a Windows machine): Podman is local, but the repo is on the Windows drive.
IS_WSL = platform.system() == "Linux" and "microsoft" in platform.uname().release.lower()


def podman_cmd(args: List[str]) -> List[str]:
    """Podman command line.

    Windows: Podman runs inside WSL, so the call goes through `wsl -d <distro>` (OFW_WSL_DISTRO, default Ubuntu).
    WSL: Podman is local; its runtime directory must be /run/user/<uid>.
    macOS and Linux: plain `podman`, unchanged.
    """
    on_windows = platform.system() == "Windows"
    if on_windows or IS_WSL:
        # Files on the Windows drive belong to the Windows user, which rootless Podman maps to container
        # root. The container's "oracle" user cannot write the compiled .fmx next to the module (FRM-10044),
        # so run exec as root.
        if args and args[0] == "exec":
            rest = list(args[1:])
            if "-u" in rest and rest.index("-u") + 1 < len(rest):
                rest[rest.index("-u") + 1] = "root"
            else:
                rest[:0] = ["-u", "root"]
            args = ["exec", *rest]
    if on_windows:
        distro = os.environ.get("OFW_WSL_DISTRO") or "Ubuntu"
        script = "export XDG_RUNTIME_DIR=/run/user/$(id -u); podman " + " ".join(shlex.quote(a) for a in args)
        return ["wsl", "-d", distro, "--", "bash", "-lc", script]
    if IS_WSL:
        return ["env", f"XDG_RUNTIME_DIR=/run/user/{os.getuid()}", "podman", *args]
    return ["podman", *args]


# Dispatches the compilation command inside the container and processes output.
def execute_compilation(cmd: str) -> int:
    """Execute the compiler inside the container via podman exec.

    Args:
        cmd: Shell command string to execute.

    Returns:
        0 on success, non-zero on compilation failure.
    """
    res = subprocess.run(podman_cmd(["exec", "-u", "oracle", "forms-server-14c", "/bin/sh", "-c", cmd]),
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    print(res.stdout)

    if "Created form file" in res.stdout or "Created menu file" in res.stdout or res.returncode == 0:
        print("Compilation SUCCESSFUL.")
        return 0
    else:
        print(f"Compilation FAILED with exit code {res.returncode}.", file=sys.stderr)
        return 1


# Parses CLI arguments for compile_form.
def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Parse command-line arguments for the form compiler.

    Args:
        argv: List of raw CLI arguments.

    Returns:
        Parsed arguments namespace.
    """
    parser = argparse.ArgumentParser(description="Headless Oracle Forms Batch Compiler")
    parser.add_argument("module", help="Path to input .fmb, .mmb, or .pll file")
    parser.add_argument("-t", "--type", default="form", choices=["form", "menu", "library"], help="Module type (default: form)")
    parser.add_argument("--offline", action="store_true", help="Compile offline without database logon (logon=no)")
    parser.add_argument("--no-publish", action="store_true", help="Leave the compiled file in the work folder; do not copy it to the Oracle Home forms folder")
    parser.add_argument("-u", "--userid", default=None, help="Database logon connection string")
    args = parser.parse_args(argv)
    if args.userid is None:
        args.userid = _default_userid()
    return args


# Main execution entry point for compile_form.py.
def main() -> int:
    """Validate inputs, resolve paths, and trigger headless compilation.

    Returns:
        0 on success, 1 on failure.
    """
    if "--init-work-dir" in sys.argv[1:]:
        try:
            print(ensure_work_dir(resolve_workspace_root()))
            return 0
        except RuntimeError as err:
            print(err, file=sys.stderr)
            return 1
    args = parse_cli_args(sys.argv[1:])
    input_path = Path(args.module).resolve()

    if not input_path.exists():
        print(f"Error: Input module not found: {input_path}", file=sys.stderr)
        return 1

    repo_root = resolve_workspace_root()
    try:
        staged = stage_module(input_path, repo_root)
        container_module_path = container_path(staged, repo_root)
    except RuntimeError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
    print(f"Staged in: {staged.parent}")

    logon_param = "logon=no" if args.offline else "userid=" + shlex.quote(format_userid(args.userid))
    extra_forms_path = resolve_extra_forms_path(repo_root)
    print(f"Compiling {input_path.name} ({args.type}) via container frmcmp_batch...")
    cmd = build_compiler_command(container_module_path, args.type, logon_param, extra_forms_path)
    code = execute_compilation(cmd)
    output = staged.with_suffix({"form": ".fmx", "menu": ".mmx", "library": ".plx"}[args.type])
    if code == 0 and output.is_file() and not args.no_publish:
        publish_outputs(output, repo_root)
        print(f"Published {output.name} to the Oracle Home forms folder.")
    elif code == 0 and not output.is_file():
        print(f"Warning: no {output.name} was written.", file=sys.stderr)
    return code


if __name__ == "__main__":
    sys.exit(main())
