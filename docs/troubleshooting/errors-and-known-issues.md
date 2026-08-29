# Errors and known issues

## The two error surfaces — read this first

The system has **two error surfaces with completely different retention**,
and confusing them produces wrong conclusions:

| Surface            | Codes   | Where it lives                                                                                    | Retention                                                                                             |
| ------------------ | ------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Valve errors**   | 0–90    | Reported by valve hardware over Saturn; surface as `valveN_ErrorFatal` / `valveN_ErrorResettable` | **Current-state flags, not history.** A transient fault that trips and clears leaves no trace at all. |
| **Controller log** | 100–204 | On-board log at `cerror_logs.cgi`                                                                 | Persistent, timestamped, 99-entry circular buffer                                                     |

So an empty controller log does **not** exclude a valve fault, and reading
`ErrorResettable` the next day tells you nothing about last night. The only
way to catch a transient valve error is to sample those flags _during_ the
event.

## Valve codes (abridged)

| Code             | Name                                                 | Meaning                                                                                                      |
| ---------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| 0                | `UNCONFIGURED_ERROR`                                 | Valve never configured                                                                                       |
| 1                | `ERROR_OK`                                           | Normal                                                                                                       |
| 2                | `RANGE_ERROR`                                        | Setpoint outside 30–49 °C                                                                                    |
| 3                | `OVERTEMP_CONTROL_ERROR`                             | Valve internal temperature unsafe; shuts down                                                                |
| 4                | `COMP_ERROR`                                         | Compensation loop cannot converge                                                                            |
| 5 / 6            | `A2D_ERROR` / `A2D_TIMEOUT`                          | Thermistor ADC failure                                                                                       |
| 7                | `OVERTEMP_OUTLET_ERROR`                              | Outlet water too hot; may mean a stuck motor                                                                 |
| 8 / 16 / 32 / 80 | `RAM_` / `EE_` / `FLASH_` / `STACK_ERROR`            | Memory faults                                                                                                |
| **35**           | `WELDED`                                             | **Safety hazard** — relay contact welded shut; the outlet cannot be turned off by anyone. Replace the valve. |
| 36               | `RELAY_FAULT`                                        | Relay failed to switch; may be intermittent                                                                  |
| 37–39            | `ALG_FAULT` / `ALG_COLD_TIMEOUT` / `ALG_HOT_TIMEOUT` | Mixing algorithm gave up; a side timeout implies that supply is missing                                      |
| 40 / 41          | `CONTROLLER_FAULT` / `BATTERY_FAULT`                 | Controller hardware; RTC/EEPROM backup battery                                                               |
| 45               | `OUTLET_ERROR`                                       | Flow sensor, solenoid or relay                                                                               |
| 60 / 61          | `M_STUCK` / `M_REALLY_STUCK`                         | Mixing motor — 60 often clears on retry, 61 needs service                                                    |
| 70 / 71          | `M_CALIB` / `M_HOMING`                               | Motor calibration / homing                                                                                   |
| 90               | `SCHEDULE_ERROR`                                     | Scheduled operation failed                                                                                   |
| 114              | `BAD_VALVE_CONFIG`                                   | Configuration corrupt                                                                                        |

## Controller log codes

| Code      | Name                                    | Meaning                                                     |
| --------- | --------------------------------------- | ----------------------------------------------------------- |
| **100**   | `DETACH_EVENT`                          | A device dropped off the bus; payload names the device type |
| 101       | `UIn_AMULET_UNRESPONSIVE`               | Interface stopped answering polls                           |
| 102       | `INVALID_SETTINGS`                      | Stored settings failed validation on load                   |
| 103       | `FFS_FATAL_ERRORS`                      | Flash filesystem fatal error                                |
| 104       | `DATA_TABLE_MISMATCH`                   | Datatable version mismatch                                  |
| 105 / 106 | `ETHERNET_LINK_DROP` / `WIFI_LINK_DROP` | Link lost                                                   |
| 107       | `FTP_UNREACHABLE`                       | Firmware-update server unreachable                          |
| 108 / 109 | `DATA_TABLE_RE_INIT` / `NETWORK_RESET`  | Recovery actions                                            |
| 110       | `BOOTLOADER_STORED_DATA_ERROR`          | Bootloader config corrupt                                   |
| 130–137   | `TASKn_EXCEPTION`                       | Unhandled task exception                                    |
| 138–145   | `TASKn_ABORT`                           | Task aborted                                                |
| 146       | `NETWORK_TASK_ABORT`                    | Network task died (see below)                               |
| 201–204   | `VALVE_n_INSTABLE`                      | Intermittent valve replies                                  |

