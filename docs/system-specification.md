# Kohler DTV+ — system specification

Technical reference for the master-bath digital shower: what the hardware is,
how it talks, and what our particular installation is configured to do.

Kohler documents none of this. Everything below is either measured on our own
controller or drawn from two independent reverse-engineering projects. Because
the confidence levels differ a great deal, every section is tagged:

| Tier                       | Meaning                                                                                                                                             |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| **[A] Ours**               | Measured on our controller at `192.168.4.80`, 2026-08-22. Source: [`2026-08-22-idle-baseline/`](../research/diagnostics/2026-08-22-idle-baseline/). |
| **[B] Shipped code**       | Read out of the controller's own web UI JavaScript. Same model and firmware as ours, so it describes the code our hardware actually runs.           |
| **[C] Reverse-engineered** | Third-party analysis of DTV+ firmware and boards. Good, but not verified against our unit.                                                          |
| **[?] Unresolved**         | Two sources disagree, or we have not checked.                                                                                                       |

Do not treat a **[C]** row as a value to write to the controller. Several of
them are describing a different board revision, and one of them (temperature
encoding) is dangerous to get wrong.

For what to do after a factory reset, see [recovery.md](repair/recovery.md) — this
document is the "what is it", that one is the "how do I put it back".

---

## 1. Model numbers **[A][C]**

| Part               | What it is                                                      | Present here |
| ------------------ | --------------------------------------------------------------- | ------------ |
| **K-99695-NA**     | DTV+ system controller — the networked box that runs everything | Yes, one     |
| **K-99693(-P-NA)** | DTV+ wall interface (touchscreen)                               | Yes, one     |
| K-99694            | Interface mounting bracket                                      | Presumed     |
| K-99696            | Bluetooth amplifier module                                      | No           |
| K-97999            | Konnect Wi-Fi module                                            | No           |

The wall interface is portrait, 3-5/16 in (84 mm) wide × 5-5/8 in (143 mm)
tall, per the K-99694 bracket drawing. Kohler's own K-99693 spec sheet says
"5-1/4 in wide by 3-5/16 in high", which describes the unit on its side and has
misled at least one CAD consumer.

---

## 2. Our installation **[A]**

Captured 2026-08-22 10:28 EDT, idle, before any actuation.

### Firmware inventory

| Component                        | Version    |
| -------------------------------- | ---------- |
| Controller                       | `0.0.3.89` |
| Wall interface — Amulet          | `0.0.7.44` |
| Wall interface — coprocessor     | `0.0.1.8`  |
| Wall interface — language        | `0.1.1.0`  |
| Wall interface — touch           | `0.0.0.2`  |
| Zone 1 valve (six-port)          | `0.12`     |
| Zone 2 valve (three-port Prompt) | `0.14`     |

`UI1_Type = 0`, which the controller's own `settings.js` renders as
"User Interface 1 (S)" — the **standard**, non-ECO variant. **[B]**

Not installed, all reporting `not_seen`: steam, music/amplifier, lighting,
rain panel, watertile, Konnect.

### Network

|         |                                                                                                                                                          |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Address | `192.168.4.80` (eero DHCP reservation)                                                                                                                   |
| Gateway | `192.168.4.1`                                                                                                                                            |
| MAC     | `00:14:6F:0F:37:82`                                                                                                                                      |
| Wi-Fi   | Off — wired only                                                                                                                                         |
| Clock   | Wall-clock minute matched the workstation, but the controller rendered `-0500` while we were on EDT (`-0400`). Correlate logs by wall clock, not offset. |

### Zones and outlets

Fitting names are decoded from the controller's own outlet icon art; Kohler
ships icons, not labels, so these are cosmetic and the decode is inference. **[?]**

**Zone 1** — six-port valve, five positions used, `v1_cal_code = 173`:

| Position | Raw type    | Fitting    | Purge-eligible |
| -------- | ----------- | ---------- | -------------- |
| 1        | `outlet_6`  | rainhead   | yes            |
| 2        | `outlet_17` | foot spray | yes            |
| 3        | `outlet_17` | foot spray | yes            |
| 4        | `outlet_17` | foot spray | yes            |
| 5        | `outlet_17` | foot spray | yes            |
| 6        | `outlet_0`  | unassigned | no             |

