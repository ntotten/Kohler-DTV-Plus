# Kohler DTV+ direct replacement controller design

Status: design proposal; no valve traffic or water actuation has been performed.

This document is the current plan. What happens in what order is
[BUILD-ORDER.md](BUILD-ORDER.md); options considered and not built are
[DECISIONS.md](DECISIONS.md), not here.

An open replacement master for the Kohler DTV+. A Raspberry Pi drives the two
Saturn valve buses over isolated serial links. The K-99695 and wall interface
become disconnected cold spares.
Returning to Kohler is a deliberate power-off cable swap, not an automatic
handoff.

### Scope

Goals:

- Control that does not depend on the K-99693 wall interface.
- A documented, inspectable protocol implementation.
- Instrumentation of the Saturn valve link.

The plan covers this installation specifically:

- Zone 1: one six-port valve, firmware `0.12`, five configured outlets.
- Zone 2: one three-port Prompt valve, firmware `0.14`, three configured
  outlets.
- Lighting, music, rain-panel and steam are not implemented and not planned.

**Steam is out of scope** — operator decision 2026-08-30, recorded in
[DECISIONS.md](DECISIONS.md#d12--like-for-like-scope-no-added-equipment-no-steam-setup).
The house has no steam generator. The DTV+ stack written before the descope
stays in the codebase, dormant and disabled — [HARDWARE.md § 12](HARDWARE.md) —
and the K-1737-K1 reference material stays in
[STEAM-ADAPTER.md](STEAM-ADAPTER.md).

Keep the household-specific configuration backup outside this public
repository.

The valve nameplates have not yet been photographed. The likely models are
K-682-K (six-port) and K-557-K1 (three-port), but those identities must be
confirmed before ordering connectors or doing any line-power work.

## Decision

Disconnect both valve data cables from the K-99695 and connect them only to a
Raspberry Pi through two packaged, isolated USB-to-RS-485 converters—one per
valve. Never electrically join the original and replacement controllers.

```text
                             wired Ethernet
                                   |
                     +-------------v--------------+
                     | Raspberry Pi 4             |
                     | API, logs, Homebridge      |
                     +--+---------+---------------+
                    USB |     USB |
              +---------v-+ +-----v-----+
              | Waveshare | | Waveshare |
              | 23949     | | 23949     |
              | isolated  | | isolated  |
              +-----+-----+ +-----+-----+
                    |             |
             RS-485 |      RS-485 |
              +-----v----+  +-----v----+
              | 6-port   |  | 3-port   |
              | valve    |  | valve    |
              | Saturn   |  | Saturn   |
              +----------+  +----------+

K-99695 ports:      disconnected, capped, and labeled
K-99695 controller: powered down after packet capture
```

The selected interface is two Waveshare `USB TO RS485/422` converters, SKU
`23949` — one per valve. Each receives its own packaged isolation barrier, automatic
direction control, protection circuitry, screw terminals, selectable 120-ohm
termination, USB cable, and DIN-rail enclosure. Only low-voltage adapter cables
and enclosure wiring remain custom.

The alternatives — a dual-controller selector, a dual-channel converter or HAT,
a bare-metal MCU, an industrial PLC — were each considered against this design
and rejected. [DECISIONS.md](DECISIONS.md) records every one, with the evidence
and the condition that would reopen it.

The tradeoffs are explicit:

- the Pi and Linux service directly own both Saturn sessions;
- there is no automatic fallback to the wall interface; and
- safety depends on measured proof that each valve closes when controller
  traffic disappears.

If the replacement fails, stop valve power and reconnect the labeled Kohler
cables manually.

## Safety boundary

Kohler specifies the likely valves as digital thermostatic mixing valves with
anti-scald protection and high-temperature limits. The reverse-engineered
firmware evidence indicates that the valve—not the K-99695—owns mixing,
thermistor sampling, motor control, over-temperature handling, and shutdown on
communication or power loss. See
[Valve Control: Safety Ownership](../../docs/devices/valve-control.md#safety-ownership)
and [Saturn Protocol](../../research/xagon0/docs/protocols/saturn-protocol.md).

Nothing is added to the installation: the valves stay in their existing
receptacles and circuits, and the safety case rests on the valve itself, proven
by measurement — commissioning must show each valve stops on controller
communication loss and power loss.

A `WELDED` fault (35) is a mechanically stuck mixing valve that no controller
can close; the service surfaces it as exactly that, and the remedy is removing
valve power.

### Acceptance thresholds

Commissioning must show each valve closes when Saturn traffic stops, the USB
adapter is unplugged, the controller process is killed or wedged, and Pi power
is removed.

Fail-off latency is measured at the outlet, from the last transmitted frame to
observed flow stop, on every fault path in the Phase 3 test list:

| Measured latency | Result                                                                           |
| ---------------- | -------------------------------------------------------------------------------- |
| ≤ 10 s           | Pass                                                                             |
| 10-30 s          | Does not proceed past Phase 3 without written justification and a second opinion |
| > 30 s           | Reject this architecture                                                         |

A failing result is not to be solved by putting an unreviewed hobby relay in a
valve's mains circuit.

Non-negotiable rules:

1. The K-99695 and replacement controller are never connected to the same valve
   bus.
2. Cable swaps happen only with both valves off and valve power removed.
3. The permanent valve master is the local Pi service, not Homebridge, a cloud
   service, or an AI system.
4. Pi boot, service restart, and watchdog-reset state is `OFF`; no prior
   water-on state is restored.
5. Loss of a valid response makes the service attempt `all-off` on the affected
   zone, close that zone's serial port, and latch that zone unavailable. The
   physical safety backstop is the valve's measured communication-loss shutdown.
   Escalation is scoped: a link fault takes one zone down; only a shared fault —
   the service process, the watchdog, the USB controller, or a failed
   configuration check — takes both down.
6. Setpoints are clamped at both ends before transmission:

   | Bound   | Value                           | Source                                                                             |
   | ------- | ------------------------------- | ---------------------------------------------------------------------------------- |
   | Ceiling | `Cx2 = 85` — 42.5 °C / 108.5 °F | 109 °F user-facing limit, rounded down to the 0.5 °C step below it                 |
   | Floor   | `Cx2 = 60` — 30 °C / 86 °F      | `MIN_SYS_VALVE_TEMP`; below it the valve returns error 3, _parameter out of range_ |

7. Custom sessions have a 20-minute hard limit. No keepalive may extend a
   session automatically. The limit sits below the Prompt 3 valve's own
   1800-second stop, whose refresh is only accepted once ≥ 900 s have elapsed;
   never sending that refresh leaves the valve's timer as an independent
   hardware backstop.
8. Calibration, arbitrary EEPROM writes, factory reset, bootloader, and valve
   firmware-update commands are absent from production firmware.
9. Invalid input and invalid wire data get different responses:

   | Condition                                                                                                                | Response                                               |
   | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
   | Request fails API validation — temperature outside the clamp, unconfigured outlet bit, unknown zone                      | Rejected to the caller. No valve state changes.        |
   | Reported fault, checksum failure on a write, malformed or out-of-range value in a valve response, missed safety response | `all-off`, zone latched unavailable until acknowledged |

10. AI, Homebridge, the Worker, and remote clients cannot send raw valve frames.
11. Existing automated HTTP reads against the K-99695 remain disabled. After
    cutover, the K-99695 is powered down.

## Hardware

The build is specified in [HARDWARE.md](HARDWARE.md): platform
decision, per-subsystem specification, GPIO map, power budget, isolation and
grounding policy, enclosure and labeling, and bench acceptance tests. Parts,
prices, and purchase links are in [SHOPPING-LIST.md](SHOPPING-LIST.md).

| Subsystem   | Choice                                                                          |
| ----------- | ------------------------------------------------------------------------------- |
| Compute     | Raspberry Pi 4 Model B 2 GB, Rust service, passive cooling, hardware watchdog   |
| Valve links | 2 × Waveshare `USB TO RS485/422` SKU `23949` — one isolated converter per valve |
| Timekeeping | DS3231 RTC on I2C. NTP sync state is still logged with every wall-clock stamp   |
| Enclosure   | IP65 non-metallic, DIN rail, low-voltage only — no mains conductor enters it    |

No custom PCB. No mains work inside the enclosure. No relay, contactor, smart
plug, or cord switch in either valve's mains path.

**Not orderable from documents.** The valve mating connectors wait for the
Phase 0 photographs, and any termination or bias component waits for the
Phase 1 capture — most likely none is needed. Everything else is ordered up
front; [SHOPPING-LIST.md](SHOPPING-LIST.md).

Official valve references:

- [Kohler K-682-K six-port specification](https://resources.kohler.com/webassets/kpna/catalog/pdf/en/K-682-K_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 three-port specification](https://techcomm.kohler.com/techcomm/pdf/K-557-K1_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 installation guide](https://resources.kohler.com/plumbing/kohlerla/pdf/1240338_2.pdf)

## Software design

### Pi controller service

Run one small controller daemon on the Pi, written in Rust. The platform
decision — Linux on a Pi rather than a bare-metal MCU — is recorded in
[HARDWARE.md § 2](HARDWARE.md), together with the condition that
would overturn it. The safety contract matters more than the language.

The two converters appear as separate USB serial interfaces. Bind logical zones
to stable device paths using each adapter's identity or physical USB path, not
incidental `/dev/ttyUSB0` enumeration order. Label each adapter after mapping
it. Refuse to start if both expected interfaces are not present and distinct.

**Set the USB-serial latency timer to 1 ms** (`latency_timer`, and
`ASYNC_LOW_LATENCY` where the driver offers it), and refuse to start if it does
not read back. The FTDI default is 16 ms, which adds up to that much delay to
every transaction and quantizes arrival times too coarsely to measure jitter
against the deadlines in the table below. `latency_timer` is FTDI-specific; if a
unit ships with a different bridge, establish that driver's equivalent before
the unit is used.

### Protocol parameters

All `[C]` — third-party reverse engineering, unverified against this hardware.
Confirm each from the Phase 1 capture before the encoder transmits.

| Parameter        | Value                                           | Note                                             |
| ---------------- | ----------------------------------------------- | ------------------------------------------------ |
| Line             | 9600 8N1, no flow control                       | RS-485 half-duplex                               |
| Max frame        | 20 bytes                                        | `AA 55` sync, addr, control, len, data, checksum |
| Checksum         | 2's complement over addr + control + len + data |                                                  |
| Valve tick       | 525 ms                                          | Master poll cadence                              |
| Response timeout | 400 ms                                          | Time to wait for a valve response                |
| Message timeout  | 320 ms                                          | Maximum time for a complete message to arrive    |
| Echo timeout     | 20 ms                                           | See the note on echo below                       |
| Enquiry rate     | 2000 ms                                         | Between address-discovery attempts               |
| Clear delay      | 2000 ms                                         | After address clear, before re-discovery         |
| Retries          | 3                                               | Read, write, and address management alike        |

Response timing is contradicted between sources and is tracked as
[I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory).

Echo: the stock master waits 20 ms for its own transmission to return on the
half-duplex bus. A converter with automatic direction control does not present
a local echo, so the replacement has no equivalent signal. The decoder and
emulator must not require one. The passive tap cannot distinguish master from
valve electrically; direction is inferred from address and content.

### Outlet index spaces

`outlet_set` in the public API is defined in configuration slot numbers — 1..6
for zone 1, 1..3 for zone 2. Three numbering schemes exist and do not agree:

| Space               | Where it appears                                                                   |
| ------------------- | ---------------------------------------------------------------------------------- |
| Configuration slot  | `one_type`..`six_type`, `valveN_outletM_func` key names, `quick_shower.cgi` digits |
| Status index (`id`) | `system_info.cgi`'s `valveNoutletM` booleans; bridged by `valveN_outletM_func.id`  |
| Saturn wire bitmap  | The bytes sent to the valve; differs per valve type                                |

Wire bitmaps per valve type:

| Valve            | Outlet numbering | First outlet mask |
| ---------------- | ---------------- | ----------------- |
| DTV 6-port       | Outlet 0..5      | `0x01`            |
| Prompt 3 generic | Outlet 1..6      | `0x04`            |

Both mappings live in one table with a regression test that permutes a slot.
The slot-to-status mapping is the identity on this system, so an identity-only
test does not exercise the code path — see
[`model.ts`](../../app/src/api/model.ts) and FIELD-NOTES §2, which records this
mapping dereferencing a null in a shipped Hubitat driver.

The service owns:

- discovery and one independent Saturn state machine per valve;
- exact request/response deadlines and checksum validation;
- the authoritative desired and actual state;
- the 20-minute monotonic runtime timer;
- setpoint and configured-outlet clamps;
- valve fault polling and all-off escalation;
- a monotonically increasing boot ID and command ID, so a restart cannot replay
  `start`;
- a local authenticated API and read-only event stream.

Safe boot sequence:

1. Open both serial interfaces without restoring or transmitting a start state.
2. Validate configuration, adapter identity, and physical port mapping.
3. Discover each valve using only the captured, approved startup sequence.
4. Read identity, firmware, outlets, temperature, and faults.
5. Send the captured all-off command and require acknowledgement.
6. Enter `READY_OFF` only after both zones are confirmed off.
7. Accept a start only after a fresh authenticated session and explicit user
   command.

If a valve cannot be confirmed off, that zone remains unavailable and the
operator is told to remove valve power.

The production encoder contains only captured and tested operational frames.
Permanently deny:

- calibration/configuration writes;
- factory reset;
- bootloader and firmware update;
- arbitrary EEPROM access;
- unknown commands and payload lengths;
- temperatures outside the local clamp;
- unconfigured outlet bits.

Address clear/allocation is allowed only if packet capture proves it is part of
normal boot discovery. It is legal only in the `DISCOVERY` state while water is
off, never after the link reaches `READY_OFF`.

Expose only constrained public operations:

- `start(zone, outlet_set, temperature_f, duration_seconds)`
- `set_temperature(zone, temperature_f)`
- `set_outlets(zone, outlet_set)`
- `pause(zone)`
- `resume(zone)`
- `stop(zone)`
- `stop_all()`
- `get_cached_state()`

The dormant steam stack exposes the same pattern (out of scope; disabled in
the deployed configuration):

- `steam_start(temperature_f, duration_minutes)`
- `steam_set_temperature(temperature_f)`
- `steam_set_duration(minutes)`
- `steam_stop()`

`stop_all()` stops steam as well as both valve zones. Power clean, deluge and
spa are denied in the encoder — [HARDWARE.md § 12](HARDWARE.md).

Homebridge and Worker status reads use the service cache. External callers
cannot trigger an extra valve transaction or send a raw Saturn frame. Commands
to the two valves are independently serialized, with at most one request
awaiting a response on each bus.

Configure the daemon as a hardened `systemd` service with no water-state
restoration, restart only into the OFF boot sequence, `WatchdogSec` application
heartbeats, the Pi hardware watchdog enabled, and bounded persistent logs. The
watchdog supports recovery and diagnosis; it does not replace the valve's
measured communication-loss shutdown.

Required logs:

- Pi boot ID, service boot ID, command ID, request source, and requested state;
- wall-clock timestamps paired with NTP sync state. The Pi 4 has no RTC, so
  stamps before first sync are wrong. Fit an RTC module or record sync state
  with each stamp;
- local safety clamps and rejection reason;
- raw RX/TX frame bytes with monotonic and wall-clock timestamps;
- acknowledgement latency, retry count, actual temperature, flow if supported,
  and valve fault flags;
- serial, watchdog, USB, service-restart, controller-power, and valve-power-loss
  events;
- session start, stop reason, duration, and maximum observed temperature.

No credential, access token, or pairing data belongs in these logs.

## Packet capture questions

The vendored reverse-engineering notes contradict themselves. Resolve these
from receive-only captures, not guesses:

1. Does the three-port valve use DTV+ master identity `0x00` or Prompt identity
   `0x10` here? _[valve-control.md](../../docs/devices/valve-control.md) says a Prompt
   3-Port always uses `0x10`;
   [saturn-protocol.md](../../research/xagon0/docs/protocols/saturn-protocol.md)
   says to always use `0x00` with DTV+ hardware — and its own worked example
   shows a `0x1E` Prompt 3-Port answering to master `0x00`. That example is
   evidence for `0x00`, but it is one third-party capture from unknown
   hardware, and it is inference until ours says the same._
2. What exact discovery and address-allocation frames does each valve use?
3. What are the exact all-off command and acknowledgement for each valve?
4. Does K-99695 write one compound desired state or separate temperature,
   outlet, pause, and primary-state values?
5. Which traffic refreshes the Prompt runtime timer, and when?
6. Does K-99695 continuously rewrite desired state or poll actual state?
7. What fault frames are observable?
8. Where are termination and idle bias applied?
9. What are the verified A/B/ground pins and polarity?
10. What is the register/control-byte map? Questions 1-9 resolve the frames;
    this resolves the numbering. Tracked with question 1 as
    [I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory).
11. Does the address-clear broadcast (`0x3A`/`0x03`) disturb calibration or
    other stored valve configuration? Zone 1 holds `v1_cal_code = 173`, zone 2
    holds `160`; both are recorded in the Phase 0 recovery baseline and re-read
    after the first discovery.
12. Is automatic purge enabled? Tracked as
    [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on).

Capture one valve at a time with no HTTP polling or other automation:

1. controller boot and discovery, water off;
2. idle polling;
3. start one outlet at 100 °F, stabilize, then stop. Record whether water flows
   before the valve reports on, and whether it continues after the stop is
   acknowledged ([I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on));
4. smallest temperature adjustment;
5. add and remove one outlet;
6. pause and resume; record whether paused time counts against the runtime
   timer;
7. normal stop;
8. a 22-minute safe-temperature run to observe timer maintenance. The refresh is
   only accepted once ≥ 900 s have elapsed;
9. orderly power cycle after the capture is saved and water is off.

The capture front end must be physically unable to transmit: termination off,
`DE` hard-strapped inactive, `RE` hard-strapped asserted, and no transmit
conductor from the USB UART. Strapping `DE` stops the transmitter; strapping
`RE` is what leaves a part that can only listen, rather than one that a glitch
or a driver can still put into driving.

The tap must also not load the bus it is listening to. The pair is already
terminated and biased at the controller and at the valve, so the tap adds
**neither** — a third 120 Ω halves the load the drivers see, and a parallel
fail-safe network shifts the idle level. These are two separate omissions, and
the bias one is the easier to leave populated by accident. Keep the stub to
inches: a long spur off a terminated pair is an unterminated reflection path.

Timestamp at the capture device. Use a logic analyzer where timing is the
finding; a 16 ms USB latency quantum does not resolve jitter on a 525 ms tick
or a 320 ms deadline.

### Purge handling

Conditional on [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on). If
automatic purge is enabled:

1. the state machine carries a purge state, and `get_cached_state()` reports
   water-is-moving separately from valve-is-on;
2. Phase 3 and Phase 4 stop-latency figures are measured against flow, not
   against the valve's acknowledgement;
3. "confirmed off" in the safe boot sequence means flow has stopped.

## Delivery phases

These are the authoritative phase definitions and gates. The sequencing view —
what is bought when, which measurement closes which order, and what can run in
parallel — is [BUILD-ORDER.md](BUILD-ORDER.md).

### Phase 0 — survey and recovery

- Revalidate the recovery backup and print the manual recovery instructions.
- Photograph both valve nameplates, plugs, controller ports, and connectors.
- Label every cable at both ends before disconnecting anything.
- Obtain or build two adapter leads so the original cables are not cut.
- Record the recovery baseline: both valve calibration codes (`v1_cal_code = 173`,
  zone 2 `160`), configured outlet slots and types, per-zone default and maximum
  temperatures, purge and runtime settings. Discovery is permitted to broadcast
  address clear; the baseline is the reference for confirming nothing else
  changed.

Gate: the factory topology can be restored from the labels in under five
minutes with power off.

### Phase 1 — passive capture

- Attach the physically receive-only front end in parallel temporarily.
- Capture the sequences above on both original buses.
- Implement a decoder and golden-frame fixtures.
- Quantify cadence, response deadlines, retry behavior, and bus utilization.

Gate: repeated captures decode without checksum errors and produce the same
state transitions.

### Phase 2 — offline controller and emulator

- Connect both packaged converters to the Pi without attaching either installed
  valve.
- Implement the allowlisted protocol and a valve emulator from captures.
- Test malformed lengths, checksum faults, delay, duplicates, partial frames,
  missing responses, address conflicts, service failure, Pi reset, and USB
  loss.
- Run the Pi, adapters, watchdog, service, and emulator continuously for seven
  days.

Gate: every injected failure ends in `OFF` without an unallowlisted write.

### Phase 3 — one-valve manual pilot

Prefer a matching donor valve on a bench, which avoids the mixed-state condition
below entirely.

**Mixed-state condition.** Moving one valve to the Pi leaves the K-99695 running
with a valve missing from one Saturn port. Its behaviour in that state is
unknown. Known: it detects device detachment within seconds and logs it
([I2](../../INVESTIGATIONS.md#i2--the-k-99693-interface-was-disconnected)), and
FIELD-NOTES §4 records that command experimentation can leave the system stuck.
The K-99693 wall interface shares that controller.

Select one option and record which:

| Option                                           | Cost                               | Condition                                                                                                                                         |
| ------------------------------------------------ | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Power the K-99695 down for the pilot (preferred) | No shower available during Phase 3 | Removes the unknown                                                                                                                               |
| Leave the K-99695 running                        | Zone 2 remains available           | Zone 2 locked out for the duration; controller error log and `values.cgi` monitored for the detach and subsequent behaviour, recorded as a result |

Procedure:

1. Stop and remove power from the pilot valve.
2. Disconnect only that valve's Kohler data cable and attach the custom cable.
3. Restore valve power. Commission at 100 °F on one outlet with the operator
   present and a thermometer on the outlet.
4. Limit the first active session to two minutes.

Test process kill, forced process hang, Pi power loss, USB disconnect, Pi
watchdog reset, A-wire open, B-wire open, bus short, and manual valve-power
removal.

Run the full manual rollback drill from the section below, timed, before leaving
this phase.

Gate: every failure stops flow or reaches the valve's measured fail-off path
within the acceptance threshold above, records a diagnostic reason, and requires
a deliberate new start. Record stop latency per fault path, maximum physical
temperature, and the K-99695's behaviour with a missing valve.

### Phase 4 — second valve and all outlets

- Repeat Phase 3 for the other valve.
- Verify every configured outlet independently with a thermometer.
- Run one zone at a time, then both zones, at 100 °F.
- Confirm the original K-99695 cables are capped, labeled, and stored at the
  controller.
- Power down the K-99695 and wall interface.

Gate: every outlet passes temperature and stop testing, and the manual rollback
drill succeeds.

### Phase 5 — removed (steam)

Steam is out of scope — operator decision 2026-08-30,
[DECISIONS.md](DECISIONS.md#d12--like-for-like-scope-no-added-equipment-no-steam-setup).
The phase number is kept so later phases and their citations stay stable.

### Phase 6 — local soak and integration

- Keep voice, cloud, Homebridge, and automatic routines disabled for a one-week
  local-only soak.
- Enable explicit Homebridge/Worker commands only after the soak has no
  unexplained reset, temperature, or bus event.
- Keep all external status reads cache-only.
- Keep automatic shower actuation disabled unless separately and explicitly
  approved.

Gate: the commissioning record is complete.

## Manual rollback to Kohler

1. Send `stop_all` and verify physical flow has stopped.
2. Remove power from both valve receptacles.
3. Remove power from the Pi and both USB converters.
4. Disconnect and cap the two custom valve data leads.
5. Reconnect each labeled OEM data cable to its original K-99695 valve port.
6. Restore K-99695 and wall-interface power.
7. Restore valve power.
8. Wait for discovery and confirm both valves appear, on the wall interface and
   independently in `values.cgi` (`valve_1_con_string` / `valve_2_con_string`
   reading `conn`). Do not rely on the touchscreen alone; its repair is an
   FDM-printed cover with an open question against it (I2).
9. Test one outlet per valve at 100 °F, then stop.
10. Re-read calibration codes and configured outlets against the Phase 0
    recovery baseline. Rollback is complete when the configuration matches the
    baseline, not when water flows.

## Captures and privacy

Raw captures may contain device serials. Keep them outside the public
repository, review any fixture before commit, and never include household
backups, network credentials, access tokens, or pairing material.

The implementation lives in this directory — the workspace layout is in
[controller/README.md](../README.md). An earlier proposed layout is recorded in
[DECISIONS.md](DECISIONS.md#w1--proposed-repository-layout-superseded).
