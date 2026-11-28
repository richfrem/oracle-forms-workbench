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
    python3 build_dependency_manifest.py --source-dir <xml-folder> --build-dir <external-folder> --decompile-binaries
    python3 build_dependency_manifest.py --source-dir <xml-folder> --build-dir <external-folder>
    python3 build_dependency_manifest.py            # prompts for the source folder; build dir defaults to <OFW_WORK_DIR>/build

Input Dependencies:
    - Source folder with *_fmb.xml, *_mmb.xml, *_olb.xml, *.pll, *.pld (and optionally binary .fmb/.mmb/.olb)
    - Stage 2 only: Podman and the oracle-forms-14c image (converter_runtime.py)
    - --build-dir must resolve outside the repository (.agent/rules/cross-project-isolation.md)

Output:
    <build-dir>/build-manifest.json (shape: assets/build-manifest.template.json); <build-dir>/stage/ (Stage 2)

Key Functions:
    - require_external(path: Path) -> Path
    - scan_source(source_dir: Path) -> List[Dict]
    - resolve_and_order(nodes: List[Dict]) -> Tuple[List[Dict], List[Dict], List[List[str]]]
    - build_manifest(source_dir: Path, build_dir: Path) -> Dict
    - convert_manifest(manifest: Dict, normalize: bool) -> int
    - decompile_binaries(manifest: Dict, build_dir: Path) -> int
    - main() -> int