Default temperature 106 °F, maximum 113 °F, default outlet position 1,
default control outlet 3.

**Zone 2** — three-port Prompt valve, all three used, `cal_code = 160`:

| Position | Raw type    | Fitting    |
| -------- | ----------- | ---------- |
| 1        | `outlet_2`  | showerhead |
| 2        | `outlet_17` | foot spray |
| 3        | `outlet_17` | foot spray |

Default temperature 101 °F, maximum 113 °F, default outlet 0.

### Global settings

`units = 0` (°F) · `ShowerConfiguration = 127` · `num_interface = 1` ·
`values.cgi` returns **307 keys / 7,539 bytes** when healthy · date `mm/dd/yy` ·
12-hour time · DST enabled · saved-defaults on · massage feature enabled but no
outlet flagged massage-capable · settings/user/web locks all off ·
cold-water shutoff disabled on both valves (`4`) · maximum runtime disabled on
both valves · automatic purge disabled.

Presets: `Nate` and `Amy` active; 3–5 disabled; 6 enabled.

> **Scald note.** `max_temp` is 113 °F on both zones, which is _above_ the
> 109 °F / 43 °C scald threshold. The controller's limit is an installer
> setting, not a safety guarantee, and the temperature it reports is the
> valve's own thermistor — not an independent measurement. Clamp to `max_temp`,
> never raise it.

---

## 3. Controller hardware **[C]**

### Processor and memory

|               |                                                                                                                                                          |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CPU           | Freescale **MCF5441X** ColdFire V4                                                                                                                       |
| Architecture  | 32-bit CISC, Motorola 68K lineage, ISA_A+ with hardware divide and MAC                                                                                   |
| Clock         | ~250 MHz, single core, BGA package                                                                                                                       |
| Internal SRAM | 64 KB, zero-wait-state                                                                                                                                   |
| External SRAM | 16 MB over FlexBus                                                                                                                                       |
| NAND flash    | Micron **MT29F2G16AABWP** (16-bit, chip ID `0x2CCA`) or **MT29F2G08** (8-bit, `0x2CDA`) — 2 Gbit / 256 MB, 2048 + 64 B pages, 128 KB blocks, 2048 blocks |

The MCF5441X is a sensible pick for this job: it integrates 8 UARTs, an
Ethernet MAC, FlexBus, GPIO, timers and DMA, which is most of what a ten-port
RS-485 controller needs on one die.

Firmware references a 512 MB NAND while the physical part is 256 MB. The
upstream analysis leaves this unexplained — possibly multi-variant support,
possibly raw-versus-ECC capacity. **[?]**

### NAND layout

| Region     | Blocks     | Contents                                                                                                       |
| ---------- | ---------- | -------------------------------------------------------------------------------------------------------------- |
| Reserved   | 0 – 499    | Bootloader (primary + backup) around block 0, configuration/calibration/keys around block 50, spare thereafter |
| Filesystem | 500 – 2047 | HCC SafeFAT — `/firmware`, `/config`, `/web`, `/logs`                                                          |

### Software stack

|                 |                                                                                                    |
| --------------- | -------------------------------------------------------------------------------------------------- |
| RTOS            | Freescale **MQX 3.8** — preemptive, message queues, semaphores, events, lightweight timers         |
| TCP/IP          | RTCS                                                                                               |
| Filesystem      | HCC SafeFlash + SafeFAT, dynamic wear levelling, per-page ECC in the OOB area, journalled metadata |
| Web server      | MQX HTTP (Freescale Embedded Web Server)                                                           |
| Build toolchain | CodeWarrior 10.2, ColdFire C/C++ 10.2, P&E Micro BDM; artifacts as `.S19` / `.elf` / `.rbin`       |

### Power and I/O

24 V AC or DC in, roughly 5–10 W for the controller alone; every peripheral is
powered separately. On-board regulators produce the 3.3 V and 5 V logic rails.
GPIO drives ten RS-485 DE/RE pairs, two dry-contact relay outputs, and reads two
dry-contact inputs.

---

## 4. Buses and ports **[C]**

Two physically distinct RS-485 systems, plus the UI link and Ethernet.

### DTV+ ports — 8 of them

