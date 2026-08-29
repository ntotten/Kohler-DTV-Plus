# Touch Screen UI Interface

## Overview

The touchscreen interface provides user interaction for the DTV+ system. Two hardware versions exist with different processors and capabilities. Discovery uses the DTV+ protocol, but ongoing data exchange uses the Amulet CRC protocol.

## Device Configuration

| Parameter | Value |
|---|---|
| Device ID (v1) | `0x30` |
| Device ID (v2) | `0x31` |
| Discovery protocol | DTV+ |
| Data protocol | Amulet CRC |
| Baud rate | 115200 |
| Tick interval | 50 ms |

## Hardware Versions

### Version 1

- Processor: MCF52252 ColdFire
- Protocol: Amulet CRC
- Default firmware version: 0.1 / 3.71
- Basic touchscreen with standard graphics

### Version 2

- Processor: RFS-based platform
- Protocol: Amulet CRC (enhanced)
- Enhanced graphics rendering
- File transfer support for firmware updates and custom assets

### Identifying the hardware version

The version string is the reliable way to tell the two apart. The controller's `amulet_version_string` field is a legacy name that outlived the hardware change — do not trust the field name, trust the number:

| Reports | Is | Firmware file line |
|---|---|---|
| `0.1.x` / 3.7x | V1 (Amulet, ColdFire MCF52252) | `ui_amulet_v*.S19` |
| `0.0.7.x` | V2 (Linux) | `dtvplus2_uiapp_v*.pack.tar` |

Corroborating evidence is public: Kohler's Konnect-module install sheet (an FCC exhibit — see [../public-record.md](../public-record.md)) requires "99693-P-NA UI sw **7.44**", which matches the Linux pack `dtvplus2_uiapp_v0.0.7.44.pack.tar` exactly.

The V2 panel reports four component versions, implying at least four updatable parts: UI app, coprocessor, language pack, and touch controller.

## Communication Model

```
+-------------------------------------------+
|            DTV+ CONTROLLER                |
|                                            |
|  +----------------+  +------------------+ |
|  |  FINDER TASK   |  |    UI TASK       | |
|  |  (DTV+ proto)  |  |  (Amulet CRC)   | |
|  |                |  |                  | |
|  |  - Discovery   |  |  - SET_*_VAR     | |
|  |  - Detect      |  |  - INVOKE_RPC    | |
|  |    device ID   |  |  - Periodic sync | |
|  +-------+--------+  +--------+---------+ |
|          |                     |           |
+----------+---------------------+-----------+
           |                     |
      DTV+ frames          Amulet CRC frames
           |                     |
+----------+---------------------+-----------+
|              RS485 BUS                     |
+----------+---------------------+-----------+
           |                     |
      +----+---------------------+----+
      |       TOUCHSCREEN UI          |
      |                               |
      |  DTV+ listener (discovery)    |
      |  Amulet CRC (data exchange)   |
      +-------------------------------+
```

The **Finder Task** uses standard DTV+ protocol to discover the touchscreen on the bus. Once discovered, the **UI Task** takes over and communicates using Amulet CRC for all datatable synchronization and RPC calls.

## Datatable Synchronization

The datatable is the shared state between the controller and the touchscreen. Synchronization flows in both directions:

### Controller to UI (SET_*_VAR)

The controller pushes variable updates to the touchscreen using `SET_BYTE_VAR`, `SET_WORD_VAR`, and `SET_STRING_VAR` commands. These update the UI display in real time.

### UI to Controller (INVOKE_RPC)

When the user touches a control (e.g., changes temperature, presses a button), the touchscreen sends an `INVOKE_RPC` command back to the controller. The controller executes the associated procedure and updates the datatable accordingly.

### Periodic Sync

The controller periodically re-sends critical variables to the touchscreen to guard against missed updates. This ensures the UI remains consistent even if a single frame is lost.

## Critical Datatable Variables

| Variable | Description |
|---|---|
| DT_B_ShowerOnOff | Shower power state (0=off, 1=on) |
| DT_B_SteamOnOff | Steam generator state (0=off, 1=on) |
| DT_W_Temperature | Current water temperature |
| DT_W_SetpointTemp | Target water temperature |
| DT_B_UserSelection | Active user preset (1-4) |
| DT_B_OutletState | Bitmask of active outlets |

## File Transfer (Version 2 Only)

Version 2 touchscreens support file transfer for firmware updates and custom UI assets.

### Transfer Sequence

1. **SET_FILE_TRANSFER (0x20):** Initiates the transfer, specifying file name and size.
2. **WRITE_LARGE_DATA:** Sends file data in chunks. Each chunk is acknowledged before the next is sent.
3. **FLUSH_MD5 (0x21):** Sends the MD5 hash of the complete file for integrity verification.
4. **FILE_COMPLETE (0x22):** Signals the transfer is done. The touchscreen verifies the hash and applies the file.

If the MD5 check fails, the touchscreen discards the file and reports an error. The controller must restart the transfer from the beginning.

## Start Screen Options

The touchscreen can be configured to show different screens on wake/power-up:

| Screen | Value | Description |
|---|---|---|
| Home | 0 | Main dashboard |
| User Selection | 1 | Choose user preset |
| Quick Start | 2 | One-touch start with last settings |
| Settings | 3 | System configuration menu |

## CGI Variables

| Variable Name | CGI Index | Description |
|---|---|---|
| interface_beep | 28 | Touch feedback beep (0=off, 1=on) |
| interface_name | 29 | User-assigned interface name |
| interface_auto_dim | 30 | Auto-dim enable (0=off, 1=on) |
| interface_start_screen | 31 | Start screen selection (0-3) |
| ui_user_lock | 97 | Lock interface to specific user |

## Auto-Dim

When enabled, the touchscreen dims its backlight after a period of inactivity. The typical timeout is **60 seconds**. Any touch on the screen resets the timer and restores full brightness immediately.

Auto-dim helps extend backlight life and reduces glare in dark shower environments.

## Error Display

The touchscreen displays the following error conditions:

| Error Message | Condition |
|---|---|
| Water Too Hot | Valve reports overtemperature at outlet |
| Steam Error | Steam generator fault (thermistor, overtemp, safety) |
| Valve Offline | Valve not responding on Saturn bus |
| Device Not Found | Expected device missing during discovery |

Error messages are displayed as modal overlays that require user acknowledgment before returning to normal operation.

## State Machine

```
+---------+
|  INIT   |
+---------+
     |
     v
+------------+
|  DISCOVER  |  (DTV+ protocol, find UI on bus)
+------------+
     |
     v
+------------------+
| SYNC_DATATABLE   |  (push all variables to UI)
+------------------+
     |
     v
+---------+       +---------------+
|  IDLE   |<----->| PROCESS_RPC   |
|         |       | (handle user  |
|         |       |  input from   |
|         |       |  touchscreen) |
+---------+       +---------------+
```

After initialization and discovery, the controller performs a full datatable sync to bring the touchscreen up to date. It then enters an idle loop, processing RPC calls from the touchscreen as the user interacts with the UI, and periodically re-syncing variables to maintain consistency.
