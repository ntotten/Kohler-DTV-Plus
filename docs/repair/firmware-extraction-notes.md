# Firmware extraction

How to get the controller's own software off a K-99695 — for bug analysis,
patching, or building a replacement. This documents what is confirmed, what
is ruled out (and why), and the paths that remain.

**Status:** extraction not yet completed by us. The filesystem map, the
HTTP-boundary proof, and the service survey below are confirmed on a
production unit running 0.0.3.89; the hardware paths are specified but not
yet executed. Contributions welcome.

## The target

`a:\images\dtvplus2_app_v0.0.3.89.S19` — 4,715,750 bytes of Motorola
S-records, ~2 MB of binary, all records targeting RAM ≥ `0x40500000`. It is
the **complete runtime image**: MQX kernel, RTCS, filesystem drivers, all
application tasks and CGI handlers, and the web UI's static content (the HTTP
document root is compiled in as TFS). It does _not_ include the bootloader
(internal flash / NAND blocks 0–499), the config/calibration/keys region
(~block 50), the UI images (sibling files), or valve firmware (on the
valves). Full breakdown:
[../docs/firmware-and-updates.md](../firmware-and-updates.md).

For most purposes — understanding the crashes, the bus protocols, or the
CGI surface — this one file is everything that executes.

## Confirmed on a live unit (0.0.3.89)

All probes were single, spaced, read-only requests (method note in
[AGENT.md](../../AGENT.md)):

1. `files.cgi` enumerates the SafeFAT volume. Every unit carries the same
   `a:\images\` staging directory with the controller app, both UI images,
   and `versions.txt`.
2. `files_available.cgi` returns the firmware inventory, including the
   valve-reflash slots.
3. `control.html` serves (200) — the web UI is alive and is TFS-resident.
4. `/images/versions.txt`, `/corys.txt`, and `/a/images/versions.txt` all
   return 404 — **the HTTP document root is not the SafeFAT volume**, and no
   URL alias to it exists.
5. TCP probes: no FTP server (port 21 closed), no telnet/shell (port 23
   closed) — consistent with the known firmware task list, which has no
   server tasks beyond HTTP.

## Why the HTTP server cannot give you the files

We reviewed the actual server source — Freescale's MQX 3.8 RTCS `httpd`
(mirror linked in [../reference/links.md](../../research/reference-links.md)), the exact
version family this firmware runs:

- The URL sanitizer (`httpd_sanitiseurl`) collapses `//`, strips `/./`, and
  strips `/../` — and it is implemented correctly (the rewind pointers cannot
  underflow). No traversal.
- The request path is **never percent-decoded**, so `%2e%2e%2f` reaches
  `fopen` literally and simply misses.