Detach device bytes: `0x03` rain panel, `0x05` steam, `0x08` LightBridge,
`0x30`/`0x31` UI, `0x40`/`0x07` amplifier.

## Known failure modes

Ordered roughly by how much they matter for a system left running unattended:

| #   | Failure                                                                                                                                        | Timeline            | Handling                                                                                                                                            |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **Flash filesystem degradation** — `FFS_FATAL_ERRORS` (103), settings stop saving                                                              | ~1 week of uptime   | Reboot reinitialises the FS and reclaims blocks. **A weekly reboot is the standard workaround.**                                                    |
| 2   | **Network stack death** — `NETWORK_TASK_ABORT` (146), RTCS memory leak. Web interface unreachable; the shower still works from the wall panel. | ~2 months of uptime | Same weekly reboot covers it.                                                                                                                       |
| 3   | **HTTP session limit** — hangs on the 3rd concurrent connection                                                                                | Immediate           | Serialise requests; ≤ 1 request/second is the safe ceiling. Every browser tab and polling loop counts.                                              |
| 4   | **Motor stuck in hard water** — `M_STUCK` (60) → `M_REALLY_STUCK` (61)                                                                         | Gradual             | Softener upstream; exercise the valve across its range; annual descale in hard-water areas.                                                         |
| 5   | **Valve comms instability** — `VALVE_n_INSTABLE` (201–204) on long runs                                                                        | Install-dependent   | Shielded twisted pair, 120 Ω termination at both ends, route away from AC and motors.                                                               |
| 6   | **FlexBus semaphore timeout** at 750 ms                                                                                                        | Intermittent        | Keep FlexBus (valve UART) payloads small.                                                                                                           |
| 7   | **Prompt3 30-minute auto-shutoff** — `PROMPT3_TIMEOUT_MAX` = 1800 s                                                                            | Every long session  | The timer resets on interaction, but only once ≥ 900 s have elapsed. Distinct from the intermittent 3–4-minute shutoff issue investigated upstream. |
| 8   | **Bootloader mode needs a password**                                                                                                           | On service          | Complicates field firmware work if unknown.                                                                                                         |

Items 1 and 2 are the argument for a scheduled weekly reboot.

## Mid-shower shutoff — open upstream investigation

There is a documented, unresolved community issue of the shower stopping
mid-use (~3–4 minutes in), where **the water stops first and the controller
only notices about a minute later** — i.e. nothing commanded it. The leading
hypothesis upstream is a tankless-heater minimum-flow cutout making the valve
unable to reach setpoint, _not_ a DTV+ fault. The full experiment queue lives
in aaronse's [INVESTIGATIONS.md](https://github.com/aaronse/Kohler-DTV-Plus/blob/master/INVESTIGATIONS.md).

## LED patterns

Visible on the controller board without a network connection:

| Pattern         | Timing                              | Meaning                                |
| --------------- | ----------------------------------- | -------------------------------------- |
| Double blink    | 200 ms on, 200 off, 200 on, 1 s off | General error — read `cerror_logs.cgi` |
| Slow blink      | 2 s on, 4 s off                     | Firmware failed CRC32; will not boot   |
| Very slow blink | 2 s on, 8 s off                     | NAND failure (critical)                |
| Solid           | continuous                          | Normal / booting                       |

No LED activity at all means no power reaching the board.
