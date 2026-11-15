#!/usr/bin/env python3
"""
build_dependency_manifest.py (CLI)
=====================================

Purpose:
    Project-agnostic batch conversion planner for Oracle Forms sources. Stage 1 reads a source folder
    (read-only), extracts each module's dependencies, sorts them so every provider precedes its consumers
    (Kahn's algorithm) and writes <build-dir>/build-manifest.json. Stage 2 copies the sources into
    <build-dir>/stage/ and converts XML to binary (XML2Forms in the Forms 14c converter image) in that order.

Layer: Tooling / Skill Script (converting-oracle-artifacts)

Usage Examples:
    python3 build_dependency_manifest.py --source-dir <xml-folder> --build-dir <external-folder> --analyze-only
    python3 build_dependency_manifest.py --source-dir <xml-folder> --build-dir <external-folder>
    python3 build_dependency_manifest.py            # prompts for the source folder; build dir defaults to <OFW_WORK_DIR>/build

Input Dependencies:
    - Source folder with *_fmb.xml, *_mmb.xml, *_olb.xml, *.pll, *.pld (and optionally binary .fmb/.mmb/.olb)
    - Stage 2 only: Podman and the forms-converter-14c image (converter_runtime.py)
    - --build-dir must resolve outside the repository (.agent/rules/cross-project-isolation.md)

Output:
    <build-dir>/build-manifest.json (schema: assets/build-manifest.schema.json); <build-dir>/stage/ (Stage 2)

Key Functions:
    - require_external(path: Path) -> Path
    - scan_source(source_dir: Path) -> List[Dict]
    - resolve_and_order(nodes: List[Dict]) -> Tuple[List[Dict], List[Dict], List[List[str]]]
    - build_manifest(source_dir: Path, build_dir: Path) -> Dict
    - convert_manifest(manifest: Dict, normalize: bool) -> int
    - main() -> int
"""

import argparse
import heapq
import json
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import converter_runtime as rt

MANIFEST_NAME = "build-manifest.json"
TYPE_ORDER = {"OLB": 0, "PLL": 1, "MMB": 2, "FMB": 3}
EXECUTABLE_EXT = {"PLL": ".plx", "MMB": ".mmx", "FMB": ".fmx", "OLB": None}
ROOT_TAGS = {"FormModule": "FMB", "MenuModule": "MMB", "ObjectLibrary": "OLB"}
BINARY_EXT = {".fmb": "FMB", ".mmb": "MMB", ".olb": "OLB", ".pll": "PLL"}
STAGE_EXT = {".xml", ".pll", ".pld", ".fmb", ".mmb", ".olb"}
DEP_KEYS = ("objectLibraries", "attachedLibraries", "menus", "parentModules")
DEP_TYPE = {"objectLibraries": "OLB", "attachedLibraries": "PLL", "menus": "MMB", "parentModules": "FMB"}
PLD_ATTACH = re.compile(r"^\s*\.?attach\s+library\s+['\"]?([A-Za-z0-9_$#]+)", re.IGNORECASE | re.MULTILINE)


def require_external(path: Path) -> Path:
    """Resolve a folder and refuse it when it is inside the repository (cross-project isolation)."""
    resolved = path.expanduser().resolve()
    try:
        resolved.relative_to(rt.repo_root().resolve())
    except ValueError:
        return resolved
    raise RuntimeError(f"{resolved} is inside the repository. Use a folder outside it (for example under OFW_WORK_DIR).")


def _local(tag: object) -> str:
    """Tag name without its XML namespace."""
    return tag.rsplit("}", 1)[-1] if isinstance(tag, str) else ""


def _module_stem(name: str) -> str:
    """File name without extension and without the Forms2XML _fmb/_mmb/_olb suffix."""
    stem = Path(name).stem
    return stem[:-4] if re.search(r"_(fmb|mmb|olb)$", stem, re.IGNORECASE) else stem


def _empty_deps() -> Dict[str, List[str]]:
    return {key: [] for key in DEP_KEYS}


def _add_dep(deps: Dict[str, List[str]], key: str, value: str) -> None:
    """Record a dependency by bare name (path and extension removed), once."""
    name = Path(value.replace("\\", "/")).stem.strip()
    if name and name not in deps[key]:
        deps[key].append(name)


def _parent_key(filename: str) -> str:
    """ParentFilename pointing at an object library is an OLB dependency; anything else is a parent module."""
    return "objectLibraries" if filename.lower().endswith(".olb") else "parentModules"


