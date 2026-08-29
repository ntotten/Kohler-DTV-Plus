# Firmware Extraction

How to get the controller's own software off a K-99695 — for bug analysis, patching, or building a replacement. This documents what is confirmed on a live unit (firmware 0.0.3.89), what is ruled out and why, and the paths that remain.

> **Status:** community work in progress. The filesystem map, the HTTP boundary proof, and the service survey below are confirmed on a production unit. The hardware paths are specified but not yet executed end-to-end.

---

## What the target file is — and isn't

`a:\images\dtvplus2_app_v0.0.3.89.S19` (4,715,750 bytes) is the **complete runtime image**: MQX kernel, RTCS TCP/IP, filesystem drivers, all application tasks and CGI handlers, and the web UI's static content (the HTTP document root is a read-only TFS compiled into this image). Roughly 4.7 MB of S-record ASCII is about 2 MB of binary; all S3 records target RAM addresses >= `0x40500000`.

| Included in the .S19 | NOT included |
|---|---|
| MQX 3.8 kernel + BSP | Bootloader (MCF54416 internal flash + NAND blocks 0-499 backup) |
| RTCS + HTTP server + CGIs | Config / calibration / keys (~NAND block 50) |
| All application tasks, bus drivers | UI images (`ui_amulet_*.S19`, `dtvplus2_uiapp_*.pack.tar`) |
| Web UI static content (TFS docroot) | Valve firmware (on the valve MCUs) |

For most purposes — the crash modes, the bus protocols, the CGI surface — this one file is everything that executes. The bootloader is separately interesting (S19 validation, the TFS recovery web server, the password-protected mode) but requires BDM or chip-off.

---

## Why the HTTP server cannot give you the files

