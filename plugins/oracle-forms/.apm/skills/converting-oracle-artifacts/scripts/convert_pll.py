#!/usr/bin/env python3
"""
convert_pll.py (CLI)
=====================================

Purpose:
    Cross-platform command-line tool to convert Oracle PL/SQL Library binary files (.pll)
    into human-readable ASCII text scripts (.pld) or compile text scripts into binary libraries (.pll/.plx)
    via an on-demand Podman container running the native Oracle Forms Batch Compiler (frmcmp_batch).

Layer: Tooling / Container Bridge

Usage Examples:
    python3 oracle-reference-stack/convert_pll.py --check
    python3 oracle-reference-stack/convert_pll.py samples/mylib.pll
    python3 oracle-reference-stack/convert_pll.py samples/mylib.pll -o ./custom/mylib.pld
    python3 oracle-reference-stack/convert_pll.py samples/mylib.pld --compile
    python3 oracle-reference-stack/convert_pll.py samples/mylib.pll --oracle-home /custom/oracle_home

Supported Object Types:
    - .pll (Oracle PL/SQL Binary Library) -> *.pld plain text script export
    - .pld (Oracle PL/SQL Text Script)    -> *.pll compiled binary library import

CLI Arguments:
    pll_path              : Path to target Oracle PL/SQL library file (.pll or .pld).
    -o, --output <path>   : Destination path for converted output file (optional).
    --compile             : Compiles .pld text script back into binary .pll, or compiles .pll to .plx.
    --check               : Probes host container engine readiness and exits.
    --oracle-home <path>  : Path to local Oracle Home directory (optional).

Input Files:
    - oracle-reference-stack/Containerfile (Container image definition)
    - oracle-reference-stack/oracle_home/bin/frmcmp_batch (Oracle Forms batch compiler)
    - Target library files (.pll or .pld)

Output:
    - <target_dir>/<module_name>.pld (Standard text export of all package specs, bodies, and procedures)
    - OR custom destination path if specified via -o / --output

Key Functions:
    - check_podman_readiness() -> int
    - resolve_oracle_home(custom_oracle_home: Optional[str]) -> Optional[Path]
    - build_container_script(file_name: str, stem: str, is_compile: bool) -> str
    - copy_to_custom_output(source_file: Path, output_path_str: Optional[str]) -> None
    - convert_library(input_path_str: str, output_path_str: Optional[str], is_compile: bool, custom_oracle_home: Optional[str]) -> int
    - parse_cli_args(argv: List[str]) -> argparse.Namespace
    - main() -> int

Script Dependencies:
    - Host Runtime:
        * Python 3.8+ (sys, shutil, platform, subprocess, pathlib, argparse, typing)
        * Podman CLI (or Docker) installed and active on host system
        * Rosetta 2 binary translation active on macOS Apple Silicon
    - Container Image ('forms-converter:latest'):
        * Base OS: Oracle Linux 8 Slim (linux/amd64)
        * Required environment variables: ORACLE_HOME, ORACLE_TERM=vt220, LD_LIBRARY_PATH

Consumed by:
    - Developers extracting PL/SQL packages and functions from legacy .pll libraries for git versioning
    - Rust native viewer ingestion pipeline for .pld AST parsing
"""

import os
import re
import sys
import shutil
import argparse
import platform
import subprocess
from pathlib import Path
from typing import Optional, List

import converter_runtime as rt  # shared 14c image, Oracle Home path, and Podman helper


# -----------------------------------------------------------------------------
# System Readiness Probe
# -----------------------------------------------------------------------------
# Validates container engine status and virtualization availability on host
def check_podman_readiness() -> int:
    """Verifies that the Podman CLI is installed, the VM is running, and the image exists.

    Returns:
        int: 0 if the container environment is ready to execute conversions, 1 otherwise.
    """
    return rt.check_readiness()


# -----------------------------------------------------------------------------
# Oracle Home Directory Resolver
# -----------------------------------------------------------------------------
# Resolves host directory containing Oracle Forms 12c installation files
def resolve_oracle_home(custom_oracle_home: Optional[str]) -> Optional[Path]:
    """Locates the Oracle Home directory containing Oracle Forms runtime binaries.

    Args:
        custom_oracle_home: Optional user-specified path to Oracle Home.

    Returns:
        Optional[Path]: Resolved Path if the directory exists, None otherwise.
    """
    if custom_oracle_home:
        custom_path = Path(custom_oracle_home).resolve()
        if custom_path.is_dir():
            return custom_path
        print(f"Warning: Specified --oracle-home '{custom_oracle_home}' is not a valid directory.")

    return rt.default_oracle_home()


