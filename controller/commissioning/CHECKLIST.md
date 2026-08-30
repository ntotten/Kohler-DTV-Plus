# Commissioning checklist

Generated from `controller/requirements.toml` by
`cargo xtask reqs --checklist commissioning/CHECKLIST.md`.
Do not edit by hand — edit the register and regenerate.

Every requirement below is one **no test in this repository can prove**.
They need hardware, a person, or both. Listing them here is what keeps
their absence from the test suite accounted for rather than silent.

Each line is a checkbox because that is how it gets used: printed, walked
through, and signed. **Record the measured value** wherever a threshold is
named — a tick against "stops within the threshold" is worth much less than
the number that was actually observed.

**125 items.**

## AGENT.md

- [ ] **OPS-02** — Never open a valve without explicit, in-the-moment operator consent; read-only work needs no permission, anything that moves water needs asking every time.
  - _Verify:_ not-testable-in-software; procedural
  - _Source:_ AGENT.md Hard rules (2); CLAUDE.md Quick orientation
- [ ] **OPS-06** — Mark inference as inference and say when something is unverified; verify claims against the hardware or a source.
  - _Verify:_ not-testable-in-software; documentation review
  - _Source:_ AGENT.md Hard rules (4); CLAUDE.md How to write here
- [ ] **SAFE-07** — Never open a valve (or otherwise move water/state) without explicit, in-the-moment operator consent for that specific action; read-only operations require no confirmation.
  - _Verify:_ not-testable-in-software; enforced by requiring an explicit, human-triggered call path with no auto-retry/auto-trigger of command endpoints
  - _Source:_ AGENT.md hard rule 2; app/CLAUDE.md
- [ ] **TEST-03** — Nothing in the automated test suite (unit tests or selftest) may ever open a valve; the only path that actually moves water is deliberately manual and operator-initiated, outside the test suite.
  - _Verify:_ not-testable-in-software beyond code review; enforce via SAFE-01..SAFE-06 so the test binary has no code path capable of reaching a >2/5 or non-exposed command
  - _Source:_ AGENT.md Conventions 'Tests'; app/CLAUDE.md 'Neither may ever open a valve.'; DESIGN.md 'The live flow test ... is deliberately manual and operator-initiated.'

## CLAUDE.md

- [ ] **TIME-04** — Do not apply the controller's HTTP-client limits (15 s idle / 5 s active polling, two sessions maximum) to the serial link, and do not apply serial timings to any HTTP client; they govern different transports.
  - _Verify:_ not-testable-in-software (design-review item); enforced by keeping the two rate limiters in separate modules
  - _Source:_ CLAUDE.md § Quick orientation; AGENT.md hard rule 3; research/FIELD-NOTES.md §1
- [ ] **SAFE-06** — Do not treat 125 F / 51.7 C as a safe temperature bound: it is a product ceiling above the 43 C / 109 F scald threshold, and the steam hazard is whole-room air at that temperature with near-100% humidity.
  - _Verify:_ not-testable-in-software (documentation and UI-warning requirement)
  - _Source:_ CLAUDE.md § Quick orientation; controller/docs/STEAM-ADAPTER.md § Safety position
- [ ] **SAFETY-03** — Never open a valve (move water) without explicit, in-the-moment operator consent, requested every time; read-only operations require no such consent.
  - _Verify:_ not-testable-in-software (human-in-the-loop consent, as this entry already said); manual/commissioning verification, though a consent-gate function can be unit tested for refusing to proceed absent a consent flag
  - _Source:_ CLAUDE.md 'Quick orientation'; AGENT.md 'Hard rules' #2

## DESIGN.md

- [ ] **PROTO-02** — The controller has no authentication; network position (LAN access) is the only access boundary, so the safety gate must not be treated as a security control, only a lockup/damage-prevention control.
  - _Verify:_ not-testable-in-software
  - _Source:_ DESIGN.md constraints table

## DISCLAIMER.md

- [ ] **TEMP-04** — Never rely on a reported temperature as proof of the real one; the valve's number is its own thermistor self-report, and delivered water must be verified with a real thermometer after any change and before anyone stands under it.
  - _Verify:_ not-testable-in-software; manual commissioning
  - _Source:_ DISCLAIMER.md § Safety warning
- [ ] **HTTP-05** — Do not upload partial or truncated firmware; it will fail CRC and the unit will not boot.
  - _Verify:_ not-testable-in-software (upload endpoints are permanently blocked)
  - _Source:_ DISCLAIMER.md § Operating limits (3)
- [ ] **OPS-01** — Change one thing at a time so the cause of a problem is known.
  - _Verify:_ not-testable-in-software
  - _Source:_ DISCLAIMER.md § Operating limits (5)
- [ ] **OPS-04** — Keep the controller on a trusted network — it has no authentication, so anything that can reach it can run the shower — and do not leave the shower able to start unattended.
  - _Verify:_ not-testable-in-software; deployment review
  - _Source:_ DISCLAIMER.md § Before you begin