|            |                                                                         |
| ---------- | ----------------------------------------------------------------------- |
| Electrical | RS-485 half-duplex, 9600 8N1, no flow control                           |
| UARTs      | The MCU's own UART 1–8                                                  |
| Topology   | Point-to-point per port — eight independent buses, up to 2 devices each |
| Direction  | GPIO-driven DE/RE per port                                              |
| Buffers    | 2048 B TX, 2048 B RX                                                    |

### Valve ports — 2 of them

|            |                                                                              |
| ---------- | ---------------------------------------------------------------------------- |
| Electrical | RS-485 half-duplex, 9600                                                     |
| UART       | External **TI TL16C752C** dual UART, 64-byte FIFOs                           |
| Bus        | FlexBus memory-mapped — channel A at `0xC0000000`, channel B at `0xC0010000` |
| Protocol   | Saturn, not DTV+                                                             |

The FlexBus UART sits behind a semaphore with a **750 ms timeout**; anything
holding it longer fails subsequent operations until released.

### UI link

UART at **115200** 8N1, no flow control, 150 ms RX timeout. Discovery happens
over the DTV+ protocol; once found, all data exchange switches to the Amulet
CRC protocol against a shared datatable.

### Ethernet

On-chip FEC, 10/100, external PHY, RJ45, T568B.

### RS-485 connector pinout

| Pin | Signal             |
| --- | ------------------ |
| 1   | A+ (non-inverting) |
| 2   | B− (inverting)     |
| 3   | Signal ground      |

---

## 5. The three protocols **[C]**

This is the single most confusing thing about the system: **three** protocols,
two of them at the same baud rate on the same kind of wire.

| Protocol       | Speaks to                                               | Baud   | Notes                                                |
| -------------- | ------------------------------------------------------- | ------ | ---------------------------------------------------- |
| **DTV+**       | Steam, rain panel, LightBridge, amplifier, UI discovery | 9600   | Native protocol                                      |
| **Saturn**     | All valves                                              | 9600   | Predates DTV+, inherited from Mira valve controllers |
| **Amulet CRC** | Touchscreen only, after discovery                       | 115200 | Datatable sync + RPC                                 |

Sending a DTV+ frame to a valve, or vice versa, yields no response or garbage.

**DTV+ framing:** `SOF(0x88) DEST SRC CMD [payload] CHECKSUM EOF(0x55)`, with a
2's-complement checksum. `0x88`, `0x55` and `0xAA` are reserved inside the frame
and byte-stuffed by prefixing `0xAA`.

**Saturn master address depends on valve type** — get this wrong and the valve
silently ignores everything:

| Valve                        | Master address |
| ---------------------------- | -------------- |
| DTV 6-port                   | `0x00`         |
| Prompt 2-port, not networked | `0x00`         |
| Prompt 2-port, networked     | `0x10`         |
| Prompt 3-port (ours, Zone 2) | `0x10`         |
| Prompt 3 flow control        | `0x10`         |

---

## 6. Devices and tasks **[C]**

| Device              | ID                   | Protocol      | Poll                  |
| ------------------- | -------------------- | ------------- | --------------------- |
| Controller          | `0x00`               | —             | master, never a slave |
| Rain panel          | `0x03`               | DTV+          | 175 ms                |
| Steam generator     | `0x05`               | DTV+          | 150 ms                |
| LightBridge         | `0x08`               | DTV+          | 200 ms                |
| Test fixture        | `0x09`               | DTV+          | —                     |
| UI v1               | `0x30`               | DTV+ → Amulet | 50 ms                 |
| UI v2               | `0x31`               | DTV+ → Amulet | 50 ms                 |
| Amplifier           | `0x40` **or** `0x07` | DTV+          | 350 ms                |
| Bootloader          | `0xF0`               | DTV+          | —                     |
| DTV 6-port valve    | `0x06`\*             | Saturn        | 525 ms                |
| Prompt 2-port valve | `0x17`\*             | Saturn        | 525 ms                |
| Prompt 3-port valve | `0x1E`\*             | Saturn        | 525 ms                |

\* Firmware type IDs, not DTV+ bus addresses.

The amplifier answering to two IDs is a real quirk, not a documentation error —
a detach event can name either one for the same physical module.

