# Where the Oracle Home lives: why it should be on Ubuntu's disk, how to move it, how to prove the copy

**Status (2026-11-14): `--verify` and boot from `~/oracle_home_14c` verified.** Moving `oracle_home_14c` to Ubuntu's ext4 reduced WebLogic boot time from **15–44 minutes** to **45 seconds** (directory walk: **8–11s** vs **259s** on `/mnt/c`). `--from-copy`, `--backup`, and `--from-installers` are **not yet run**.

## On Windows: install there from the start

Do not install into `oracle-reference-stack/oracle_home_14c` (on the slow Windows drive) and move it later. Set `OFW_ORACLE_HOME=/home/<you>/oracle_home_14c` in `.env` **before** the installs, create the folder (`mkdir -p "$OH" && chmod 777 "$OH"`), and follow [installing-forms-14c.md](installing-forms-14c.md); every command there uses `$OH`. The installers can stay on the Windows drive. The sections below are for an Oracle Home that already exists in the repo folder.

## Contents

- [Why](#why)
- [The setting](#the-setting)
- [The protocol](#the-protocol)
- [How to test that it was copied correctly](#how-to-test-that-it-was-copied-correctly)
- [Risks and unknowns](#risks-and-unknowns)

## Why

The Oracle Home (`oracle-reference-stack/oracle_home_14c/`) is the installed Forms and Reports 14c software, the WebLogic domain, and every adjustment that took hours to get right (domain hooks, libraries, `tnsnames.ora`). It is 5.4 GB. Containers mount it as `/oracle_home_14c`.

By default it sits inside the repo, and on Windows the repo is on the Windows drive, seen from Ubuntu as `/mnt/c` (a network-style 9p mount). That mount is slow for the huge number of small file reads that WebLogic makes while it boots. On this machine the Forms server took **15, 18, and 44 minutes** to reach RUNNING, against about 2 minutes on a Mac. An earlier thread dump showed one thread doing file-existence checks for most of that time. The same effect is why the Rust build output already lives on Ubuntu's disk (`~/ofw-target`). Putting the Oracle Home on Ubuntu's own disk (ext4) removes that "Windows tax". **The speed-up is expected, not yet measured here.**

What is in git is the **procedure**, not the data: the 5.4 GB cannot be committed. So the Oracle Home can always be rebuilt or moved by one script instead of by memory:

| In the repo | Not in git |
|---|---|
| `oracle-reference-stack/provision_oracle_home.sh` | the Oracle Home itself |
| `oracle-reference-stack/apply_forms14c_config.sh` and `forms14c-config/` | the installers (`oracle-reference-stack/installer/`, git-ignored) |
| the response files in `installer-templates/` | backup archives |

**Updating or recreating containers and images never touches the Oracle Home.** It is a host folder mounted in, not part of any image or container. What can lose it is deleting or resetting the Ubuntu distro (`wsl --unregister`) or disk failure, which is why there is a backup step and a rebuild path.

## The setting

`OFW_ORACLE_HOME` (in the git-ignored `.env`, or the environment) is the host folder of the Oracle Home. Empty means the default, `oracle-reference-stack/oracle_home_14c`, so macOS does not change. Use an absolute path, for example `/home/<you>/oracle_home_14c`. It is read by `provision_oracle_home.sh`, `apply_forms14c_config.sh`, `converter_runtime.py` (so the converters and `test_conversions.py` follow it), `compose.yaml`, `oracle_preview.rs`, `launch_fsal.py`, and `run_form.py`. Not yet updated: `testing-forms-parity/scripts/test_parity.py`, which still has the repo path in two places.

## The protocol

Run everything from the repo root in the Ubuntu shell, after `set -a; source .env; set +a`.

1. **Dry run.** Prints the plan and changes nothing:
   ```bash
   bash oracle-reference-stack/provision_oracle_home.sh --dry-run --from-copy oracle-reference-stack/oracle_home_14c --target ~/oracle_home_14c
   ```
2. **Copy.** A throwaway container runs as root (which maps to your own user, so it can read files owned by the container's mapped user IDs) and streams `tar` from the source, mounted read-only, to the target. It refuses a target that is not empty and checks free space first:
   ```bash
   bash oracle-reference-stack/provision_oracle_home.sh --from-copy oracle-reference-stack/oracle_home_14c --target ~/oracle_home_14c
   ```
   The copy is never a move; the original stays until you delete it. A plain `cp` would lose the ownership the containers need.
3. **Verify.** The copy step runs this automatically; run it again at any time:
   ```bash
   bash oracle-reference-stack/provision_oracle_home.sh --verify --target ~/oracle_home_14c --against oracle-reference-stack/oracle_home_14c
   ```
4. **Switch the stack to it.** Set `OFW_ORACLE_HOME=/home/<you>/oracle_home_14c` in `.env`, then recreate `forms-server-14c` with that folder as the mount source. **This needs a new container but not a new image:** a container's mounts are fixed when it is created. The script does not do this; it prints the step. The container's current settings are saved in `temp/image-backup/forms-server-14c.inspect.json` (command, mounts, network `oracle-net`, ports 7001 and 9001, environment). `oracle23ai-lite` is unaffected. Removing or recreating a container is the owner's decision.
5. **Back up:**
   ```bash
   bash oracle-reference-stack/provision_oracle_home.sh --backup temp/backups --target ~/oracle_home_14c
   ```
   `temp/` is git-ignored and on the Windows drive, so the archive survives losing the Ubuntu distro.
6. **Rebuild from scratch** (if both copies are lost): `provision_oracle_home.sh --from-installers --target PATH` runs the two silent installs, creates the domain, and applies the configuration. **Unverified on the Ubuntu disk.**

## How to test that it was copied correctly

`--verify` reports these, in order. Read them like this:

| Line | Meaning | Pass when |
|---|---|---|
| `OK <file>` / `MISSING <file>` | Ten required files exist (`frmcmp_batch`, `frmweb`, `rwconverter`, `frmsal.jar`, `frmxmltools.jar`, `libfrmjapi.so.0`, `startWebLogic.sh`, `setUserOverrides.sh`, `boot.properties`, `tnsnames.ora`) | all OK |
| `frmweb has no missing libraries` | `ldd` on `frmweb` shows nothing `not found` | OK |
| `target : files=N links=N bytes=N tree=HEX` | A fingerprint of the copy: counts, total bytes, and a hash of the sorted (path, type, size) list. Independent of owners and timestamps | informational |
| `source : files=... tree=...` | The same for the source (with `--against`) | informational |
| `tree fingerprints are IDENTICAL` | Source and copy have the same files, symlinks, and sizes | IDENTICAL |
| `target : walked N files in Ns` and `source : walked N files in Ns` | A timed walk of each tree. This is a **direct measure of the disk difference** that slows WebLogic | the target is much faster |
| `VERIFIED.` | Every check above passed | VERIFIED |

What it does **not** prove is that WebLogic boots from the copy. That final test is to recreate `forms-server-14c` on the new folder, run `python3 start_testbed.py`, and read the "Forms ... took" time in its summary. Compare it with the 15 to 44 minutes recorded before, and write the numbers in the change log.

Extra checks after the stack runs from the new folder: `python3 plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts/test_conversions.py` (converters use `OFW_ORACLE_HOME` automatically), and `python3 plugins/oracle-forms/.apm/skills/setting-up-oracle-testbed/scripts/health_check.py`.

## Risks and unknowns

- **Not yet run.** The copy, fingerprint, and backup commands use only standard tools in the image (`tar`, `find`, `sha256sum`), but the first run is the test. Record what it reports.
- **Ownership.** Files created by the containers are owned by Podman's mapped user IDs, so the host user cannot read them directly (`podman unshare` is needed). On ext4 that stays true. The copy preserves it.
- **Host paths inside the domain.** The WebLogic domain should contain only in-container paths (`/oracle_home_14c/...`), so moving the folder should not break it. **Not checked.**
- **Same filesystem rules.** Ubuntu's disk is ext4, so symlinks and execute bits work without the `metadata` mount option that `/mnt/c` needs.
- **The repo itself is also on `/mnt/c`.** Git and file reads pay the same cost. Moving it is a bigger change and is not part of this.

For an existing macOS installation, check the running container’s actual Oracle Home bind mount before choosing `OFW_WORK_DIR`. The current compiler requires a work directory outside the checkout and the pipeline needs the directory visible to the container. An Oracle Home inside the checkout cannot satisfy both conditions using the default `<Oracle Home>/ofw-work` path. Provision and verify a copy of Oracle Home outside the checkout before switching the mount.

### Check the active mount before troubleshooting XML conversion

`OFW_ORACLE_HOME` and `OFW_WORK_DIR` configure the host tools. Podman stores the bind mount when it creates the Forms container. Changing the settings or restarting the container leaves the original mount in place.

1. Compare the absolute `OFW_ORACLE_HOME` setting with the source of the `/oracle_home_14c` mount. Use `podman inspect forms-server-14c --format '{{json .Mounts}}'`.
2. Verify the external Home includes the Oracle binaries and WebLogic domain. Keep `OFW_WORK_DIR` beneath that Home so host staging maps to `/oracle_home_14c/ofw-work`.
3. With the owner's approval, recreate the Forms container using the verified Home and current checkout mounts. The database can keep running. Check the servlet and confirm the staged XML is readable inside the replacement before retrying compilation.

### Replace the Forms container after a configuration change

For a Compose-managed setup using the current Compose image and network, load the updated host settings and recreate just the Forms service from the repository root:

```sh
set -a
source .env
set +a
podman compose -f oracle-reference-stack/compose.yaml up -d --no-deps --force-recreate forms-server-14c
```

Before replacing a legacy container, compare its image, ports and network with the current Compose file. Reuse its known-good image and network if those differ and the current image is unavailable. Check for required packages installed only in the old container's writable layer. Verify the external Home against the image that will run it. For a rollback option, stop and rename the old container before creating its replacement. Once the new mount, servlet and database reachability are confirmed, remove the stopped backup container by its exact name. Keep persistent Oracle Home and application data on host mounts.

The provisioner uses a portable disk-space check on macOS and WSL. If a Podman-based copy encounters file or symlink permission errors on macOS, a host-side clone into a fresh external directory with `cp -ac` can preserve the tree. Run the provisioning verifier on that copy before using it.

### Use the same container layout on macOS and WSL

Set `OFW_ORACLE_HOME` to an absolute path outside the checkout on each host and put `OFW_WORK_DIR` beneath it. Use the Mac's filesystem with Podman Machine and Rosetta on Apple Silicon. Use Ubuntu's filesystem with Podman inside WSL2 on Windows. Both configurations map Oracle Home to `/oracle_home_14c` and use the same staging and application deployment layout.

The shared `Containerfile.14c` already declares `libaio` and `libnsl2`. Verify the actual image used for replacement. A container created from an older image can have packages installed only in its writable layer. Before switching, run `provision_oracle_home.sh --verify` with the intended image and confirm the library check passes. After switching, check the staged XML inside the container and confirm Forms servlet and database connectivity. A Workbench browser refresh only verifies the Rust UI connection.

### Image and container naming

Use `oracle-forms-14c:latest` for the 14c image on both hosts. The optional 12c converter remains a separate image. To rename an existing Forms container, use `podman rename <current-name> oracle-forms-14c` and set `OFW_FORMS_CONTAINER=oracle-forms-14c` in the same checkout's `.env`. Compose, startup/status, Workbench preview, conversion, and compilation use that setting. For manual Podman commands, substitute the configured container name. Leaving it empty preserves `forms-server-14c` for existing WSL and Mac installations. The Compose service key remains `forms-server-14c`.

An image retag keeps the existing image contents. A running container may still show the image name recorded when it was created. Verify its image ID and current tags to tell whether it uses the shared image build or a retagged legacy image.