## app/CLAUDE.md

- [ ] **POLL-02** — Never exceed 2 concurrent HTTP sessions to the controller, counting any other client on the LAN (the controller's own web UI, a second copy of this service from e.g. a hot-reload restart). The K-99693 RS-485 wall interface, if present, does not count against this budget.
  - _Verify:_ not-testable-in-software (the budget counts HTTP clients outside this process, which no test here can observe); manual/operational; enforceable in-process only via a shared session/queue limiter
  - _Source:_ app/CLAUDE.md Hard limits table and 'The two-session budget is easy to blow without noticing'

## docs/devices/valve-control.md

- [ ] **TEMP-08** — Treat the reported temperature as the valve's own thermistor reading, not an independent measurement, and require a physical-thermometer verification of delivered temperature once at commissioning and after any calibration change.
  - _Verify:_ manual commissioning
  - _Source:_ docs/devices/valve-control.md — Safety Ownership item 4; research/xagon0/docs/control-logic/temperature-system.md — Safety Warning
- [ ] **SAFE-04** — Record that a DIY master operating inside the documented envelope is functionally equivalent on paper but is not a listed installation — the valve's UL/CSA listing covers the assembly as shipped.
  - _Verify:_ not-testable-in-software
  - _Source:_ docs/devices/valve-control.md — Safety Ownership CRITICAL note

## controller/CLAUDE.md

- [ ] **PHY-09** — Leave our end of the steam RS-485 link unterminated; the K-1737-K1 adapter already terminates the bus at approximately 114 ohm.
  - _Verify:_ manual commissioning (resistance measurement)
  - _Source:_ controller/CLAUDE.md § Settled — do not re-litigate; controller/docs/STEAM-ADAPTER.md §5
- [ ] **PHY-10** — Connect signal ground on the steam link (not optional there, because the adapter's SMBJ28A clamps are unidirectional and remove the negative common-mode range) and drive pin 4 of the DTV+ header, which bus-powers the adapter's transceiver through R16.
  - _Verify:_ manual commissioning
  - _Source:_ controller/CLAUDE.md § Settled — do not re-litigate; controller/docs/STEAM-ADAPTER.md §5

## controller/docs/DESIGN.md

- [ ] **SAFE-01** — The K-99695 and the replacement controller must never be connected to the same valve bus.
  - _Verify:_ not-testable-in-software (physical wiring); commissioning inspection
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Non-negotiable rules (1)
- [ ] **SAFE-02** — Cable swaps happen only with both valves off and valve power removed.
  - _Verify:_ not-testable-in-software; manual commissioning procedure
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Non-negotiable rules (2)
- [ ] **SAFE-06** — Treat the valve's measured communication-loss shutdown as the physical safety backstop; the software fail-off path does not replace it.
  - _Verify:_ manual commissioning (Phase 3 fail-off measurement)
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Non-negotiable rules (5)
- [ ] **K99695-01** — Existing automated HTTP reads against the K-99695 remain disabled; after cutover the K-99695 is powered down.
  - _Verify:_ unit/config check that no poller is scheduled; manual commissioning for power-down
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Non-negotiable rules (11)
- [ ] **BOOT-09** — If automatic purge is enabled (I4), 'confirmed off' in the safe boot sequence means flow has stopped, not that the valve acknowledged.
  - _Verify:_ manual commissioning; emulator e2e once purge behaviour is known
  - _Source:_ controller/docs/DESIGN.md § Purge handling (3)
- [ ] **API-03** — `stop_all()` stops steam as well as both valve zones.
  - _Verify:_ emulator e2e; manual commissioning in Phase 5
  - _Source:_ controller/docs/DESIGN.md § Software design → public operations
- [ ] **SER-01** — Bind logical zones to stable device paths using each adapter's identity or physical USB path, not incidental `/dev/ttyUSB0` enumeration order.
  - _Verify:_ unit on the resolver; manual commissioning with adapters re-plugged in a different order
  - _Source:_ controller/docs/DESIGN.md § Software design → Pi controller service
- [ ] **SER-04** — `latency_timer` is FTDI-specific; if a unit ships with a different bridge, establish that driver's equivalent before the unit is used.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Software design → Pi controller service
- [ ] **PROTO-01** — All protocol parameters are `[C]` third-party reverse engineering, unverified against this hardware, and each must be confirmed from the Phase 1 capture before the encoder transmits.
  - _Verify:_ manual commissioning (Phase 1 capture) gating the encoder
  - _Source:_ controller/docs/DESIGN.md § Software design → Protocol parameters
- [ ] **SYSD-01** — Configure the daemon as a hardened `systemd` service with no water-state restoration, restart only into the OFF boot sequence, `WatchdogSec` application heartbeats, the Pi hardware watchdog enabled, and bounded persistent logs.
  - _Verify:_ unit file review; emulator e2e restart test; manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Software design → systemd
- [ ] **SYSD-02** — The watchdog supports recovery and diagnosis; it does not replace the valve's measured communication-loss shutdown.
  - _Verify:_ not-testable-in-software; Phase 3 fail-off measurement
  - _Source:_ controller/docs/DESIGN.md § Software design → systemd
- [ ] **TEMP-02** — Evaluate every threshold on the offset-corrected surface-clamp value, characterized against the Therma K probe at commissioning, because a surface clamp reads pipe wall, lags, and reads low.
  - _Verify:_ unit on the correction function; manual commissioning to obtain the offset
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Independent temperature measurement
- [ ] **TEMP-03** — Treat the interlock as covering only the instrumented outlet — each zone's default outlet — with every other outlet protected only by the setpoint clamp and fault monitoring until further channels are fitted.
  - _Verify:_ not-testable-in-software; recorded in the commissioning report
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Independent temperature measurement
- [ ] **LAT-01** — Measure fail-off latency at the outlet, from the last transmitted frame to observed flow stop, on every fault path in the Phase 3 test list.
  - _Verify:_ manual commissioning (Phase 3)
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Acceptance thresholds
- [ ] **LAT-02** — A measured fail-off latency of ≤ 10 s is a pass.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Acceptance thresholds table
- [ ] **LAT-03** — A measured fail-off latency of 10-30 s does not proceed past Phase 3 without written justification and a second opinion.
  - _Verify:_ manual commissioning; documented sign-off
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Acceptance thresholds table
- [ ] **LAT-04** — A measured fail-off latency of > 30 s rejects this architecture.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Acceptance thresholds table
- [ ] **LAT-05** — A failing latency result is not to be solved by putting an unreviewed hobby relay in a valve's mains circuit.
  - _Verify:_ not-testable-in-software; design review
  - _Source:_ controller/docs/DESIGN.md § Safety boundary → Acceptance thresholds
- [ ] **LAT-06** — If automatic purge is enabled (I4), measure Phase 3 and Phase 4 stop-latency figures against flow, not against the valve's acknowledgement.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Purge handling (2)
- [ ] **PH0-01** — Record the recovery baseline — both valve calibration codes (`v1_cal_code = 173`, zone 2 `160`), configured outlet slots and types, per-zone default and maximum temperatures, purge and runtime settings — before any replacement traffic, and use it as the reference for confirming nothing else changed.
  - _Verify:_ manual commissioning; software re-reads and diffs against the baseline after first discovery
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 0
- [ ] **PH0-02** — Discovery is permitted to broadcast address clear, but nothing else in the baseline configuration may change.
  - _Verify:_ manual commissioning: re-read baseline after first discovery
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 0
- [ ] **PH0-03** — Phase 0 does not close until the factory topology can be restored from the labels in under five minutes with power off.
  - _Verify:_ not-testable-in-software; timed drill
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 0 gate
- [ ] **PH1-02** — The capture front end must be physically unable to transmit: termination off, `DE` hard-strapped inactive, `RE` hard-strapped asserted, and no transmit conductor from the USB UART.
  - _Verify:_ not-testable-in-software; hardware inspection
  - _Source:_ controller/docs/DESIGN.md § Packet capture questions
- [ ] **PH1-03** — Capture one valve at a time with no HTTP polling or other automation running.
  - _Verify:_ not-testable-in-software (manual capture procedure, performed by a person at the bus); disable pollers in configuration
  - _Source:_ controller/docs/DESIGN.md § Packet capture questions
- [ ] **PH1-04** — Timestamp at the capture device and use a logic analyzer where timing is the finding, because a 16 ms USB latency quantum does not resolve jitter on a 525 ms tick or a 320 ms deadline.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Packet capture questions
- [ ] **PH1-06** — The capture tap must add neither termination nor fail-safe bias to the pair, which is already terminated and biased at the controller and the valve, and its stub must be inches rather than feet.
  - _Verify:_ not-testable-in-software; hardware inspection of the tap before it is attached
  - _Source:_ controller/docs/DESIGN.md § Packet capture questions
- [ ] **PH2-03** — Run the Pi, adapters, watchdog, service, and emulator continuously for seven days.
  - _Verify:_ not-testable-in-software (seven-day soak on real hardware); manual soak
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 2
- [ ] **PH3-01** — Commission the one-valve pilot at 100 °F on one outlet with the operator present and outside the spray path, hand on the manual disconnect, and the independent probe reading the outlet throughout.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 3 procedure (3)
- [ ] **PH3-02** — Limit the first active session to two minutes.
  - _Verify:_ manual commissioning; configurable duration cap in software
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 3 procedure (4)
- [ ] **PH3-03** — Test process kill, forced process hang, Pi power loss, USB disconnect, Pi watchdog reset, A-wire open, B-wire open, bus short, and manual valve-power removal.
  - _Verify:_ manual commissioning against the live valve
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 3
- [ ] **PH3-04** — Phase 3 closes only when every failure stops flow or reaches the valve's measured fail-off path within the acceptance threshold, records a diagnostic reason, and requires a deliberate new start.
  - _Verify:_ manual commissioning plus log inspection for the diagnostic reason
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 3 gate
- [ ] **PH3-05** — Record stop latency per fault path, maximum physical temperature, and the K-99695's behaviour with a missing valve, and run the full timed manual rollback drill before leaving Phase 3.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 3 gate
- [ ] **PH4-01** — Verify every configured outlet independently with the calibrated thermometer, then run one zone at a time and then both zones at 100 °F.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 4
- [ ] **PH4-02** — Phase 4 closes only when every outlet passes temperature and stop testing and the manual rollback drill succeeds.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 4 gate
- [ ] **PH5-01** — Bring up the DTV+ codec against the emulator first, then against the adapter with the generator's own control still able to stop it.
  - _Verify:_ emulator e2e then manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 5
- [ ] **PH5-03** — Commission one steam session at the 110 °F default for the 10-minute default with the operator present.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 5
- [ ] **PH5-04** — Confirm that `stop_all()` stops steam and that a degraded DTV+ link commands `steam_stop` before latching.
  - _Verify:_ emulator e2e for the degraded-link ordering; manual commissioning to confirm
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 5
- [ ] **PH5-05** — Measure the hard case by pulling the DTV+ link mid-session and recording what the generator does; Phase 5 closes only when a steam session starts, holds setpoint, stops on command and on timer, and the hard-link-loss behaviour is measured and recorded whatever it turns out to be.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 5 gate
- [ ] **PH6-01** — Keep voice, cloud, Homebridge, and automatic routines disabled for a one-week local-only soak.
  - _Verify:_ not-testable-in-software (one-week soak on real hardware); configuration check; manual soak
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 6
- [ ] **PH6-02** — Enable explicit Homebridge/Worker commands only after the soak has no unexplained reset, temperature, or bus event.
  - _Verify:_ not-testable-in-software (a person reads the soak logs and judges what counts as unexplained); log review after the soak
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 6
- [ ] **PH6-05** — Phase 6 closes only with a signed commissioning report and homeowner-visible emergency instructions.
  - _Verify:_ not-testable-in-software
  - _Source:_ controller/docs/DESIGN.md § Delivery phases → Phase 6 gate
- [ ] **RB-01** — Manual rollback begins by sending `stop_all` and verifying physical flow has stopped, and is complete only when the configuration matches the Phase 0 recovery baseline — not when water flows.
  - _Verify:_ manual commissioning; software re-reads calibration codes and configured outlets for the comparison
  - _Source:_ controller/docs/DESIGN.md § Manual rollback to Kohler
- [ ] **RB-02** — Confirm both valves appear independently in `values.cgi` (`valve_1_con_string` / `valve_2_con_string` reading `conn`) and do not rely on the touchscreen alone.
  - _Verify:_ manual commissioning; read-only API check
  - _Source:_ controller/docs/DESIGN.md § Manual rollback to Kohler (8)
- [ ] **WELD-01** — State in the posted emergency procedure, and surface in software, that a `WELDED` fault (35) is a mechanically stuck mixing valve that no controller can turn off; the only remedy is valve power removal and the hot and cold service shutoffs.
  - _Verify:_ unit: fault-code 35 renders the correct operator message; not-testable-in-software for the physical remedy
  - _Source:_ controller/docs/DESIGN.md § Safety boundary and § Manual rollback to Kohler
- [ ] **OPS-08** — Keep the household-specific configuration backup outside this public repository.
  - _Verify:_ not-testable-in-software (a test cannot assert the absence of a file it cannot name); repository review
  - _Source:_ controller/docs/DESIGN.md § Scope

## controller/docs/HARDWARE.md

- [ ] **USB-01** — At service start, set the FTDI latency_timer to 1 ms and set ASYNC_LOW_LATENCY on every RS-485 converter port, then read the value back and refuse to start if it does not read back 1 ms.
  - _Verify:_ unit (mock ioctl/sysfs) plus manual commissioning on real FTDI hardware
  - _Source:_ controller/docs/HARDWARE.md §5 'The three USB failure modes and their mitigations'; §13 check 2
- [ ] **USB-02** — Bind each zone to its converter by physical USB port path, never by /dev/ttyUSB* enumeration order, because duplicate or blank USB serial numbers are an expected failure mode.
  - _Verify:_ unit (device-path resolver against fixture sysfs trees) plus manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §5 'The three USB failure modes and their mitigations'
- [ ] **USB-04** — If a converter arrives with a non-FTDI bridge, establish that driver's equivalent low-latency setting and adjust the start-up read-back before the unit is used; do not ship a unit whose low-latency setting is unverified.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §5 (note after failure-mode table); §13 check 3
- [ ] **PROTO-04** — Do not implement the 20 ms Saturn echo timeout: auto-direction converters present no local echo, so it does not apply to this build.
  - _Verify:_ not-testable-in-software (hardware property); asserted by absence of echo-wait code
  - _Source:_ controller/docs/HARDWARE.md §6 table ('20 ms — n/a'); controller/docs/DECISIONS.md D2
- [ ] **PROTO-08** — Treat all §6 protocol parameters as unverified against this hardware and confirm them from the Phase 1 capture before they are relied on.
  - _Verify:_ manual commissioning (Phase 1 capture)
  - _Source:_ controller/docs/HARDWARE.md §6 preamble
- [ ] **TEMP-01** — Fit one PT1000 Class A 3-wire pipe-surface-clamp probe per zone, read through a MAX31865 (Adafruit PID 3648, 4300 Ω 0.1 % reference) on SPI0 with one chip select per channel; two channels fitted, electronics support four.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §7 'Chain'
- [ ] **TEMP-02** — Power each MAX31865 VIN from the Pi's 3V3 rail (never 5 V), because the breakout's level shifting follows VIN and SDO would otherwise drive 5 V onto a 3.3 V GPIO; configure each breakout for 3-wire.
  - _Verify:_ not-testable-in-software (wiring); manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §7 'Chain' note
- [ ] **TEMP-03** — Characterise the surface-clamp offset at commissioning against the Therma K immersion probe across the working range, and apply that correction before evaluating any threshold.
  - _Verify:_ unit (correction applied before threshold evaluation) plus manual commissioning (deriving the offset)
  - _Source:_ controller/docs/HARDWARE.md §7 'Placement and its limitation', consequence 1
- [ ] **TEMP-05** — Clamp the probe to the supply pipe of that zone's default outlet, as close to the valve as accessible pipe allows; exact location and pipe OD are set at the Phase 0 survey.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §7 'Placement and its limitation'
- [ ] **TEMP-06** — Scope the temperature interlock to the instrumented (default) outlet only; when a non-instrumented outlet is active there is no independent continuous measurement, and this limitation must be recorded in the commissioning report.
  - _Verify:_ unit (interlock keyed to instrumented outlet) plus manual commissioning (report entry)
  - _Source:_ controller/docs/HARDWARE.md §7 'Placement and its limitation', consequence 2
- [ ] **TEMP-07** — Verify every outlet individually with the immersion probe at Phase 4; the setpoint clamp and valve fault monitoring still apply to non-instrumented outlets.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §7 'Placement and its limitation', consequence 2
- [ ] **TIME-02** — Fit an Adafruit DS3231 Precision RTC (STEMMA QT, PID 5188) on I2C1 with a CR1220 coin cell, which is not supplied with the board.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §8 table
- [ ] **GPIO-01** — Wire the 40-pin header exactly as specified: pin 1 3V3 to both MAX31865 VIN; pin 3 GPIO2 I2C1 SDA to DS3231 SDA; pin 5 GPIO3 I2C1 SCL to DS3231 SCL; pin 6 GND to DS3231 GND; pin 9 GND to MAX31865 #1 GND; pin 14 GND to MAX31865 #2 GND; pin 17 3V3 to DS3231 VIN; pin 19 GPIO10 SPI0 MOSI to both SDI; pin 21 GPIO9 SPI0 MISO to both SDO; pin 23 GPIO11 SPI0 SCLK to both SCK; pin 24 GPIO8 SPI0 CE0 to MAX31865 #1 CS (zone 1); pin 26 GPIO7 SPI0 CE1 to MAX31865 #2 CS (zone 2).
  - _Verify:_ unit (chip-select-to-zone mapping constants) plus manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §8 'Pi 40-pin header assignments'
- [ ] **PWR-01** — Assume no mains conductor enters the enclosure; only the low-voltage output of an external USB-C supply passes through a gland, so software has no mains-side actuator to model.
  - _Verify:_ not-testable-in-software
  - _Source:_ controller/docs/HARDWARE.md §9 'Mains policy'; §3 closing note; §14 row 2
- [ ] **PWR-02** — Never join field-side grounds between links; one complete galvanic barrier exists per valve bus, the Pi's USB ground is host-side only and never reaches a valve, and field-side PE is connected per-zone only and only if measurement shows it is required.
  - _Verify:_ not-testable-in-software; bench checks §13 items 5 and 7
  - _Source:_ controller/docs/HARDWARE.md §9 'Isolation policy' items 1–3; §5 'Configuration at assembly' item 3
- [ ] **PWR-03** — Add RS-485 termination only where the factory topology has it; ship with termination jumpers off on both valve converters until the factory bus is captured and measured.
  - _Verify:_ manual commissioning; bench check §13 item 4
  - _Source:_ controller/docs/HARDWARE.md §9 'Isolation policy' item 4; §5 'Configuration at assembly' item 2
- [ ] **PWR-05** — Stay inside the power budget: total ≈ 6.6 W against a 15.3 W supply, and worst-case downstream USB draw of 600 mA across three converters, inside the Pi 4's documented USB budget.
  - _Verify:_ not-testable-in-software; manual measurement
  - _Source:_ controller/docs/HARDWARE.md §9 'Budget'
- [ ] **PWR-06** — Enable the BCM2711 hardware watchdog and systemd RuntimeWatchdogSec/WatchdogSec; on a watchdog reset the service must boot to READY_OFF with no state restored.
  - _Verify:_ emulator e2e (forced hang) plus manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §4 table; §2 table; §13 check 11
- [ ] **PWR-07** — Use wired Gigabit Ethernet for the control path with Wi-Fi and Bluetooth disabled in firmware; no radio is in the control path.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §4 table; §14 row 7
- [ ] **PWR-08** — Mount the root filesystem with journaling and write frame logs and session logs to a separate partition with bounded rotation.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §4 'Storage policy'
- [ ] **STEAM-19** — Make no assumption about the hard-loss case (USB gone, cable pulled, service dead, Pi unpowered): a dead process enforces no session limit, so steam falls entirely to the generator's own behaviour, which is not yet measured.
  - _Verify:_ not-testable-in-software; Phase 5 measurement
  - _Source:_ controller/docs/HARDWARE.md §12 'Losing the DTV+ link'
- [ ] **STEAM-20** — Enforce the service's own session limit for the degraded case, and assume the worst for the hard case until Phase 5 measures what the generator does when the DTV+ link drops mid-session.
  - _Verify:_ unit (session timer) plus manual commissioning (Phase 5)
  - _Source:_ controller/docs/HARDWARE.md §12 'Losing the DTV+ link' closing paragraphs
- [ ] **STEAM-21** — Give the steam link its own Waveshare SKU 23949 converter on a Pi USB 2.0 port with its own isolation barrier, and never join its field-side PE to either valve's.
  - _Verify:_ not-testable-in-software; manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §12 'Hardware' table
- [ ] **STEAM-22** — Wire the adapter lead as four conductors: connector B/A/GND (pins 1/2/3) to the converter's TB/TA/PE, plus pin 4 to a +V rail supplied by our master; pin 3 GND is mandatory on this link, unlike the valve links.
  - _Verify:_ not-testable-in-software; manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §12 'Before the connector can be built' — pinout table and the Ground row
- [ ] **STEAM-23** — Leave our converter's termination jumper off on the steam link: the adapter is already terminated (114 Ω measured across pins 1 and 2, R27), and a second terminator halves the bus to 60 Ω (40 Ω with a chained second adapter), below the 54 Ω RS-485 drivers are specified against.
  - _Verify:_ not-testable-in-software; bench measurement
  - _Source:_ controller/docs/HARDWARE.md §12 'Before the connector can be built' — Termination row and 'Why our termination stays off'
- [ ] **STEAM-24** — Add a 12 V rail to the enclosure to drive adapter pin 4, and confirm it before committing: apply 12 V to pin 4 from a current-limited supply with ground on pin 3 and measure IC2 pin 8 — about 5 V confirms the rail and the supply chain, and the current reading sizes the permanent supply.
  - _Verify:_ manual commissioning (bench measurement)
  - _Source:_ controller/docs/HARDWARE.md §12 'New build requirement: a 12 V rail in the enclosure'
- [ ] **STEAM-25** — Treat the generator and everything behind the adapter as out of scope; the generator owns its own safety envelope (low water/dry fire 0140-A, tank high-limit 0140-B, automatic fill shutoff, ¾″ pressure relief, room over-temperature 0120, session auto-shutoff) and this controller only sends setpoints.
  - _Verify:_ not-testable-in-software
  - _Source:_ controller/docs/HARDWARE.md §12 preamble and 'Out of scope'
- [ ] **STEAM-26** — Record the in-enclosure user-interface deviation in the commissioning report: Kohler's WARNING requires a user interface inside the steam enclosure, this design powers the K-99693 down at Phase 4, and the operator accepted the deviation on 2026-08-29.
  - _Verify:_ manual commissioning (report entry)
  - _Source:_ controller/docs/HARDWARE.md §12 'The in-enclosure interface requirement'
- [ ] **TEST-01** — Run all bench acceptance checks with all three converters on the Pi and nothing attached to the field side (Phase 2).
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 preamble
- [ ] **TEST-02** — Check 1: both converters enumerate with distinct USB serial numbers; pass requires two distinct IDs or documented port-path binding in use.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 1
- [ ] **TEST-03** — Check 2: latency_timer reads back 1 ms on both converters; the service refuses to start otherwise.
  - _Verify:_ unit plus manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 2
- [ ] **TEST-04** — Check 3: the bridge chip is identified and recorded; pass requires an FTDI part, or the driver's low-latency equivalent established.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 3
- [ ] **TEST-05** — Check 4: termination jumpers OFF, verified visually and by resistance — no 120 Ω across A/B.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 4
- [ ] **TEST-06** — Check 5: field-side PE terminals not joined — open circuit between the two zones' PE.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 5
- [ ] **TEST-07** — Check 6: loopback A↔B per converter in both directions; frames must decode without checksum error. May not be skipped — it catches a wiring error capable of bridging two buses.
  - _Verify:_ manual commissioning (with the decoder under test)
  - _Source:_ controller/docs/HARDWARE.md §13 table row 6 and the note below the table
- [ ] **TEST-08** — Check 7: zone-to-zone isolation — no continuity between zone 1 and zone 2 field terminals. May not be skipped.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 7 and the note below the table
- [ ] **TEST-09** — Check 8: RTD channels read ambient and read a known reference against the Therma K probe, within Class A tolerance plus the characterised offset.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 8
- [ ] **TEST-10** — Check 9: inject RTD open-circuit and short-circuit; the MAX31865 fault register must set and the service must command all-off.
  - _Verify:_ unit (fault-register handling) plus manual commissioning (injection)
  - _Source:_ controller/docs/HARDWARE.md §13 table row 9
- [ ] **TEST-11** — Check 10: the RTC holds time across a full power removal — time correct on the next boot before NTP.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 10
- [ ] **TEST-12** — Check 11: the hardware watchdog fires on a forced service hang; the Pi resets and boots to READY_OFF with no state restored.
  - _Verify:_ emulator e2e plus manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §13 table row 11
- [ ] **TEST-13** — Check 12: enclosure interior ≤ 40 °C after 7 days sealed, logged from the Pi's own thermal sensor.
  - _Verify:_ manual commissioning (7-day soak, with the service logging Pi thermals)
  - _Source:_ controller/docs/HARDWARE.md §13 table row 12; §10 'Interior ambient'
- [ ] **TEST-14** — Check 13: every label present and correct, and the emergency card in the lid.
  - _Verify:_ manual commissioning (visual)
  - _Source:_ controller/docs/HARDWARE.md §13 table row 13; §10 'Labelling and test points'
- [ ] **TEST-16** — State on the lid card that a WELDED fault (35) is a mechanically stuck mixing valve that no controller can close; the only remedy is removing valve power and closing the hot and cold service shutoffs.
  - _Verify:_ manual commissioning; the software must not present fault 35 as controller-recoverable
  - _Source:_ controller/docs/HARDWARE.md §10 (paragraph after the labelling table)
- [ ] **EXCL-01** — Exclude any relay, contactor, smart plug or cord switch in valve mains — a failing stop latency is not solved with an unreviewed relay.
  - _Verify:_ not-testable-in-software; design review
  - _Source:_ controller/docs/HARDWARE.md §14 row 1
- [ ] **EXCL-02** — Exclude mains wiring inside the enclosure, a UPS on the Pi, Wi-Fi in the control path, a custom PCB, and an industrial PLC.
  - _Verify:_ not-testable-in-software; design review
  - _Source:_ controller/docs/HARDWARE.md §14 rows 2, 3, 7, 8, 9
- [ ] **EXCL-03** — Exclude any dual-channel RS-485 converter or HAT (one isolation barrier shared by both zones), and generic MAX485/MAX3485/TTL-to-RS485 modules (mostly unisolated; some assert the transmitter during boot).
  - _Verify:_ not-testable-in-software; parts review
  - _Source:_ controller/docs/HARDWARE.md §14 rows 4 and 6; §5 'Why not a dual-channel part'
- [ ] **EXCL-04** — Do not use a bidirectional USB-RS-485 adapter as a capture tap — hardware automatic direction control is not physically receive-only.
  - _Verify:_ not-testable-in-software
  - _Source:_ controller/docs/HARDWARE.md §14 row 5
- [ ] **EXCL-05** — Do not add a second temperature sensor on the same element: a redundant sensor is not a redundant measurement; the immersion probe is the reference.
  - _Verify:_ not-testable-in-software; design review
  - _Source:_ controller/docs/HARDWARE.md §14 row 10
- [ ] **OPEN-01** — Do not transmit on a valve bus until Phase 1 capture establishes cable polarity (which conductor is A+); the TA/TB labels are the converter's convention, not Kohler's.
  - _Verify:_ manual commissioning (blocks first transmission)
  - _Source:_ controller/docs/HARDWARE.md §15 row 3; §11 'Cable polarity'
- [ ] **OPEN-02** — Resolve the Saturn response timeout — 320 ms or 400 ms — from the Phase 1 capture (I5) before fixing decoder deadlines.
  - _Verify:_ manual commissioning (Phase 1 capture); then unit constants
  - _Source:_ controller/docs/HARDWARE.md §15 row 8
- [ ] **OPEN-04** — Do not order field cabling by assumption: valve model/nameplate, connector housing/keying/pin count, factory termination and idle bias, outlet pipe OD and sensor location, valve mains voltage/receptacles/circuits/GFCI, and DTV+ peripheral-port pinout each stay open until their named measurement closes them.
  - _Verify:_ manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §15 rows 1, 2, 4, 5, 6, 10; §11
- [ ] **OPEN-05** — Record which FTDI part is fitted, FT232RL or FT232RNL; this blocks nothing but must be recorded.
  - _Verify:_ manual commissioning (inspection on arrival)
  - _Source:_ controller/docs/HARDWARE.md §15 row 7; §5 'Configuration at assembly' item 4
- [ ] **OPEN-06** — Measure at Phase 5 what the generator does when the DTV+ link drops mid-session (Kohler case #07797183 runs in parallel); until then the worst case is assumed.
  - _Verify:_ manual commissioning (Phase 5)
  - _Source:_ controller/docs/HARDWARE.md §15 row 11; §12 'Losing the DTV+ link'
- [ ] **ARCH-02** — If Phase 3 measures that a valve does not close on communication loss, the acceptance thresholds reject this architecture outright and a redesign — not a platform swap — is required.
  - _Verify:_ manual commissioning (Phase 3)
  - _Source:_ controller/docs/HARDWARE.md §2 (closing [I])
- [ ] **ARCH-03** — Never cut the original Kohler cables — they are the rollback path; use adapter leads instead, and keep the K-99695 ports disconnected, capped and labeled.
  - _Verify:_ not-testable-in-software; manual commissioning
  - _Source:_ controller/docs/HARDWARE.md §11 closing note; §3 block-diagram footer; §10 label 'OEM — DO NOT CUT'

## controller/docs/STEAM-ADAPTER.md

- [ ] **LIMIT-03** — Do not treat the configured maximum temperature as a safety guarantee: on the shipped controller it is an unbounded settings field written to save_variable.cgi index 58 with no min or max attribute.
  - _Verify:_ not-testable-in-software; enforced by LIMIT-01's independent hard bound
  - _Source:_ controller/docs/STEAM-ADAPTER.md §7 ("125 F is not a firmware constant, it is a settings field"); DISCLAIMER.md

## research/xagon0/PROVENANCE.md

- [ ] **META-01** — Treat every Saturn protocol constant below as tier [C] reverse-engineered third-party material: none of the five source documents carries a per-item tier tag, and research/xagon0/PROVENANCE.md establishes the whole xagon0 tree as an unlicensed verbatim copy of external reverse engineering, not measurement on our hardware.
  - _Verify:_ not-testable-in-software
  - _Source:_ research/xagon0/PROVENANCE.md — 'Vendored: xagon0/Kohler-DTV-Plus'

## research/xagon0/docs/protocols/dtv-plus-protocol.md

- [ ] **PHY-01** — Configure every DTV+ serial link as 9600 baud, 8 data bits, no parity, 1 stop bit, no flow control.
  - _Verify:_ unit (port-open config assertion) plus manual commissioning
  - _Source:_ research/xagon0/docs/protocols/dtv-plus-protocol.md § Physical Layer; controller/docs/STEAM-ADAPTER.md §2
- [ ] **PHY-03** — Assert DE and de-assert RE before transmitting, then de-assert DE and assert RE after the last byte, allowing a turnaround delay of at least one bit time (104.17 us at 9600 baud).
  - _Verify:_ manual commissioning with a scope; not-testable-in-software when the USB converter drives direction automatically
  - _Source:_ research/xagon0/docs/protocols/dtv-plus-protocol.md § Half-Duplex Control

## research/xagon0/docs/protocols/saturn-protocol.md

- [ ] **PHY-01** — Drive the Saturn bus as RS485 half-duplex at 9600 baud, 8 data bits, no parity, 1 stop bit, no hardware flow control.
  - _Verify:_ manual commissioning (scope/analyser); unit-testable only as a UART config assertion
  - _Source:_ research/xagon0/docs/protocols/saturn-protocol.md — Physical Layer
- [ ] **ADDR-03** — Make the master address a per-link configuration value with the two legal settings 0x00 (DTV master) and 0x10 (Prompt master), rather than hard-coding either — the two sources disagree on which applies to a Prompt 3 valve behind a DTV+ controller (see contradiction 'Master address selection rule').
  - _Verify:_ unit (config plumbed through); resolution requires manual commissioning against the real bus
  - _Source:_ research/xagon0/docs/protocols/saturn-protocol.md — Addressing / Implementation Notes; docs/devices/valve-control.md — Master Address Selection
- [ ] **ADDR-04** — Default the master address to 0x00 when integrating with DTV+ hardware, and expose 0x10 as an override to try when a valve does not answer at 0x00.
  - _Verify:_ manual commissioning
  - _Source:_ research/xagon0/docs/protocols/saturn-protocol.md — Implementation Notes / Master Address Selection
- [ ] **RESP-05** — Treat the byte order of every 2-byte response field (temperature 0x0B, flow 0x0C, fault bitmap 0x0F, outlet states 0x07) as unresolved: no source states endianness, so the codec must expose it as a configurable and log both interpretations until a real capture settles it.
  - _Verify:_ emulator e2e cannot settle this; needs a real bus capture (manual commissioning)
  - _Source:_ research/xagon0/docs/protocols/saturn-protocol.md — Response Packet Lengths (no endianness given anywhere in the document)