def scan_xml(path: Path) -> Optional[Dict]:
    """Read one Forms XML export and return its module type, name and dependencies (None if not Forms XML)."""
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError:
        return None
    kind = ROOT_TAGS.get(_local(root.tag))
    if not kind:
        return None
    deps = _empty_deps()
    for element in root.iter():
        tag = _local(element.tag)
        if tag == "AttachedLibrary" and element.get("Name"):
            _add_dep(deps, "attachedLibraries", element.get("Name", ""))
        if tag == "FormModule" and element.get("MenuModule"):
            _add_dep(deps, "menus", element.get("MenuModule", ""))
        parent = element.get("ParentFilename")
        if parent:
            _add_dep(deps, _parent_key(parent), parent)
        elif element.get("ParentModule"):
            _add_dep(deps, "parentModules", element.get("ParentModule", ""))
    own = _module_stem(path.name).casefold()
    for key in DEP_KEYS:
        deps[key] = [n for n in deps[key] if n.casefold() != own]
    return {"type": kind, "name": _module_stem(path.name), "source": path, "deps": deps, "convert": True}


def scan_pld(path: Path) -> List[str]:
    """Library names a .pld text export attaches (`.attach library NAME`)."""
    return PLD_ATTACH.findall(path.read_text(encoding="utf-8", errors="ignore"))


def _binary_node(path: Path, source_dir: Path) -> Dict:
    """Node for a binary module; its attachments come from the name-matching heuristic (may over-include)."""
    deps = _empty_deps()
    for found in rt.find_dependencies(path, [source_dir]):
        key = {".olb": "objectLibraries", ".pll": "attachedLibraries", ".mmb": "menus"}.get(found.suffix.lower())
        if key and found.stem.casefold() != path.stem.casefold():
            _add_dep(deps, key, found.name)
    return {"type": BINARY_EXT[path.suffix.lower()], "name": path.stem, "source": path, "deps": deps, "convert": False}


def scan_source(source_dir: Path) -> List[Dict]:
    """Nodes for every module in the source folder. XML wins over a binary of the same name; .pld adds
    attachments to its .pll; a .pld with no .pll is reported by resolve_and_order as unresolved."""
    nodes: Dict[Tuple[str, str], Dict] = {}
    files = sorted((p for p in source_dir.iterdir() if p.is_file()), key=lambda p: (p.name.casefold(), p.name))
    for path in files:
        if path.suffix.lower() == ".xml":
            node = scan_xml(path)
            if node:
                nodes[(node["type"], node["name"].casefold())] = node
    for path in files:
        kind = BINARY_EXT.get(path.suffix.lower())
        if kind and (kind, path.stem.casefold()) not in nodes:
            nodes[(kind, path.stem.casefold())] = _binary_node(path, source_dir)
    for path in files:
        if path.suffix.lower() == ".pld":
            node = nodes.get(("PLL", path.stem.casefold()))
            for lib in scan_pld(path):
                if node:
                    _add_dep(node["deps"], "attachedLibraries", lib)
                else:
                    nodes.setdefault(("PLD", path.stem.casefold()), {"type": "PLD", "name": path.stem, "source": path, "deps": {**_empty_deps(), "attachedLibraries": scan_pld(path)}, "convert": False})
    return sorted(nodes.values(), key=lambda n: (n["name"].casefold(), TYPE_ORDER.get(n["type"], 9)))


def _unresolved_pld(nodes: List[Dict]) -> List[Dict]:
    """A .pld with no .pll cannot become a library headlessly in 14c: report it instead of building it."""
    return [{"module": n["name"], "dependency": n["name"], "kind": "PLL",
             "reason": "only a .pld source exists; PLD to PLL has no command-line route on 14c, provide the .pll"}
            for n in nodes if n["type"] == "PLD"]


def _find_provider(index: Dict[Tuple[str, str], Dict], key: str, name: str) -> Optional[Dict]:
    return index.get((DEP_TYPE[key], name.casefold()))


