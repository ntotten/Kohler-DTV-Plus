# CGI Endpoints Reference

Complete reference for all CGI endpoints exposed by the Kohler DTV+ web interface.

> **Important:** The DTV+ supports only **2 concurrent HTTP sessions**. Exceeding this causes dropped connections. Many requests fail in tools like Postman or PowerShell due to header handling -- Chrome or `curl` work most reliably. Always set `cache: false` in AJAX calls. CGI responses often omit the `Content-Length` header, so read until the socket closes.

---

## Shower Control

### `quick_shower.cgi`

Start a shower with explicit valve/outlet/temperature parameters.

| Parameter | Type | Description |
|-----------|------|-------------|
| `valve_num` | `1` or `2` | Number of valves to activate |
| `valve1_outlet` | string | Outlet combination for valve 1 (e.g. `"12"`, `"123"`) |
| `valve1_massage` | int | Massage mode: `0` = none, `1` = wave, `2` = single, `3` / `4` = custom |
| `valve1_temp` | float | Target temperature in Celsius |
| `valve2_outlet` | string | Outlet combination for valve 2 (empty string if unused) |
| `valve2_massage` | int | Massage mode for valve 2 |
| `valve2_temp` | float | Target temperature in Celsius for valve 2 |

**Example -- single valve, outlets 1 and 2, 40 C, no massage:**

```
GET /quick_shower.cgi?valve_num=1&valve1_outlet=12&valve1_massage=0&valve1_temp=40&valve2_outlet=&valve2_massage=0&valve2_temp=38
```

**Example -- dual valve:**

```
GET /quick_shower.cgi?valve_num=2&valve1_outlet=1&valve1_massage=1&valve1_temp=41.5&valve2_outlet=23&valve2_massage=0&valve2_temp=39
```

### `stop_shower.cgi`

Stop the currently running shower. No parameters.

```
GET /stop_shower.cgi
```

### `start_user.cgi`

Start a saved user preset.

| Parameter | Type | Description |
|-----------|------|-------------|
| `user` | int (1-6) | User preset number |

```
GET /start_user.cgi?user=3
```

### `stop_user.cgi`

Stop the current user session. No parameters.

### `massage_toggle.cgi`

Toggle the massage function on the active shower. No parameters.

---

## Steam Control

### `steam_on.cgi`

Activate the steam generator.

| Parameter | Type | Description |
|-----------|------|-------------|
| `temp` | int | Target temperature (in Fahrenheit by default, depends on unit setting) |
| `time` | int | Duration in minutes |

```
GET /steam_on.cgi?temp=110&time=20
```

### `steam_off.cgi`

Turn off the steam generator. No parameters.

### `powerclean_check.cgi`

Check or trigger the steam generator power-clean cycle. No parameters.

---

## Lighting Control

### `light_on.cgi`

Turn on a light module at a given intensity.

| Parameter | Type | Description |
|-----------|------|-------------|
| `module` | int (1-3) | Light module number |
| `intensity` | int (0-100) | Brightness percentage |

```
GET /light_on.cgi?module=1&intensity=75
```

### `light_off.cgi`

Turn off a light module.

| Parameter | Type | Description |
|-----------|------|-------------|
| `module` | int (1-3) | Light module number |

```
GET /light_off.cgi?module=2
```

---

## Rain Panel Control

### `rain_on.cgi`

Activate the rain panel with a solid color or lighting effect.

| Parameter | Type | Description |
|-----------|------|-------------|
| `mode` | int | `1` = solid color, `2` = effect |
| `color` | int | Hue value (0-360) or `-1` for white. Used when `mode=1`. |
| `effect` | int (0-7) | Effect index. Used when `mode=2`. |

**Color values (hue):**

| Hue | Color |
|-----|-------|
| 0 | Red |
| 30 | Orange |
| 60 | Yellow |
| 115 | Green |
| 235 | Blue |
| 270 | Violet |
| 305 | Purple |
| 330 | Pink |
| -1 | White |

