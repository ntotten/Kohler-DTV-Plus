# Firmware Update Process

## Update Trigger

Firmware updates can be initiated in two ways:

1. **Manual trigger:** Calling `/check_updates.cgi` sets `BYTE_G[34]` to `true`, which initiates the update check immediately.
2. **Scheduled check:** The controller computes a daily update time based on its serial number (see below).

## Schedule Calculation

The controller derives its daily update time from the serial number to distribute update checks across the fleet and avoid overwhelming the FTP server:

```
minutes_after_midnight = (first_4_digits_of_serial % 144) * 10
```

- `first_4_digits_of_serial` is read from `WORD_S[32]`
- The modulo 144 produces values 0-143
- Multiplied by 10 gives 0-1430 minutes (spanning nearly 24 hours)
- This creates 144 possible time slots, each 10 minutes apart

For example, a serial number starting with `2847` would check at `(2847 % 144) * 10 = 126 * 10 = 1260` minutes after midnight (9:00 PM).

## Update Process

Once triggered, the controller performs these steps in order:

1. **Verify no active writes:** Confirm no firmware images are currently being written or downloaded
2. **Verify internet connectivity:**
   - Resolve `*.kohler.com` via DNS (via 8.8.8.8, hardcoded)
   - Ping the resolved address to confirm reachability
3. **Connect to FTP server:**
   - Username: `ftpuser`
   - Navigate to the update folder (see below)
4. **Download version manifest:** Retrieve and parse `versions.txt` from the update folder
5. **Store latest versions:**
   - Controller version stored in `STRING_S[27]`
   - UI version stored in `STRING_S[29]`
6. **Compare versions:** Check if the server versions are newer than the installed versions
7. **Download if newer:** Fetch the updated firmware files from the FTP server
8. **Trigger image swap:** Fire the image swap event to install the new firmware on next boot

## Update Folder

| Mode | Folder | Source |
|------|--------|--------|
| Standard (consumer) | `00` | Default folder on FTP server |
| Hospitality (hotel) | Custom folder name | Configured in `STRING_S[39]` |

Hospitality mode allows hotel installations to use a separate update channel with firmware tailored for commercial deployments.

## Checking FTP/Update Status

The endpoint `/ftp_status.cgi` returns a JSON response with the current update state:

| Field | Description |
|-------|-------------|
| `internet_status` | Whether the controller can reach the internet |
| `upload_enable` | Whether firmware upload is currently permitted |

This is useful for verifying connectivity before triggering a manual update, or for monitoring the progress of an in-flight update.

### The full update matrix

The same endpoint exposes every component the updater can stage. Field set observed on 0.0.3.89:

| Field | Component |
|-------|-----------|
| `ftp_ctl_image_size` | Controller application |
| `ftp_ui_image_size` | Amulet (V1) UI |
| `ftp_ui_app_file` | Linux (V2) UI pack |
| `ftp_ui_rfs_file0` ... `ftp_ui_rfs_file7` | Eight UI resource (rfs) files |
| `ftp_ui_lang_file` | UI language pack |
| `ftp_ui_touch_file` | UI touch controller |
| `ftp_coproc_image_size` | UI coprocessor |
| `ftp_prompt2_flash_size` / `ftp_prompt2_eeprom_size` | Prompt 2 **valve** flash / EEPROM |
| `ftp_prompt3_flash_size` / `ftp_prompt3_eeprom_size` | Prompt 3 **valve** flash / EEPROM |
| `ftp_versions_file` | The `versions.txt` manifest |
| `ftp_lang_image_size` | Additional language image |
| `ftp_downloaded_size`, `ftp_file_count`, `ftp_current_file_count`, `ftp_file_id`, `ftp_download_error` | Transfer progress bookkeeping |

The `*_size` values on an idle unit match the staged files shown by `/files.cgi` — confirming firmware moves over plaintext FTP. The valve fields are notable: the update system can reflash the valve MCUs, not just the controller and interfaces.

## Update Server Status (2026)

The configured update server (`pr0d3ct-upd.kohler.com`) is **decommissioned** — DNS returns NXDOMAIN. Consequences:

- No DTV+ can ever update over the air again; `check_updates.cgi` returns success but the transfer fails silently.
- Every deployed unit is frozen at its shipped build (most: 0.0.3.89).
- The flow is plaintext and DNS-directed, so on a LAN you control it can be intercepted for research (credential capture, manifest inspection). See [security.md](security.md) finding 2 and [repair/firmware-extraction.md](repair/firmware-extraction.md) path 4 for the safe version of that exercise.