- The served path is `strcpy(root) + strcat(path)` with `/`→`\` conversion,
  and MQX's I/O layer binds the volume by prefix (`tfs:`) **before** path
  resolution — so even a surviving `..` could not cross from the TFS volume
  to `a:\`.
- The server supports per-prefix aliases, but none are configured (probes
  4–5 above).

So the firmware files sit on a volume the web server cannot see, behind a
CGI table that has no file-download entry point. The ~68 known CGIs were
enumerated from firmware strings upstream; blind name-guessing for a hidden
`download.cgi` has low expected yield.

## Ruled out, with reasons

| Idea                                                                    | Why it fails                                                                                                                                                                                                                                                                                                   |
| ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Static GET / traversal of `a:\images\*.S19`                             | Above — proven impossible in source and observed empirically                                                                                                                                                                                                                                                   |
| FTP / telnet services                                                   | Ports closed; no such tasks in the firmware task list                                                                                                                                                                                                                                                          |
| Public copies of the firmware                                           | Update server NXDOMAIN; nothing in the Wayback Machine, git histories, or code search                                                                                                                                                                                                                          |
| Hidden file-read CGI                                                    | The endpoint list comes from firmware-string analysis upstream; near-complete                                                                                                                                                                                                                                  |
| `edit_dt.cgi` out-of-bounds datatable reads                             | Plausible primitive, but ~1 byte/request at safe pacing is useless for a 4.7 MB image; crash-prone                                                                                                                                                                                                             |
| Long-URI heap non-termination (`strncpy(path, cp, max_uri)`, `httpd.c`) | Real bug class, but each failed attempt likely costs a task exception — a power cycle per datapoint                                                                                                                                                                                                            |
| Serving crafted firmware via the update flow                            | It's an _install_ path, not extraction — and risks replacing the only working copy                                                                                                                                                                                                                             |
| `unpack_bin.cgi` archive traversal                                      | SafeFAT is FAT: no symlinks; write-only primitive                                                                                                                                                                                                                                                              |
| CVE-2021-22680 (BadAlloc, MQX `mem_alloc`)                              | Needs an attacker-sized allocation; none exists in the reviewed network-edge code (session buffers are fixed-size; the FTP client's response reader is bounded). A trigger could hide in Kohler's own CGI handlers, which are unreviewable without the binary — circular. Held in reserve, never a first move. |

## The paths that remain

Ranked by (probability × yield) ÷ risk:

1. **Serial console — J904.** The board has a 4-pin header footprint
   (top-left, near the RS-485 transceivers) consistent with a console UART.
   A 3.3 V USB-UART cable, 115200 8N1 to start. If the MQX shell or RTCS
   debug console was left enabled, this is interactive filesystem access —
   `dir`/`type`/`copy` on SafeFAT, and potentially an outbound FTP `put` of
   the image to a server you run. Even pure boot-log output is valuable
   intel. Cheapest real attempt; try before BDM.
2. **ColdFire BDM — J201.** The unpopulated 2×13 footprint on the right side
   matches Freescale's standard 26-pin ColdFire BDM header. With a P&E Micro
   Multilink/Cyclone (or a TBLCF DIY build): halt the CPU, then dump
   (a) the 16 MB external SRAM containing the _running_ application,
   (b) the 256 KB internal flash = **bootloader + TFS recovery app**,
   (c) NAND contents via the CPU's NAND controller. Guaranteed and
   non-destructive. Photograph your board against the public reference photo
   (linked in [../docs/controller-hardware.md](../controller-hardware.md))
   before buying hardware.
3. **Update-flow interception (intelligence only).** On a LAN whose DNS you
   control, point the update hostname at your own FTP server and observe:
   the device logs in with its hardcoded password (cleartext capture) and
   fetches `versions.txt`. Serve a manifest reporting "no update" so nothing
   is downloaded. This yields the credential and the URL scheme — it does
   **not** yield firmware. Never serve the device an unverified image.
4. **Donor unit + chip-off.** Used K-99695 boards/units appear on the usual
   resale channels. The NAND dump procedure (T48/TL866-class programmer,
   320–340 °C rework) is documented upstream; doing it on a donor carries
   zero risk to an installed system.
5. **Ask the community.** The depth of the upstream analysis (task names,
   FlexBus addresses, NAND block maps) indicates firmware has been read out
   before. A polite ask costs nothing.

## After you have the image

- Strip S-records (all addresses ≥ `0x40500000`) to a raw binary.
- Disassemble as ColdFire V4 (ISA_A+). Ghidra's 68k module covers most of
  it; expect to annotate ColdFire-specific instructions.
- High-value first targets: the CGI table (the true endpoint list), the RTCS
  leak path behind `NETWORK_TASK_ABORT` (146), the Prompt3 polling and
  30-minute timer behavior, and the exact Saturn framing per valve
  generation.
- The bootloader (only via BDM/chip-off) answers: S19 validation, the TFS
  recovery web server, and the password-protected mode.

## Related

- [../SECURITY-REPORT.md](../security-report.md) — the security framing,
  including what was verified _not_ to work.
- [../docs/controller-hardware.md](../controller-hardware.md) — board
  detail, debug footprints, LED codes.
- Upstream NAND-recovery and boot-process guides — linked from
  [../reference/links.md](../../research/reference-links.md).