"""

import argparse
import difflib
import heapq
import json
import os
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
BUILTIN_MENUS = {"DEFAULT", "DEFAULT&SMARTBAR"}  # Forms built-in menus, not files
PLD_ATTACH = re.compile(r"^\s*\.?attach\s+library\s+['\"]?([A-Za-z0-9_$#]+)", re.IGNORECASE | re.MULTILINE)
# Top-level program units of a .pld text export start in column 1 (members inside a package are indented).
PLD_DEFINES = re.compile(r"^(?:PACKAGE(?:\s+BODY)?|PROCEDURE|FUNCTION)\s+([A-Za-z][A-Za-z0-9_$#]*)", re.IGNORECASE | re.MULTILINE)
# Attributes that carry PL/SQL in a Forms XML export (TriggerText, ProgramUnitText, StartupCode, MenuItemCode, ...).
PLSQL_ATTR = re.compile(r"(Text|Code)$")
PLSQL_NOISE = re.compile(r"'(?:[^']|'')*'|--[^\n]*|/\*.*?\*/", re.DOTALL)
# An identifier not preceded by a dot (package members such as Foo.Bar count once, as Foo).
PLSQL_IDENT = re.compile(r"(?<![A-Za-z0-9_$#.])([A-Za-z][A-Za-z0-9_$#]*)")


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
        # Forms2XML wraps the module: <Module><FormModule|MenuModule|ObjectLibrary .../></Module>
        kind = next((ROOT_TAGS[_local(child.tag)] for child in root if _local(child.tag) in ROOT_TAGS), None)
    if not kind:
        return None
    deps = _empty_deps()
    for element in root.iter():
        tag = _local(element.tag)
        if tag == "AttachedLibrary" and element.get("Name"):
            _add_dep(deps, "attachedLibraries", element.get("Name", ""))
        if tag == "FormModule" and element.get("MenuModule") and element.get("MenuModule", "").upper() not in BUILTIN_MENUS:
            _add_dep(deps, "menus", element.get("MenuModule", ""))
        parent = element.get("ParentFilename")
        if parent:
            _add_dep(deps, _parent_key(parent), parent)
        elif element.get("ParentModule"):
            _add_dep(deps, "parentModules", element.get("ParentModule", ""))
    inner = next((e.get("Name", "") for e in root.iter() if _local(e.tag) in ROOT_TAGS), "")
    own = {_module_stem(path.name).casefold(), inner.casefold()}
    for key in DEP_KEYS:
        deps[key] = [n for n in deps[key] if n.casefold() not in own]
    return {"type": kind, "name": _module_stem(path.name), "moduleName": inner, "source": path, "deps": deps, "convert": True,
            "identifiers": _plsql_identifiers(root), "ownUnits": _own_units(root)}


def _plsql_identifiers(root: ET.Element) -> set:
    """Casefolded identifiers used in the module's PL/SQL (strings and comments removed). Some 12c exports
    double-encode line breaks, so a literal `&#10;` is read as a line break first."""
    found: set = set()
    for element in root.iter():
        for attr, value in element.attrib.items():
            if value and PLSQL_ATTR.search(attr):
                code = PLSQL_NOISE.sub(" ", value.replace("&#10;", "\n").replace("&#13;", ""))
                found.update(m.casefold() for m in PLSQL_IDENT.findall(code))
    return found


def _own_units(root: ET.Element) -> set:
    """Program units the module defines itself; they shadow library units of the same name."""
    return {e.get("Name", "").casefold() for e in root.iter() if _local(e.tag) == "ProgramUnit" and e.get("Name")}


def scan_pld_defines(path: Path) -> List[str]:
    """Top-level packages, procedures and functions a .pld text export defines."""
    return sorted({m for m in PLD_DEFINES.findall(path.read_text(encoding="utf-8", errors="ignore"))}, key=str.casefold)


def infer_library_attachments(nodes: List[Dict]) -> List[Dict]:
    """Add the libraries an XML module calls but does not attach (12c exports can omit <AttachedLibrary>).

    A library counts as used when the module's PL/SQL names one of the library's top-level units and the
    module does not define a unit of that name itself. Each inferred library is added to the module's
    attachedLibraries (so ordering, closure and symlinks include it) and listed in node["inferred"] with the
    names that matched. Names defined by more than one library are not inferred; they are returned as notes."""
    owners: Dict[str, List[str]] = {}
    for node in nodes:
        for unit in {u.casefold() for u in node.get("defines", [])}:  # a package and its body count once
            owners.setdefault(unit, []).append(node["name"])
    notes: List[Dict] = []
    for node in nodes:
        used = node.get("identifiers")
        if not used or node["type"] not in ("FMB", "MMB"):  # object libraries are not compiled; their consumers attach
            continue
        attached = {n.casefold() for n in node["deps"]["attachedLibraries"]}
        found: Dict[str, List[str]] = {}
        for ident in sorted(used - node.get("ownUnits", set())):
            libs = owners.get(ident, [])
            if len(libs) > 1:
                notes.append({"module": node["name"], "identifier": ident, "definedBy": sorted(libs)})
            elif libs and libs[0].casefold() not in attached and not (node["type"] == "PLL" and libs[0].casefold() == node["name"].casefold()):
                found.setdefault(libs[0], []).append(ident)
        for lib, idents in sorted(found.items()):
            node["deps"]["attachedLibraries"].append(lib)
            node.setdefault("inferred", []).append({"library": lib, "uses": idents[:10]})
    return notes


def dependency_closure(nodes: List[Dict], targets: List[str]) -> List[Dict]:
    """The target modules plus every module they depend on, followed recursively (a target is a module name,
    optionally with its extension or _fmb.xml suffix; a form is preferred when several types share the name)."""
    index: Dict[Tuple[str, str], Dict] = {}
    for n in nodes:
        index[(n["type"], n["name"].casefold())] = n
        if n.get("moduleName"):
            index.setdefault((n["type"], n["moduleName"].casefold()), n)
    pending: List[Dict] = []
    for target in targets:
        stem = _module_stem(Path(target).name).casefold()
        match = next((index[(kind, stem)] for kind in ("FMB", "MMB", "PLL", "OLB") if (kind, stem) in index), None)
        if match is None:
            raise RuntimeError(f"Target {target} is not in the source folder.")
        pending.append(match)
    keep: Dict[int, Dict] = {}
    while pending:
        node = pending.pop()
        if id(node) in keep:
            continue
        keep[id(node)] = node
        for key in DEP_KEYS:
            for name in node["deps"][key]:
                provider = _find_provider(index, key, name)
                if provider is not None and id(provider) not in keep:
                    pending.append(provider)
    return [n for n in nodes if id(n) in keep]


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


def scan_source(source_dir: Path, prefer_binaries: bool = False) -> List[Dict]:
    """Nodes for every module in the source folder. XML wins over a binary of the same name; .pld is the
    authoritative source for a PLL when present, falling back to a raw .pll binary if no .pld exists.

    With `prefer_binaries`, an original .fmb/.mmb/.olb wins over its XML export: the binary is staged and
    compiled as is (no XML2Forms round-trip, which loses named colours and inherited content). Its dependencies
    still come from the XML export of the same module when there is one (exact), else from the name heuristic."""
    nodes: Dict[Tuple[str, str], Dict] = {}
    files = sorted((p for p in source_dir.iterdir() if p.is_file()), key=lambda p: (p.name.casefold(), p.name))
    binaries = {(BINARY_EXT[p.suffix.lower()], p.stem.casefold()): p for p in files
                if p.suffix.lower() in (".fmb", ".mmb", ".olb")} if prefer_binaries else {}
    for path in files:
        if path.suffix.lower() == ".xml":
            node = scan_xml(path)
            if node:
                binary = binaries.get((node["type"], node["name"].casefold()))
                if binary is not None:  # stage the original binary, keep the XML's exact dependency list
                    node = {**node, "source": binary, "convert": False, "xmlExport": path}
                nodes[(node["type"], node["name"].casefold())] = node
    for path in files:
        if path.suffix.lower() == ".pld":
            deps = _empty_deps()
            for lib in scan_pld(path):
                _add_dep(deps, "attachedLibraries", lib)
            nodes[("PLL", path.stem.casefold())] = {
                "type": "PLL",
                "name": path.stem,
                "source": path,
                "deps": deps,
                "convert": True,
                "defines": scan_pld_defines(path),
            }
    for path in files:
        kind = BINARY_EXT.get(path.suffix.lower())
        if kind and (kind, path.stem.casefold()) not in nodes:
            nodes[(kind, path.stem.casefold())] = _binary_node(path, source_dir)
    return sorted(nodes.values(), key=lambda n: (n["name"].casefold(), TYPE_ORDER.get(n["type"], 9)))


def _unresolved_pld(_nodes: List[Dict]) -> List[Dict]:
    """No longer unresolved: 14c builds .pll from .pld via frmcmp_batch parse=yes logon=no."""
    return []


def _find_provider(index: Dict[Tuple[str, str], Dict], key: str, name: str) -> Optional[Dict]:
    return index.get((DEP_TYPE[key], name.casefold()))


def _similar_names(index: Dict[Tuple[str, str], Dict], kind: str, name: str) -> List[str]:
    """Modules of the same type whose names look like a renamed version of a missing dependency (shared
    prefix or close spelling, e.g. a library referenced by an old file name)."""
    wanted = name.casefold()
    found = {n["name"] for (k, _), n in index.items() if k == kind and (
        n["name"].casefold().startswith(wanted) or wanted.startswith(n["name"].casefold()) and len(n["name"]) >= 4)}
    close = difflib.get_close_matches(wanted, [n["name"].casefold() for (k, _), n in index.items() if k == kind], n=3, cutoff=0.75)
    found.update(n["name"] for (k, c), n in index.items() if k == kind and c in close)
    return sorted(found)[:5]


def _build_edges(real: List[Dict], unresolved: List[Dict]) -> Tuple[Dict[int, set], set]:
    """Provider -> consumer edges, plus the set of soft edges (consumer is a binary whose dependencies came
    from the name-matching heuristic and may over-include)."""
    index = {(n["type"], n["name"].casefold()): n for n in real}
    for n in real:  # a parent form is named by its module name, which can differ from its file name
        if n.get("moduleName"):
            index.setdefault((n["type"], n["moduleName"].casefold()), n)
    edges: Dict[int, set] = {id(n): set() for n in real}
    soft: set = set()
    for node in real:
        for key in DEP_KEYS:
            for name in node["deps"][key]:
                provider = _find_provider(index, key, name)
                if provider is None:
                    item = {"module": node["name"], "dependency": name, "kind": DEP_TYPE[key]}
                    similar = _similar_names(index, DEP_TYPE[key], name)
                    if similar:
                        item["suggestions"] = similar
                    unresolved.append(item)
                elif id(provider) != id(node):
                    edges[id(provider)].add(id(node))
                    if not node["convert"]:
                        soft.add((id(provider), id(node)))
    return edges, soft


def _kahn(by_id: Dict[int, Dict], edges: Dict[int, set]) -> Tuple[List[Dict], set]:
    """Deterministic Kahn ordering (ties: type tier OLB->PLL->MMB->FMB, then name). Returns (ordered nodes, ids left unordered)."""
    indegree = {i: 0 for i in by_id}
    for consumers in edges.values():
        for consumer in consumers:
            indegree[consumer] += 1
    sort_key = lambda n: (TYPE_ORDER[n["type"]], n["name"].casefold(), n["name"])
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
    return ordered, {i for i, d in indegree.items() if d > 0}


def _loops(ids: set, edges: Dict[int, set]) -> List[List[int]]:
    """Strongly connected components (Tarjan, iterative) with more than one member: the real cycles."""
    index_of: Dict[int, int] = {}
    low: Dict[int, int] = {}
    on_stack: set = set()
    stack: List[int] = []
    found: List[List[int]] = []
    counter = 0
    for root in sorted(ids):
        if root in index_of:
            continue
        work = [(root, iter(sorted(edges[root] & ids)))]
        index_of[root] = low[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, children = work[-1]
            advanced = False
            for child in children:
                if child not in index_of:
                    index_of[child] = low[child] = counter
                    counter += 1
                    stack.append(child)
                    on_stack.add(child)
                    work.append((child, iter(sorted(edges[child] & ids))))
                    advanced = True
                    break
                if child in on_stack:
                    low[node] = min(low[node], index_of[child])
            if advanced:
                continue
            work.pop()
            if work:
                low[work[-1][0]] = min(low[work[-1][0]], low[node])
            if low[node] == index_of[node]:
                members = []
                while True:
                    member = stack.pop()
                    on_stack.discard(member)
                    members.append(member)
                    if member == node:
                        break
                if len(members) > 1:
                    found.append(members)
    return found


def resolve_and_order(nodes: List[Dict]) -> Tuple[List[Dict], List[Dict], List[List[str]]]:
    """Return (ordered nodes, unresolved dependencies, cycles). Providers precede consumers; ties break by
    case-insensitive name then type so the order is deterministic. If the order stalls, soft (heuristic)
    edges inside the stalled set are dropped with a note on stderr; only real loops are reported as cycles."""
    real = [n for n in nodes if n["type"] != "PLD"]
    by_id = {id(n): n for n in real}
    unresolved = _unresolved_pld(nodes)
    edges, soft = _build_edges(real, unresolved)
    dropped: List[str] = []
    ordered, left = _kahn(by_id, edges)
    while left:
        group = {member: n for n, loop in enumerate(_loops(left, edges)) for member in loop}
        drop = {(p, c) for p, c in soft if p in group and c in group and group[p] == group[c] and c in edges[p]}
        if not drop:
            break
        for provider, consumer in drop:
            edges[provider].discard(consumer)
            dropped.append(f"{by_id[consumer]['name']} ({by_id[consumer]['type']}) -> {by_id[provider]['name']} ({by_id[provider]['type']})")
        ordered, left = _kahn(by_id, edges)
    if dropped:
        print(f"Note: dropped {len(dropped)} heuristic binary dependencies that formed loops: " + "; ".join(sorted(dropped)), file=sys.stderr)
    sort_key = lambda n: (n["name"].casefold(), TYPE_ORDER[n["type"]], n["name"])
    cycles = [[f"{n['name']} ({n['type']})" for n in sorted((by_id[i] for i in loop), key=sort_key)] for loop in _loops(left, edges)]
    return ordered, unresolved, sorted(cycles)

def _module_entry(order: int, node: Dict) -> Dict:
    """One manifest module record with build-dir-relative paths."""
    kind, name, source = node["type"], node["name"], node["source"]
    is_convert = node.get("convert", source.suffix.lower() in (".xml", ".pld"))
    binary_ext = ".pll" if kind == "PLL" else f".{kind.lower()}"
    exe_ext = EXECUTABLE_EXT[kind]
    return {
        "order": order, "name": name, "artifactType": kind, "sourceFile": str(source),
        "stagedInputFile": f"stage/{source.name}", "binaryFile": f"stage/{name}{binary_ext}",
        "executableFile": f"stage/{name}{exe_ext}" if exe_ext else None,
        "requiresConversion": is_convert, "dependencies": node["deps"], "status": "pending",
        **({"inferredAttachedLibraries": node["inferred"]} if node.get("inferred") else {}),
    }


def _callers(source_dir: Path, names: List[str]) -> Dict[str, List[str]]:
    """Other source files whose text mentions each module name (call_form/open_form arguments, menu items).
    A text search only: calls built from variables or stored in a database are not seen."""
    patterns = {n: re.compile(r"(?<![A-Za-z0-9_$])" + re.escape(n) + r"(?![A-Za-z0-9_$])", re.IGNORECASE) for n in names}
    found: Dict[str, List[str]] = {n: [] for n in names}
    for path in sorted(source_dir.iterdir()):
        if not path.is_file() or path.suffix.lower() not in (".xml", ".pld"):
            continue
        text = path.read_text(encoding="utf-8", errors="ignore")
        for name, pattern in patterns.items():
            if not _module_stem(path.name).casefold() == name.casefold() and pattern.search(text):
                found[name].append(path.name)
    return found


def _needs_review(unresolved: List[Dict], source_dir: Path) -> List[Dict]:
    """One entry per module with missing dependencies: what is missing, similar files present, and which
    other files still mention the module (a hint whether it is still used)."""
    grouped: Dict[str, List[Dict]] = {}
    for item in unresolved:
        if item["module"] != item["dependency"]:
            grouped.setdefault(item["module"], []).append(item)
    callers = _callers(source_dir, sorted(grouped)) if grouped else {}
    return [{"module": module, "missing": [{"dependency": i["dependency"], "kind": i["kind"], "suggestions": i.get("suggestions", [])} for i in items],
             "mentionedBy": callers.get(module, [])} for module, items in sorted(grouped.items())]


STAGE_RECORDS = ("decompiled", "validated", "converted", "compiled", "status", "diagnostics")


def _carry_over_stage_records(modules: List[Dict], build_dir: Path) -> None:
    """Keep each module's stage records (decompiled, validated, converted, compiled and its status) of an existing manifest when Stage 1 is re-run."""
    path = build_dir / MANIFEST_NAME
    if not path.is_file():
        return
    try:
        previous = {m["name"].casefold(): m for m in json.loads(path.read_text(encoding="utf-8")).get("modules", [])}
    except (ValueError, KeyError):
        return
    for module in modules:
        old = previous.get(module["name"].casefold(), {})
        for key in STAGE_RECORDS:
            if key in old and not (key == "status" and old[key] == "pending"):
                module[key] = old[key]


