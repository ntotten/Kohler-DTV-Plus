# Valve Control Module

## Overview

The valve control module manages water mixing valves for shower and bath systems. It communicates over the **Saturn protocol** (not DTV+), which is a distinct serial protocol with its own framing and addressing.

## Supported Valve Types

| Valve Type | Device ID | Ports | Notes |
|---|---|---|---|
| DTV 6-Port | `0x06` | 6 | Legacy 6-port mixing valve |
| Prompt 2-Port | `0x17` | 2 | Two-outlet valve |
| Prompt 3-Port | `0x1E` | 3 | Three-outlet valve |
| Prompt 3 Flow Ctrl | `0xFF` | 3 | Three-outlet with flow control |

## System Configuration

| Parameter | Value |
|---|---|
| Max valves per port | 2 |
| Max valves per system | 4 |
| Tick interval | 525 ms |
| Baud rate | 9600 |
| Communication timeout | 320 ms |

## Master Address Selection

**CRITICAL:** The master address depends on the valve type and network configuration. Using the wrong address means the valve will never respond.

| Valve Type | Condition | Master Address |
|---|---|---|
| DTV 6-Port | Always | `0x00` |
| Prompt 2-Port | Not networked | `0x00` |
| Prompt 2-Port | Networked | `0x10` |
| Prompt 3-Port | Always | `0x10` |
| Prompt 3 Flow Ctrl | Always | `0x10` |

If you send commands with `0x00` to a networked Prompt 2-Port or any Prompt 3, you will get **no response**. Always verify which address the target valve expects.

## Temperature Control

Temperatures use **Q-format Cx2** (Celsius times 2). This avoids floating-point math on the embedded controller.

**Conversion:** `Cx2 = Celsius * 2`

**Example:** 35 degrees C = 70 in Cx2 format.

| Constant | Cx2 Value | Actual Temp |
|---|---|---|
| MIN_SYS_VALVE_TEMP | 60 | 30 degrees C |
| MAX_WATER_TEMP | 98 | 49 degrees C |

## Outlet Bitmaps

### DTV 6-Port Outlets

| Outlet | Bit | Mask |
|---|---|---|
| 0 | 0 | `0x01` |
| 1 | 1 | `0x02` |
| 2 | 2 | `0x04` |
| 3 | 3 | `0x08` |
| 4 | 4 | `0x10` |
| 5 | 5 | `0x20` |

### Prompt 3 Generic Outlets

| Outlet | Mask |
|---|---|
| 1 | `0x04` |
| 2 | `0x08` |
| 3 | `0x10` |
| 4 | `0x20` |
| 5 | `0x40` |
| 6 | `0x80` |

## Write Primary Flags

These flags are combined as a bitmask when writing to the valve primary register:

| Flag | Value | Description |
|---|---|---|
| ON | `0x01` | Valve is active |
| PAUSE | `0x02` | Pause water flow |
| FULL_COLD | `0x04` | Full cold water only |
| DUTY_FLUSH | `0x20` | Duty flush cycle |
| DISINFECT | `0x40` | Disinfection mode |

## Valve State Machine

```
                +--------+
                | INIT 1 |
                +--------+
                    |
                    v
                +--------+
                | INIT 2 |
                +--------+
                    |
                    v
                +--------+
                | INIT 3 |
                +--------+
                    |
                    v
              +----------+
              |  INIT 4  |
              +----------+
                    |
                    v
              +----------+
              |  INIT 5  |
              +----------+
                    |
                    v
              +----------+
              |  INIT 6  |
              +----------+
                    |
                    v
              +----------+
              |  INIT 7  |
              +----------+
                    |
                    v
              +----------+
              |  INIT 8  |
              +----------+
                    |
                    v
               +--------+
          +--->|  OFF   |<---+
          |    +--------+    |
          |        |         |
          |        v         |
          |    +--------+    |
          +----+   ON   +----+
          |    +--------+    |
          |        |         |
          |        v         |
          |    +--------+    |
          +----+ PAUSE  +----+
               +--------+
```

Initialization steps (INIT 1-8) run sequentially on power-up to read valve configuration, firmware version, calibration data, and outlet assignments. After init completes, the valve enters OFF state and is ready for operation.

## Communication Examples

### Read Firmware Version

Saturn read command targeting register `0x15` (firmware info):

```
Master addr | Slave addr | Read cmd | Register 0x15 | CRC
```

The response contains a multi-byte firmware version string.

### Write Valve Primary (Turn On)

To activate the valve at 35 degrees C with outlet 0 enabled:

```
Master addr | Slave addr | Write cmd | Register primary |
  Flags: 0x01 (ON) | Temp: 0x46 (70 Cx2 = 35C) | Outlets: 0x01 | CRC
```

### Valve Reset

Write to the reset register to restart the valve controller. This clears all error states and returns to INIT 1.

## Prompt 3 Timeout (30-Minute Auto-Shutoff)

Prompt 3 valves enforce a **1800-second (30-minute) maximum runtime**. A countdown timer runs whenever the valve is ON. When it reaches zero, the valve shuts off without warning.

**CRITICAL:** You must monitor the remaining time and reset the counter when it drops below 900 seconds. If you fail to reset the timer, the shower will turn off unexpectedly after 30 minutes.

```
if remaining_seconds < 900:
    send_timer_reset()
```

## Error Codes

