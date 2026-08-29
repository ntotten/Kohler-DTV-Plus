# Kohler DTV+ direct replacement controller design

Status: design proposal; no valve traffic or water actuation has been performed.

This plan replaces the unstable K-99695 controller as the active controller for
both installed valves. The K-99695 and wall interface become disconnected cold
spares. Returning to Kohler is a deliberate power-off cable swap, not an
automatic handoff.

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
- a posted emergency procedure;
- measured proof during commissioning that each valve stops on controller
  communication loss and power loss.

This design is acceptable only if commissioning proves that each valve closes
reliably when Saturn traffic stops, the USB adapter is unplugged, the controller
process is killed or wedged, and Pi power is removed. If that test fails or the
stop latency is unacceptable, reject this architecture. Do not solve it by
putting an unreviewed hobby relay in a valve's mains circuit.

Non-negotiable rules:

1. The K-99695 and replacement controller are never connected to the same valve
   bus.
2. Cable swaps happen only with both valves off and valve power removed.
3. The permanent valve master is the local Pi service, not Homebridge, a cloud
   service, or an AI system.
4. Pi boot, service restart, and watchdog-reset state is `OFF`; no prior
   water-on state is restored.
5. Loss of a valid response makes the service attempt `all-off`, close both
   serial ports, and latch the affected zone unavailable. The physical safety
   backstop is the valve's measured communication-loss shutdown.
6. Custom control starts with a 109 °F user-facing limit. Saturn uses 0.5 °C
   steps, so the on-wire ceiling is `Cx2 = 85` (42.5 °C / 108.5 °F), not the
   next higher step.
7. Custom sessions have a 20-minute hard limit. No keepalive may extend a
   session automatically.
8. Calibration, arbitrary EEPROM writes, factory reset, bootloader, and valve
   firmware-update commands are absent from production firmware.
9. A fault, invalid temperature, invalid outlet bitmap, checksum failure on a
   write, or missed safety response causes `all-off` and latches the zone
   unavailable until acknowledged.
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
|   1 | Temperature instrument    | Calibrated fast-response immersion probe thermometer               | Independently verify delivered water temperature.                                    |
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

The two FTDI-based converters appear as separate USB serial interfaces. Bind
logical zones to stable device paths using each adapter's identity or physical
USB path, not incidental `/dev/ttyUSB0` enumeration order. Label each adapter
after mapping it. Refuse to start if both expected interfaces are not present
and distinct.

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
   `0x10` here?
2. What exact discovery and address-allocation frames does each valve use?
3. What are the exact all-off command and acknowledgement for each valve?
4. Does K-99695 write one compound desired state or separate temperature,
   outlet, pause, and primary-state values?
5. Which traffic refreshes the Prompt runtime timer, and when?
6. Does K-99695 continuously rewrite desired state or poll actual state?
7. What fault frames are observable?
8. Where are termination and idle bias applied?
9. What are the verified A/B/ground pins and polarity?

Capture one valve at a time with no HTTP polling or other automation:

1. controller boot and discovery, water off;
2. idle polling;
3. start one outlet at 100 °F, stabilize, then stop;
4. smallest temperature adjustment;
5. add and remove one outlet;
6. pause and resume;
7. normal stop;
8. a 16-minute safe-temperature run to observe timer maintenance;
9. orderly power cycle after the capture is saved and water is off.

The capture front end must be physically unable to transmit: termination off,
`DE` hard-strapped inactive, and no transmit conductor from the USB UART.

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

Prefer a matching donor valve on a bench. If none is available:

1. Leave one valve connected to Kohler.
2. Stop and remove power from the pilot valve.
3. Disconnect only that valve's Kohler data cable and attach the custom cable.
4. Restore valve power and commission at 100 °F on one outlet with an operator
   present.
5. Keep the first active session to two minutes.

Test process kill, forced process hang, Pi power loss, USB disconnect, Pi
watchdog reset, A-wire open, B-wire open, bus short, and manual valve-power
removal.

Gate: every failure stops flow or reaches the valve's measured fail-off path,
records a diagnostic reason, and requires a deliberate new start. Record stop
latency and maximum physical temperature.

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
8. Wait for discovery and confirm both valves appear on the wall interface.
9. Test one outlet per valve at 100 °F, then stop.

If water will not stop at any point: leave the shower, remove valve power, and
close both hot and cold service shutoffs. Do not troubleshoot a continuing-flow
or over-temperature event while standing in the shower.

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
