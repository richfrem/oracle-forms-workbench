# JNLP & `formsweb.cfg` Launch Reference (Legacy Web Start vs. FSAL)

This reference documents the sanitized, generic JNLP and `formsweb.cfg` templates under `assets/` and summarizes when to use **FSAL (`frmsal.jar`)** vs. **Java Web Start (`.jnlp`)** on Oracle Forms 14c.

## Assets

| File | Purpose |
|---|---|
| [`../assets/sample_local.jnlp`](../assets/sample_local.jnlp) | Sanitized, generic localhost `.jnlp` descriptor (`config=orders`, `orders_entry.fmx`). Local testbed only—never use plain HTTP or unpinned JARs on non-local networks. |
| [`../assets/formsweb.cfg.template`](../assets/formsweb.cfg.template) | Security-annotated `formsweb.cfg` named-section template (`webstart=enabled`, `restrictedURLparams`, `allowStaticHTML=false`, no `userid=`). |

## Choosing a Client Vector

1. **FSAL (`frmsal.jar` via `launch_fsal.py`) — Preferred:** Runs on patched Java 17 or 21 without browser plugins or Java Web Start (`config=standaloneapp`, interactive logon by default).
2. **Java Web Start (`.jnlp`) — Legacy Only:** Use only when an application requires `<component-desc/>` / `<extension>` client JAR descriptors that cannot be loaded via FSAL. Requires JDK 8 `javaws` or OpenWebStart, `<all-permissions/>`, and explicit `formsweb.cfg` hardening (`restrictedURLparams`, `allowStaticHTML=false`).
3. **Native Rust Web UI (`Preview in Rust`) — Zero Thick-Client:** Eliminates workstation JAR downloads and JNLP descriptors entirely.

## Security & Hardening Controls

- Never embed `userid=` or database credentials in `.jnlp` descriptors, launch URLs, or `formsweb.cfg`.
- Require HTTPS (`https://`) for `codebase` and `frmservlet` on any non-loopback 14c server.
- Lock down `restrictedURLparams` in `formsweb.cfg` (`envFile,userid,serverURL,otherparams,...`) to prevent query-string overrides.
- Full threat matrix (`JNLP-R1`–`R5`) and hardening checklist (`S15.1`–`S15.4`): [`docs/plans/15-JNLP-AND-LEGACY-LAUNCH-SECURITY-RISK-ANALYSIS.md`](../../../../../../docs/plans/15-JNLP-AND-LEGACY-LAUNCH-SECURITY-RISK-ANALYSIS.md).