MQX runs 15+ tasks; the ones worth knowing: `MAIN_TASK` priority 17,
`SHOWER_TASK`/`STEAM_TASK`/`RAIN_PANEL_TASK`/`LIGHT_BRIDGE_TASK`/`RELAY_TASK`
at 15, `VALVE_TASK`/`VALVE2_TASK` and the eight `DTV_PLUS_PORT` handlers at 14,
`UI_TASK` at 13.

Ports 5–8 double as simulation ports (steam, rain, LightBridge, amplifier
respectively) when no physical device is attached. Our `sim_dev_values.cgi`
shows `rain_status`, `amp_status`, `light_status` and `steam_status` all at `2`
with `real_valve_attached = 1`. **[A]**

---

## 7. Valves **[C]**

|                       |        |
| --------------------- | ------ |
| Max valves per port   | 2      |
| Max valves per system | 4      |
| Tick                  | 525 ms |
| Comms timeout         | 320 ms |
| Retry limit           | 4      |

Write-primary flags are a bitmask: `ON 0x01`, `PAUSE 0x02`, `FULL_COLD 0x04`,
`DUTY_FLUSH 0x20`, `DISINFECT 0x40`.

Outlet bitmaps differ by valve family. DTV 6-port uses bits 0–5 (`0x01`…`0x20`);
Prompt 3 generic uses `0x04`, `0x08`, `0x10`, `0x20`, `0x40`, `0x80` for outlets
1–6.

On power-up a valve walks INIT 1 → INIT 8 (reading configuration, firmware
version, calibration and outlet assignments) before settling into
OFF ⇄ ON ⇄ PAUSE.

**The controller does not run a PID loop.** It sends a setpoint and reads back
the valve's thermistor; all proportional mixing happens inside the valve's own
firmware. If the temperature is wrong, the controller is the messenger.

---

## 8. Temperature encoding **[C]**

The two encodings in this system are a genuine footgun.

| Consumer        | Encoding                 |
| --------------- | ------------------------ |
| Valves          | **Cx2** — Celsius × 2    |
| Steam generator | **Fx2** — Fahrenheit × 2 |

Cx2 buys 0.5 °C resolution with pure integer math on an FPU-less part.
Conversion across the boundary is `Fx2 = ((Cx2 × 9) / 5) + 64`.

| Cx2 | °C   | °F    | Fx2 |
| --- | ---- | ----- | --- |
| 60  | 30.0 | 86.0  | 172 |
| 70  | 35.0 | 95.0  | 190 |
| 80  | 40.0 | 104.0 | 208 |
| 90  | 45.0 | 113.0 | 226 |
| 98  | 49.0 | 120.2 | 240 |

`MIN_SYS_VALVE_TEMP` is Cx2 60 (30 °C) — below it the valve raises a Full Cold
flag, meaning it cannot reach the setpoint, usually because hot supply is
unavailable. `MAX_WATER_TEMP` is Cx2 98 (49 °C).

Sending an Fx2 value to a valve asks for 104 °C. Sending a Cx2 value to steam
asks for 4 °C. Always convert.

Note that **none of this is what the CGI layer takes** — `quick_shower.cgi`
wants whole degrees in the system's configured unit (°F here). The Cx2/Fx2
encodings live below the web interface, on the wire.

---

## 9. Web interface **[B][C]**

The controller's HTTP server is the only surface we actually touch. Three
properties of it govern everything in `packages/api/src/kohler/`:

1. **`.cgi` endpoints answer in HTTP/0.9** — a bare body, no status line and no
   headers. Node `fetch` and Python `requests` both reject it outright; `curl`
   needs `--http0.9`. Static files (`.html`, `.js`, `.png`) do get a normal
   `HTTP/1.0 200 OK`. Our capture script uses `curl --http0.9`.
2. **Two concurrent HTTP sessions, total.** Exceeding it wedges the web server
   for roughly 20 seconds. Browser tabs, polling loops and scripts all count.
   Serialise everything and leave a gap (~120 ms upstream; we use 8 s in the
   diagnostic capture path).
3. **There is no authentication of any kind.** Anything on the LAN can start the
   shower. The `cloudflared` tunnel is the entire auth boundary.

Some replies are Python `repr` rather than strict JSON — `True`/`False`/`None`
and single quotes. Parse defensively.

