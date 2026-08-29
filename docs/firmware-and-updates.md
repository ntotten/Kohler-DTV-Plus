# Firmware files, boot, and the update pipeline

## The firmware inventory

Every controller carries its full staged firmware set in `a:\images\`
(enumerable via `files.cgi` / `files_available.cgi`):

| File                               | Component                       | Size (0.0.3.89-era set)                  |
| ---------------------------------- | ------------------------------- | ---------------------------------------- |
| `dtvplus2_app_v*.*.*.*.S19`        | Controller application          | 4,715,750 B (`v0.0.3.89`, 9,214 lines)   |
| `ui_amulet_v*.*.*.*.S19`           | Amulet (V1) touchscreen UI      | 12,992,824 B (`v0.1.3.72`, 47,769 lines) |
| `dtvplus2_uiapp_v*.*.*.*.pack.tar` | Linux (V2) touchscreen UI pack  | 6,440,960 B (`v0.0.7.44`)                |
| `versions.txt`                     | Update manifest from the server | 171 B                                    |
| `temp.txt`                         | scratch                         | 16 B                                     |

ECO-variant units use `eco_dtvplus2_app_v*.S19` / `eco_ui_amulet_v*.S19`.

Known controller builds: **0.0.3.56** (hospitality reference), **0.0.3.89**
(common production), **0.0.3.90** (patched release). Datatable version 65.

### What `dtvplus2_app_*.S19` is — and isn't

The controller app file is the **complete runtime image**: MQX kernel, RTCS
TCP/IP, filesystem drivers, all application tasks, all CGI handlers, and the
web UI's static content (the HTTP document root is a read-only TFS compiled
into this image). ~4.7 MB of S-record ASCII ≈ a ~2 MB binary; all S3 records
target RAM addresses ≥ `0x40500000`.

It does **not** contain: the bootloader (MCF54416 internal flash, plus a
backup in NAND blocks 0–499), the config/calibration/keys region (~block 50),
the UI images (separate files), or valve firmware (on the valves).

## The update matrix

`ftp_status.cgi` exposes every component the updater can stage — the full
field set is: controller app (`ftp_ctl_image_size`), Amulet UI
(`ftp_ui_image_size`), Linux UI pack (`ftp_ui_app_file`), eight UI resource
(`rfs`) files, UI language, UI touch, UI **coprocessor**, Prompt 2 and
Prompt 3 **valve flash and EEPROM**, and LightBridge. The update system is
not just for the controller — it can reflash nearly every MCU in the system.

## Boot flow

```
Power on
  → MCU internal-flash bootloader
  → NAND reset, read chip ID
  → chip ID mismatch?  → infinite hang, no network, no bypass
  → mount SafeFAT (formats if unformatted)
  → find a:/images/dtvplus2_app_v*.*.*.*.S19
  → not found? → fall back to TFS recovery (below)
  → validate: per-line checksums, CRC32 from the S0 header,
              all S3 addresses ≥ 0x40500000
  → CRC bad? → slow-blink LED, will not boot
  → load to RAM, jump to application
```

**TFS** is a read-only filesystem compiled into the bootloader binary
carrying `/default.S19` and `/eco_ui_default.S19`. A completely blank NAND is
still recoverable: the bootloader boots the built-in recovery app, which
serves a minimal web interface for uploading firmware. The NAND chip-ID check
is hard-coded with no bypass — a wrong chip hangs forever.

The bootloader also has a password-protected mode; the password is unknown to
the community. (Upstream known-issue #8.)

## The over-the-air update pipeline

Two triggers: `check_updates.cgi` arms an immediate check, and the controller
computes a daily check time from its serial number —
`(first_4_digits_of_serial % 144) × 10` minutes after midnight — spreading
the fleet across 144 ten-minute slots.

Once triggered:

1. Confirm no image write/download is in progress.
2. Verify connectivity: resolve `*.kohler.com` (via hardcoded 8.8.8.8) and
   ping the result.
3. Open FTP (plaintext, port 21) with username **`ftpuser`**, and enter the
   update folder: **`00`** for standard consumers, or a custom folder for
   hospitality/hotel deployments (configured in a datatable string).
4. Download and parse `versions.txt`; stash the server's controller/UI
   version strings.
5. Compare against installed versions; if newer, download the image(s).
6. Fire the image-swap event — install happens on next boot.

**As of 2026 the update server (`pr0d3ct-upd.kohler.com`) is decommissioned**
(DNS returns NXDOMAIN). No DTV+ can ever update again over the air; the
check fails silently. Consequences and mitigations are in
[../SECURITY-REPORT.md](security-report.md).

## Firmware upload (manual)

- Upload order matters: **controller app first, UI second, Linux UI pack
  last**, then power cycle.
- `fileupload.cgi` takes a multipart POST (field `myfile`);
  `unpack_bin.cgi` unpacks an uploaded `.tar` into the filesystem.
- **No signature verification has been observed** — validation is the
  bootloader's CRC32 + address-range check. A truncated or corrupt file fails
  CRC and the unit will not boot (slow-blink LED). A crafted file that
  _passes_ CRC would run. Treat both endpoints as brick-class.
- Recovery from a bad flash is the TFS recovery web UI; recovery from a dead
  NAND is chip-off reprogramming (upstream guide linked in
  [../reference/links.md](../research/reference-links.md)).

## Related reading

- [api-reference.md](web-interface/api-reference.md) — `files.cgi`, `files_available.cgi`,
  `ftp_status.cgi`, `check_updates.cgi`, upload endpoints.
- [../research/firmware-extraction.md](repair/firmware-extraction-notes.md) —
  getting these files **off** the controller.
- [wall-interface.md](devices/wall-interface.md) — how UI packs travel from the
  staging directory to the panel (MD5-verified chunked transfer).