def compute_required_symlinks(modules: List[Dict]) -> List[Dict]:
    """Identify Linux case-variant symlinks required in stage/ and app/ for mixed/uppercase dependency names."""
    providers: Dict[Tuple[str, str], Tuple[str, str]] = {}
    for mod in modules:
        bin_name = Path(mod["binaryFile"]).name if mod.get("binaryFile") else ""
        exe_name = Path(mod["executableFile"]).name if mod.get("executableFile") else ""
        providers[(mod["artifactType"], mod["name"].casefold())] = (bin_name, exe_name)

    grouped: Dict[Tuple[str, str, str], set] = {}

    def _add(location: str, ref: str, ext: str, target: str, caller: str) -> None:
        if not target:
            return
        link = f"{ref}{ext}"
        if link == target:
            return
        grouped.setdefault((location, link, target), set()).add(caller)

    dep_specs = (
        ("parentModules", "FMB", ".fmb", ""),
        ("attachedLibraries", "PLL", ".pll", ".plx"),
        ("menus", "MMB", ".mmb", ".mmx"),
        ("objectLibraries", "OLB", ".olb", ""),
    )
    for mod in modules:
        deps = mod.get("dependencies") or {}
        caller = mod["name"]
        for key, dep_type, stage_ext, app_ext in dep_specs:
            for ref in deps.get(key, []):
                bin_target, exe_target = providers.get((dep_type, ref.casefold()), ("", ""))
                _add("stage", ref, stage_ext, bin_target, caller)
                if app_ext:
                    _add("app", ref, app_ext, exe_target, caller)

    return [
        {"location": loc, "link": link, "target": target, "referencedBy": sorted(callers)}
        for (loc, link, target), callers in sorted(grouped.items())
    ]


