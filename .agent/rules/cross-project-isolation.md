---
trigger: always_on
description: Keeps other projects read-only and prevents their identifying information or artifacts from entering this public repository.
globs: ["**/*"]
---

# Rule: Cross-project information isolation

## 1. The Iron Law

> When work in this repository involves another project using the workbench, information specific to that project MUST NOT be included anywhere in this repository. Document learning only as generalized, non-identifying guidance.

## 2. Invariants & Forbidden Actions

1. **Scope**: This rule applies both when changing the workbench for another project and when helping another project use the workbench.
2. **No project material**: NEVER copy or include another project's name, repository identity, code, file or directory paths, filenames, schemas, configuration, logs, data, screenshots, or other identifying details.
3. **All repository locations**: This restriction includes tracked, untracked, ignored, temporary, generated, test, sample, and documentation files in this repository. Ignoring a file does not make it an approved location.
4. **Keep source material separate**: Work with project-specific materials only in that project's own repository or in a user-controlled location outside this repository. Do not copy them here for convenience.
5. **Generalize learning**: A reusable lesson may be documented only after removing project names, source references, exact artifacts, and details that could identify or reconstruct the source project. Do not link to, quote, or cite its files.
6. **When uncertain**: Omit the detail. Ask the user before adding anything that may be project-specific; user-provided material is not permission to publish it here.
7. **Repository outputs**: Apply these restrictions to examples, test fixtures, generated reports, comments, change logs, and other content prepared for this repository.

## 3. Read-only inspection and output boundaries

1. Treat every external project directory, including a directory configured by `OFW_FORMS_PATH_EXTRA` or supplied for a task, as a read-only source. Do not modify, rename, delete, or write beside its source files.
2. Never copy external `.fmb`, `.mmb`, `.olb`, `.pll`, `.rdf`, `.xml`, `.sql`, or `.env` files into this repository, including `samples/`, `temp/`, `context/`, `tests/`, `oracle-reference-stack/`, and ignored or generated folders.
3. Never write external-project outputs into this repository. This includes XML exports, SQL sidecars, diffs, compiled `.fmx`/`.plx`/`.mmx` files, conversion intermediates, and diagnostic logs. Use an `OFW_WORK_DIR` outside the repository or a separate, user-designated output directory outside the source tree. If the configured work directory resolves inside this repository, stop before writing and report the conflict.
4. `temp/` and `context/` are for repository-internal scratch work only. Their ignored status does not authorize placing external project material there.
5. Before using a tool that may write beside its input, choose and verify an external output location first. Prefer read-only inspection options; do not rely on cleanup after an external artifact has been written into the repository.

## 4. Pattern Distillation Protocol

When an external project exposes a bug, missing built-in, XML edge case, or compilation dependency issue:

1. Diagnose in place using read-only operations against the external path. Point CLI and skill tools at the source directly, and direct every output to an external work directory.
2. Reduce the observation to a generic Oracle Forms/Reports mechanism, such as a child form inheriting an object-library property while attaching PL/SQL libraries, or a query block using a bind variable.
3. If a test or sample is needed, author a minimal synthetic fixture from scratch with generic names such as `DEPT`, `EMP`, `ORDERS`, `ITEMS`, and `UTIL_LIB`. Never redact, transform, or find-and-replace an external form to create a fixture.
4. Document only the generalized mechanism and verified remedy in the owning reference and change log. Use placeholders such as `<external-forms-dir>`; omit external project names, source references, object identifiers, SQL, and identifying log text.
5. Keep evidence status explicit. If the behavior was suggested but not reproduced with a synthetic or approved test, label it as unverified rather than documenting it as a repository finding.

## 5. Pre-write and pre-commit gate

Before creating or editing a sample, test, script, reference, or change-log entry—and again before finishing—inspect `git status` and `git diff`, including untracked files. Review ignored work areas without copying or exposing their contents. Confirm that no external project identity, object/schema names, paths, filenames, code, settings, or pasted logs have entered repository outputs. If the scan finds pre-existing external material, do not move or delete it without authorization; do not claim the workspace is clear, and keep it out of all new work.

## 6. Evaluation Checklist

- Does this repository content contain any other project's identity or project-specific material?
- Are any paths, filenames, code excerpts, schemas, settings, logs, or data traceable to another project?
- If documenting a lesson, is it generalized without links or details that could reveal its source?
- Have temporary and ignored locations been treated with the same restriction as tracked files?
