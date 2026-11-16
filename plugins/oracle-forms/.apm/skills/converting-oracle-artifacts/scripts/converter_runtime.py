"""
converter_runtime.py: shared container settings and Podman helpers for the convert_*.py scripts.

Purpose:
    The three converters (convert_fmb.py, convert_pll.py, convert_rpt.py) run Oracle's own tools inside the
    Forms 14c container image. This module holds what they share, so each script stays small:
      - the image name and the Oracle Home path inside the container (14c by default);
      - the default Oracle Home folder on the host (oracle-reference-stack/oracle_home_14c);
      - a Podman command builder that also works from Windows PowerShell and inside WSL;
      - the readiness check behind `--check`.

Settings:
    OFW_CONVERTER_IMAGE   Container image to run (default: oracle-forms-14c:latest). Set it to
                          forms-converter:latest to use the older 12c image; the Oracle Home path inside the
                          container then becomes /oracle_home instead of /oracle_home_14c.
    OFW_WSL_DISTRO        WSL distribution that runs Podman when a script is started from Windows PowerShell
                          (default: Ubuntu).

Platforms:
    macOS and Linux: plain `podman`, unchanged.
    WSL (Linux on Windows): Podman is local; the runtime directory is /run/user/<uid>, and `podman run` uses
    --user root, because rootless Podman maps the container's "oracle" user to a UID that cannot write files
    on the Windows drive (/mnt/c).
    Windows PowerShell: the call goes through `wsl -d <distro>` and host paths are converted with `wslpath`.

Input Dependencies:
    - Podman (inside WSL on Windows) and the oracle-forms-14c image; the Oracle Home folder (OFW_ORACLE_HOME)
    - Optional git-ignored .env at the repo root (OFW_ORACLE_HOME, OFW_WORK_DIR, OFW_FORMS_PATH_EXTRA)

Key Functions:
    - repo_root() -> Path; default_oracle_home() -> Optional[Path]; work_dir() -> Path; ensure_dir(folder: Path) -> None
    - podman_cmd(args: List[str]) -> List[str]; host_path(path: Path) -> str; check_readiness() -> int
    - find_dependencies(module: Path, search_dirs: List[Path]) -> List[Path]  (name-match heuristic for binaries)
    - stage_for_conversion(input_path: Path) -> Path  (single-module staging into <work dir>/convert-stage)
    - server_running() -> bool; server_workdir(folder: Path) -> Optional[str]; exec_in_server(script: str) -> List[str]
      (run in the long-lived forms-server-14c with `podman exec -u oracle`; throwaway `run --rm` is only the fallback)

Consumed by:
    - convert_fmb.py, convert_pll.py, convert_rpt.py, test_conversions.py, build_dependency_manifest.py
      (the batch, dependency-ordered path; see references/batch-dependency-conversion.md)
"""

import os
import platform
import shlex
import shutil
import subprocess
from pathlib import Path
from typing import List, Optional

IMAGE = os.environ.get("OFW_CONVERTER_IMAGE", "oracle-forms-14c:latest")
IS_14C = "14c" in IMAGE
# Oracle Home path inside the container.
HOME = "/oracle_home_14c" if IS_14C else "/oracle_home"
IS_WINDOWS = platform.system() == "Windows"
IS_WSL = platform.system() == "Linux" and "microsoft" in platform.uname().release.lower()


def repo_root() -> Path:
    """Repository root (this file lives in plugins/oracle-forms/.apm/skills/<skill>/scripts)."""
    return Path(__file__).resolve().parents[6]


def _dotenv_value(name: str) -> str:
    """One KEY from the git-ignored .env at the repo root, or an empty string."""
    env_file = repo_root() / ".env"
    if env_file.is_file():
        for raw in env_file.read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if line and not line.startswith("#") and "=" in line:
                key, value = line.split("=", 1)
                if key.strip() == name:
                    return value.strip().strip('"').strip("'")
    return ""


def default_oracle_home() -> Optional[Path]:
    """Oracle Home folder on the host that matches the image, if it exists.

    For the 14c image, OFW_ORACLE_HOME (environment, then .env) overrides the default
    oracle-reference-stack/oracle_home_14c, so the Oracle Home can live on Ubuntu's own disk
    (see the setup skill's references/oracle-home-location.md).
    """
    if IS_14C:
        override = os.environ.get("OFW_ORACLE_HOME") or _dotenv_value("OFW_ORACLE_HOME")
        if override:
            path = Path(os.path.expanduser(override))
            return path if path.is_dir() else None
    folder = "oracle_home_14c" if IS_14C else "oracle_home"
    candidate = repo_root() / "oracle-reference-stack" / folder
    return candidate if candidate.is_dir() else None