The filesystem is visible (`files.cgi` enumerates `a:\images\`), but no endpoint downloads from it. This is structural, not an oversight — verified both empirically and by source review of MQX 3.8's RTCS `httpd` (the exact stack and version family the firmware runs; a public mirror is linked under [references](#references)):

| Property of MQX 3.8 `httpd` | Consequence |
|---|---|
| `httpd_sanitiseurl` collapses `//` and strips `/./` and `/../` (implemented correctly — the rewind pointers cannot underflow) | No `../` traversal |
| The request path is never percent-decoded | `%2e%2e%2f` reaches `fopen` literally and simply misses |
| Served path is `strcpy(root) + strcat(path)` with `/` -> `\` conversion, and the MQX I/O layer binds the volume by prefix (`tfs:`) **before** path resolution | Even a surviving `..` cannot cross from TFS to `a:\` |
| Per-prefix alias support exists (`root_dir[].alias`) | None configured — confirmed by probing |

Empirical confirmation on 0.0.3.89 (single spaced requests, read-only):

| Probe | Result |
|---|---|
| `GET /control.html` | 200 — docroot healthy, TFS-resident |
| `GET /images/versions.txt` | 404 — docroot is not `a:\`, no `images` alias |
| `GET /corys.txt` (a file at the root of `a:\`) | 404 — docroot is not `a:\` root |
| `GET /a/images/versions.txt` | 404 — no `a` alias |

## Service survey (0.0.3.89)

| Port | Service | Status |
|---|---|---|
| 80/tcp | MQX HTTP | Open (the only service) |
| 21/tcp | FTP | Closed |
| 23/tcp | Telnet / MQX shell | Closed |

The closed services match the firmware's known task list — there are no ftpd or telnetd tasks.

---

## Ruled out, with reasons

| Idea | Why it fails |
|---|---|
| Static GET / traversal of `a:\images\*.S19` | Proven above, in source and empirically |
| FTP / telnet / shell servers | Not running (see survey) |
| Public copies of the firmware | Update server is decommissioned (NXDOMAIN); nothing in the Wayback Machine, public git histories, or code search as of 2026 |
| Hidden file-read CGI | The known endpoint set was enumerated from firmware strings; blind name-guessing has low expected yield |
| `edit_dt.cgi` out-of-bounds datatable reads | Plausible primitive but ~1 byte per request at safe pacing is useless for a 4.7 MB image, and crash-prone |
| Long-URI heap non-termination (`strncpy(path, cp, max_uri)` in `httpd.c`) | Real bug class, but every failed attempt likely costs a task exception — a power cycle per datapoint |
| Serving crafted firmware via the update flow | An *install* path, not extraction — and it risks replacing the only working copy |
| `unpack_bin.cgi` archive traversal | SafeFAT is FAT: no symlinks; write-only primitive |
| CVE-2021-22680 ("BadAlloc", MQX `mem_alloc` integer overflow) | Needs an attacker-sized allocation; none exists in the reviewed network-edge code (session buffers are fixed-size; the FTP client response reader is bounded). A trigger could hide in Kohler's own CGI handlers — which are unreviewable without the binary. Held in reserve. |

---

## Path 1: Serial console (J904) — try this first

The board has a 4-pin header footprint (top-left, near the RS485 transceivers) consistent with a console UART. See [hardware.md](../hardware.md#factory-debug-and-service-access) for the footprint map.

| Parameter | Value |
|---|---|
| Header | J904 (4-pin) |
| Level | 3.3 V (verify before connecting) |
| Starting guess | 115200 8N1 (same as the UI link); walk down standard rates if garbage |

If the MQX shell or RTCS debug console was left enabled, this is interactive filesystem access — `dir` / `type` / `copy` over SafeFAT, and potentially an outbound FTP `put` of the image to a server you run. Even pure boot-log output is valuable.

## Path 2: ColdFire BDM (J201) — guaranteed

The unpopulated 2x13 (26-pin) footprint on the right side of the board matches Freescale's standard ColdFire BDM header. With a P&E Micro Multilink/Cyclone (or a TBLCF DIY build):

1. Halt the CPU.
2. Dump the 16 MB external SRAM — the running application (loaded at `0x40500000`).
3. Read the 256 KB internal flash — the bootloader plus the TFS recovery app.
4. Read NAND contents via the CPU's NAND controller.

Non-destructive and complete. Photograph your board against `Images/KohlerBoardOverall.png` first to confirm the footprints match.

## Path 3: Donor unit + chip-off

Used K-99695 boards appear on resale channels. The NAND dump procedure (T48/TL866-class programmer, 320-340 C rework) is in [nand-flash-recovery.md](../../research/xagon0/docs/repair/nand-flash-recovery.md) — doing it on a donor carries zero risk to an installed system.

## Path 4: Update-flow interception (intelligence only)

On a LAN whose DNS you control, point the update hostname at your own FTP server and observe the check: the device logs in with its hardcoded credentials (cleartext capture) and fetches `versions.txt`. This yields the credential and URL scheme.

> **CRITICAL:** Serve only a manifest reporting "no update". Never serve the device an image you have not verified — the install path has no signature verification beyond CRC32. See [firmware-files.md](../../research/xagon0/docs/repair/firmware-files.md) and [../firmware-update.md](../firmware-update.md).

---

## After extraction

| Step | Notes |
|---|---|
| Strip S-records | All addresses >= `0x40500000`; produce a raw binary |
| Disassemble | ColdFire V4 (ISA_A+); Ghidra's 68k module covers most of it — annotate ColdFire-specific instructions |
| First targets | The CGI table (true endpoint list), the RTCS leak path behind `NETWORK_TASK_ABORT` (146), Prompt3 polling and the 30-minute timer, exact Saturn framing per valve generation |

---

## References

- MQX 3.8.1 source mirror (reviewed: `rtcs/source/httpd/httpd.c`, `httpd_supp.c`; `rtcs/source/apps/ftpclnt.c`): [wk2325272/MQX_3.8.1](https://github.com/wk2325272/MQX_3.8.1)
- Board photograph with the debug footprints: [Images/Images.md](../../research/xagon0/Images/Images.md)
- CVE-2021-22680: [NVD](https://nvd.nist.gov/vuln/detail/CVE-2021-22680), [CISA ICSA-21-119-04](https://www.cisa.gov/uscert/ics/advisories/icsa-21-119-04)
