# Kohler DTV+ direct replacement controller design

Status: design proposal; no valve traffic or water actuation has been performed.

This plan makes a Raspberry Pi the active master for both installed valves,
speaking Saturn to them directly. The K-99695 and wall interface become
disconnected cold spares. Returning to Kohler is a deliberate power-off cable
swap, not an automatic handoff.

### Why, stated honestly

An earlier draft of this document called the K-99695 "unstable". That is not
what this project's own record shows, and it is worth correcting in the place
someone would read it. Every controller lockup observed here was self-inflicted:
[STORY-LOG.md](../../STORY-LOG.md) 2026-08-04 23:05 traces both hangs to
concurrent HTTP sessions of our own making, and notes that Kohler's own web page
polls at the same interval without trouble. The K-99695 has been reliable when
driven within its documented limits.

The actual reasons:

- **Control that does not depend on a $2013 touchscreen** in a wet wall, whose
  connector already failed once and whose repair is a printed part with an open
  question against it.
- **Owning the protocol**, so behaviour is inspectable and changeable rather
  than inferred from a CGI surface that omits, among other things, any measured
  water temperature.
- **Instrumenting the valve link**, which is the one place the open
  investigation can actually be seen from.

**This is not expected to fix
[I1](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use).** The leading
hypothesis for the mid-use shutoffs is a tankless heater minimum-flow cutout —
outside the DTV+ entirely. Replacing the master would not change it. What this
work does offer I1 is the passive capture in Phase 1, which can see the failure
directly; that is a diagnostic benefit, not a repair, and it should not be
described as one.

The plan covers this installation specifically:

- Zone 1: one six-port valve, firmware `0.12`, five configured outlets.
- Zone 2: one three-port Prompt valve, firmware `0.14`, three configured
  outlets.
- No steam, lighting, music, rain-panel, or other DTV+ peripherals are present.

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
                         +----------v-----------+
                         | Raspberry Pi 4       |
                         | API, logs, Homebridge|
                         +----------+-----------+
                                USB|         |USB
                     +-------------v-+     +-v-------------+
                     | Waveshare USB |     | Waveshare USB |
                     | TO RS485/422  |     | TO RS485/422  |
                     | isolated      |     | isolated      |
                     +--------+------+     +------+--------+
                              |                   |
                       RS-485 |                   | RS-485
                         +----v---+           +---v------+
                         | 6-port |           | 3-port  |
                         | valve  |           | valve   |
                         +--------+           +---------+