**Effect values:**

| Index | Effect |
|-------|--------|
| 0 | Color Cycle |
| 1 | Warm |
| 2 | Cool |
| 3 | Rise |
| 4 | Set |
| 5 | Clouds |
| 6 | Reflect |
| 7 | Thunder |

**Example -- solid blue:**

```
GET /rain_on.cgi?mode=1&color=235
```

**Example -- Thunder effect:**

```
GET /rain_on.cgi?mode=2&effect=7
```

### `rain_off.cgi`

Turn off the rain panel. No parameters.

---

## Music / Audio Control

### `music_on.cgi`

Start audio playback.

| Parameter | Type | Description |
|-----------|------|-------------|
| `volume` | int (0-100) | Volume level |

```
GET /music_on.cgi?volume=50
```

### `music_off.cgi`

Stop audio playback. No parameters.

### `bt_disconnect.cgi`

Disconnect the current Bluetooth audio device. No parameters.

### `BTKey.cgi`

Set the Bluetooth pairing key.

### `BTPin.cgi`

Set the Bluetooth PIN code.

---

## System Information

### `system_info.cgi`

Returns system information (firmware versions, serial numbers, hardware revision).

### `values.cgi`

Read datatable values. See [values.cgi Guide](../../research/xagon0/docs/web-interface/values-cgi-guide.md) for full details.

| Parameter | Type | Description |
|-----------|------|-------------|
| `page` | string | Page identifier |
| `type` | string | Data type filter |

Response is JSON with a `values` array.

### `languages.cgi`

Returns the list of available languages.

### `files_available.cgi`

Lists the firmware images currently staged in `\images`. Example from 0.0.3.89:

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

`*_size` fields are human-readable strings, not byte counts; modules with value `0` are not installed. The `prompt2_*` / `prompt3_*` fields show the update system can also stage **valve** firmware.

### `ftp_status.cgi`