There are ~68 CGI endpoints; the web pages are jQuery 1.9.1 / jQuery UI 1.10.2.
Two parallel data paths exist: `values.cgi` reads the datatable directly, while
`save_variable.cgi` goes through a named-variable layer with extra logic. The
variable ID table is defined **twice** — once in the controller's C firmware and
once in the UI's JavaScript — and the two must stay in sync across updates.

### Endpoints we consider safe to read

| Endpoint             | Returns                                              |
| -------------------- | ---------------------------------------------------- |
| `values.cgi`         | Full configuration plus coarse state (307 keys here) |
| `system_info.cgi`    | Live status the wall interface polls                 |
| `sim_dev_values.cgi` | Real-versus-simulated attachment state               |
| `cerror_logs.cgi`    | Controller error log                                 |
| `kerror_logs.cgi`    | Konnect error log (empty here — no module)           |
| `languages.cgi`      | Installed language packs                             |

`mac.cgi` and `serial.cgi` are documented as causing system lockups, return
empty on this hardware, and the MAC is in `values.cgi` anyway. `powerclean_check.cgi`
can _trigger_ the steam power-clean cycle rather than merely report it. Treat
all three as read-hostile despite the names.

### Commands

All `GET` with query parameters; success returns `:)`.

`quick_shower.cgi` takes the **complete desired state on every call**, so it is
simultaneously start, change-outlets and change-temperature. `valve1_outlet` is
the selected positions concatenated into one string — outlets 1, 3 and 4 are
`134`. An empty string means none, but the controller's own UI calls
`stop_shower.cgi` rather than sending an empty set.

Massage modes are `0` off, `1` single, `2` wave, `3`/`4` custom — **as labelled
by the controller's own `control.html`**. The xagon0 analysis has 1 and 2 the
other way round; we follow the shipped code. Worth verifying before relying on
it. **[?]**

Configuration writes can be silently dropped while water is running:
`CGI_SHOWER_START 0x01` (start in progress), `CGI_SHOWER_LOCK 0x02` (running).

### Endpoints not to call

Rated 4/5 or 5/5 upstream: `reset_factory.cgi`, `clear_dt.cgi`,
`fileupload.cgi`, `unpack_bin.cgi`, `edit_dt.cgi`, `rpc.cgi`, `set_device.cgi`,
`swapvalves.cgi`, `forget_devices.cgi`, `reset_default.cgi`, `reset_users.cgi`,
`save_default.cgi`. Also blocked at 3/5: `saveDT.cgi`, `saveUI.cgi`,
`check_updates.cgi`, `hiding.cgi`, `remove_module.cgi`, the `reset_*fault`
family, `reset_user.cgi`.

### Default lock codes

Settings menu `1020`, web page access `0922` — factory defaults, changeable by
the installer. Ours are reported unlocked. **[A]**

---

## 10. Error model **[C]**

The most important thing here is not the code list. It is that **the system has
two error surfaces with completely different retention**, and confusing them
produces wrong conclusions:

| Surface            | Codes     | Where it lives                                                                                     | Retention                                                                                             |
| ------------------ | --------- | -------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Valve errors**   | 0 – 90    | Reported by valve hardware over Saturn; surfaces as `valveN_ErrorFatal` / `valveN_ErrorResettable` | **Current-state flags, not history.** A transient fault that trips and clears leaves no trace at all. |
| **Controller log** | 100 – 204 | On-board error log, readable at `cerror_logs.cgi`                                                  | Persistent, timestamped                                                                               |

So an empty controller log does **not** exclude a valve fault, and reading
`ErrorResettable` the next day tells you nothing about last night. The only way
to catch a transient valve error is to sample those flags _during_ the event.

### Valve codes (abridged)