def assign_waves(modules: List[Dict]) -> None:
    """Wave 1 holds modules with no dependency in the manifest; wave N holds modules whose dependencies are all
    in earlier waves (1 + the deepest provider). Modules in one wave do not depend on each other."""
    index: Dict[Tuple[str, str], Dict] = {(m["artifactType"], m["name"].casefold()): m for m in modules}
    for module in modules:  # manifest order puts every provider before its consumers
        waves = [0]
        for key in DEP_KEYS:
            kinds = ("FMB", "OLB") if key == "parentModules" else (DEP_TYPE[key],)
            for name in module["dependencies"][key]:
                provider = next((index[(k, name.casefold())] for k in kinds if (k, name.casefold()) in index), None)
                if provider is not None and provider is not module:
                    waves.append(provider.get("wave", 1))
        module["wave"] = max(waves) + 1


def build_manifest(source_dir: Path, build_dir: Path, targets: Optional[List[str]] = None, infer: bool = True,
                   prefer_binaries: bool = False) -> Dict:
    """Stage 1: scan, infer undeclared library attachments, keep only the targets' recursive dependency
    closure (when targets are given), resolve and order; return the manifest dictionary."""
    nodes = scan_source(source_dir, prefer_binaries)
    notes = infer_library_attachments([n for n in nodes if n.get("convert", True)]) if infer else []
    if targets:
        nodes = dependency_closure(nodes, targets)
        names = {n["name"] for n in nodes}
        notes = [n for n in notes if n["module"] in names]
    ordered, unresolved, cycles = resolve_and_order(nodes)
    modules = [_module_entry(i, n) for i, n in enumerate(ordered, 1)]
    assign_waves(modules)
    _carry_over_stage_records(modules, build_dir)
    return {
        "schemaVersion": 1, "generatedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "sourceDir": str(source_dir), "buildDir": str(build_dir),
        **({"targets": list(targets)} if targets else {}),
        "ambiguousLibraryUnits": notes,
        "modules": modules,
        "unresolvedDependencies": unresolved,
        "requiredSymlinks": compute_required_symlinks(modules),
        "cycles": cycles,
        "needsReview": _needs_review(unresolved, source_dir),
    }


