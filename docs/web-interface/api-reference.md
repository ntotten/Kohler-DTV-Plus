# Kohler DTV+ System Controller — Local API Reference

Reverse-engineered API documentation for the **Kohler K-99695-NA DTV+
System Controller Module**.

**Status:** community reference; not endorsed by Kohler. Built from
direct testing, source review of the device's web UI, and prior
reverse-engineering work by [Tim Ellery](https://github.com/timelery/Kohler-DTV-Plus).

OpenAPI specification: [`../openapi.yaml`](openapi.yaml)

> This document previously lived at the repository root. It was moved to
> `docs/` when the repo grew into a full research archive — see the
> [root README](../README.md) for the map.

---

## Table of contents

1. [Quick start](#quick-start)
2. [Authentication](#authentication)
3. [Conventions and gotchas](#conventions-and-gotchas)
4. [System endpoints](#system-endpoints)
5. [Files endpoints](#files-endpoints)
6. [Configuration endpoints](#configuration-endpoints)
7. [Hardware control endpoints](#hardware-control-endpoints)
8. [Updates](#updates)
9. [Error logs](#error-logs)
10. [Reset / recovery](#reset--recovery)
11. [Bluetooth](#bluetooth)
12. [REST API](#rest-api)
13. [Internal / unclassified](#internal--unclassified)
14. [Confidence levels](#confidence-levels)

---

## Quick start

```bash
# Find your controller
arp -a | grep '0:14:6f'   # Kohler MAC OUI

# Read identity (safe)
curl http://192.168.1.100/system_info.cgi

# List firmware files on the device
curl http://192.168.1.100/files_available.cgi
curl http://192.168.1.100/files.cgi

# Read error logs (safe)
curl http://192.168.1.100/cerror_logs.cgi
```

> ⚠️ `serial.cgi` and `mac.cgi` are omitted on purpose: both are documented
> upstream as able to **lock the system up**, and the same values are already
> present in `values.cgi`. See the caution list below.

---

## Authentication

**There is none.**

Every endpoint accepts unauthenticated GET (or POST) from any host on
the LAN. There is no API key, no session cookie, no CSRF token, no
referer check, no origin restriction.

This means:

- Anyone on your local network can issue any of these calls.
- A web page in a browser on your LAN, loaded from any origin, can
  issue these calls (no CORS protection).
- This includes the [hardware actuator endpoints](#hardware-control-endpoints)
  that physically operate the shower.

**Owner mitigation:** put the controller on a VLAN that only your
admin host can reach. See [`../SECURITY-REPORT.md`](../security-report.md).

---

## Conventions and gotchas

### Rate limiting

The MQX RTOS HTTP server has a small, fixed-size socket pool.
**Bursting requests crashes the controller.** Treat **1 request
per second** as the practical maximum. If you need to scrape multiple
endpoints, sleep between calls (8 s was used for the careful read-only
captures in this project). A crashed controller requires a manual power
cycle.

### Method support

The web server supports `GET` and `POST`. `HEAD` is **not** supported
— calls with `-I` to curl will return 404 even on existing endpoints.
Use plain `curl`.

### Response formats

- `.cgi` endpoints answer in **HTTP/0.9** — a bare body, no status line,
  no headers. Node `fetch` and Python `requests` reject it outright;
  curl needs `--http0.9`.
- Static files (`.html`, `.js`, `.png`) get a normal `HTTP/1.0 200 OK`.
- Most `.cgi` endpoints return JSON; some return plain text
  (`/files.cgi`, `/cerror_logs.cgi`, `/serial.cgi`, `/mac.cgi`).
- Some replies are Python-`repr`-flavored rather than strict JSON
  (`True`/`False`/`None`, single quotes). Parse defensively.
- Error responses are HTML: a ~109-byte `<HTML>...404 Not Found...</HTML>`.
- Many CGI responses omit `Content-Length`; read until the socket closes.

### URL parameters

Query params on CGI endpoints are passed as standard `?name=value`
pairs. The web UI also adds a cache-buster `_=<timestamp_ms>` to every
GET; this is jQuery's default and the device tolerates / ignores it.

### Destructive endpoints

Several endpoints mutate or destroy state on plain GET with no
confirmation. These are clearly marked ⚠️ in the sections below. Pay
attention.

---

## System endpoints

Read-only identity and status.

### `GET /serial.cgi`

Returns the controller's serial number as plain text.

> ⚠️ **Caution:** documented upstream as able to cause system lockups,
> and observed to return empty on current hardware. The serial is also
> visible via `values.cgi`. Avoid.

### `GET /mac.cgi`

Returns the controller's MAC address.

> ⚠️ **Caution:** same lockup report as `serial.cgi`. The MAC is present
> in `values.cgi`; use that instead.

### `GET /system_info.cgi`

Returns a JSON object with system metadata. Full schema not
characterized — varies by firmware version. Used by the settings UI on
load; the wall interface polls the equivalent state over its own link.

### `GET /values.cgi`

Returns the current values of indexed device variables (~300+ keys).
The settings UI polls this periodically as part of live state refresh.

Note: `values.cgi` carries **setpoints**, not measured water
temperature. No CGI endpoint surfaces the valve thermistor reading; the
controller only pushes it to the wall interface over the datatable
(`DT_W_Temperature`).

### `GET /landing_url.cgi`

Returns `{"url": "settings.html"}` (or another target). Called by
`index.html` on first load to determine where to redirect.

### `GET /languages.cgi`

Returns the list of UI languages supported by the controller.

### `GET /id_interface.cgi`

Returns information identifying the connected UI interface module.
Behavior not fully characterized.

### `GET /powerclean_check.cgi`

The settings UI polls this periodically as a heartbeat.

> ⚠️ **Caution:** despite the name, this can _trigger_ the steam
> power-clean cycle, not merely report it. Avoid on systems with steam.

---

## Files endpoints

### `GET /files.cgi`

Returns a directory listing of the device's filesystem (drive `a:\`),
rendered as simple HTML.

Every production controller shows the same shape:

```text
a:\
corys.txt                         size 144 bytes
\images
    temp.txt                      size 16 bytes
    dtvplus2_app_v0.0.3.89.S19    size 4715750 bytes
    ui_amulet_v0.1.3.72.S19       size 12992824 bytes
    versions.txt                  size 171 bytes
    dtvplus2_uiapp_v0.0.7.44.pack.tar  size 6440960 bytes
data_table.txt                    size 10221 bytes
data_table_default.txt            size 10221 bytes
\backup
```

**Note:** there is no corresponding _download_ CGI, and the web server's
document root is a read-only filesystem compiled into the firmware — it
cannot reach `a:\`. This endpoint enumerates filenames only. Extraction
requires hardware access; see
[`../research/firmware-extraction.md`](../repair/firmware-extraction-notes.md).

### `GET /files_available.cgi`

Returns the firmware images currently installed in `\images`.

```json
{
  "dtv2_app": "dtvplus2_app_v0.0.3.89.S19",
  "dtv2_app_size": "4.497 MB",
  "ui_coldfire": 0,
  "ui_amulet": "ui_amulet_v0.1.3.72.S19",
  "ui_amulet_size": "12.390 MB",
  "ui_language": 0,
  "prompt2_eeprom": 0,
  "prompt2_flash": 0,
  "prompt3_eeprom": 0,
  "light_bridge": 0,
  "prompt3_flash": 0
}
```

`*_size` fields are human-readable strings, not byte counts. Modules
with value `0` are not installed. The `prompt2_*` / `prompt3_*` fields
show the update system can also stage **valve** firmware.

---

## Configuration endpoints

Read and write the device's configuration tables.

### `GET /save_variable.cgi?index=N&value=V`

The most-called write endpoint. Used by every form input in the
settings UI to persist a single indexed variable (indices 1–105).

```bash
# Set variable index 42 to value "300"
curl 'http://192.168.1.100/save_variable.cgi?index=42&value=300'
```

The index space is opaque. The variable-ID table is defined twice —
once in controller firmware, once in the UI's JavaScript — and the two
must stay in sync across updates. See the mirrored `settings.js`
(upstream, linked in [../reference/links.md](../../research/reference-links.md)).

### `GET /edit_dt.cgi`

Direct datatable access. **Omitting `value` reads; providing it writes.**

| Parameter | Description                                                    |
| --------- | -------------------------------------------------------------- |
| `type`    | `0` byte, `1` word, `2` color, `3` string                      |
| `page`    | `'s'`/`'S'` stationary, `'g'`/`'G'` ghost, or page number 0–29 |
| `index`   | variable index within the page                                 |
| `value`   | value to write (max 25 chars for strings) — omit to read       |

Responds `:)` on success, `:(` on invalid page. ⚠️ Rated dangerous
upstream as a write; reads are low risk.

### `GET /datatable.cgi`

Full datatable debug view (~90 KB of HTML). Read-only, low risk.

### `POST /saveDT.cgi`

⚠️ Persists the in-memory data table to `data_table.txt` on flash.
Calling without a fully populated table risks corrupting config.

### `POST /saveUI.cgi`

Persists UI-related settings.

### `POST /save_default.cgi`

⚠️ Saves the current configuration as the new "default" used by the
Reset to Default operation. **This overwrites the saved default.**

### `POST /clear_dt.cgi`

⚠️ Wipes data table state.

### `POST /set_device.cgi`

Configures simulated devices using an 11-character binary string (one
bit per simulated device family; valve-type bits are mutually exclusive
per slot). ⚠️ Dangerous.

### `POST /swapvalves.cgi`

Swaps the configured order/assignment of the two valves. ⚠️

### `POST /change_user.cgi`

Switches or modifies a user profile.

### `GET /hiding.cgi`

Related to which UI controls are visible, plus debug flags. ⚠️

### `GET /sim_dev_values.cgi`

Returns JSON describing real-versus-simulated device attachment
(`real_valve_attached`, per-valve detection, steam/rain/light/amp sim
status). Read-only.

---

## Hardware control endpoints

⚠️ **These actuate physical hardware in the user's bathroom.** Calling
any of them turns water, lights, audio, or steam on or off in real
time. They take no confirmation and execute on plain GET.

| Endpoint                  | Effect                                                                                                         |
| ------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `GET /quick_shower.cgi`   | Start a shower with the **complete desired state** (start, change outlets, and change temperature in one call) |
| `GET /start_user.cgi`     | Begin a user's saved shower preset (`?user=N`)                                                                 |
| `GET /stop_user.cgi`      | Stop the active user shower preset                                                                             |
| `GET /stop_shower.cgi`    | Stop the shower                                                                                                |
| `GET /light_on.cgi`       | Turn on shower lighting (`module`, `intensity`)                                                                |
| `GET /light_off.cgi`      | Turn off shower lighting                                                                                       |
| `GET /music_on.cgi`       | Turn on shower audio (`volume`)                                                                                |
| `GET /music_off.cgi`      | Turn off shower audio                                                                                          |
| `GET /steam_on.cgi`       | Activate steam generator (`temp`, `time`)                                                                      |
| `GET /steam_off.cgi`      | Deactivate steam generator                                                                                     |
| `GET /rain_on.cgi`        | Activate rain panel (`mode=1&color=…` solid, `mode=2&effect=…`)                                                |
| `GET /rain_off.cgi`       | Deactivate rain panel                                                                                          |
| `GET /massage_toggle.cgi` | Toggle massage mode                                                                                            |

`quick_shower.cgi` details: `valve_num` is 1 or 2; each valve takes
`valveN_outlet` (selected positions concatenated, e.g. `"134"`; empty =
none), `valveN_massage`, and `valveN_temp` **in the system's configured
unit** (°F on US systems). Massage modes as labelled by the
controller's own `control.html`: `0` off, `1` single, `2` wave,
`3`/`4` custom. (One upstream source has 1/2 swapped; the shipped UI is
the authority. Verify on your unit before relying on it.)

Configuration writes can be silently dropped while water is running:
`CGI_SHOWER_START 0x01` / `CGI_SHOWER_LOCK 0x02`.

---

## Updates

### `GET /check_updates.cgi`

Arms an update check against `pr0d3ct-upd.kohler.com`. The controller
resolves the host (via 8.8.8.8, hardcoded), then opens an FTP
connection to port 21.

**As of 2026, the configured update server is decommissioned**
(see [`../SECURITY-REPORT.md`](../security-report.md) finding 2). Calls
return success but the underlying FTP connection times out silently.

### `POST /update_change.cgi`

Modifies update behavior. Parameters not characterized. ⚠️ Avoid
unless you understand what it changes.

### `GET /ftp_status.cgi`

Returns the controller's FTP update client state.

```json
{
  "value": true,
  "internet_status": "con",
  "loc_upload_status": false,
  "upload_enable": false,
  "ftp_ctl_image_size": 4715750,
  "ftp_ui_image_size": 12992824,
  "ftp_ui_app_file": 6440960,
  "ftp_versions_file": 171,
  "ftp_lang_image_size": 0,
  "ftp_coproc_image_size": 0,
  "ftp_prompt3_flash_size": 0,
  "ftp_prompt3_eeprom_size": 0,
  "ftp_ui_rfs_file0": 0,
  "ftp_ui_rfs_file1": 0,
  "ftp_ui_rfs_file2": 0,
  "ftp_ui_rfs_file3": 0,
  "ftp_ui_rfs_file4": 0,
  "ftp_ui_rfs_file5": 0,
  "ftp_ui_rfs_file6": 0,
  "ftp_ui_rfs_file7": 0,
  "ftp_ui_lang_file": 0,
  "ftp_ui_touch_file": 0,
  "ftp_downloaded_size": 0,
  "ftp_file_count": 0,
  "ftp_current_file_count": 0,
  "ftp_file_id": 0,
  "ftp_download_error": 0
}
```

The `ftp_*_size` fields match exactly the file sizes shown in
`/files.cgi` — confirming the device transfers firmware via FTP
(plaintext, port 21). The field set is the complete update matrix:
controller app, Amulet UI, Linux UI app pack, eight UI `rfs` resource
files, UI language, UI touch, UI coprocessor, Prompt2/Prompt3 valve
flash **and** EEPROM, and LightBridge.

### `POST /fileupload.cgi`

⚠️ Multipart file upload (`multipart/form-data`, field `myfile`). Used
by the service page to push firmware images directly. Signature
verification has not been observed. **Uploaded files are unpacked and
applied.** A bad image bricks the unit (boot CRC failure → slow-blink
LED).

### `POST /unpack_bin.cgi`

⚠️ Unpacks an uploaded `.tar` archive into the device's filesystem.
Called as part of the firmware install flow. Risky to invoke outside
that flow.

---

## Error logs

### `GET /cerror_logs.cgi`

Returns the controller's error log as plain text — a 99-entry circular
buffer, persistent across power cycles.

```text
[05:05.52 p.m. 05/01/2026] 100:  UI Error
[04:17.15 p.m. 05/01/2026] 100:  Prompt3 Error
[12:31.08 p.m. 12/31/2015] 100:  DTV Error
```

Each line is `[HH:MM.SS am/pm MM/DD/YYYY] CODE: SOURCE [DESCRIPTION]`.
Entries dated `12/31/2015` or `01/01/2016` are MQX RTC defaults (used
before time sync).

Crucially, this log is **not** where valve faults live — valve errors
are current-state flags only. See
[errors-and-known-issues.md](../troubleshooting/errors-and-known-issues.md).

### `GET /kerror_logs.cgi`

Returns the K-97999 Konnect bridge error log. Empty if no bridge is
installed.

### `GET /reset_cfault.cgi`

⚠️ Empties `/cerror_logs.cgi`'s contents.

### `GET /reset_kfault.cgi`

⚠️ Empties `/kerror_logs.cgi`'s contents.

### `GET /reset_fault.cgi`

⚠️ Older alias seen in timelery's RE notes. Likely equivalent to
`reset_cfault.cgi` on this firmware version.

---

## Reset / recovery

⚠️⚠️⚠️ All of these are destructive and execute on plain GET with no
confirmation.

### `GET /reset_factory.cgi`

⚠️⚠️⚠️ **Factory reset.** Wipes ALL configuration — installed valves,
paired devices, user profiles, network settings, error logs.
**Cannot be undone.** Returns the controller to the state it was in
before installation.

### `GET /reset_default.cgi`

⚠️ Reverts configuration to whatever was last saved via
`/save_default.cgi`. Less destructive than factory reset but still
wipes any unsaved changes.

### `GET /reset_users.cgi`

⚠️ Removes all configured user profiles and their saved presets.

### `GET /reset_user.cgi`

⚠️ Resets a single user profile (parameters not characterized — likely
takes a user index).

### `GET /forget_devices.cgi`

⚠️ Clears the list of paired Bluetooth devices and external modules
(prompts, light bridges).

---

## Bluetooth

### `GET /BTKey.cgi`

Read or set the Bluetooth pairing key.

### `GET /BTPin.cgi`

Read or set the Bluetooth PIN.

### `GET /bt_disconnect.cgi`

⚠️ Disconnects any active BT-paired audio device.

---

## REST API

A newer REST surface coexists with the CGIs.

### `GET /api/v1/device/status`

Documented by timelery's RE notes. Behavior not extensively
characterized. Returns a JSON object describing device state.

There may be additional `/api/v1/...` paths in firmware that the web
UI doesn't currently call. Worth a careful enumeration.

---

## Internal / unclassified

### `POST /rpc.cgi`

Generic RPC entry point by index (`?index=N`). These are the
UI-to-controller RPCs the wall interface uses over its own bus link —
system/feature/state-change commands, not a file or memory interface.
⚠️ Rated dangerous upstream; avoid invoking blind.

---

## Confidence levels

Endpoints have varying levels of evidence behind them:

| Confidence              | Endpoints                                                                                                                                                                         | Source                                                                                                                                           |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Tested directly**     | `/files.cgi`, `/files_available.cgi`, `/ftp_status.cgi`, `/cerror_logs.cgi`, `/kerror_logs.cgi`, `/check_updates.cgi`, `/system_info.cgi`, `/powerclean_check.cgi`, `/values.cgi` | Direct GET, observed response                                                                                                                    |
| **JS source confirmed** | `/save_variable.cgi`, `/reset_default.cgi`, `/reset_factory.cgi`, `/reset_users.cgi`, `/reset_cfault.cgi`, `/reset_kfault.cgi`, `/languages.cgi`, etc.                            | `$.ajax` calls in the device's own `js/settings.js` and `js/service.js` (mirrored upstream — see [../reference/links.md](../../research/reference-links.md)) |
| **Community RE only**   | All hardware control endpoints, BT endpoints, `unpack_bin.cgi`, `update_change.cgi`, `rpc.cgi`                                                                                    | timelery/Kohler-DTV-Plus                                                                                                                         |
| **Inferred**            | `/api/v1/device/status`                                                                                                                                                           | timelery's notes mention but path may differ                                                                                                     |

Where a parameter is documented, the source is one of:

- Direct testing (parameters that worked)
- `$.ajax` `data: {...}` blocks in the device's own JS
- Conventional naming for similar consumer IoT devices

---

## Building a custom client

If you're rebuilding the UI:

1. **Use the OpenAPI spec** with `openapi-generator-cli` to scaffold
   a typed client:

   ```bash
   npx @openapitools/openapi-generator-cli generate \
     -i openapi.yaml \
     -g typescript-fetch \
     -o ./client
   ```

2. **Reference the existing UI's JS** for every parameter shape the
   controller accepts. The controller's own web UI has been mirrored
   upstream (see [../reference/links.md](../../research/reference-links.md)); it is
   your ground truth for what the controller will accept.

3. **Throttle requests** to no more than 1/sec to avoid crashing the
   controller. Implement a request queue, not parallel calls.

4. **Don't expose hardware actuators** to anyone but yourself.
   Wrapping these in your own UI without auth recreates the original
   security flaw at a different layer.

5. **Existing community libraries:**
   - [niemyjski/kohler-python](https://github.com/niemyjski/kohler-python) — Python library, working API client
   - [niemyjski/homeassistant-kohler](https://github.com/niemyjski/homeassistant-kohler) — Home Assistant integration
   - [dcmeglio/hubitat-kohlerdtv](https://github.com/dcmeglio/hubitat-kohlerdtv) — Hubitat integration

   Read these before writing your own — they've already solved many of
   the parameter-format gaps documented above.