| Code             | Name                                               | Meaning                                                                                        |
| ---------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| 0                | `UNCONFIGURED_ERROR`                               | Valve never configured                                                                         |
| 1                | `ERROR_OK`                                         | Normal                                                                                         |
| 2                | `RANGE_ERROR`                                      | Setpoint outside 30–49 °C                                                                      |
| 3                | `OVERTEMP_CONTROL_ERROR`                           | Valve internal temperature unsafe; shuts down                                                  |
| 4                | `COMP_ERROR`                                       | Compensation loop cannot converge                                                              |
| 5 / 6            | `A2D_ERROR` / `A2D_TIMEOUT`                        | Thermistor ADC failure                                                                         |
| 7                | `OVERTEMP_OUTLET_ERROR`                            | Outlet water too hot; may mean a stuck motor                                                   |
| 8 / 16 / 32 / 80 | `RAM_` / `EE_` / `FLASH_` / `STACK_ERROR`          | Memory faults                                                                                  |
| **35**           | `WELDED`                                           | **Safety hazard** — relay contact welded shut, outlet cannot be turned off. Replace the valve. |
| 36               | `RELAY_FAULT`                                      | Relay failed to switch; may be intermittent                                                    |
| 37–39            | `ALG_FAULT`, `ALG_COLD_TIMEOUT`, `ALG_HOT_TIMEOUT` | Mixing algorithm gave up; a side timeout implies that supply is missing                        |
| 40 / 41          | `CONTROLLER_FAULT` / `BATTERY_FAULT`               | Controller hardware; RTC/EEPROM backup battery                                                 |
| 45               | `OUTLET_ERROR`                                     | Flow sensor, solenoid or relay                                                                 |
| 60 / 61          | `M_STUCK` / `M_REALLY_STUCK`                       | Mixing motor — 60 often clears on retry, 61 needs service                                      |
| 70 / 71          | `M_CALIB` / `M_HOMING`                             | Motor calibration / homing                                                                     |
| 90               | `SCHEDULE_ERROR`                                   | Scheduled operation failed                                                                     |
| 114              | `BAD_VALVE_CONFIG`                                 | Configuration corrupt                                                                          |

### Controller log codes

| Code      | Name                                                  | Meaning                                                      |
| --------- | ----------------------------------------------------- | ------------------------------------------------------------ |
| **100**   | `DETACH_EVENT`                                        | A device dropped off the bus. Payload names the device type. |
| 101–103   | `UIn_AMULET_UNRESPONSIVE`, `LIGHT_BRIDGE_MODULE_DROP` | Device stopped answering polls                               |
| 102       | `INVALID_SETTINGS`                                    | Stored settings failed validation on load                    |
| 103       | `FFS_FATAL_ERRORS`                                    | Flash filesystem fatal error                                 |
| 104       | `DATA_TABLE_MISMATCH`                                 | Datatable version mismatch                                   |
| 105 / 106 | `ETHERNET_LINK_DROP` / `WIFI_LINK_DROP`               | Link lost                                                    |
| 107       | `FTP_UNREACHABLE`                                     | Firmware-update server unreachable                           |
| 108 / 109 | `DATA_TABLE_RE_INIT` / `NETWORK_RESET`                | Recovery actions                                             |
| 110       | `BOOTLOADER_STORED_DATA_ERROR`                        | Bootloader config corrupt                                    |
| 130–137   | `TASKn_EXCEPTION`                                     | Unhandled task exception                                     |
| 138–145   | `TASKn_ABORT`                                         | Task aborted                                                 |
| 146       | `NETWORK_TASK_ABORT`                                  | Network task died — see §12                                  |
| 201–204   | `VALVE_n_INSTABLE`                                    | Intermittent valve replies                                   |

Detach device bytes: `0x03` rain panel, `0x05` steam, `0x08` LightBridge,
`0x30`/`0x31` UI, `0x40`/`0x07` amplifier.

### What our own log shows **[A]**

84 parsed events spanning 2026-08-12 20:30 → 2026-08-22 09:38: **83 ×
`100: Prompt3 Error`** (82 distinct timestamps, one exact duplicate) and one
`100: UI Error`. Intervals are irregular — median ~6,090 s, minimum 0, maximum
~263,000 s — so this is not a fixed-period poll artefact. Twenty-seven events
fell between midnight and 05:59, i.e. during almost certainly idle hours.

"Prompt3" strongly implies the three-port Prompt valve, which live topology puts
in Zone 2 — but the Prompt3-to-zone mapping is nowhere explicitly tabulated, so
that step is inference, not fact. **[?]**

---

## 11. LED patterns **[C]**

Visible on the controller board without a network connection.