def write_manifest(manifest: Dict) -> Path:
    """Write the manifest into its build folder atomically (temp file + os.replace) and return its path."""
    target = Path(manifest["buildDir"]) / MANIFEST_NAME
    temp = target.with_name(target.name + ".tmp")
    temp.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    os.replace(temp, target)
    return target


def stage_sources(source_dir: Path, build_dir: Path, manifest: Optional[Dict] = None) -> Path:
    """Copy source files into <build-dir>/stage/ (the source folder is never modified). With a targeted
    manifest only the modules in it are copied. Inferred library attachments are then written into the
    staged XML copies."""
    stage = build_dir / "stage"
    rt.ensure_dir(stage)
    wanted = {Path(m["sourceFile"]).name for m in manifest["modules"]} if manifest and manifest.get("targets") else None
    for path in sorted(source_dir.iterdir()):
        if path.is_file() and path.suffix.lower() in STAGE_EXT and (wanted is None or path.name in wanted):
            shutil.copy2(path, stage / path.name)
    if manifest:
        add_inferred_attachments(manifest, stage)
    return stage


ATTACH_ANCHOR = re.compile(r"<(FormModule|MenuModule|ObjectLibrary)\b[^>]*?(/?)>")


def add_inferred_attachments(manifest: Dict, stage: Path) -> List[str]:
    """Insert <AttachedLibrary> tags for each module's inferred libraries into its staged XML copy, after any
    leading Coordinate/Alert elements (the position a Forms2XML export uses). Returns the changed file names."""
    changed: List[str] = []
    for module in manifest["modules"]:
        libs = [item["library"] for item in module.get("inferredAttachedLibraries", [])]
        staged = stage / Path(module["stagedInputFile"]).name
        if not libs or staged.suffix.lower() != ".xml" or not staged.is_file():
            continue
        text = staged.read_text(encoding="utf-8")
        anchor = ATTACH_ANCHOR.search(text)
        if anchor is None or anchor.group(2):
            continue
        pos = anchor.end()
        while True:
            child = re.match(r"\s*<(Coordinate|Alert)\b", text[pos:])
            if not child:
                break
            start = pos + child.start(1) - 1
            closed = re.match(r"<%s\b[^>]*/>" % child.group(1), text[start:])
            pos = start + closed.end() if closed else text.index("</%s>" % child.group(1), start) + len(child.group(1)) + 3
        tags = "".join(f'\n      <AttachedLibrary Name="{lib.upper()}" LibraryLocation="{lib.lower()}" LibrarySource="File"/>'
                       for lib in libs if not re.search(r'<AttachedLibrary\b[^>]*Name="%s"' % re.escape(lib), text, re.IGNORECASE))
        if tags:
            staged.write_text(text[:pos] + tags + text[pos:], encoding="utf-8")
            changed.append(staged.name)
    return changed


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
    """XML2Forms command for one staged XML: `podman exec` into the running forms-server-14c when the stage folder
    is under the Oracle Home, otherwise a throwaway container with the stage folder mounted at /work."""
    home = rt.HOME
    server_dir = rt.server_workdir(stage)
    workdir = server_dir or "/work"
    rel = xml_path.relative_to(stage).as_posix()
    outdir = workdir + ("/" + Path(rel).parent.as_posix() if Path(rel).parent.as_posix() != "." else "")
    classpath = f"{home}/jlib/frmjdapi.jar:{home}/jlib/frmxmltools.jar:{home}/oracle_common/modules/oracle.xdk/xmlparserv2.jar"
    script = (f"export ORACLE_HOME={home}; export FORMS_API_TK_BYPASS=TRUE; export LD_LIBRARY_PATH={home}/lib; "
              f"export FORMS_PATH={workdir}:{home}/forms; "
              f"java -Djava.library.path={home}/lib -classpath {classpath} oracle.forms.util.xmltools.XML2Forms "
              f"OVERWRITE=YES OUTPUTDIR={outdir} {workdir}/{rel}")
    if server_dir:
        return rt.exec_in_server(script)
    mounts = ["-v", f"{rt.host_path(stage)}:/work:Z"]
    oracle_home = rt.default_oracle_home()
    if oracle_home:
        mounts += ["-v", f"{rt.host_path(oracle_home)}:{home}:Z"]
    return rt.podman_cmd(["run", "--rm", "--platform", "linux/amd64", *mounts, rt.IMAGE, "bash", "-c", script])


