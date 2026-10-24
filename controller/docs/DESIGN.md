# Kohler DTV+ direct replacement controller design

Status: design proposal; no valve traffic or water actuation has been performed.

This document is the current plan. What happens in what order is
[BUILD-ORDER.md](BUILD-ORDER.md); options considered and not built are
[DECISIONS.md](DECISIONS.md), not here.

An open replacement master for the Kohler DTV+. One microcontroller board sits
in line with both Saturn valve cables and takes the valves over in stages,
while the K-99695 and its wall interface keep working; a new wall panel
replaces the K-99693 at cut-over, and the K-99695 and K-99693 become cold
spares. Returning to Kohler is a deliberate power-off plug move, not an
automatic handoff.

### Scope

Goals:

- Control that does not depend on the K-99693 wall interface.
- A new wall panel that works with the internet down —
  [DECISIONS.md D16](DECISIONS.md#d16--a-new-wall-panel-on-its-own-board-wired).
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
confirmed before ordering connectors.

## Decision

Put the controller in line with each valve cable: one port faces the K-99695's
valve jack, the other faces the valve. The RS-485 pair (pins 3 and 4) is split
at the controller; ground (pin 2) and the valve's +12.7 V (pin 5) pass straight
through. Operator decision 2026-10-24 —
[DECISIONS.md D13](DECISIONS.md#d13--in-line-through-the-k-99695-staged-no-bypass).

```text
                               wired Ethernet: LAN API
                                         |
  K-99695            +-------------------v--------------------+        valves
  valve 1 jack <---->| K1 ── W5500-EVB-Pico2 (RP2350) ── V1    |<----> six-port
  valve 2 jack <---->| K2 ── Rust firmware on Embassy ── V2    |<----> three-port Prompt
                     |                     P                   |
                     +---------------------+-------------------+
                                           |
                             in-wall cable to the new wall panel

  pins 2 and 5: straight through, K1 to V1 and K2 to V2
```

Each valve moves through three stages, set per valve in the controller's
configuration:

| Stage        | Valve segment (`V`)                                      | K-99695 segment (`K`)                                                 | Wall interface                                                                   |
| ------------ | -------------------------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Pass-through | The controller retransmits the K-99695's bytes unchanged | The K-99695 is master; the controller retransmits the valve's replies | K-99693, through the K-99695                                                     |
| Take-over    | The controller is master, from its own state machine     | The K-99695 is master; the controller answers it for the valve        | K-99693; its commands become API operations under the controller's rails **[I]** |
| Cut-over     | The controller is master                                 | Unplugged; the K-99695 is powered down                                | The new panel                                                                    |

The selected hardware is one WIZnet W5500-EVB-Pico2 — an RP2350 and a W5500
Ethernet controller — with one TI THVD1410 transceiver per port: two per valve
and one for the panel
([DECISIONS.md D14](DECISIONS.md#d14--rp2350-firmware-in-rust-not-a-linux-computer),
[D15](DECISIONS.md#d15--one-board-for-both-valves-and-the-panel-plain-transceivers)).

The alternatives — the direct cable swap, a dual-controller selector, a
bypass relay, USB converters on a Linux computer, an isolated part per link,
an industrial PLC — were each considered and rejected. [DECISIONS.md](DECISIONS.md)
records every one, with the evidence and the condition that would reopen it.

The tradeoffs are explicit:

- the controller firmware owns every valve segment it has taken over;
- from pass-through on, the controller is in both valves' signal path, and a
  dead controller stops both;
- there is no automatic fallback to the K-99695; and
- safety depends on measured proof that each valve closes when controller
  traffic disappears. For the K-99695's own power loss it does, in about 2 s
  **[A]**.

If the replacement fails, stop valve power and plug the labeled Kohler cables
back into the K-99695.

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
can close; the controller surfaces it as exactly that, and the remedy is removing
valve power.

### Acceptance thresholds

Commissioning must show each valve closes when Saturn traffic stops, the
controller's firmware hangs or resets, its valve-side cable is pulled, and its
power is removed.

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

1. Every bus segment has exactly one master. The K-99695 and the controller
   never both transmit as master on one segment: in pass-through the K-99695
   masters the `K` segment and the controller the `V` segment; in take-over the
   controller never forwards the K-99695's frames to the valve. _Revision A
   read "the K-99695 and replacement controller are never connected to the same
   valve bus"; superseded 2026-10-24 by D13, which connects them through the
   pass-through conductors on purpose._
2. Cable swaps happen only with both valves off and valve power removed.
3. The permanent valve master is the controller's firmware, not the wall
   panel, Homebridge, a cloud service, or an AI system.
4. Controller boot, firmware restart, and watchdog-reset state is `OFF`; no
   prior water-on state is restored. A valve in pass-through resumes
   forwarding, so what happens next is the K-99695's own behaviour.
5. Loss of a valid response makes the controller attempt `all-off` on the affected
   zone, close that zone's serial port, and latch that zone unavailable. The
   physical safety backstop is the valve's measured communication-loss shutdown.
   Escalation is scoped: a link fault takes one zone down; only a shared fault —
   the firmware, the watchdog, or a failed configuration check — takes both
   down. Losing the network or the wall panel is not a valve fault.
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

10. AI, Homebridge, the Worker, the wall panel, and remote clients cannot send
    raw valve frames. In pass-through the controller forwards the K-99695's
    frames unmodified and originates none.
11. Existing automated HTTP reads against the K-99695 remain disabled. After
    cut-over, the K-99695 is powered down.
12. The controller never takes power from pin 5 and never feeds it; the valve
    supplies it **[A]**.

## Hardware

The build is specified in [HARDWARE.md](HARDWARE.md): platform
decision, per-subsystem specification, GPIO map, power budget, isolation and
grounding policy, enclosure and labeling, and bench acceptance tests. Parts,
prices, and purchase links are in [SHOPPING-LIST.md](SHOPPING-LIST.md).

| Subsystem   | Choice                                                                                     |
| ----------- | ------------------------------------------------------------------------------------------ |
| Compute     | WIZnet W5500-EVB-Pico2: RP2350 and W5500 wired Ethernet, Rust firmware, hardware watchdog  |
| Valve links | 4 × TI THVD1410, one per segment, 120 Ω at each; pins 2 and 5 straight through             |
| Panel link  | 1 × TI THVD1410 on the in-wall cable, passive DC from the panel's own adapter              |
| Timekeeping | Monotonic timer; wall clock from SNTP, sync state logged with every wall-clock stamp       |
| Enclosure   | Small non-metallic box beside the K-99695, low-voltage only — no mains conductor enters it |

No mains work inside the box. No relay, contactor, smart plug, or cord switch
in either valve's mains path, and no bypass relay around the controller.

**Not orderable from documents.** The valve connectors wait for the Phase 0
photograph of the plug, and the valve-segment bias values are set in Phase 3.
Everything else is ordered up front; [SHOPPING-LIST.md](SHOPPING-LIST.md).

Official valve references:

- [Kohler K-682-K six-port specification](https://resources.kohler.com/webassets/kpna/catalog/pdf/en/K-682-K_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 three-port specification](https://techcomm.kohler.com/techcomm/pdf/K-557-K1_spec_US-CA_Kohler_en.pdf)
- [Kohler K-557-K1 installation guide](https://resources.kohler.com/plumbing/kohlerla/pdf/1240338_2.pdf)

## Software design

### Controller firmware

Rust firmware on the RP2350, built on Embassy. The platform decision is
recorded in [HARDWARE.md § 2](HARDWARE.md) and
[DECISIONS.md D14](DECISIONS.md#d14--rp2350-firmware-in-rust-not-a-linux-computer),
together with what would overturn it. The safety contract matters more than
the language.

Ports are fixed by the board's wiring ([HARDWARE.md § 8](HARDWARE.md)), so
there is no enumeration step: `V1` and `V2` are always the valves, `K1` and
`K2` always the K-99695's jacks. Every port starts with its driver disabled,
and a resetting board cannot enable one (HARDWARE.md § 5).

The bus tasks — forwarding, the per-valve state machines, the panel link —
run at a higher interrupt priority than the network stack, and only they feed
the watchdog **[I]**. A wedged network task cannot delay a frame, and a wedged
bus task resets the board.

In pass-through the firmware forwards byte by byte: each byte received on one
port of a zone is sent on the other as soon as it has arrived, adding about one
byte time, ~1.04 ms, per hop **[I]**. It never forwards its own echo.

### Protocol parameters

All `[C]` — third-party reverse engineering, unverified against this hardware —
except where marked `[A]`. Confirm each from the Phase 1 capture before the
encoder transmits.

| Parameter        | Value                                                  | Note                                                                                                           |
| ---------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| Line             | 9600 8N1, no flow control                              | RS-485 half-duplex                                                                                             |
| Max frame        | 20 bytes                                               | `AA 55` sync, addr, control, len, data, checksum                                                               |
| Checksum         | 2's complement over sync + addr + control + len + data | `[A]`, measured 2026-10-24 (`research/diagnostics/2026-10-24-saturn-valve-bus/`): the whole frame sums to zero |
| Valve tick       | 525 ms                                                 | Master poll cadence; `[A]` as an average, 2026-10-24                                                           |
| Response timeout | 400 ms                                                 | Time to wait for a valve response                                                                              |
| Message timeout  | 320 ms                                                 | Maximum time for a complete message to arrive                                                                  |
| Echo timeout     | 20 ms                                                  | See the note on echo below                                                                                     |
| Enquiry rate     | 2000 ms                                                | Between address-discovery attempts                                                                             |
| Clear delay      | 2000 ms                                                | After address clear, before re-discovery                                                                       |
| Retries          | 3                                                      | Read, write, and address management alike                                                                      |

Response timing is contradicted between sources and is tracked as
[I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory).

Echo: the stock master waits 20 ms for its own transmission to return on the
half-duplex bus. Each of the controller's ports keeps its receiver enabled, so
it hears its own bytes, and the firmware compares them with what it sent
([HARDWARE.md § 5](HARDWARE.md)). The capture decoder must not require an echo:
a passive tap cannot distinguish master from valve electrically, and direction
is inferred from address and content. In pass-through the controller knows
which port each byte arrived on.

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

The firmware owns:

- each valve's stage — pass-through, take-over or cut-over — from
  configuration, changed only with that valve off;
- forwarding for every valve in pass-through;
- discovery and one independent Saturn state machine per valve it masters;
- exact request/response deadlines and checksum validation;
- the authoritative desired and actual state;
- the 20-minute monotonic runtime timer;
- setpoint and configured-outlet clamps;
- valve fault polling and all-off escalation;
- a monotonically increasing boot ID and command ID, so a restart cannot replay
  `start`;
- a local authenticated API and read-only event stream;
- the wall-panel link ([HARDWARE.md § 16](HARDWARE.md));
- in take-over, answering the K-99695 for the valve.

Safe boot sequence:

1. Bring up all five ports with every driver disabled, restoring and
   transmitting nothing.
2. Validate configuration and each valve's stage.
3. For a valve in pass-through, start forwarding; the K-99695 runs it as it
   does today, and steps 4–6 do not apply. For a valve the controller masters,
   discover it using only the captured, approved startup sequence.
4. Read identity, firmware, outlets, temperature, and faults.
5. Send the captured all-off command and require acknowledgement.
6. Enter `READY_OFF` only after every zone the controller masters is
   confirmed off.
7. Accept a start only after a fresh authenticated API session or a wall-panel
   command, and an explicit user command.

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

The wall panel calls the same operations over its own link, and in take-over
a K-99695 command decoded off the `K` segment is mapped to one of them; both
are checked against the same clamps as the API. A K-99695 command that maps to
none is not sent to the valve, and is logged **[I]**.

Homebridge and Worker status reads use the firmware's cache. External callers
cannot trigger an extra valve transaction or send a raw Saturn frame. Commands
to the two valves are independently serialized, with at most one request
awaiting a response on each bus.

A firmware reset boots only into the OFF sequence and restores no water state.
The watchdog supports recovery and diagnosis; it does not replace the valve's
measured communication-loss shutdown.

Required logs:

- boot ID, reset reason, command ID, request source (API, wall panel,
  K-99695), and requested state;
- monotonic microsecond timestamps, and wall-clock time with its SNTP sync
  state once synced. The board has no battery-backed clock;
- local safety clamps and rejection reason;
- raw RX/TX frame bytes with monotonic and wall-clock timestamps, and in
  pass-through the port each byte arrived on;
- acknowledgement latency, retry count, actual temperature, flow if supported,
  and valve fault flags;
- serial, watchdog, reset, stage-change, controller-power, and
  valve-power-loss events;
- session start, stop reason, duration, and maximum observed temperature.

The logs live in RAM and are read over the LAN API; nothing is written to
flash ([HARDWARE.md § 4](HARDWARE.md)). No credential, access token, or pairing
data belongs in these logs.

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
8. Where are termination and idle bias applied? _Termination answered
   2026-10-24: both ends **[A]**. The valve's own bias is not settled._
9. ~~What are the verified A/B/ground pins and polarity?~~ _Answered
   2026-10-24: pin 2 ground, pin 3 negative, pin 4 positive **[A]**
   ([bus capture](../../research/diagnostics/2026-10-24-saturn-valve-bus/README.md#connector-a))._
10. What is the register/control-byte map? Questions 1-9 resolve the frames;
    this resolves the numbering. Tracked with question 1 as
    [I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory).
11. Does the address-clear broadcast (`0x3A`/`0x03`) disturb calibration or
    other stored valve configuration? Zone 1 holds `v1_cal_code = 173`, zone 2
    holds `160`; both are recorded in the Phase 0 recovery baseline and re-read
    after the first discovery.
12. Is automatic purge enabled? Tracked as
    [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on).

In pass-through the controller records both segments from inside the path,
which serves the questions above without a parallel tap **[I]**. A parallel
tap, if one is used again, follows the rules below.

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
- Obtain or build the two `K` leads, so the original cables are not cut.
- With both valves' power off and their cables out, check continuity between
  pin 2 of the K-99695's two valve jacks
  ([HARDWARE.md § 15](HARDWARE.md) item 8).
- Count the wired contacts on the in-wall panel cable's plug at the controller
  end (item 13).
- Record the recovery baseline: both valve calibration codes (`v1_cal_code = 173`,
  zone 2 `160`), configured outlet slots and types, per-zone default and maximum
  temperatures, purge and runtime settings. Discovery is permitted to broadcast
  address clear; the baseline is the reference for confirming nothing else
  changed.

Gate: the factory topology can be restored from the labels in under five
minutes with power off.

### Phase 1 — passive capture

- Capture the sequences above on both original buses.
- Implement a decoder and golden-frame fixtures.
- Quantify cadence, response deadlines, retry behavior, and bus utilization.

Most of it was done with the scope on 2026-10-24 **[A]**
([bus capture](../../research/diagnostics/2026-10-24-saturn-valve-bus/README.md)).
What remains is recorded by the controller itself in Phase 3's pass-through.

Gate: repeated captures decode without checksum errors and produce the same
state transitions.

### Phase 2 — offline controller and emulator

- Build the controller on the bench with nothing installed attached. The two
  revision A USB converters play the K-99695 and the valve
  ([HARDWARE.md § 13](HARDWARE.md)).
- Implement the allowlisted protocol and a valve emulator from captures.
- Test malformed lengths, checksum faults, delay, duplicates, partial frames,
  missing responses, address conflicts, firmware hangs, resets, power loss,
  and network loss, in pass-through and in take-over.
- Run the controller, the bench rig and the emulator continuously for seven
  days.

Gate: every injected failure ends in `OFF` without an unallowlisted write.

### Phase 3 — in line: pass-through, then one valve taken over

**3a, pass-through.** Both valves.

1. Stop both valves and remove their power.
2. Unplug each valve cable from the K-99695 and plug it into its `V` port; plug
   the `K` leads into the K-99695's valve jacks.
3. Set the valve-segment bias against the factory idle level
   ([HARDWARE.md § 5](HARDWARE.md)), then restore valve power with both valves
   in pass-through.
4. Use the shower as normal through the K-99693. The controller forwards and
   logs, and originates nothing.

Measure the delay added per frame, the longest reply delay the K-99695
tolerates ([HARDWARE.md § 15](HARDWARE.md) item 6), and anything the K-99695
logs. Gate: a week of normal use with no checksum error and no K-99695 detach
event the factory bus does not have **[I]**.

**3b, take-over.** One valve.

1. With that valve off, set its stage to take-over.
2. Commission at 100 °F on one outlet with the operator present and a
   thermometer on the outlet.
3. Limit the first active session to two minutes.

The mixed-state condition revision A planned around — the K-99695 running with
a valve missing, behaviour unknown, the K-99693 sharing that controller — does
not arise if the K-99695 accepts the controller's answers for the valve (item
9). If it does not, 3b runs with the K-99695 powered down, revision A's
preferred option, and that valve returns to pass-through until cut-over.

Test firmware hang, controller reset, controller power loss, `V` cable pulled,
A-wire open, B-wire open, bus short, network loss, K-99695 power loss, and
manual valve-power removal.

Run the full manual rollback drill from the section below, timed, before leaving
this phase.

Gate: every failure stops flow or reaches the valve's measured fail-off path
within the acceptance threshold above, records a diagnostic reason, and requires
a deliberate new start. Record stop latency per fault path, maximum physical
temperature, and the K-99695's behaviour while it is answered for the valve.

### Phase 4 — second valve and all outlets

- Repeat Phase 3b for the other valve.
- Verify every configured outlet independently with a thermometer.
- Run one zone at a time, then both zones, at 100 °F.
- Keep the K-99695 and K-99693 powered: until Phase 7 they are the wall
  interface.

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

Gate: the commissioning record for the in-line stages is complete.

### Phase 7 — cut-over to the new wall panel

- Pass the panel's bench checks P1–P4 ([HARDWARE.md § 16](HARDWARE.md)).
- With both valves off and their power removed, install the panel on the
  in-wall cable, unplug the `K` leads, and store them with the K-99695's
  labels.
- Power down the K-99695 and the K-99693, and set both valves to cut-over.

Gate: P1–P4 pass in place, and the commissioning record is complete.

## Manual rollback to Kohler

A valve not yet at cut-over can go back to the K-99695 without touching a
cable: with it off, set its stage to pass-through, and the K-99695 runs it
again **[I]**. The full rollback:

1. Send `stop_all` and verify physical flow has stopped.
2. Remove power from both valve receptacles.
3. Remove power from the controller.
4. Unplug the valve cables from `V1` and `V2`, and the `K` leads from the
   K-99695.
5. Plug each labeled OEM valve cable into its original K-99695 valve jack.
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
