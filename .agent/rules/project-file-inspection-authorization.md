---
trigger: always_on
description: Prohibits inspecting or reading the contents of project-specific test cases or external files unless explicitly authorized by the operator.
globs: ["**/*"]
---

# Rule: Explicit Authorization for Project-Specific File Inspection

## 1. Principle
Repository code, configurations, documentation, and generic samples in `samples/` within `oracle-forms-workbench` may be freely inspected, navigated, and modified as needed for development and maintenance.

However, all project-specific test cases, external forms, schemas, data, and external work artifacts used when evaluating or testing the workbench are strictly gated: their contents may only be inspected when explicitly authorized by the operator.

## 2. Invariants & Rules
1. **No Unauthorized Content Inspection**: The agent must NEVER open, read, print, or scan the contents of project-specific files (such as external forms located in `OFW_FORMS_PATH_EXTRA`, external repositories, or project-specific test cases staged in `OFW_WORK_DIR`) unless the operator explicitly authorizes or requests inspection of that specific artifact.
2. **Command & Metadata Operations**: When assisting with external or project-specific runs, the agent should prioritize providing commands for the operator to run, or work with high-level exit statuses, summary counts, and sanitized compiler output shared by the operator.
3. **Relation to Cross-Project Isolation**: Complements [`.agent/rules/cross-project-isolation.md`](cross-project-isolation.md). Even when the operator explicitly authorizes inspecting an external project file to troubleshoot a compiler, parser, or runtime issue, the agent must NEVER copy proprietary names, schema objects, connect strings, or identifiable source excerpts into this repository's tracked files, commit messages, or change logs.