def pld2pll_command(stage: Path, pld_path: Path, output_filename: str) -> List[str]:
    """frmcmp_batch command to parse .pld text into a .pll binary library: `podman exec` into the running
    forms-server-14c when the stage folder is under the Oracle Home, otherwise a throwaway container."""
    home = rt.HOME
    server_dir = rt.server_workdir(stage)
    workdir = server_dir or "/work"
    pld_name = pld_path.name
    script = (
        f"export ORACLE_HOME={home}; export ORACLE_TERM=vt220; export TERM=vt220; "
        f"export LD_LIBRARY_PATH={home}/lib:{home}/oracle_common/lib:$LD_LIBRARY_PATH; "
        f"export FORMS_PATH={workdir}:{home}/forms; cd {workdir}; "
        f"{home}/bin/frmcmp_batch module=\\\"{pld_name}\\\" output_file=\\\"{output_filename}\\\" "
        f"module_type=library parse=yes logon=no batch=yes"
    )
    if server_dir:
        return rt.exec_in_server(script)
    mounts = ["-v", f"{rt.host_path(stage)}:/work:Z"]
    oracle_home = rt.default_oracle_home()
    if oracle_home:
        mounts += ["-v", f"{rt.host_path(oracle_home)}:{home}:Z"]
    return rt.podman_cmd(["run", "--rm", "--platform", "linux/amd64", *mounts, rt.IMAGE, "bash", "-c", script])


