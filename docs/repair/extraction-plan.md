# DTV+ software extraction — plan and research

Working notes for getting the controller's firmware (`dtvplus2_app_v0.0.3.89.S19`)
and supporting software off the K-99695 at `192.168.4.80`, so bugs can be
analyzed/patched or a replacement controller built. Companion to
[system-specification.md](../system-specification.md) — that file says what the device is; this one is
about getting the code off it.

> **Target anchor.** The subject of this document is exactly one device: the
> **K-99695-NA system controller** (Kohler spec sheet:
> `research/kohler-official/K-99695_spec_US-CA_Kohler_en.pdf`; board
> PCB-0424-00-R04, ColdFire MCF54416CMJ256, MQX 3.8). The Konnect module
> (K-97999, FCC N82-KOHLER029) and the cloud-module family in
> `research/fcc/` are **sibling-product research only** — they are not
> installed on this system and are not extraction targets; they were examined
> for shared-credential patterns and as GPL-track evidence.

**Request discipline (from the operator, binding):** strictly one request at a
time, ~8 s gaps, `Connection: close`, `--http0.9` for `.cgi`. If the device
stops answering, STOP and have it power-cycled. 2026-08-25: device was
unreachable at L2 (no ARP reply, correct-but-stale ARP entry) before any
probing began — zero requests were sent to it from this work.

## Live probe results (2026-08-25, post-reboot window)

Device power-cycled by operator; healthy on first contact. All probes were
single sequential requests, `Connection: close`, ≥8 s gaps. Total requests
sent: 8 (plus 3 pings, 2 TCP connect probes).