# -----------------------------------------------------------------------------
# Container Bash Command Builder
# -----------------------------------------------------------------------------
# Generates the shell execution script to run frmcmp_batch inside container
def build_container_script(file_name: str, stem: str, is_compile: bool, userid: Optional[str] = None,
                           no_batch: bool = False) -> str:
    """Generates the headless bash script string to be executed inside the container.

    Args:
        file_name: Base file name including extension.
        stem: File name without extension.
        is_compile: True if compiling script into library, False if exporting to text.

    Returns:
        str: Bash command sequence setting up environment and running frmcmp_batch.
    """
    home = rt.HOME
    env = (
        f"export ORACLE_HOME={home}; "
        f"export ORACLE_TERM=vt220; "
        f"export TERM=vt220; "
        f"export LD_LIBRARY_PATH={home}/lib:{home}/oracle_common/lib:$LD_LIBRARY_PATH; "
        f"export FORMS_PATH=/work:/work/shared:$FORMS_PATH; "
    )
    # Forms 14c needs batch=yes for the PL/SQL library step (without it: PDE-PER001).
    batch = " batch=yes" if rt.IS_14C and not no_batch else ""
    if is_compile:
        # Parses .pld text script into binary .pll, or compiles .pll to executable .plx
        if file_name.lower().endswith(".pld"):
            return env + (
                f"cd /work; "
                f"echo 'Executing Oracle frmcmp_batch to parse .pld into library {stem}.pll...'; "
                f"{home}/bin/frmcmp_batch module={file_name} output_file={stem}.pll module_type=library parse=yes logon=no{batch}; "
            )
        return env + (
            f"cd /work; "
            f"echo 'Executing Oracle frmcmp_batch to compile library {file_name}...'; "
            f"{home}/bin/frmcmp_batch module={file_name} module_type=library script=no compile_all=yes logon=no{batch}; "
        )

    # Exports binary .pll into plain-text .pld script
    if userid:
        # Variant from the reference procedure: database logon, parse=yes, explicit output_file
        return env + (
            f"cd /work; "
            f"echo 'Executing Oracle frmcmp_batch (userid, parse=yes) to export PL/SQL library to .pld...'; "
            f"{home}/bin/frmcmp_batch module={file_name} userid=\"$OFW_PLL_USERID\" module_type=library "
            f"parse=yes script=yes output_file=/work/{stem}.pld{batch}; "
        )
    return env + (
        f"cd /work; "
        f"echo 'Executing Oracle native frmcmp_batch utility to export PL/SQL library to .pld...'; "
        f"{home}/bin/frmcmp_batch module={file_name} module_type=library script=yes logon=no{batch}; "
    )

# -----------------------------------------------------------------------------
# Custom Output File Copier
# -----------------------------------------------------------------------------
# Copies generated .pld or .pll to user-specified destination path
def copy_to_custom_output(source_file: Path, output_path_str: Optional[str]) -> None:
    """Copies generated output file to a user-specified destination path if provided.

    Args:
        source_file: Path to the generated sidecar file.
        output_path_str: Optional user-specified output path or directory.
    """
    if not output_path_str or not source_file.is_file():
        return

    dest = Path(output_path_str).resolve()
    if dest.is_dir() or output_path_str.endswith(("/", "\\")):
        dest.mkdir(parents=True, exist_ok=True)
        dest = dest / source_file.name

    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source_file, dest)
    print(f"SUCCESS: Copied output file to: {dest}")


# -----------------------------------------------------------------------------
# Library Conversion Execution
# -----------------------------------------------------------------------------
# Mounts host directory and dispatches frmcmp_batch inside container
def convert_library(
    input_path_str: str,
    output_path_str: Optional[str] = None,
    is_compile: bool = False,
    custom_oracle_home: Optional[str] = None,
    stage: bool = True,
    userid: Optional[str] = None,
    no_batch: bool = False,
) -> int:
    """Mounts host directory into container and executes library export or compilation.

    Args:
        input_path_str: String path to the target Oracle PL/SQL library file (.pll or .pld).
        output_path_str: Optional destination path for converted file.
        is_compile: True to compile into binary library, False to export to text script.
        custom_oracle_home: Optional path to external Oracle Home runtime folder.

    Returns:
        int: Process exit code (0 for success, 1 for fatal error, 2 for missing runtime).
    """
    input_path = Path(input_path_str).resolve()
    if not input_path.is_file():
        print(f"Error: Input file '{input_path}' does not exist.")
        return 1

    source_dir = input_path.parent
    target_dir = rt.stage_for_conversion(input_path) if stage else source_dir
    file_name = input_path.name
    file_ext = input_path.suffix.lower()
    oracle_home_path = resolve_oracle_home(custom_oracle_home)

    extra_mount: List[str] = []
    if oracle_home_path:
        extra_mount = ["-v", f"{rt.host_path(oracle_home_path)}:{rt.HOME}:Z"]

    print("=================================================================")
    print(" Oracle PL/SQL Library Containerized Converter (frmcmp_batch)")
    print("=================================================================")
    print(f" OS:             {platform.system()} ({platform.machine()})")
    print(f" File:           {file_name}")
    print(f" Host Directory: {target_dir}")
    print(f" Mode:           {'Compile (.pld -> .pll)' if is_compile else 'Export (.pll -> .pld)'}")
    if output_path_str:
        print(f" Target Output:  {output_path_str}")
    if extra_mount:
        print(f" Oracle Home:    {oracle_home_path} (mounted to {rt.HOME})")
    else:
        print(f" Oracle Home:    Default internal container path ({rt.HOME})")
    print("=================================================================")

    if is_compile:
        expected_output = target_dir / f"{input_path.stem}.pll"
    else:
        expected_output = target_dir / f"{input_path.stem}.pld"

    container_cmd = build_container_script(file_name, input_path.stem, is_compile, userid, no_batch)
    env_args = ["-e", "OFW_PLL_USERID"] if userid else []
    cmd = rt.podman_cmd([
        "run", "--rm", "--platform", "linux/amd64", *env_args,
        "-v", f"{rt.host_path(target_dir)}:/work:Z",
        *extra_mount,
        rt.IMAGE, "bash", "-c", container_cmd,
    ])

    try:
        run_env = {**os.environ, "OFW_PLL_USERID": userid} if userid else None
        res = subprocess.run(cmd, env=run_env)
        print(f"frmcmp_batch container exit code: {res.returncode}")
        if expected_output.is_file():
            print(f"SUCCESS: Output file available at {expected_output}")
            copy_to_custom_output(expected_output, output_path_str)
        else:
            print(f"FAILED: frmcmp_batch wrote no {expected_output.name}. It prints no error when it fails; "
                  "see the change log (convert_pll.py exports no .pld).")
            return res.returncode or 1
        return res.returncode
    except Exception as err:
        print(f"Execution failed: {err}")
        return 1