K-99695 valve ports: disconnected, capped, and labeled
K-99695 controller: powered down after packet capture
```

This is materially simpler than a dual-controller selector:

- two isolated valve links instead of four;
- no bus selector relays, relay drivers, interlocks, or arbitration proxy;
- no possibility of two masters transmitting on one valve bus;
- fewer cable paths and failure states;
- factory rollback remains possible with the original labeled cables.

The selected interface is two Waveshare `USB TO RS485/422` converters, SKU
`23949`. Each valve receives its own packaged isolation barrier, automatic
direction control, protection circuitry, screw terminals, selectable 120-ohm
termination, USB cable, and DIN-rail enclosure.

Waveshare's cheaper dual-channel SKU `27646` isolates its field side from USB
and supports independent communication, but the manufacturer does not document
channel-to-channel galvanic isolation. Two separate converters cost about $36
total and avoid making that assumption.

This removes the Pico, transceiver evaluation boards, isolated DC/DC modules,
external MCU watchdog, permanent custom PCB, and Pi-to-MCU protocol from the
design. Only low-voltage adapter cables and enclosure wiring remain custom.

The tradeoffs are explicit:

- the Pi and Linux service directly own both Saturn sessions;
- there is no automatic fallback to the wall interface; and
- safety depends on measured proof that each valve closes when controller
  traffic disappears.

If the replacement fails, stop valve power and reconnect the labeled Kohler
cables manually. An approximately $500 industrial PLC was considered and
rejected as poor value for this two-link installation.

## Safety boundary

Kohler specifies the likely valves as digital thermostatic mixing valves with
anti-scald protection and high-temperature limits. The reverse-engineered
firmware evidence indicates that the valve—not the K-99695—owns mixing,
thermistor sampling, motor control, over-temperature handling, and shutdown on
communication or power loss. See
[Valve Control: Safety Ownership](../devices/valve-control.md#safety-ownership)
and [Saturn Protocol](../../research/xagon0/docs/protocols/saturn-protocol.md).

That makes a replacement master feasible. It does not make the modified system
listed or certified. The official three-port specification identifies the
shipped valve under ASSE/ASME/CSA plumbing standards and UL 1951. A licensed
plumber and electrician should review the permanent installation.

The packaged adapter does not provide a separate safety processor or a way to
cut valve power. The installation therefore needs:

- accessible, clearly labeled manual power disconnects for both valve
  receptacles;
- accessible hot and cold service shutoffs;
- a posted emergency procedure, which must state that a `WELDED` fault (35) is
  a stuck mixing valve that **no controller can turn off** — the only remedy is
  valve power removal and the hot and cold service shutoffs;
- an independent outlet temperature measurement (see below);
- measured proof during commissioning that each valve stops on controller
  communication loss and power loss.

### One independent number

Everything else in this design trusts the valve's own thermistor, which
[DISCLAIMER.md](../../DISCLAIMER.md) explicitly says not to trust: the reported
temperature is the device's self-report, not a measurement. The commissioning
probe checks it once, at build time, and is then packed away.

Install a permanent temperature sensor on the outlet plumbing, read by the Pi,
logged with every session, and wired into the same `all-off` path as the valve's
own fault flags. This is not the separate safety processor rejected above — it
adds no actuation authority and cannot open anything. It is the only number in
the system not self-reported by the thing being watched.

It also happens to be the instrument
[INVESTIGATIONS.md](../../INVESTIGATIONS.md) E5 has been waiting on:
*"Confirming it needs a temperature sensor on the outlet, not a better poller"*
(STORY-LOG, 2026-08-04 22:55). One part, two jobs.

### The acceptance threshold, set in advance

This design is acceptable only if commissioning proves that each valve closes
reliably when Saturn traffic stops, the USB adapter is unplugged, the controller
process is killed or wedged, and Pi power is removed.

**Fail-off latency must be ≤ 10 seconds** from the last transmitted frame to
observed flow stop, measured at the outlet, on every fault path in the Phase 3
test list. Between 10 and 30 seconds the architecture is not rejected outright
but does not proceed past Phase 3 without a written justification and a second
opinion. Above 30 seconds, reject it.

The number is written here, before the measurement, on purpose. A gate whose
threshold is chosen after the result is not a gate — and the person running the
test will be the person who wants it to pass. Do not solve a failing result by
putting an unreviewed hobby relay in a valve's mains circuit.

Non-negotiable rules:

1. The K-99695 and replacement controller are never connected to the same valve
   bus.
2. Cable swaps happen only with both valves off and valve power removed.
3. The permanent valve master is the local Pi service, not Homebridge, a cloud
   service, or an AI system.
4. Pi boot, service restart, and watchdog-reset state is `OFF`; no prior
   water-on state is restored.
5. Loss of a valid response makes the service attempt `all-off` on the affected
   zone, close *that zone's* serial port, and latch that zone unavailable. The
   physical safety backstop is the valve's measured communication-loss shutdown.
   **Escalation is scoped deliberately:** a link fault takes down one zone; only
   a shared fault — the service process, the watchdog, the USB controller, or a
   failed configuration check — takes down both. Dropping a running shower in
   the healthy zone is itself a hazard, and is not the correct response to a
   moved USB cable in the other one.
6. Custom control is clamped at **both ends**. The user-facing ceiling starts at
   109 °F; Saturn uses 0.5 °C steps, so the on-wire ceiling is `Cx2 = 85`
   (42.5 °C / 108.5 °F), not the next higher step. The floor is `Cx2 = 60`
   (30 °C / 86 °F), which is `MIN_SYS_VALVE_TEMP` — a setpoint below it is
   rejected by the valve as error 3, *parameter out of range*, and under rule 9
   that would latch a zone off over a low request. Clamp, do not discover.
7. Custom sessions have a 20-minute hard limit. No keepalive may extend a
   session automatically. This sits deliberately *below* the Prompt 3 valve's
   own 1800-second hard stop, whose refresh is only accepted once ≥ 900 s have
   elapsed. Never sending that refresh means the valve's own timer remains an
   independent hardware backstop that fires even if the service is lying about
   elapsed time.
8. Calibration, arbitrary EEPROM writes, factory reset, bootloader, and valve
   firmware-update commands are absent from production firmware.
9. **Bad input and bad wire data are different failures and get different
   responses.** A request that fails validation at the API boundary —
   temperature outside the clamp, an unconfigured outlet bit, an unknown zone —
   is rejected with an error to the caller and changes no valve state. Only
   anomalies *on the wire* — a reported fault, a checksum failure on a write, a
   malformed or out-of-range value in a valve response, or a missed safety
   response — cause `all-off` and latch the zone unavailable until acknowledged.
   Conflating the two means one bad Homebridge value takes a zone out of
   service.
10. AI, Homebridge, the Worker, and remote clients cannot send raw valve frames.
11. Existing automated HTTP reads against the K-99695 remain disabled. After
    cutover, the K-99695 is powered down.

## Hardware to buy and install

Current manufacturer-direct and approved-reseller purchase links are maintained
in [SHOPPING-LIST.md](SHOPPING-LIST.md).

### Buy now

| Qty | Component                 | Specific choice / requirement                                      | Purpose                                                                              |
| --: | ------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
|   1 | Application computer      | Raspberry Pi 4 Model B, 2 GB                                       | Valve controller, local API, logs, and wired Ethernet.                               |
|   1 | Pi power supply           | Official Raspberry Pi 15 W USB-C supply                            | Independent low-voltage power; never tap a valve supply.                             |
|   2 | Storage cards             | 64 GB high-endurance microSD                                       | One installed and one imaged recovery spare.                                         |
|   2 | Valve interfaces          | Waveshare `USB TO RS485/422`, SKU `23949`; one dedicated per valve | Two separately isolated RS-485 links in DIN enclosures.                               |
|   1 | Passive-capture interface | Physically receive-only isolated RS-485 front end                  | Capture the factory buses before the replacement transmits.                          |
|   1 | Temperature instrument    | Calibrated fast-response immersion probe thermometer               | Independently verify delivered water temperature at commissioning.                   |
|   1 | Permanent outlet sensor   | Pi-readable temperature probe on outlet plumbing                   | The one number not self-reported by the valve; logged continuously. Also serves E5. |
|   1 | Electrical test set       | True-RMS meter; borrow an oscilloscope and isolated differential probe | Verify pins, polarity, idle bias, termination, and waveform without assuming labels. |

References:

- [Raspberry Pi 4 specifications](https://www.raspberrypi.com/products/raspberry-pi-4-model-b/specifications/)
- [Waveshare USB TO RS485/422](https://www.waveshare.com/usb-to-rs485-422.htm)
- [Waveshare documentation](https://www.waveshare.com/wiki/USB_TO_RS485/422)

### Permanent installation

No custom controller PCB is required. Install the Pi and two packaged
converters in a dry, serviceable enclosure with strain relief, labeled A/B test
points, and removable adapter leads for the two factory cables. Each converter
includes its USB cable and receives its isolated-side power from USB.

Configure each unit for two-wire RS-485: its `TA` terminal is A+, `TB` is B-,
and `RA`/`RB` are unused. Treat those labels as the converter-side convention;
capture and verify the Kohler cable polarity before making an adapter lead.

Leave both 120-ohm termination jumpers disabled until the factory topology has
been measured. Connect each converter's `PE` signal-ground terminal only if the
captured factory wiring and electrical review show that the reference conductor
is required. Do not join the two field-side `PE` terminals. Valve line power
remains in the original listed receptacles and wiring.

Verify the USB bridge chip on arrival rather than assuming it. More
importantly, **confirm the two converters report distinct USB serial numbers**
before installing either. Adapters in this class frequently ship with blank or
duplicated serials; two identical ones make a `by-id` symlink resolve both zones
onto the same device, which the "present and distinct" start check cannot catch
because the path does resolve. If the serials collide, bind by physical USB port
path instead and physically label the ports.

### Field-select after inspecting the installation

Do not order these by assumption:

| Component                         | Confirm first                                                                                                                                                |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| OEM mating connectors or pigtails | Photograph both ends, record keying and pin count, then verify A/B/ground continuity with all equipment unpowered.                                           |
| Custom valve cables               | Prefer adapter leads that preserve the factory cables. Do not cut the only OEM cable.                                                                        |
| RS-485 termination                | Measure the unpowered bus and capture the original waveform. Add only the termination present in the proven Kohler topology.                                 |
| Manual valve-power disconnect     | Electrician confirms both valve nameplates, actual voltage, receptacles, branch circuits, and GFCI protection before selecting a directly acting disconnect. |
| Replacement/donor valve           | Confirm K-number and revision from the installed label. A matching donor valve is preferred for bench testing.                                               |

Official valve references:

- [Kohler K-682-K six-port specification](https://resources.kohler.com/webassets/kpna/catalog/pdf/en/K-682-K_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 three-port specification](https://techcomm.kohler.com/techcomm/pdf/K-557-K1_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 installation guide](https://resources.kohler.com/plumbing/kohlerla/pdf/1240338_2.pdf)

## Software design

### Pi controller service

Run one small controller daemon on the Pi. Rust is preferred for the protocol
and state-machine implementation, but the safety contract matters more than the
language.

The two converters appear as separate USB serial interfaces. Bind logical zones
to stable device paths using each adapter's identity or physical USB path, not
incidental `/dev/ttyUSB0` enumeration order. Label each adapter after mapping
it. Refuse to start if both expected interfaces are not present and distinct.

**Set the USB-serial latency timer to 1 ms** (`latency_timer`, and
`ASYNC_LOW_LATENCY` where the driver offers it). The FTDI default is 16 ms,
which is larger than the protocol's 20 ms echo timeout and coarse enough to
smear every deadline in the table below. It is a one-line `udev` rule and it is
not optional — at this default, measured timings are the adapter's, not the
bus's.

### Protocol parameters

Starting values, all `[C]` — third-party reverse engineering, not yet verified
against our hardware. **Confirm every one of these from the Phase 1 capture
before the encoder transmits anything.** They are recorded here so the
implementation does not have to re-derive them, and so a disagreement with the
capture is visible rather than silently absorbed.

| Parameter | Value | Note |
| --- | --- | --- |
| Line | 9600 8N1, no flow control | RS-485 half-duplex |
| Max frame | 20 bytes | `AA 55` sync, addr, control, len, data, checksum |
| Checksum | 2's complement over addr + control + len + data | |
| Valve tick | 525 ms | Master poll cadence |
| Response timeout | 400 ms | Time to wait for a valve response |
| Message timeout | 320 ms | Maximum time for a complete message to arrive |
| Echo timeout | 20 ms | See the note on echo below |
| Enquiry rate | 2000 ms | Between address-discovery attempts |
| Clear delay | 2000 ms | After address clear, before re-discovery |
| Retries | 3 | Read, write, and address management alike |

The two vendored sources disagree on one of these:
[valve-control.md](../devices/valve-control.md) gives a single 320 ms
"communication timeout", while
[saturn-protocol.md](../../research/xagon0/docs/protocols/saturn-protocol.md)
splits it into a 400 ms response timeout and a 320 ms message timeout. The
capture settles it.

**On echo.** The stock master waits 20 ms for its own transmission to come back
on the half-duplex bus. A converter with automatic direction control generally
does not present a local echo, so the replacement has no equivalent signal.
That is not a safety problem with one master per bus, but the decoder and the
emulator must not expect echoes, and the passive tap cannot tell master from
valve electrically — direction has to be inferred from address and content.

### Outlet index spaces

`outlet_set` in the public API is defined in **configuration slot numbers**
(1..6 for zone 1, 1..3 for zone 2) and nothing else. Three different numbering
schemes are in play and they do not agree:

| Space | Where it appears |
| --- | --- |
| Configuration slot | `one_type`..`six_type`, `valveN_outletM_func` key names, `quick_shower.cgi` digits |
| Status index (`id`) | `system_info.cgi`'s `valveNoutletM` booleans — bridged by `valveN_outletM_func.id` |
| Saturn wire bitmap | The bytes actually sent to the valve — **and the two valves differ** |

The wire bitmaps are not the same shape on the two zones. The six-port valve
maps outlet 0 to bit 0 (`0x01`); the Prompt 3 generic map starts outlet 1 at
`0x04`. Encoding one valve's convention onto the other opens the wrong fitting.

Both mappings live in one table with a regression test that deliberately
permutes a slot, for the same reason
[`model.ts`](../../app/src/api/model.ts) already has one: on this system the
slot-to-status mapping happens to be the identity, so an identity-only test
proves nothing. FIELD-NOTES §2 records this trap dereferencing a null in a
shipped Hubitat driver.

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
- wall-clock timestamps **paired with NTP sync state** — a Pi 4 has no RTC, so
  every stamp before first sync is wrong, and correlating a shutoff against the
  tankless unit's own fault log needs real time. Add an RTC module or record
  the sync state alongside each stamp; do not emit a bare wall clock;
- the independent outlet temperature alongside the valve's reported one, so the
  two can be compared over the life of the installation rather than once;
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
   `0x10` here? *[valve-control.md](../devices/valve-control.md) says a Prompt
   3-Port always uses `0x10`;
   [saturn-protocol.md](../../research/xagon0/docs/protocols/saturn-protocol.md)
   says to always use `0x00` with DTV+ hardware — and its own worked example
   shows a `0x1E` Prompt 3-Port answering to master `0x00`. That example is
   evidence for `0x00`, but it is one third-party capture from unknown
   hardware, and it is inference until ours says the same.*
2. What exact discovery and address-allocation frames does each valve use?
3. What are the exact all-off command and acknowledgement for each valve?
4. Does K-99695 write one compound desired state or separate temperature,
   outlet, pause, and primary-state values?
5. Which traffic refreshes the Prompt runtime timer, and when?
6. Does K-99695 continuously rewrite desired state or poll actual state?
7. What fault frames are observable?
8. Where are termination and idle bias applied?
9. What are the verified A/B/ground pins and polarity?
10. **What is the actual register/control-byte map?** Tracked as
    [I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory).
    Questions 1-9 resolve the *frames*; this one resolves the *numbering*, and
    an encoder built on the wrong map sends a well-formed frame to the wrong
    register. Question 1 above is part of the same contradiction.
11. **Does the address-clear broadcast (`0x3A`/`0x03`) disturb calibration or
    any other stored valve configuration?** The design permits address clear
    during discovery. Zone 1 holds `v1_cal_code = 173` and zone 2 holds `160`;
    both are recorded in the recovery baseline before any capture begins, and
    both are re-read after the first discovery to confirm they survived.
12. **Is automatic purge on or off?** Tracked as
    [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on) — two of our own
    documents disagree, and the answer changes what `start` and `stop` mean
    physically. See the purge note below.

Capture one valve at a time with no HTTP polling or other automation:

1. controller boot and discovery, water off;
2. idle polling;
3. start one outlet at 100 °F, stabilize, then stop — **watching for purge**:
   note whether water flows before the valve reports on, and whether it
   continues after the stop command is acknowledged;
4. smallest temperature adjustment;
5. add and remove one outlet;
6. pause and resume — and whether paused time counts against the runtime timer;
7. normal stop;
8. a **22-minute** safe-temperature run to observe timer maintenance. The
   refresh is only accepted once ≥ 900 s have elapsed, so the original
   16-minute run left about a minute of window to catch it in;
9. orderly power cycle after the capture is saved and water is off;
10. **a run in the failing configuration, until it fails.** See below.

The capture front end must be physically unable to transmit: termination off,
`DE` hard-strapped inactive, and no transmit conductor from the USB UART.

Timestamp at the capture device, and prefer a logic-analyzer capture over
USB-serial for anything where the timing is the finding: a 16 ms USB latency
quantum cannot resolve jitter on a 525 ms tick or a 320 ms deadline.

### Scenario 10: capture a real shutoff

Everything above is the healthy path. This one is the reason the tap is worth
building at all.

[I1](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use) — the shower stops
mid-use — has been open since July, and its central difficulty is that the
failure is invisible to controller telemetry: the water stops first and the
K-99695 finds out roughly a minute later, through a timeout. E6 has been queued
behind that problem the whole time. A receive-only Saturn tap sits on the other
side of it. It sees the valve's own fault flags and the exact frame at which
state changes, without the controller's timeout in the way.

Method: once the tap is proven on scenarios 1-9, run the configuration that
fails — handshower alone, matching the 2026-07-14 conditions as closely as
possible — with the tap recording, until it stops or a reasonable time passes.

- **A valve fault frame precedes the stop:** the answer is in the valve or its
  supply, and we have the code. H0 and H4 separate immediately.
- **The valve stops with no fault and no command:** points hard at power or at
  the valve's own logic, and rules out anything the controller did.
- **Nothing stops:** also a result, and it constrains the conditions.

Cost, once the tap exists: one shower. This is the strongest evidence anyone
could hand Kohler about I1, and it needs no water actuation the operator would
not otherwise perform.

**⚠️ Consent:** moves water. Operator present. Record the result in
[STORY-LOG.md](../../STORY-LOG.md) and the verdict in
[INVESTIGATIONS.md](../../INVESTIGATIONS.md).

### The purge question

If automatic purge is enabled — and FIELD-NOTES §3 says it is on this
controller, while [system-specification.md](../system-specification.md) says it
is not — then water flows before the valve reports on, and possibly after the
stop is acknowledged. That changes three things in this design:

1. the state machine needs a purge state, and `get_cached_state()` must report
   water-is-moving distinctly from valve-is-on;
2. the Phase 3 and Phase 4 stop-latency measurements are measuring the wrong
   edge unless purge is accounted for;
3. "confirmed off" in the safe boot sequence means *flow has stopped*, not
   *the valve acknowledged off*.

Resolve it from the capture and from a fresh reading of the live controller
before commissioning, not from either document.

## Delivery phases

### Phase 0 — survey and recovery

- Revalidate the recovery backup and print the manual recovery instructions.
- Photograph both valve nameplates, plugs, controller ports, connectors,
  service shutoffs, and receptacles.
- Verify both hot and cold shutoffs are accessible.
- Have an electrician confirm GFCI protection and how to remove power from both
  valves quickly.
- Label every cable at both ends before disconnecting anything.
- Obtain or build two adapter leads so the original cables are not cut.
- Record the recovery baseline: both valve calibration codes (`v1_cal_code = 173`,
  zone 2 `160`), configured outlet slots and types, per-zone default and maximum
  temperatures, and the purge and runtime settings. Capture is permitted to
  broadcast address clear; the baseline is what proves nothing else moved.

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

Prefer a matching donor valve on a bench. That path avoids the whole problem
described next, and is worth real effort to obtain.

**The mixed state is the risk in this phase, and it is not the Pi.** Taking one
valve off the K-99695 leaves the controller running with a valve missing from
one of its two Saturn ports. Nobody knows what it does then. What we do know:
it notices device detachment within seconds and logs it — that is how
[I2](../../INVESTIGATIONS.md#i2--the-k-99693-interface-was-disconnected) was
diagnosed — and FIELD-NOTES §4 is titled *"Experimenting with commands can leave
the system stuck"*. Whether it retries discovery indefinitely, faults, or wedges
the way it wedges under concurrent HTTP is unknown. The wall interface sharing
that controller is the ~$2013 part that was just machined back into service.

Two acceptable options. Choose one deliberately and record which:

- **Preferred: power the K-99695 down for the whole pilot.** The household has
  no shower during Phase 3. This is the honest cost of the pilot and it removes
  the unknown entirely.
- **Otherwise: treat the K-99695's reaction as an observed result, not a
  background condition.** Before the pilot, predict what it will do. During it,
  watch the controller error log and `values.cgi` for the detach and for
  anything after it. Lock out zone 2 for the duration — nobody starts a shower
  from the wall interface while a bench test is running on the other bus.

Then:

1. Stop and remove power from the pilot valve.
2. Disconnect only that valve's Kohler data cable and attach the custom cable.
3. Restore valve power and commission at 100 °F on one outlet, operator present
   and **outside the spray path**, hand on the manual disconnect, independent
   probe reading the outlet throughout.
4. Keep the first active session to two minutes.

Test process kill, forced process hang, Pi power loss, USB disconnect, Pi
watchdog reset, A-wire open, B-wire open, bus short, and manual valve-power
removal.

Finish the phase by running the **full manual rollback drill** from the section
below, timed, with one valve moved. Doing it here rather than first at Phase 4
proves the five-minute claim from the Phase 0 gate while only one bus has
changed and the cost of it going wrong is lowest.

Gate: every failure stops flow or reaches the valve's measured fail-off path
**within the 10-second threshold set above**, records a diagnostic reason, and
requires a deliberate new start. Record stop latency per fault path and maximum
physical temperature. Record what the K-99695 did about its missing valve.

### Phase 4 — second valve and all outlets

- Repeat Phase 3 for the other valve.
- Verify every configured outlet independently with the calibrated thermometer.
- Run one zone at a time, then both zones, at 100 °F.
- Confirm the original K-99695 cables are capped, labeled, and stored at the
  controller.
- Power down the K-99695 and wall interface.

Gate: every outlet passes temperature and stop testing, and the manual rollback
drill succeeds.

### Phase 5 — local soak and integration

- Keep voice, cloud, Homebridge, and automatic routines disabled for a one-week
  local-only soak.
- Enable explicit Homebridge/Worker commands only after the soak has no
  unexplained reset, temperature, or bus event.
- Keep all external status reads cache-only.
- Keep automatic shower actuation disabled unless separately and explicitly
  approved.

Gate: signed commissioning report and homeowner-visible emergency instructions.

## Manual rollback to Kohler

1. Send `stop_all` and verify physical flow has stopped.
2. Remove power from both valve receptacles.
3. Remove power from the Pi and both USB converters.
4. Disconnect and cap the two custom valve data leads.
5. Reconnect each labeled OEM data cable to its original K-99695 valve port.
6. Restore K-99695 and wall-interface power.
7. Restore valve power.
8. Wait for discovery and confirm both valves appear — on the wall interface,
   and independently in `values.cgi` (`valve_1_con_string` / `valve_2_con_string`
   reading `conn`). Do not depend on the touchscreen alone: it is an
   FDM-printed rear cover in a shower wall with an open question about whether
   the repair holds, and the day it fails is exactly the day a rollback is
   being attempted.
9. Test one outlet per valve at 100 °F, then stop.
10. Re-read the calibration codes and configured outlets against the Phase 0
    recovery baseline. Rollback is not complete until the configuration matches
    what was recorded, not merely until water flows.

If water will not stop at any point: leave the shower, remove valve power, and
close both hot and cold service shutoffs. Do not troubleshoot a continuing-flow
or over-temperature event while standing in the shower. A `WELDED` fault (35)
is a mechanically stuck valve that no controller — ours or Kohler's — can close;
the shutoffs are the only remedy, and the valve needs replacing.

## Proposed repository layout for implementation

```text
controller/
  README.md                 # installation, connector labels, rollback
  protocol/                 # decoder, fixtures, emulator, capture schema
  service/                  # Pi daemon, local API, logs, install unit
  commissioning/            # test scripts and signed reports
hardware/replacement-controller/
  README.md                 # adapter leads, test points, enclosure layout
```

Raw captures may contain device serials. Keep them outside the public
repository, review any fixture before commit, and never include household
backups, network credentials, access tokens, or pairing material.