Check the status of an ongoing FTP transfer. The full field set — including the per-component update matrix (`ftp_ctl_image_size`, `ftp_ui_image_size`, `ftp_ui_app_file`, eight `ftp_ui_rfs_fileN`, `ftp_ui_touch_file`, `ftp_ui_lang_file`, `ftp_coproc_image_size`, valve flash/EEPROM sizes) — is documented in [../firmware-update.md](../firmware-update.md#the-full-update-matrix).

### `check_updates.cgi`

Sets the check-for-updates flag. The controller will poll for firmware updates on next opportunity.

### `landing_url.cgi`

Returns the default landing page URL.

```json
{"url": "settings.html"}
```

---

## Configuration / Datatable

### `save_variable.cgi`

**The primary settings endpoint.** Used by the web UI to write configuration values.

| Parameter | Type | Description |
|-----------|------|-------------|
| `index` | int (1-105) | Variable ID -- see [save_variable Reference](../../research/xagon0/docs/web-interface/save-variable-reference.md) |
| `value` | varies | Value to write |
| `module` | int (1-3) | Optional: light module selector |
| `valve` | int (1-2) | Optional: valve selector |
| `outlet` | varies | Optional: outlet selector |

**Example -- set music volume to 60:**

```
GET /save_variable.cgi?index=43&value=60
```

**Example -- set light brightness on module 2:**

```
GET /save_variable.cgi?index=53&value=80&module=2
```

See [save_variable Reference](../../research/xagon0/docs/web-interface/save-variable-reference.md) for the complete variable ID table.

### `edit_dt.cgi`

Direct datatable read/write access.

| Parameter | Type | Description |
|-----------|------|-------------|
| `type` | int | `0` = byte, `1` = word, `2` = color, `3` = string |
| `page` | char/int | `'s'` or `'S'` = stationary, `'g'` or `'G'` = ghost, or page number `0-29` |
| `index` | int | Variable index within the page |
| `value` | varies | Value to write (max 25 chars for strings). Omit to read. |

**Response:** `":)"` on success, `":("` on invalid page.

**Example -- write byte 42 to stationary page index 10:**

```
GET /edit_dt.cgi?type=0&page=s&index=10&value=42
```

**Example -- read word from page 20, index 5:**

```
GET /edit_dt.cgi?type=1&page=20&index=5
```

**Example -- write a string to string page 0, index 3:**

```
GET /edit_dt.cgi?type=3&page=s&index=3&value=MyShowerName
```

See [Datatable Structure](../../research/xagon0/docs/web-interface/datatable-structure.md) for page layouts and variable maps.

### `datatable.cgi`

View the full datatable contents (debug use).

### `saveDT.cgi`

Persist the current datatable to flash memory.

### `saveUI.cgi`

Save UI-specific settings to flash.

### `clear_dt.cgi`

Clear the entire datatable (destructive -- resets all configuration).

### `rpc.cgi`

Invoke a Remote Procedure Call by index.

| Parameter | Type | Description |
|-----------|------|-------------|
| `index` | int | RPC command index |

Response: `":)"` on success.

```
GET /rpc.cgi?index=7
```

See [RPC Reference](../../research/xagon0/docs/web-interface/rpc-reference.md) for the full command table.

---

## Device Simulation

### `set_device.cgi`

Configure simulated devices using an 11-character binary string.

| Parameter | Type | Description |
|-----------|------|-------------|
| `value` | string | 11-character binary string (`0` or `1` per position) |

**Bit positions:**

| Bit | Device |
|-----|--------|
| 0 | 6-port valve 1 |
| 1 | 6-port valve 2 |
| 2 | Prompt3 valve 1 |
| 3 | Prompt3 valve 2 |
| 4 | Prompt2 valve 1 |
| 5 | Prompt2 valve 2 |
| 6 | Cold water |
| 7 | Steam |
| 8 | Rain panel |
| 9 | LightBridge |
| 10 | Amplifier |

**Mutual exclusion:** Only one valve type can be active per valve slot. For example, you cannot enable both bit 0 (6-port valve 1) and bit 2 (Prompt3 valve 1) simultaneously.

**Example -- simulate a 6-port valve 1 with steam and rain panel:**

```
GET /set_device.cgi?value=10110000001
```

**Example -- simulate all peripherals, no valves:**

```
GET /set_device.cgi?value=11110000000
```

### `sim_dev_values.cgi`

Returns JSON describing both real and simulated device status.

**Response fields:**

| Field | Description |
|-------|-------------|
| `real_valve_attached` | Whether a real valve is physically connected |
| `Valve_1_attached` / `Valve_2_attached` | Per-valve real detection |
| `v1_status` / `v2_status` | 6-port valve simulation status |
| `v1_P3status` / `v2_P3status` | Prompt3 valve simulation status |
| `v1_P2status` / `v2_P2status` | Prompt2 valve simulation status |
| `steam_status` | `0` = real, `1` = sim ON, `2` = sim available |
| `rain_status` | Same as above |
| `light_status` | Same as above |
| `amp_status` | Same as above |
| `coldwater` | Cold water simulation state |
| `warning` | Warning flags (bitfield) |

**Port assignments for simulation:**

| Port | Simulated Device |
|------|-----------------|
| 5 | Steam |
| 6 | Rain panel |
| 7 | LightBridge |
| 8 | Amplifier |

---

## Error Logs

### `cerror_logs.cgi`

Retrieve controller error logs.

### `kerror_logs.cgi`

Retrieve Konnect (UI processor) error logs.

### `reset_cfault.cgi`

Clear controller fault logs.

### `reset_kfault.cgi`

Clear Konnect (UI) fault logs.

---

## Reset Operations

### `reset_factory.cgi`

Full factory reset -- erases all configuration, user data, and paired devices.

### `reset_default.cgi`

Reset system settings to defaults (preserves some user data).

### `reset_users.cgi`

Reset all user presets.

### `reset_user.cgi`

Reset a specific user preset.

### `reset_fault.cgi`

Reset fault flags without clearing the log.

### `forget_devices.cgi`

Forget all paired Bluetooth devices.

---

## File Operations

### `fileupload.cgi`

Upload firmware or resource files.

- **Method:** `POST`
- **Content-Type:** `multipart/form-data`

**curl example:**

```bash
curl -X POST \
  -F "file=@firmware_update.bin" \
  http://192.168.1.100/fileupload.cgi
```

### `files.cgi`

List files stored on the system — an HTML rendering of drive `a:\`. Every production unit shows the same shape (firmware staging directory plus config tables):

```text
a:\
corys.txt                            size 144 bytes
\images
    temp.txt                         size 16 bytes
    dtvplus2_app_v0.0.3.89.S19       size 4715750 bytes
    ui_amulet_v0.1.3.72.S19          size 12992824 bytes
    versions.txt                     size 171 bytes
    dtvplus2_uiapp_v0.0.7.44.pack.tar  size 6440960 bytes
data_table.txt                       size 10221 bytes
data_table_default.txt               size 10221 bytes
\backup
```

> **Important:** there is no corresponding *download* CGI, and the web server's document root is a read-only filesystem compiled into the firmware image — it cannot reach `a:\`. This endpoint enumerates filenames only. Extraction requires hardware access; see [../repair/firmware-extraction.md](../repair/firmware-extraction.md).

### `unpack_bin.cgi`

Unpack an uploaded binary file (used during firmware update process).

---

## Other Endpoints

### `swapvalves.cgi`

Swap the configuration of valve 1 and valve 2.

### `hiding.cgi`

Control UI element visibility and debug flags.

### `id_interface.cgi`

Identify the interface by flashing its LED.

### `mac.cgi`

Retrieve the MAC address.

> **Safety 3/5 -- this endpoint can cause system lockups.** Use with caution.

### `serial.cgi`

Retrieve the serial number.

> **Safety 3/5 -- this endpoint can cause system lockups.** Use with caution.

### `change_user.cgi`

Switch the active user context.

### `update_change.cgi`

Notify the system of a configuration update.

### `remove_module.cgi`

Remove a light module from the system configuration.

---

## CGI Lock Flags

Some CGI operations are gated by lock flags to prevent conflicts during shower operation.

| Flag | Value | Description |
|------|-------|-------------|
| `CGI_SHOWER_START` | `0x00000001` | Set when a shower start is in progress |
| `CGI_SHOWER_LOCK` | `0x00000002` | Set while the shower is actively running |

When `CGI_SHOWER_LOCK` is active, configuration-changing endpoints may return errors or be silently ignored. Always check shower state before making settings changes programmatically.

---

## General Notes

1. **Session limit:** Only 2 concurrent HTTP sessions are supported. Open browser tabs, AJAX polling, and API scripts all count.
2. **Tool compatibility:** Chrome works most reliably. Postman and PowerShell `Invoke-WebRequest` often fail due to header handling differences.
3. **Caching:** Always set `cache: false` (or append a cache-busting query parameter) in AJAX calls.
4. **Response parsing:** Many CGI responses omit the `Content-Length` header. Read until the socket closes rather than relying on a content length.
5. **HTTP/0.9 for `.cgi`:** CGI endpoints answer with a bare body — no status line, no headers. Node `fetch` and Python `requests` reject this outright; use `curl --http0.9`. Static files (`.html`, `.js`, `.png`) get a normal `HTTP/1.0` response. Error responses are small HTML documents.
6. **No `HEAD`:** `HEAD` requests return 404 even for existing endpoints. Use `GET`.
7. **Base URL:** All endpoints are relative to the DTV+ IP address, e.g. `http://192.168.1.100/quick_shower.cgi?...`