# -----------------------------------------------------------------------------
# CLI Argument Parser
# -----------------------------------------------------------------------------
# Sets up and parses command-line flags and parameters
def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Configures and parses command-line arguments using standard argparse.

    Args:
        argv: Command line argument list.

    Returns:
        argparse.Namespace: Parsed options and flags.
    """
    parser = argparse.ArgumentParser(
        description="Cross-platform Oracle PL/SQL Library (.pll/.pld) containerized converter.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "pll_path",
        nargs="?",
        help="Path to the target Oracle PL/SQL library file (.pll or .pld).",
    )
    parser.add_argument(
        "-o",
        "--output",
        dest="output_path",
        help="Destination path or directory for generated output file (.pld or .pll).",
    )
    parser.add_argument(
        "--compile",
        action="store_true",
        help="Compiles text script .pld into binary library .pll, or compiles .pll to .plx.",
    )
    parser.add_argument(
        "--oracle-home",
        dest="oracle_home",
        help="Path to host directory containing Oracle Home runtime.",
    )
    parser.add_argument(
        "--userid-from-env",
        action="store_true",
        help="Export with a DB logon (userid, parse=yes, output_file) taken from OFW_PREVIEW_DB_LOGON "
             "(environment or .env); the value is never printed.",
    )
    parser.add_argument(
        "--no-batch",
        action="store_true",
        help="Omit batch=yes so frmcmp_batch may print its own messages (diagnostic).",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Probes host container engine readiness and exits.",
    )
    return parser.parse_args(argv)


# -----------------------------------------------------------------------------
# Main Entry Point
# -----------------------------------------------------------------------------
# Dispatches execution based on parsed command line options
def read_logon_from_env() -> Optional[str]:
    """Returns OFW_PREVIEW_DB_LOGON from the environment or the repo .env (never printed)."""
    if os.environ.get("OFW_PREVIEW_DB_LOGON"):
        return os.environ["OFW_PREVIEW_DB_LOGON"]
    env_file = Path(__file__).resolve().parents[6] / ".env"
    if env_file.is_file():
        m = re.search(r"^OFW_PREVIEW_DB_LOGON=(.*)$", env_file.read_text(), re.M)
        if m:
            return m.group(1).strip().strip("\"'")
    return None


def main() -> int:
    """Parses command-line arguments and dispatches probe or library conversion actions.

    Returns:
        int: Standard system exit code.
    """
    args = parse_cli_args(sys.argv[1:])

    if args.check:
        return check_podman_readiness()

    if not args.pll_path:
        print("Usage: convert_pll.py <file.pll|.pld> [-o <output>] [--compile] [--oracle-home <path>]")
        print("       convert_pll.py --check")
        return 1

    userid = None
    if args.userid_from_env:
        userid = read_logon_from_env()
        if not userid:
            print("Error: OFW_PREVIEW_DB_LOGON is not set in the environment or .env.")
            return 1

    return convert_library(
        userid=userid,
        no_batch=args.no_batch,
        input_path_str=args.pll_path,
        output_path_str=args.output_path,
        is_compile=args.compile,
        custom_oracle_home=args.oracle_home,
    )


if __name__ == "__main__":
    sys.exit(main())
