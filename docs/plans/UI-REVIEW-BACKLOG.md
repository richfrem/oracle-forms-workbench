# UI review backlog

This is an active source-inspection backlog, not a record of browser certification. The wider
runtime-parity work is tracked in [10-NEXT-PHASE-SESSION-PROMPT.md](10-NEXT-PHASE-SESSION-PROMPT.md)
and [14-MODERN-IDE-DUAL-ENGINE-PLAN.md](14-MODERN-IDE-DUAL-ENGINE-PLAN.md).
Items below were checked against the current source; visual behavior still requires browser or
Oracle runtime verification where noted.

| # | Item | Evidence it is still open |
|---|---|---|
| 1 | Reassess horizontal overflow in the header and canvas controls at narrow widths | `ui/styles.css` still applies `overflow-x: auto` to the top bar and canvas controls; responsive behavior has not been browser-verified here *(context menus were grouped into `Export / Convert`, `Build`, and `Run / Preview` dropdowns in `#47`)* |
| 2 | Filter `<New_Item>` placeholder menu entries when extracting binary `.mmb` files | No matching placeholder filter was found in the Rust or UI source; extraction behavior remains unverified |
| 3 | Define cache behavior for embedded UI assets | No `Cache-Control` or `no-store` policy was found in the server source |
| 4 | Add a standalone `.mmb` menu-tree inspector | `MenuItem` PL/SQL (`0x01d0` / `MenuItemCode`) now renders in PL/SQL Studio (`#51`), but the UI source still has no standalone `.mmb` visual menu-tree renderer |
| 5 | Verify native-vs-Oracle visual fidelity | `<Point>` polygons, `CompoundText`/`TextSegment`, `[No Canvas]` title clearing, and Designer `<ListItemElement>` fallback landed in `#46`–`#49`; font persistence, `preview.js` live `<ListItemElement>` dropdown options, theme/zoom/DPI, and tab/overlay cases remain open; compilation is not visual parity |
| 6 | Verify native-vs-Oracle runtime behavior | Golden trace comparisons and entry/navigation-flow checks remain open under Track K |

Source inspection only (`32` frontend unit checks pass in `tests/ui_*.test.cjs`); no browser,
accessibility, live database, or Oracle runtime checks were performed for this backlog update.