def convert_one(module: Dict, build_dir: Path, normalize: bool) -> Tuple[bool, str]:
    """Convert one XML or PLD module to its binary; return (ok, diagnostics)."""
    stage = build_dir / "stage"
    input_file = stage / Path(module["stagedInputFile"]).name
    binary = build_dir / module["binaryFile"]
    if input_file.suffix.lower() == ".pld":
        cmd = pld2pll_command(stage, input_file, binary.name)
    else:
        xml_path = _normalize_xml(input_file) if normalize else input_file
        cmd = xml2forms_command(stage, xml_path)
    res = subprocess.run(cmd, capture_output=True, text=True)
    actual = next((p for p in binary.parent.iterdir() if p.name.casefold() == binary.name.casefold()), None) if binary.parent.is_dir() else None
    if actual is not None:
        if actual.name != binary.name:
            module["binaryFile"] = f"{Path(module['binaryFile']).parent.as_posix()}/{actual.name}"
            if module.get("executableFile"):
                module["executableFile"] = str(Path(module["binaryFile"]).with_suffix(Path(module["executableFile"]).suffix).as_posix())
        return True, ""
    tool_name = "frmcmp_batch" if input_file.suffix.lower() == ".pld" else "XML2Forms"
    return False, f"{tool_name} did not create {binary.name} (exit {res.returncode}):\n{res.stdout}{res.stderr}".strip()


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
        module["converted"] = {"status": "done" if ok else "failed", "note": note}
        module["diagnostics"] = note
        failed = failed or not ok
        write_manifest(manifest)
        if not ok:
            print(note, file=sys.stderr)
    write_manifest(manifest)
    return 1 if failed else 0


def decompile_binaries(manifest: Dict, build_dir: Path) -> int:
    """Gate step: decompile every binary-only module (.pll to .pld; .fmb/.mmb/.olb to XML) with the 14c tooling.
    Inputs are copied to <build-dir>/decompiled/in/, results land in <build-dir>/decompiled/out/ and each module's
    `decompiled` record is written straight into the manifest (saved after every module, so progress survives a stop)."""
    import convert_fmb as cf
    work_in, work_out = build_dir / "decompiled" / "in", build_dir / "decompiled" / "out"
    rt.ensure_dir(work_in)
    rt.ensure_dir(work_out)
    done = failed = 0
    for module in manifest["modules"]:
        if module["requiresConversion"]:
            continue
        source = Path(module["sourceFile"])
        staged = work_in / source.name
        shutil.copy2(source, staged)
        print(f"Decompile {source.name}")
        code = cf.convert_file(str(staged), str(work_out) + "/", stage=False)
        outputs = sorted(p.name for p in work_out.glob(f"{source.stem}*") if p.suffix.lower() in (".pld", ".xml"))
        ok = code == 0 and bool(outputs)
        module["decompiled"] = {"status": "done" if ok else "failed", "output": f"decompiled/out/{outputs[0]}" if outputs else ""}
        done, failed = done + ok, failed + (not ok)
        write_manifest(manifest)
    names = [m["name"] for m in manifest["modules"] if m.get("decompiled", {}).get("status") == "failed"]
    print(f"Decompiled {done} of {done + failed}; failed: {', '.join(names) or 'none'}")
    return 1 if failed else 0