def _wsl_distro() -> str:
    """WSL distribution that runs Podman when started from Windows PowerShell."""
    return os.environ.get("OFW_WSL_DISTRO") or "Ubuntu"


def podman_cmd(args: List[str]) -> List[str]:
    """Podman command line for the current platform (see the module docstring)."""
    args = list(args)
    if (IS_WINDOWS or IS_WSL) and args and args[0] == "run":
        # Files on the Windows drive belong to the Windows user, which rootless Podman maps to container
        # root. Run as root so the container can write the converted file next to the input.
        args[1:1] = ["--user", "root"]
    if IS_WINDOWS:
        script = "export XDG_RUNTIME_DIR=/run/user/$(id -u); podman " + " ".join(shlex.quote(a) for a in args)
        return ["wsl", "-d", _wsl_distro(), "--", "bash", "-lc", script]
    if IS_WSL:
        return ["env", f"XDG_RUNTIME_DIR=/run/user/{os.getuid()}", "podman", *args]
    return ["podman", *args]


def host_path(path: Path) -> str:
    """Host path in the form Podman needs for a -v mount."""
    if IS_WINDOWS:
        converted = subprocess.run(["wsl", "-d", _wsl_distro(), "--", "wslpath", "-u", str(path)],
                                   capture_output=True, text=True)
        if converted.returncode == 0 and converted.stdout.strip():
            return converted.stdout.strip()
    return path.as_posix()


# The one long-lived Forms server container. Start it once before Stage 1 and leave it running through Stage 3;
# Stages 1 and 2 run their tools in it with `podman exec`. A throwaway `podman run --rm` container is only the
# fallback for when it is not running.
SERVER_CONTAINER = "forms-server-14c"
EXEC_USER = "oracle"


def server_running() -> bool:
    """True when the forms-server-14c container exists and its state is `running` (checked on every call, so a
    container that stops mid-run makes the next file fall back to a throwaway container)."""
    res = subprocess.run(podman_cmd(["inspect", "-f", "{{.State.Status}}", SERVER_CONTAINER]),
                         capture_output=True, text=True)
    return res.returncode == 0 and res.stdout.strip() == "running"


def server_path(path: Path) -> Optional[str]:
    """Path of a host file or folder as forms-server-14c sees it (the Oracle Home is mounted at HOME there), or
    None when it is outside the Oracle Home folder and so not visible in that container."""
    oracle_home = default_oracle_home()
    if not oracle_home:
        return None
    try:
        relative = path.resolve().relative_to(oracle_home.resolve())
    except ValueError:
        return None
    return f"{HOME}/{relative.as_posix()}".rstrip("/")


def exec_in_server(script: str) -> List[str]:
    """Command that runs a bash script inside the running forms-server-14c container as the oracle user."""
    return podman_cmd(["exec", "-u", EXEC_USER, SERVER_CONTAINER, "bash", "-c", script])


def server_workdir(folder: Path) -> Optional[str]:
    """Container path of `folder` when the tools should run in forms-server-14c (it is running and the folder is
    under the Oracle Home mount), else None, meaning: use a throwaway container with the folder mounted at /work."""
    return server_path(folder) if server_running() else None


def check_readiness() -> int:
    """Implements `--check`: the Podman CLI, the Podman machine (macOS), and the converter image."""
    if not shutil.which("wsl" if IS_WINDOWS else "podman"):
        print("NOT_READY: 'podman' command not found in system PATH")
        return 1
    if platform.system().lower() == "darwin":
        try:
            res = subprocess.run(["podman", "machine", "list"], capture_output=True, text=True, timeout=10)
            if "running" not in res.stdout.lower():
                print("NOT_READY: Podman machine is not running. Run: podman machine start")
                return 1
        except Exception as err:
            print(f"NOT_READY: Error querying podman machine: {err}")
            return 1
    try:
        # The first Podman call after a restart can be slow, so allow more than a few seconds.
        res = subprocess.run(podman_cmd(["image", "exists", IMAGE]), capture_output=True, timeout=20)
    except Exception as err:
        print(f"NOT_READY: Error querying podman image: {err}")
        return 1
    if res.returncode != 0:
        print(f"NOT_READY: Image '{IMAGE}' not found. Build it in oracle-reference-stack "
              f"(or set OFW_CONVERTER_IMAGE to another image).")
        return 1
    print(f"READY: Podman engine and '{IMAGE}' image are operational.")
    return 0