def resolve_and_order(nodes: List[Dict]) -> Tuple[List[Dict], List[Dict], List[List[str]]]:
    """Return (ordered nodes, unresolved dependencies, cycles). Providers precede consumers; ties break by
    case-insensitive name then type so the order is deterministic."""
    real = [n for n in nodes if n["type"] != "PLD"]
    index = {(n["type"], n["name"].casefold()): n for n in real}
    unresolved = _unresolved_pld(nodes)
    edges: Dict[int, set] = {id(n): set() for n in real}
    indegree = {id(n): 0 for n in real}
    by_id = {id(n): n for n in real}
    for node in real:
        for key in DEP_KEYS:
            for name in node["deps"][key]:
                provider = _find_provider(index, key, name)
                if provider is None:
                    unresolved.append({"module": node["name"], "dependency": name, "kind": DEP_TYPE[key]})
                elif id(provider) != id(node) and id(node) not in edges[id(provider)]:
                    edges[id(provider)].add(id(node))
                    indegree[id(node)] += 1
    sort_key = lambda n: (n["name"].casefold(), TYPE_ORDER[n["type"]], n["name"])
    ready = [(sort_key(by_id[i]), i) for i, d in indegree.items() if d == 0]
    heapq.heapify(ready)
    ordered: List[Dict] = []
    while ready:
        _, current = heapq.heappop(ready)
        ordered.append(by_id[current])
        for consumer in edges[current]:
            indegree[consumer] -= 1
            if indegree[consumer] == 0:
                heapq.heappush(ready, (sort_key(by_id[consumer]), consumer))
    leftover = sorted((by_id[i] for i, d in indegree.items() if d > 0), key=sort_key)
    cycles = [[f"{n['name']} ({n['type']})" for n in leftover]] if leftover else []
    return ordered, unresolved, cycles


def _module_entry(order: int, node: Dict) -> Dict:
    """One manifest module record with build-dir-relative paths."""
    kind, name, source = node["type"], node["name"], node["source"]
    is_xml = source.suffix.lower() == ".xml"
    binary_ext = ".pll" if kind == "PLL" else f".{kind.lower()}"
    exe_ext = EXECUTABLE_EXT[kind]
    return {
        "order": order, "name": name, "artifactType": kind, "sourceFile": str(source),
        "stagedInputFile": f"stage/{source.name}", "binaryFile": f"stage/{name}{binary_ext}",
        "executableFile": f"stage/{name}{exe_ext}" if exe_ext else None,
        "requiresConversion": is_xml, "dependencies": node["deps"], "status": "pending",
    }


def build_manifest(source_dir: Path, build_dir: Path) -> Dict:
    """Stage 1: scan, resolve and order; return the manifest dictionary."""
    ordered, unresolved, cycles = resolve_and_order(scan_source(source_dir))
    return {
        "schemaVersion": 1, "generatedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "sourceDir": str(source_dir), "buildDir": str(build_dir),
        "modules": [_module_entry(i, n) for i, n in enumerate(ordered, 1)],
        "unresolvedDependencies": unresolved, "cycles": cycles,
    }


def write_manifest(manifest: Dict) -> Path:
    """Write the manifest into its build folder and return its path."""
    target = Path(manifest["buildDir"]) / MANIFEST_NAME
    target.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return target


def stage_sources(source_dir: Path, build_dir: Path) -> Path:
    """Copy source files into <build-dir>/stage/ (the source folder is never modified)."""
    stage = build_dir / "stage"
    rt.ensure_dir(stage)
    for path in sorted(source_dir.iterdir()):
        if path.is_file() and path.suffix.lower() in STAGE_EXT:
            shutil.copy2(path, stage / path.name)
    return stage


def _normalize_xml(xml_path: Path) -> Path:
    """Optional Oracle-compatibility copy of a hand-written XML (uses run_form.prepare_oracle_preview_xml)."""
    scripts = Path(__file__).resolve().parents[2] / "compiling-and-running-oracle-forms" / "scripts"
    sys.path.insert(0, str(scripts))
    try:
        import run_form
    finally:
        sys.path.remove(str(scripts))
    return run_form.prepare_oracle_preview_xml(xml_path)


def xml2forms_command(stage: Path, xml_path: Path) -> List[str]:
    """Throwaway-container XML2Forms command for one staged XML; the stage folder is mounted at /work."""
    home = rt.HOME
    rel = xml_path.relative_to(stage).as_posix()
    outdir = "/work" + ("/" + Path(rel).parent.as_posix() if Path(rel).parent.as_posix() != "." else "")
    classpath = f"{home}/jlib/frmjdapi.jar:{home}/jlib/frmxmltools.jar:{home}/oracle_common/modules/oracle.xdk/xmlparserv2.jar"
    script = (f"export ORACLE_HOME={home}; export FORMS_API_TK_BYPASS=TRUE; export LD_LIBRARY_PATH={home}/lib; "
              f"export FORMS_PATH=/work:{home}/forms; "
              f"java -Djava.library.path={home}/lib -classpath {classpath} oracle.forms.util.xmltools.XML2Forms "
              f"OVERWRITE=YES OUTPUTDIR={outdir} /work/{rel}")
    mounts = ["-v", f"{rt.host_path(stage)}:/work:Z"]
    oracle_home = rt.default_oracle_home()
    if oracle_home:
        mounts += ["-v", f"{rt.host_path(oracle_home)}:{home}:Z"]
    return rt.podman_cmd(["run", "--rm", "--platform", "linux/amd64", *mounts, rt.IMAGE, "bash", "-c", script])