| # | Probe | Result |
| --- | --- | --- |
| 1 | `GET /system_info.cgi` | 200, 782 B — matches baseline exactly. Healthy. |
| 2 | `GET /files.cgi` | **Full `a:\` map**: `corys.txt` (144 B), `data_table.txt`/`data_table_default.txt` (10,221 B each), `\backup\` (empty), and `\images\` containing `temp.txt` (16 B), **`dtvplus2_app_v0.0.3.89.S19` (4,715,750 B)**, **`ui_amulet_v0.1.3.72.S19` (12,992,824 B)**, **`dtvplus2_uiapp_v0.0.7.44.pack.tar` (6,440,960 B)**, `versions.txt` (171 B). |
| 3 | `GET /images/versions.txt` | 404 — docroot is NOT `a:\`; no `images` alias. |
| 4 | `GET /control.html` | 200, 17,993 B — docroot healthy; static content is TFS compiled into the firmware. |
| 5 | `GET /corys.txt` | 404 — confirms docroot is TFS, not `a:\`. |
| 6 | TCP connect `192.168.4.80:21` | closed — no FTP server. |
| 7 | TCP connect `:23` | closed — no telnet/shell server. |
| 8 | `GET /files_available.cgi` | `{"dtv2_app":"dtvplus2_app_v0.0.3.89.S19",…,"ui_amulet":"ui_amulet_v0.1.3.72.S19",…,"prompt3_flash":0,…}` — confirms inventory; controller can also reflash **valve** firmware (prompt2/prompt3 eeprom+flash slots, all 0 = none staged). |
| 9 | `GET /a/images/versions.txt` | 404 — no `a` alias. |

## What the target file is (and isn't)

`a:\images\dtvplus2_app_v0.0.3.89.S19` (4,715,750 B ≈ ~2 MB binary) is the
**complete application firmware**: MQX 3.8 kernel + RTCS + filesystem drivers
+ all Kohler application code (all tasks, bus drivers, CGI handlers),
statically linked into one RAM image (S3 records all ≥ `0x40500000`). It also
contains the **TFS web UI** — the docroot is compiled into this image, which
is why `/control.html` serves while `a:\` holds no web files (verified in
probes 4–5). Extracting this one file yields every byte of code that executes
on the controller.

Not in the file: the **bootloader** (internal flash of the MCF54416, plus a
backup in NAND blocks 0–499 — does S19 CRC validation and carries the TFS
recovery web app); the **config/calibration/keys** region (~NAND block 50);
the **wall-interface** images (`ui_amulet_v0.1.3.72.S19`,
`dtvplus2_uiapp_v0.0.7.44.pack.tar`, staged in the same `a:\images\`); and
the **valve** firmware (on the valve MCUs). The bootloader is the only piece
worth additionally extracting (recovery web server + validation logic), and
it requires BDM (internal flash) or chip-off.

## Research results

### CVE / public-vuln landscape (checked 2026-08-25, NVD + CISA + web)

| Finding | Relevance |
| --- | --- |
| **CVE-2021-22680** (BadAlloc, Microsoft Section 52, ICSA-21-119-04): integer overflow in MQX `mem_alloc`/`_lwmem_alloc`/`_partition`, "NXP MQX 5.1 and prior", CVSS 7.3 | MQX 3.8 predates 5.1 and shares the allocator code — plausibly affected. Reachability needs an attacker-sized allocation reachable over HTTP (CGI query parsing is the candidate surface). Blind heap corruption on this fragile unit is a LAST resort; a crash here means a power cycle. |
| HCC Embedded CVEs (CVE-2020-25767, -2021-31226/7/8, -2021-31400/1, -2021-36762) | All are **InterNiche/NicheStack** — a different TCP/IP stack. Ours is MQX **RTCS**. The HCC product in our device is SafeFAT/SafeFlash (filesystem), not the network stack. Do not confuse the two. |
| ColdFire / MCF5441 | Nothing applicable (only a QEMU emulator CVE). |
| Kohler / DTV | **Nothing published.** No CVEs, no public exploits. |
| Design-level weaknesses (no CVE needed) | No authentication on any endpoint; HTTP/0.9 CGI; `fileupload.cgi` accepts firmware with **no observed signature verification** (only bootloader CRC32 + address-range checks per HARDWARE.md §13); `unpack_bin.cgi` extracts an uploaded tar into the SafeFAT filesystem; `edit_dt.cgi` reads/writes the raw datatable; update client uses plaintext FTP with hardcoded user `ftpuser`. |

### Off-device firmware sources — all dead ends, confirmed

- `pr0d3ct-upd.kohler.com` (the update server named in
  `packages/api/src/kohler/client.ts`) is **NXDOMAIN** — decommissioned.
- Wayback Machine: no captures of the update host; no `.S19`/firmware
  downloads archived on kohler.com.
- Neither upstream repo (`aaronse`, `xagon0`, both re-cloned during this work)
  contains firmware images. `xagon0`'s NAND doc assumes you already have a
  donor dump or a working unit.

**Conclusion: the firmware has to come off the device (or a donor unit).**

## Extraction vectors — post-probe assessment

### Proven closed (do not retry)

- **HTTP static → `a:\`** — MQX 3.8 `rtcs/source/httpd` (source reviewed from
  the `wk2325272/MQX_3.8.1` mirror, exact version family):
  `httpd_sanitiseurl` strips `//`, `/./`, and `/../` (the sanitize is actually
  sound — `to`/`last` never underflow); there is **no percent-decoding** of the
  path (so `%2e%2e` reaches `fopen` literally and just misses); the final path
  is `strcpy(root) + strcat(path)` with `/`→`\` conversion, and MQX binds the
  volume by the `tfs:` prefix **before** path resolution, so `..` can never
  cross volumes even if it survived. Alias mechanism exists
  (`root_dir[].alias` prefix match) but probes 3/5/9 show none configured.
- **Hidden network services** — 21/23 closed; the firmware task list (15
  tasks, from xagon0's disassembly) has no ftpd/telnetd task.
- **Public firmware** — NXDOMAIN update server, no Wayback captures, nothing
  in either repo's git history, nothing on grep.app/DDG. Confirmed exhausted.
- **Hidden CGIs** — the ~68-endpoint list came from firmware string analysis
  by someone who had the binary; blind name-guessing has low expected yield.

### Remaining paths, ranked by (probability × yield) / risk

1. **Serial console, J904** (4-pin header footprint, top-left of board near
   the RS-485 transceivers). Cheapest hardware attempt: 3.3 V USB-UART cable,
   no soldering if pads accept a header. If MQX was built with the shell or
   even RTCS debug console enabled, this is a full interactive shell on the
   device — `dir`/`type`/`copy` over SafeFAT, and potentially an outbound FTP
   `put` of the .S19 files to a server we run. Verify baud 115200 8N1 (same as
   the UI link) and walk down if garbage. **This is the first hardware thing
   to try — do it before BDM.**
2. **ColdFire BDM, J201** (unpopulated 2×13 = 26-pin footprint right side —
   the standard Freescale ColdFire BDM layout; SW201 pushbutton and SW101
   slide switch nearby are likely reset/boot-mode). P&E USB Multilink
   Universal / Cyclone (or a TBLCF DIY) → halt CPU → dump: (a) the 16 MB
   external SRAM containing the *running* application (loaded at
   `0x40500000`), (b) the MCF54416's 256 KB internal flash = **bootloader +
   TFS recovery app**, (c) NAND contents via the CPU's NAND controller.
   Guaranteed, non-destructive, ~$60–$500 of probe hardware. The board is
   PCB-0424-00-R04; photograph our unit's board before buying to confirm the
   footprints match xagon0's photo.
3. **FTP-intercept intelligence op** — DNS-override the update hostname (eero
   custom DNS → dnsmasq on a LAN host), run an FTP server, call
   `check_updates.cgi` once. Captures the hardcoded FTP password in cleartext
   at our server and the exact URL scheme. Serve a `versions.txt` reporting
   "no update" so nothing is downloaded. **Does not extract firmware** — its
   value is the password string (also useful for the GPL case as evidence of
   the update mechanism). Optional; needs a network-wide DNS change.
4. **Community ask** — xagon0's docs cite MQX task names, FlexBus addresses,
   and NAND block maps that only exist in the binary: they have extracted
   this exact firmware. An issue/email asking for `dtvplus2_app_v0.0.3.89.S19`
   (or their NAND dump) costs nothing and carries zero device risk.
5. **Donor unit + chip-off** — used K-99695 controllers on eBay; NAND dump
   per xagon0's nand-flash-recovery.md on the donor, zero risk to ours.

### CVE-2021-22680 (BadAlloc) — applicability analysis

The one real CVE for this stack: integer overflow in MQX `mem_alloc`,
`_lwmem_alloc`, `_partition` (MQX ≤ 5.1; 3.8 shares the allocator). Read the
full network-facing code path before attempting: **no attacker-influenced
allocation size exists in the reviewed MQX 3.8 edge code.** Session buffers
are fixed at config (`max_uri+1`, `max_auth+1`); the line reader
(`httpd_readln`) is byte-at-a-time into a fixed buffer with a hard bound; the
FTP client's `FTP_receive_message`/`_pasv` allocate a fixed window and flush
at the boundary. BadAlloc needs a size the attacker controls; the httpd layer
never exposes one. A trigger could still exist inside Kohler's own CGI
handlers (unknowable without the binary — circular), and each attempt costs a
power cycle if it faults a task. **Verdict: held in reserve, never a first
move.** Same reasoning retired the long-URI `strncpy` non-termination idea
(httpd.c:331).

### MQX 3.8 FTP client (`rtcs/source/apps/ftpclnt.c`) — reviewed

Fixed-size window allocations, bounded byte-at-a-time reads, flush at window
boundary; PASV parser guards on comma count. **No remote-overflow primitive
from a malicious server.** The DNS-spoof/FTP-intercept plan therefore yields
credential capture + intel only, not code exec.

### Kohler sibling products (FCC public record, checked 2026-08-25)

Grantee N82 (Kohler Co.) filings: **N82-KOHLER010** DTV+ Amplifier (2014);
**N82-KOHLER021/-022** "UART/RS485 CLOUD MODULE" (2017); **N82-KOHLER032/-033**
"UART/RS485 CLOUD MODULE" (2019); **N82-KOHLER029 "DTV Konnect Module"**
(K-97999, 2019) — internal photos saved to `research/fcc/`: ARM-class SoC +
ISSI SDRAM + Kingston NAND + **microSD slot**, Wi-Fi by Laird-tested module.
That is a Linux-class board (GPL-covered) with removable storage.
No leaked credentials anywhere in the filings, manuals, S3 guesses
(`kohler-*` buckets all 404/403), or web searches. No public exploit for ANY
Kohler connected product exists as of today.

**Konnect install sheet version matrix** (1332919-2-A, saved): requires UI
99693-P-NA sw **7.44**, Eco UI 8.11, controller 99695-NA sw **3.75**, Eco
4.14. Our panel reports `0.0.7.44` = the 7.44 line = the **Linux UI pack**
(`dtvplus2_uiapp_v0.0.7.44.pack.tar`). This **resolves HARDWARE.md §14's open
question: our wall interface is the Linux variant** — relevant to the GPL
track: Kohler ships at least two Linux systems in this product family (panel
+ Konnect) and has never offered source.

### Explicitly rejected after analysis

- `edit_dt.cgi` OOB reads: plausibly an unbounded relative RAM read, but
  1 byte/request at the mandated 8 s spacing is ~435 days for the 4.7 MB
  image. Fine for a targeted secret, useless for bulk. Also crash-prone.
- Long-URI heap non-termination (`strncpy(path, cp, max_uri)` at
  httpd.c:331): real bug class, but each attempt risks an unhandled task
  exception — a power cycle per datapoint on a box we cannot auto-reboot.
- Serving crafted firmware via the update flow: it's an *install* path, not
  extraction, and risks replacing the only working copy. Never.
- `unpack_bin.cgi` tar-slip: SafeFAT has no symlinks; write-only primitive.

## After extraction

## After extraction

- `.S19` → strip records ≥ `0x40500000` (RAM image per HARDWARE.md §13) →
  raw binary → disassemble as ColdFire V4 (ISA_A+). Ghidra needs the
  `68000` with ColdFire extensions; `objdump -m m68k` partial.
- Immediate questions the binary answers: real HTTP session-pool size, the
  RTCS leak path (NETWORK_TASK_ABORT), Prompt3 polling and the
  `PROMPT3_TIMEOUT_MAX` behavior, exact Saturn framing per valve generation.
- The alternative end-state (replace the controller with a Pi + RS-485
  speaking Saturn) needs xagon0 `docs/protocols/saturn-protocol.md` plus the
  valve-side details only the binary confirms.

### DIY controller — safety ownership (verified 2026-08-25)

Scald/temperature protection is **valve-side**, which is what makes the DIY
controller path safe to pursue. The valve owns the mixing loop (no PID to
write), rejects setpoints outside 30–49 °C (`RANGE_ERROR`), trips on
over-temp at the outlet and at its own board, detects thermistor/motor/relay
faults, and fails closed on comms loss or power loss. The replacement master
must reimplement only: (1) the ≤ 113 °F setpoint clamp (same as
`MAX_SHOWER_TEMP_F` in `packages/api/src/kohler/constants.ts`), (2) fault
polling (`0x0F`) with immediate all-off on any trip, (3) deliberate
management of the Prompt3 30-min runtime timer (resets only accepted after
≥ 900 s elapsed — naive constant polling does NOT hold it off), and (4) a
one-time independent thermometer check at commissioning. Residual hardware
risk that no controller can fix: welded relay (error 35) — replace the valve.
Note for a permanent install: the valve's listing presumably covers the
assembly as shipped; a DIY master is functionally equivalent on paper but not
a listed installation.