DEPENDENCY_EXTENSIONS = (".pll", ".olb", ".mmb", ".fmb")


def dependency_search_dirs(input_dir: Path) -> List[Path]:
    """Folders searched for attached modules: the input folder, then OFW_FORMS_PATH_EXTRA (.env or environment)."""
    dirs = [input_dir]
    extra = os.environ.get("OFW_FORMS_PATH_EXTRA") or _dotenv_value("OFW_FORMS_PATH_EXTRA")
    for item in extra.split(os.pathsep if os.pathsep in extra else ":"):
        if item.strip() and Path(item.strip()).is_dir():
            dirs.append(Path(item.strip()))
    return dirs


def find_dependencies(module: Path, search_dirs: List[Path]) -> List[Path]:
    """Heuristic: a module's attachments are stored as plain names in the binary, so any identifier that
    matches the stem of a .pll/.olb/.mmb/.fmb in the search folders is treated as a dependency. Repeats for
    each dependency found. May over-include (a name that only appears in code); extra files are harmless."""
    import re
    index = {}
    for directory in search_dirs:
        for path in directory.iterdir():
            if path.suffix.lower() in DEPENDENCY_EXTENSIONS and path.is_file():
                index.setdefault((path.stem.lower(), path.suffix.lower()), path)
    found, queue = {}, [module]
    while queue:
        current = queue.pop()
        tokens = {t.decode("latin-1").lower() for t in re.findall(rb"[A-Za-z][A-Za-z0-9_$]{1,40}", current.read_bytes())}
        for (stem, ext), path in index.items():
            if stem in tokens and path.resolve() != module.resolve() and path not in found.values():
                if ext == ".fmb":
                    continue
                found[(stem, ext)] = path
                queue.append(path)
    return sorted(found.values())


def work_dir() -> Path:
    """Folder for staged conversion inputs: OFW_WORK_DIR, else <Oracle Home>/ofw-work. Never inside the repository.

    The same rule as compile_form.work_dir in the compiling skill (kept in step by hand: the skills do not import each other).
    """
    raw = os.environ.get("OFW_WORK_DIR") or _dotenv_value("OFW_WORK_DIR")
    if raw:
        return Path(os.path.expanduser(raw))
    home = os.environ.get("OFW_ORACLE_HOME") or _dotenv_value("OFW_ORACLE_HOME")
    base = Path(os.path.expanduser(home)) if home else repo_root() / "oracle-reference-stack" / "oracle_home_14c"
    return base / "ofw-work"


def ensure_dir(folder: Path) -> None:
    """Create a folder the host user can write. Inside the Oracle Home the container user owns everything, so on
    Linux and WSL the folder is created through `podman unshare` (owned by the host user, world-writable)."""
    if folder.is_dir() and os.access(folder, os.W_OK):
        return
    try:
        folder.mkdir(parents=True, exist_ok=True)
        folder.chmod(0o777)
        return
    except OSError:
        pass
    quoted = shlex.quote(str(folder))
    res = subprocess.run(podman_cmd(["unshare", "sh", "-c", f"mkdir -p {quoted} && chown 0:0 {quoted} && chmod 777 {quoted}"]),
                         capture_output=True, text=True)
    if res.returncode != 0 or not os.access(folder, os.W_OK):
        raise RuntimeError(f"Cannot create a writable work folder {folder}. Set OFW_WORK_DIR to a writable folder.\n{res.stdout}{res.stderr}")


def stage_for_conversion(input_path: Path) -> Path:
    """Returns the folder to run the conversion in. If the module has dependencies outside its own folder,
    copies the module and all dependencies into <work folder>/convert-stage/<name> (outside the repository) and
    returns it; otherwise returns the module's own folder. Never copies into samples/ or the source folders."""
    deps = find_dependencies(input_path, dependency_search_dirs(input_path.parent))
    missing = [d for d in deps if d.parent.resolve() != input_path.parent.resolve()]
    if not missing:
        print(f" Dependencies:   {len(deps)} found beside the input" if deps else " Dependencies:   none detected")
        return input_path.parent
    stage = work_dir() / "convert-stage" / input_path.stem
    ensure_dir(stage)
    for path in [input_path, *deps]:
        shutil.copy2(path, stage / path.name)
    print(f" Dependencies:   staged {len(deps)} module(s) with the input in {stage}")
    return stage