def validate_text_set(manifest: Dict, build_dir: Path) -> int:
    """Gate step after decompile: validate each module's XML (its source XML, else its decompiled XML) against the
    14c schema with validate_forms_xml.py and write a `validated` record into the manifest."""
    script = Path(__file__).resolve().parents[2] / "compiling-and-running-oracle-forms" / "scripts" / "validate_forms_xml.py"
    checked = failed = 0
    for module in manifest["modules"]:
        target = Path(module["sourceFile"])
        if target.suffix.lower() != ".xml":
            output = module.get("decompiled", {}).get("output", "")
            target = build_dir / output if output.lower().endswith(".xml") else None
        if target is None:
            continue
        proc = subprocess.run([sys.executable, str(script), str(target)], capture_output=True, text=True)
        ok = proc.returncode == 0
        module["validated"] = {"status": "pass" if ok else "fail", "file": str(target)}
        if not ok:
            module["validated"]["errors"] = (proc.stdout or proc.stderr).strip()[:500]
        checked, failed = checked + 1, failed + (not ok)
        write_manifest(manifest)
    print(f"Schema validation: {checked - failed} passed, {failed} failed")
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
    parser.add_argument("--decompile-binaries", action="store_true", help="Decompile binary-only modules to XML/PLD into <build-dir>/decompiled/ and record the result in the manifest")
    parser.add_argument("--validate-xml", action="store_true", help="After the decompile step, validate each module's XML against the 14c schema and record `validated` in the manifest")
    parser.add_argument("--convert-only", action="store_true", help="Stage 2 only: convert the modules in the existing <build-dir>/build-manifest.json (no Stage 1 re-run, no re-staging)")
    parser.add_argument("--normalize", action="store_true", help="Apply the Oracle-compatibility XML normalization before XML2Forms")
    parser.add_argument("--target", action="append", default=[], metavar="MODULE",
                        help="Keep only this module and its recursive dependencies (repeatable; default: every module in the folder)")
    parser.add_argument("--prefer-binaries", action="store_true",
                        help="Use an original .fmb/.mmb/.olb instead of its XML export when both are present (keeps named colours and inherited content)")
    parser.add_argument("--no-infer-libraries", action="store_true",
                        help="Do not add libraries that a module calls but does not attach (12c exports can omit <AttachedLibrary>)")
    return parser.parse_args(argv)


def main() -> int:
    """Run Stage 1 and, unless --analyze-only, Stage 2."""
    args = parse_cli_args(sys.argv[1:])
    if args.convert_only:
        try:
            build_dir = require_external(Path(args.build_dir) if args.build_dir else rt.work_dir() / "build")
            manifest_path = build_dir / "build-manifest.json"
            if not manifest_path.is_file():
                raise RuntimeError(f"No manifest at {manifest_path}; run Stage 1 first.")
            return convert_manifest(json.loads(manifest_path.read_text(encoding="utf-8")), args.normalize)
        except RuntimeError as err:
            print(f"Error: {err}", file=sys.stderr)
            return 1
    try:
        source_dir = Path(args.source_dir) if args.source_dir else ask_source_dir()
        if not source_dir.is_dir():
            raise RuntimeError(f"Source folder not found: {source_dir}")
        build_dir = require_external(Path(args.build_dir) if args.build_dir else rt.work_dir() / "build")
        rt.ensure_dir(build_dir)
        manifest = build_manifest(source_dir.resolve(), build_dir, args.target, not args.no_infer_libraries, args.prefer_binaries)
    except RuntimeError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1
    path = write_manifest(manifest)
    print(f"Manifest: {path} ({len(manifest['modules'])} modules, {len(manifest.get('requiredSymlinks', []))} case symlinks)")
    for wave in sorted({m["wave"] for m in manifest["modules"]}):
        members = [f"{m['name']} ({m['artifactType']})" for m in manifest["modules"] if m["wave"] == wave]
        print(f"  Wave {wave}: {', '.join(members)}")
    for module in manifest["modules"]:
        for item in module.get("inferredAttachedLibraries", []):
            print(f"  INFERRED: {module['name']} ({module['artifactType']}) calls {item['library']} without attaching it"
                  f" (uses {', '.join(item['uses'][:5])}); the staged copy gets <AttachedLibrary Name=\"{item['library'].upper()}\">")
    for note in manifest.get("ambiguousLibraryUnits", []):
        print(f"  AMBIGUOUS: {note['module']} uses {note['identifier']}, defined by {', '.join(note['definedBy'])}; attach one by hand", file=sys.stderr)
    for item in manifest["unresolvedDependencies"]:
        print(f"  UNRESOLVED: {item['module']} needs {item['dependency']} ({item['kind']}) {item.get('reason', '')}".rstrip()
              + (f" - similar present: {', '.join(item['suggestions'])}" if item.get("suggestions") else ""), file=sys.stderr)
    for cycle in manifest["cycles"]:
        print(f"  CYCLE: {', '.join(cycle)}", file=sys.stderr)
    if args.decompile_binaries or args.validate_xml:
        code = decompile_binaries(manifest, build_dir) if args.decompile_binaries else 0
        return max(code, validate_text_set(manifest, build_dir) if args.validate_xml else 0)
    if manifest["cycles"] or (manifest["unresolvedDependencies"] and not args.allow_missing):
        print("Failing closed: resolve the problems above (or pass --allow-missing for missing providers).", file=sys.stderr)
        return 2
    if args.analyze_only:
        return 0
    try:
        stage_sources(source_dir.resolve(), build_dir, manifest)
        return convert_manifest(manifest, args.normalize)
    except RuntimeError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
