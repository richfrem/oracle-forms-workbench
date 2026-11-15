# UI review backlog

This is an active source-inspection backlog, not a record of browser certification. The wider
runtime-parity work is tracked in [10-NEXT-PHASE-SESSION-PROMPT.md](10-NEXT-PHASE-SESSION-PROMPT.md).
Items below were checked against the current source; visual behavior still requires browser or
Oracle runtime verification where noted.

| # | Item | Evidence it is still open |
|---|---|---|
| 1 | Reassess horizontal overflow in the header and canvas controls at narrow widths | `ui/styles.css` still applies `overflow-x: auto` to the top bar and canvas controls; the responsive behavior has not been browser-verified here |
| 2 | Filter `<New_Item>` placeholder menu entries when extracting binary `.mmb` files | No matching placeholder filter was found in the Rust or UI source; extraction behavior remains unverified |
| 3 | Define cache behavior for embedded UI assets | No `Cache-Control` or `no-store` policy was found in the server source |
| 4 | Add a standalone `.mmb` menu-tree inspector | The UI source has no standalone menu-tree renderer; menu-module data support is not the same as an inspector |
| 5 | Verify native-vs-Oracle visual fidelity | Font persistence, theme/zoom/DPI, and tab/overlay cases remain open; compilation is not visual parity |
| 6 | Verify native-vs-Oracle runtime behavior | Golden trace comparisons and entry/navigation-flow checks remain open under Track K |

Source inspection only; no browser, accessibility, live database, or Oracle runtime checks were
performed for this backlog update.