| Pattern         | Timing                              | Meaning                                           | Severity |
| --------------- | ----------------------------------- | ------------------------------------------------- | -------- |
| Double blink    | 200 ms on, 200 off, 200 on, 1 s off | General error — read `cerror_logs.cgi`            | Variable |
| Slow blink      | 2 s on, 4 s off                     | Firmware failed CRC32; controller will not run it | High     |
| Very slow blink | 2 s on, 8 s off                     | NAND failure                                      | Critical |
| Solid           | Continuous                          | Normal / booting                                  | None     |

Error patterns override normal LED behaviour. No LED activity at all means no
power reaching the board.

---

## 12. Known failure modes **[C]**

The ones that matter for a system left running unattended:

| #   | Failure                                                                                                                                        | Timeline            | Handling                                                                                             |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- | ---------------------------------------------------------------------------------------------------- |
| 1   | **Flash filesystem degradation** — `FFS_FATAL_ERRORS` (103), settings stop saving                                                              | ~1 week of uptime   | Reboot reinitialises the FS and reclaims blocks. A weekly reboot is the standard workaround.         |
| 2   | **Network stack death** — `NETWORK_TASK_ABORT` (146), RTCS memory leak. Web interface unreachable; the shower still works from the wall panel. | ~2 months of uptime | Same weekly reboot covers it.                                                                        |
| 3   | **HTTP session limit** — hangs on the 3rd concurrent connection                                                                                | Immediate           | Serialise. See §9.                                                                                   |
| 4   | **Motor stuck in hard water** — `M_STUCK` (60) → `M_REALLY_STUCK` (61)                                                                         | Gradual             | Softener upstream; exercise the valve across its range; annual descale in hard-water areas.          |
| 5   | **Valve comms instability** — `VALVE_n_INSTABLE` (201–204) on long runs                                                                        | Install-dependent   | Shielded twisted pair, 120 Ω termination at both ends, keep away from AC and motors.                 |
| 6   | **FlexBus semaphore timeout** at 750 ms                                                                                                        | Intermittent        | Keep FlexBus payloads small.                                                                         |
| 7   | **Prompt3 30-minute auto-shutoff** — `PROMPT3_TIMEOUT_MAX` = 1800 s                                                                            | Every long session  | The timer resets on touchscreen interaction, or programmatically, but only once ≥900 s have elapsed. |
| 8   | **Bootloader mode needs a password**                                                                                                           | On service          | Complicates field firmware work if unknown.                                                          |

Items 1 and 2 are the argument for a scheduled reboot; we do not currently run
one.

Item 3 is the one that has actually bitten us. Homebridge polled
`/v1/shower/status` every 60 seconds and produced 38,810 failed status requests
with zero successes, at roughly 5-second transport timeouts, against this
controller's small fixed socket pool. That is why every automated read is now
disabled: Homebridge is command-only, Alexa's shower endpoints are
`retrievable: false`, the web dashboard is static, and explicit commands are
serialised behind a 1.5-second minimum gap. Treat one request per second as the
absolute ceiling and prefer zero. **[A]**

Item 7 is worth keeping distinct from the intermittent-shutoff symptom
documented upstream, which occurred at 3–4 minutes and is a different problem.

---

## 13. Boot and recovery **[C]**

```
Power on
  → MCU internal flash bootloader
  → NAND reset, read chip ID
  → chip ID mismatch?  →  infinite hang, no network, no bypass
  → mount HCC SafeFAT (formats if unformatted)
  → find a:/images/dtvplus2_app_v*.*.*.*.S19
  → not found?  →  fall back to TFS recovery
  → validate: per-line checksums, CRC32 from the S0 header,
              all S3 addresses >= 0x40500000
  → CRC bad?  →  slow-blink LED, will not boot
  → load to RAM, jump to application
```

**TFS** is a read-only filesystem compiled into the bootloader binary carrying
`/default.S19` and `/eco_ui_default.S19`. This is why a completely blank NAND is
still recoverable: the bootloader boots the built-in recovery app, which serves
a minimal web interface for uploading firmware.

The NAND chip-ID check is hard-coded with no bypass. A wrong chip means an
infinite loop, and the only fix is fitting the correct part.

### Firmware files

| Component      | Filename pattern                   |
| -------------- | ---------------------------------- |
| Controller app | `dtvplus2_app_v*.*.*.*.S19`        |
| Amulet UI      | `ui_amulet_v*.*.*.*.S19`           |
| Linux UI       | `dtvplus2_uiapp_v*.*.*.*.pack.tar` |
| ECO controller | `eco_dtvplus2_app_v*.*.*.*.S19`    |
| ECO Amulet UI  | `eco_ui_amulet_v*.*.*.*.S19`       |

