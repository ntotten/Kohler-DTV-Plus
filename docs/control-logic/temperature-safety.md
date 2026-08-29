# Temperature safety: where the protection actually lives

Anyone building tooling around this system — and especially anyone building a
**replacement controller** — needs to know which safety layer lives where.
The answer is good news: almost everything that matters is in the valve.

## Valve-side (hardware you keep)

The valve is not a dumb mixer. Its own firmware owns:

| Layer                         | Mechanism                                                                                                                                                        |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **The mixing loop itself**    | Proportional control against the valve's own thermistor. The controller sends a setpoint and reads back the result — **there is no PID loop in the controller**. |
| **A hard operating envelope** | Setpoints outside **30–49 °C (86–120.2 °F)** are rejected (`RANGE_ERROR`). `MAX_WATER_TEMP` = Cx2 98 (49 °C) is the hardware ceiling.                            |
| **Over-temperature trips**    | `OVERTEMP_OUTLET` (delivered water too hot → shutdown), `OVERTEMP_CONTROL` (valve board overheat), plus inlet-too-hot / inlet-too-cold trips.                    |
| **Component fault detection** | Thermistor open/short (A2D), motor stuck / homing failure, welded-relay detection. Each trips and shuts down.                                                    |
| **Fail-closed behavior**      | Comms loss times out and closes the valve (Prompt 3: 30-minute runtime timer); power loss closes the solenoids. The failure direction is always OFF.             |

## Controller-side (what the stock controller adds)

These are conveniences and policies, not interlocks:

1. **The configured `max_temp`** (factory 113 °F) — a config clamp on what
   setpoint may be sent. It is an _installer setting, not a safety
   guarantee_, and it is above the 109 °F / 43 °C scald threshold. The
   temperature it reports is the valve's own thermistor — not an independent
   measurement.
2. **Fault monitoring and display** — surfacing valve trips to the user.
3. **Prompt 3 runtime-timer management** — resetting the 30-minute valve
   timer at the right time (only possible once ≥ 900 s have elapsed).
4. **Preset/setpoint logic** — per-user temps, warm-up, pause, spa programs.

## Rules for any replacement master

If you drive these valves yourself:

1. **Clamp setpoints at or below 45 °C / 113 °F** (Cx2 90) in your own code.
   Never treat the valve's 49 °C hardware ceiling as a comfort limit, and
   never raise the stock `max_temp`.
2. **Poll the fault register and act on it.** On any over-temp, motor, or
   sensor fault: command all outlets off immediately and latch an alarm. The
   valve protects itself regardless — this is belt and braces, and cheap.
3. **Manage the Prompt 3 timer deliberately.** Its reset is only accepted
   once ≥ 900 s have elapsed; constant polling alone does not hold it off.
   Decide your own max-runtime policy explicitly.
4. **Commission with an independent thermometer** at the fitting. The
   reported temperature is the valve's own sensor; verify it before trusting
   setpoints.
5. **Use the right temperature encoding on the wire** — Cx2 to valves, Fx2 to
   steam; the trap is documented in [protocols.md](../protocols/overview.md).

## What no software can fix

- **Welded relay (error 35)** — a physically welded contact means an outlet
  cannot be turned off by _any_ controller. That is a replace-the-valve
  hardware fault, and it exists with the stock system too.
- **Listing.** The valve assembly's UL/CSA listing presumably covers it as
  shipped, driven by the stock controller. A DIY master operating within the
  documented envelope is functionally equivalent on paper, but the result is
  not a listed installation. Consider that before a permanent change; it is
  irrelevant for bench work.

## Why this layering matters for repair

The controller is the fragile part of this system (its failure modes are in
[errors-and-known-issues.md](../troubleshooting/errors-and-known-issues.md)), and it is the part
Kohler no longer supports. The valves are comparatively robust _and_ own
their own safety. A system whose controller died has not lost its protection
— it has lost its scheduler, its UI, and its network API.