def convert_one(module: Dict, build_dir: Path, normalize: bool) -> Tuple[bool, str]:
    """Convert one XML module to its binary; return (ok, diagnostics)."""
    xml_path = stage / Path(module["stagedInputFile"]).name
    if normalize:
        xml_path = _normalize_xml(xml_path)
    res = subprocess.run(xml2forms_command(stage, xml_path), capture_output=True, text=True)
    stage = build_dir / "stage"
    binary = build_dir / module["binaryFile"]
    actual = next((p for p in binary.parent.iterdir() if p.name.casefold() == binary.name.casefold()), None) if binary.parent.is_dir() else None
    if actual is not None:
        if actual.name != binary.name:
            module["binaryFile"] = f"{Path(module['binaryFile']).parent.as_posix()}/{actual.name}"
            if module.get("executableFile"):
                module["executableFile"] = str(Path(module["binaryFile"]).with_suffix(Path(module["executableFile"]).suffix).as_posix())
        return True, ""
    return False, f"XML2Forms did not create {binary.name} (exit {res.returncode}):\n{res.stdout}{res.stderr}".strip()


def convert_manifest(manifest: Dict, normalize: bool = False) -> int:
    """Stage 2: convert every module that needs it, in manifest order, updating status as it goes."""
    build_dir = Path(manifest["buildDir"])
    failed = False
    for module in manifest["modules"]:
        if not module["requiresConversion"]:
            continue  # already binary: staged as is, stays pending until compiled
        print(f"[{module['order']}] XML2Forms {module['name']} ({module['artifactType']})")
        ok, note = convert_one(module, build_dir, normalize)
        module["status"] = "converted" if ok else "failed"
        module["diagnostics"] = note
        failed = failed or not ok
        write_manifest(manifest)
        if not ok:
            print(note, file=sys.stderr)
    write_manifest(manifest)
    return 1 if failed else 0


def ask_source_dir() -> Path:
    """Prompt for the source folder when --source-dir is absent (interactive sessions only)."""
    if not sys.stdin.isatty():
        raise RuntimeError("--source-dir is required when input is not interactive.")
    return Path(input("Folder with the Forms XML / PLL sources (read-only, copied to the build folder): ").strip())


def parse_cli_args(argv: List[str]) -> argparse.Namespace:
    """Command-line options."""
    parser = argparse.ArgumentParser(description="Dependency-ordered conversion manifest for Oracle Forms sources")
    parser.add_argument("--source-dir", help="Read-only folder with the exported sources (prompted for when omitted)")
    parser.add_argument("--build-dir", help="External build folder (default: <OFW_WORK_DIR>/build)")
    parser.add_argument("--analyze-only", action="store_true", help="Write the manifest only; do not copy or convert")
    parser.add_argument("--allow-missing", action="store_true", help="Continue when a required provider is missing")
    parser.add_argument("--normalize", action="store_true", help="Apply the Oracle-compatibility XML normalization before XML2Forms")
    return parser.parse_args(argv)


def main() -> int:
    """Run Stage 1 and, unless --analyze-only, Stage 2."""
    args = parse_cli_args(sys.argv[1:])
    try:
        source_dir = Path(args.source_dir) if args.source_dir else ask_source_dir()
        if not source_dir.is_dir():
            raise RuntimeError(f"Source folder not found: {source_dir}")
        build_dir = require_external(Path(args.build_dir) if args.build_dir else rt.work_dir() / "build")
        rt.ensure_dir(build_dir)
        manifest = build_manifest(source_dir.resolve(), build_dir)
    except RuntimeError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
    path = write_manifest(manifest)
    print(f"Manifest: {path} ({len(manifest['modules'])} modules)")
    for item in manifest["unresolvedDependencies"]:
        print(f"  UNRESOLVED: {item['module']} needs {item['dependency']} ({item['kind']}) {item.get('reason', '')}".rstrip(), file=sys.stderr)
    for cycle in manifest["cycles"]:
        print(f"  CYCLE: {', '.join(cycle)}", file=sys.stderr)
    if manifest["cycles"] or (manifest["unresolvedDependencies"] and not args.allow_missing):
        print("Failing closed: resolve the problems above (or pass --allow-missing for missing providers).", file=sys.stderr)
        return 2
    if args.analyze_only:
        return 0
    try:
        stage_sources(source_dir.resolve(), build_dir)
        return convert_manifest(manifest, args.normalize)
    except RuntimeError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