| Error Name | Code | Description |
|---|---|---|
| ERROR_OK | 1 | No error, normal operation |
| UNCONFIGURED | 0 | Valve not configured |
| OVERTEMP_CONTROL | 3 | Control board overtemperature |
| OVERTEMP_OUTLET | 7 | Outlet water overtemperature |
| WELDED | 35 | Mixing valve mechanically stuck (welded) |
| RELAY_FAULT | 36 | Relay driver fault |
| M_STUCK | 60 | Motor stuck, cannot move |
| M_HOMING | 71 | Motor homing failure |

## Safety Ownership

Anyone building a replacement master needs to know which safety layer lives where. The answer is good news: almost everything that matters is in the valve.

**Valve-side (comes free with the hardware):**

| Layer | Mechanism |
|---|---|
| Mixing loop | Proportional control against the valve's own thermistor. The controller sends a setpoint and reads back the result — **there is no PID loop in the controller** |
| Hard envelope | Setpoints outside 30-49 C rejected (`RANGE_ERROR`); `MAX_WATER_TEMP` (Cx2 98 / 49 C / 120 F) is the hardware ceiling |
| Over-temperature trips | `OVERTEMP_OUTLET` (delivered water too hot), `OVERTEMP_CONTROL` (board overheat), inlet too hot/cold |
| Component fault detection | Thermistor open/short, motor stuck/homing, welded relay |
| Fail-closed | Comms loss times out and closes the valve; power loss closes the solenoids. The failure direction is always OFF |

**Controller-side (a replacement master must reimplement):**

1. **The max-temp clamp.** The stock `max_temp` (factory 113 F / 45 C) is just a config clamp on what setpoint may be sent. Clamp in your own code at or below it; never treat the valve's 49 C ceiling as a comfort limit.
2. **Fault monitoring.** Poll the fault register (`0x0F`); on any over-temp/motor/sensor fault, command all outlets off and latch an alarm.
3. **Prompt 3 timer management.** The 30-minute runtime timer's reset is only accepted once >= 900 s have elapsed — naive constant polling does NOT hold it off. Decide your own max-runtime policy explicitly.
4. **Independent commissioning check.** The reported temperature is the valve's own thermistor, not an independent measurement. Verify delivered temperature with a real thermometer once at build time.
5. **Encoding discipline.** Cx2 to valves, Fx2 to steam — see [../control-logic/temperature-system.md](../control-logic/temperature-system.md).

> **CRITICAL:** A welded relay (`WELDED`, 35) cannot be turned off by ANY controller — that is a replace-the-valve hardware fault, present with the stock system too. And note: the valve's UL/CSA listing covers the assembly as shipped; a DIY master within the documented envelope is functionally equivalent on paper but is not a listed installation.

## Massage Timing Constants

Massage patterns use staggered activation of outlets with specific timing:

| Parameter | Duration |
|---|---|
| Stagger delay | 500 ms |
| Wave slow ON | 1300 ms |
| Wave fast ON | 700 ms |
| Single slow ON | 600 ms |
| Single fast ON | 300 ms |
| Wave OFF | 300 ms |

## Calibration System

Each DTV 6-Port valve ships with a **factory-set calibration code** stored in EEPROM. This code compensates for manufacturing variation in the mixing mechanism. Calibration codes typically fall in the range **160-200**.

### Reading Calibration

Read calibration via Saturn register `0x15`. The response contains the current calibration value for the addressed valve.

### Writing Calibration

Write calibration via Saturn register `0x95`. This permanently updates the EEPROM value.

### CGI Access

| Variable | CGI Index | Description |
|---|---|---|
| six_port_calibration_valve1 | 61 | Calibration code for valve 1 |
| six_port_calibration_valve2 | 62 | Calibration code for valve 2 |

### Calibration Value Interpretation

- **Lower values (160-170):** More aggressive temperature correction. Valve reacts faster to deviations but may overshoot.
- **Mid values (175-185):** Balanced correction. Good general-purpose setting.
- **Higher values (190-200):** Conservative correction. Slower reaction, less overshoot, may undershoot during rapid changes.

Calibration codes are **per-valve** (they compensate for individual manufacturing variation), not per-plumbing installation.

### Tuning Process

1. Read the current calibration code from the valve.
2. Run the shower at a stable setpoint and observe temperature stability.
3. If temperature oscillates (overshoots then undershoots), increase the calibration value by 5.
4. If temperature is slow to reach setpoint or consistently undershoots, decrease the calibration value by 5.
5. Repeat until temperature holds steady within acceptable tolerance.
6. Write the final value to EEPROM.

## Prompt 3 Flow Valve Calibration

For Prompt 3 valves with flow control, use Saturn command `0xF7` to trigger the flow valve calibration routine. The valve will cycle through its range and store the calibration result internally.

## CGI Variables

| Variable Name | CGI Index | Description |
|---|---|---|
| valve_outlet_order | 32 | Outlet activation order |
| valve_outlet_massage | 33 | Massage pattern assignment |
| valve_outlet_default | 34 | Default outlet on startup |
| valve_outlet_flow | 35 | Outlet flow rate setting |
| valve_outlet_ramp | 36 | Temperature ramp rate |
| valve_outlet_type | 37 | Outlet type identifier |
| valve_default_temp | 38 | Default temperature (Cx2) |
| valve_max_temp | 39 | Maximum temperature (Cx2) |
| valve_massage_order | 40 | Massage outlet order |
| valve_auto_purge | 41 | Auto-purge on shutoff |
| valve_cold_water | 42 | Cold water flush setting |
| six_port_calibration_valve1 | 61 | DTV 6-port cal code, valve 1 |
| six_port_calibration_valve2 | 62 | DTV 6-port cal code, valve 2 |
| max_valve_runtime | 99 | Maximum runtime in seconds |