Upload order matters: controller app first, UI second, Linux UI pack last, then
power cycle. `dtvplus2_app_v0.0.3.89.S19` — our version — is ~4.7 MB / 9,214
lines. Known controller builds are 0.0.3.56, 0.0.3.89 (common production) and
0.0.3.90; datatable version 65.

We have not attempted a firmware update and `check_updates.cgi` is rated 3/5.
Any firmware work is a separate, deliberate exercise with an operator present.

---

## 14. Open questions

| Question                                                                          | Status                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Is our wall interface the ColdFire Amulet (MCF52252) or the newer ARM/Linux unit? | **Resolved 2026-08-25: ours is the V2 (Linux) UI.** Kohler's Konnect install sheet (public FCC exhibit, `research/fcc/N82-KOHLER029-user-manual.pdf`) requires "99693-P-NA UI sw **7.44**" — matching our `amulet_version_string = 0.0.7.44` and the staged `dtvplus2_uiapp_v0.0.7.44.pack.tar` (the Linux UI pack per xagon0's firmware-file taxonomy). The `amulet_*` field name is legacy. Upstream detail: xagon0 `touchscreen-ui.md` — V1 = MCF52252 Amulet (fw 0.1/3.71); V2 = newer SoC platform ("RFS-based", SoC unidentified) with enhanced graphics and **file transfer over the UI link** (SET_FILE_TRANSFER 0x20 → WRITE_LARGE_DATA chunks → FLUSH_MD5 0x21 → FILE_COMPLETE 0x22). That's how `a:\images\` packs reach the panel. |
| Does "Prompt3" in the error log mean Zone 2?                                      | Strong inference from live topology; never explicitly tabulated.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Firmware claims 512 MB NAND, the part is 256 MB                                   | Unexplained upstream.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Massage mode numbering — 1 = single or 1 = wave?                                  | We follow the controller's own `control.html` (1 = single). xagon0 says the reverse. Untested here.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| What are the four `outlet_17` positions on Zone 1 physically?                     | Icon decode says "foot spray" ×4, which is unlikely for a real install. The type is cosmetic, so this may simply be whatever the installer picked.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Why does the controller render `-0500` while on EDT?                              | Timezone/DST setting probably wrong; not corrected, because correcting it means a config write.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

---

## 15. Sources

| Source                                                                           | What it is                                                                                                                                                                                                                                 | Trust                                   |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------- |
| [`2026-08-22-idle-baseline/`](../research/diagnostics/2026-08-22-idle-baseline/) | Our own controller, captured read-only and sequentially                                                                                                                                                                                    | Direct measurement                      |
| `aaronse/Kohler-DTV-Plus`                                                        | Reverse-engineered from a **live K-99695 on firmware 0.0.3.89** — same model and build as ours — by mirroring the controller's web UI and reading its JavaScript. Source of the CGI risk ratings, transport quirks and outlet-type decode. | High; same firmware                     |
| `xagon0/Kohler-DTV-Plus`                                                         | Deepest hardware and wire-protocol analysis: ColdFire part, NAND, RS-485 framing, error codes, boot process, NAND recovery. Vendored inside the aaronse repo at `research/xagon0/`.                                                        | Good, unverified against our unit       |
| Kohler User Guide 1241234-5-D, `techcomm.kohler.com`                             | Kohler's own service literature — interface screens, install docs                                                                                                                                                                          | Primary, but says nothing about the API |

**Licensing.** `xagon0/Kohler-DTV-Plus` publishes **no license**. Its material
is reference-only and must not be redistributed; this document is a summary
written for our own use, not a copy. Kohler documents and supports none of this,
and support case **#07797183** (Apr 2026) is the open thread with Kohler
engineering about controller documentation.

The upstream repositories were originally cloned to temporary directories
during the 2026-08-22 investigation. On 2026-08-25, during the firmware
extraction work (see [extraction-plan.md](repair/extraction-plan.md)), the analysis-critical
subsets were vendored into [`research/`](../research/) so they survive upstream
removal — same reference-only, do-not-redistribute terms apply.
