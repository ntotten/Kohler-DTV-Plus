# Decision record

Settled decisions, the alternatives they beat, and what would reopen them. The
plan documents state each decision once and cite this file for the reasoning —
the plan stays readable, and the record stays complete, per
[AGENT.md](../../AGENT.md) rule 5.

Each of these was decided against a real alternative. Reopen one only with new
evidence, and record why here.

## Index

| #                                                                       | Decision                                                                                      | Record                                    |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ----------------------------------------- |
| [D1](#d1--direct-cable-swap-not-a-dual-controller-selector)             | ~~Direct cable swap, no dual-controller selector~~ — superseded 2026-10-24 by D13             | Below                                     |
| [D2](#d2--raspberry-pi--rust-on-linux-not-a-bare-metal-mcu)             | ~~Raspberry Pi + Rust on Linux, not an MCU~~ — superseded 2026-10-24 by D14                   | Below                                     |
| [D3](#d3--three-single-channel-converters-not-a-dual-channel-part)      | ~~Three single-channel converters, no shared HAT~~ — superseded 2026-10-24 by D15             | Below                                     |
| [D4](#d4--no-industrial-plc)                                            | No industrial PLC                                                                             | Below                                     |
| [D5](#d5--steam-through-the-adapter-not-direct-to-the-generator)        | Steam through the adapter, not direct _(moot — steam out of scope, D12)_                      | Below                                     |
| D6                                                                      | Our RS-485 termination stays **off** (steam)                                                  | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D7                                                                      | Signal ground **connected** on the steam link                                                 | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D8                                                                      | Pin 4 of the DTV+ bus is driven by us                                                         | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D9                                                                      | Kohler's in-enclosure interface `WARNING` — accepted deviation, operator decision 2026-08-29  | [STEAM-ADAPTER.md § 6](STEAM-ADAPTER.md)  |
| D10                                                                     | No UPS on the controller — power loss must reach the valves                                   | [HARDWARE.md § 4](HARDWARE.md)            |
| D11                                                                     | No relay, contactor, smart plug or cord switch in valve mains, and the rest of the exclusions | [HARDWARE.md § 14](HARDWARE.md)           |
| [W1](#w1--proposed-repository-layout-superseded)                        | Proposed repository layout — superseded                                                       | Below                                     |
| [W2](#w2--the-modular-jack-inference-superseded)                        | The modular-jack inference — superseded                                                       | [STEAM-ADAPTER.md § 5](STEAM-ADAPTER.md)  |
| [W3](#w3--the-independent-temperature-sensor-removed)                   | The independent temperature sensor — removed 2026-08-30                                       | Below                                     |
| [D12](#d12--like-for-like-scope-no-added-equipment-no-steam-setup)      | Like-for-like scope: no added equipment, no steam setup — operator decision 2026-08-30        | Below                                     |
| [W4](#w4--the-sync-excluded-saturn-checksum-superseded)                 | The sync-excluded Saturn checksum — superseded 2026-10-24                                     | Below                                     |
| [D13](#d13--in-line-through-the-k-99695-staged-no-bypass)               | In line through the K-99695, staged, no bypass relay — operator decision 2026-10-24           | Below                                     |
| [D14](#d14--rp2350-firmware-in-rust-not-a-linux-computer)               | RP2350 firmware in Rust, not a Linux computer                                                 | Below                                     |
| [D15](#d15--one-board-for-both-valves-and-the-panel-plain-transceivers) | One board for both valves and the panel, plain transceivers                                   | Below                                     |
| [D16](#d16--a-new-wall-panel-on-its-own-board-wired)                    | A new wall panel on its own board, wired — operator decision 2026-10-24                       | Below                                     |
| [W5](#w5--revision-a-build-superseded)                                  | Revision A build (Pi 4 and two USB converters) — superseded 2026-10-24                        | Below                                     |

D6–D11 are specifications with their evidence attached, so their records live
in the spec; this index exists so nothing settled has to be rediscovered by
re-reading everything.

---

## D1 — Direct cable swap, not a dual-controller selector

**Superseded 2026-10-24 by [D13](#d13--in-line-through-the-k-99695-staged-no-bypass)**, an operator
decision: the controller goes in line with each valve cable instead. That design does electrically join the
two controllers, through the ground and pin 5 conductors that pass straight through it. What it keeps from
this record is the property that mattered: two masters never transmit on one segment of a bus. The record
below is unchanged.

**Decision.** Disconnect both valve data cables from the K-99695 and connect
them only to the Pi. Returning to Kohler is a deliberate power-off cable swap.
Never electrically join the original and replacement controllers.

**Alternative.** A dual-controller selector: both masters stay wired, with bus
selector relays switching each valve between them.

**Why the swap wins.** It is materially simpler:

- three isolated links instead of six;
- no bus selector relays, relay drivers, interlocks, or arbitration proxy;
- no possibility of two masters transmitting on one valve bus;
- fewer cable paths and failure states;
- factory rollback remains possible with the original labeled cables.

Choosing packaged, isolated USB converters also removed the Pico, transceiver
evaluation boards, isolated DC/DC modules, external MCU watchdog, permanent
custom PCB, and Pi-to-MCU protocol from earlier drafts of the design. Only
low-voltage adapter cables and enclosure wiring remain custom.

**Would reopen it.** A demonstrated need for automatic fallback to the wall
interface — which the operator has not asked for, and which the manual rollback
procedure in [DESIGN.md](DESIGN.md) covers deliberately instead.

## D2 — Raspberry Pi + Rust on Linux, not a bare-metal MCU

**Superseded 2026-10-24 by [D14](#d14--rp2350-firmware-in-rust-not-a-linux-computer).** The reopen
clause below names only a safety reason, and that is not what changed. The in-line design needs five serial
ports rather than two, the operator asked for a smaller and cheaper build than the Pi and its USB converters,
and both criteria this record gave the Pi now have microcontroller answers. The record below is unchanged.

**Decision.** Raspberry Pi 4 Model B 2 GB running a Rust service on Linux.

**Alternative.** A bare-metal MCU (RP2040/STM32, Rust via `embassy` or RTIC)
driving RS-485 transceivers directly.

| Criterion                    | Pi 4 + Linux                                                     | Bare-metal MCU                              | Decides        |
| ---------------------------- | ---------------------------------------------------------------- | ------------------------------------------- | -------------- |
| Language                     | Rust                                                             | Rust                                        | Neither        |
| Tightest binding deadline    | 150 ms (steam tick); 320 ms Saturn message timeout               | same                                        | Pi — see below |
| 20 ms echo timeout           | Not applicable — auto-direction converters present no local echo | Not applicable, same reason                 | Neither        |
| Ethernet, local API          | On-board                                                         | Needs W5500/PHY + TCP + HTTP stack          | **Pi**         |
| NTP-stamped logs, 7-day soak | `systemd`, journald, real filesystem                             | Needs external flash, log format, transport | **Pi**         |
| Trusted computing base       | Large                                                            | Small                                       | **MCU**        |
| Watchdog                     | BCM2711 hardware watchdog + `systemd` `WatchdogSec`              | On-die                                      | Neither        |

**Why the deadlines do not favour an MCU.** Every figure that binds is ≥ 150 ms
([HARDWARE.md § 6](HARDWARE.md)). At 9600 baud a byte takes ~1.04 ms and a
maximum 20-byte Saturn frame takes ~21 ms to clock out; the wire, not the
scheduler, dominates. The only sub-100 ms number in the protocol is the 20 ms
echo timeout, and [DESIGN.md](DESIGN.md) already establishes it does not apply
to this build.

**Why determinism is not a safety argument here.** The master is a protocol
translator, not a safety controller.
[valve-control.md § Safety Ownership](../../docs/devices/valve-control.md#safety-ownership)
places mixing, the temperature envelope, over-temperature trips and
fail-closed-on-comms-loss inside the valve **[C]**. A late frame produces a
valve timeout and closure — the designed-for outcome, and the outcome Phase 3
measures. An MCU would buy determinism the failure model does not need.

**Would reopen it.** **[I]** An MCU becomes the right answer only if Phase 3
measures that a valve does **not** close on communication loss. In that case
the acceptance thresholds in [DESIGN.md](DESIGN.md) reject this architecture
outright, and a redesign — not a platform swap — is required.

## D3 — Three single-channel converters, not a dual-channel part

**Superseded 2026-10-24 by [D15](#d15--one-board-for-both-valves-and-the-panel-plain-transceivers).** In
line, each port shares its cable's ground conductor, so a barrier per port isolates nothing. The question
left over, whether one board may join the two valves' grounds, is a measurement on the K-99695 (D15). The
record below is unchanged.

**Decision.** One Waveshare `USB TO RS485/422` SKU `23949` per link — a
complete, separate isolation barrier for each of the three buses.

**Alternatives**, both rejected for the same property:

| Part                                                              | Isolation architecture                                                                                           | Verdict      |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------ |
| Waveshare SKU `27646`, dual-channel USB                           | Manufacturer does not document channel-to-channel galvanic isolation                                             | Rejected     |
| Waveshare `2-CH RS485 HAT`, SKU `17221` (SPI, SC16IS752 + SP3485) | Board carries **one** `B0505LS` isolated supply and **one** `π142M61` digital isolator for both channels **[V]** | Rejected     |
| Waveshare `USB TO 4CH RS485 (B)` (CH344L)                         | Manufacturer states USB↔RS485 isolation with **no isolation between the four RS485 channels** **[V]**            | Rejected     |
| 3 × Waveshare SKU `23949`                                         | One complete isolation barrier per link                                                                          | **Selected** |

**[I]** The 2-CH HAT would otherwise be attractive — it removes USB entirely and
mirrors Kohler's own architecture, which drives both valve ports from a
TL16C752C SPI/FlexBus dual UART **[C]** ([hardware.md](../../docs/hardware.md)).
It is rejected because a single barrier puts both zones on one isolated ground,
which is the exact property that disqualified SKU `27646`. SKU `27646` does
isolate its field side from USB and supports independent communication, but the
manufacturer does not document channel-to-channel galvanic isolation; separate
converters cost about $17 each and avoid making that assumption.

The 4-channel part (checked 2026-08-30,
[Waveshare wiki](https://www.waveshare.com/wiki/USB_TO_4CH_RS485_%28B%29)) fails the
same way, and this time the manufacturer says so directly: "the USB and four
RS485 interfaces are isolated, but there is no isolation between the four RS485
interfaces" **[V]**. All three field grounds would share one isolated domain —
and the steam link's ground tie is mandatory (D7), which would join it to both
valve buses, the exact thing the isolation policy forbids. It would also put
all three links behind one USB device and one CH344L bridge — a single failure
taking every link down, and a non-FTDI bridge whose low-latency equivalent
would have to be established before use ([HARDWARE.md § 5](HARDWARE.md)). At
3 × ~$17 for the single-channel units, the saving is roughly nothing.

**Would reopen it.** Manufacturer documentation establishing true
channel-to-channel galvanic isolation on a multi-channel part — and even then,
the saving is about $17.

## D4 — No industrial PLC

An approximately $500 industrial PLC (e.g. Unipi Patron) was considered and
rejected as poor value for this two-valve installation. It would still need the
same protocol work, the same capture campaign, and the same commissioning
measurements; the money buys packaging, not safety, because the valve owns the
safety envelope either way.

**Would reopen it.** Nothing foreseeable; recorded so the option is not
re-shopped.

## D5 — Steam through the adapter, not direct to the generator

**Decision.** The steam link talks DTV+ to the K-1737-K1 adapter. The
generator's native keypad port is not used.

Both are possible. The adapter wins on protocol, and it is not close:

|                  | Via the adapter                                                                                                        | Direct to the generator                                                                                                                                               |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Protocol         | **DTV+, documented** — framing, stuffing, checksum, discovery, command set and steam payloads all written down **[C]** | The generator's native keypad protocol. **No public analysis exists** — Kohler publishes installation and homeowner guides only, and no teardown or capture was found |
| Physical layer   | 4-pin header, opto-isolated RS-485 **[A][I]**                                                                          | 6-position modular jack, pinout and electrical standard unknown                                                                                                       |
| Room temperature | The kit's remote sensor wires to the adapter                                                                           | **Ours to solve.** In a native install the sensor lives in the in-room keypad, so a direct connection has to supply what the generator expects                        |
| Isolation        | Comes with the adapter                                                                                                 | Ours to add                                                                                                                                                           |
| Cost             | Already owned                                                                                                          | —                                                                                                                                                                     |
| Support          | Kohler's own topology; the installer wires it normally                                                                 | Unsupported                                                                                                                                                           |

Going direct trades a documented protocol for an undocumented one and inherits
the room-temperature problem. It buys one fewer box.

**Would reopen it:**

- The adapter fails or becomes unobtainable.
- The generator's installation guide, once a model is chosen, shows a simple
  interface. Worth ten minutes of reading at that point, not before.

**[I]** And if the native protocol is ever wanted, the cheapest route to it is
the adapter itself: put the receive-only analyser on the modular cable between
adapter and generator and record the adapter talking. That is a far better
starting point than probing the generator blind — but it needs a generator to
exist first.

---

## W1 — Proposed repository layout, superseded

The design originally proposed this layout for the implementation:

```text
controller/
  README.md                 # installation, connector labels, rollback
  protocol/                 # decoder, fixtures, emulator, capture schema
  service/                  # Pi daemon, local API, logs, install unit
  commissioning/            # test scripts and signed reports
hardware/replacement-controller/
  README.md                 # adapter leads, test points, enclosure layout
```

Superseded by the workspace as built — a Rust crate-per-concern layout under
[controller/](../README.md), with the design documents moved alongside it in
`controller/docs/` on 2026-08-30. `hardware/replacement-controller/` was never
created; the hardware documentation is [HARDWARE.md](HARDWARE.md).

## W2 — The modular-jack inference, superseded

An earlier reading of Kohler's adapter documentation inferred that DTV+
peripheral ports are telephone-style modular jacks and that a screw-terminal
converter would not be sufficient. **Both inferences were wrong** — the
2026-08-29 teardown found a 4-pin polarized header, and the DTV+ side is plain
half-duplex RS-485. The struck-through original text stays where it was
written, in [STEAM-ADAPTER.md § 5](STEAM-ADAPTER.md), with the correction;
the measured record is
[research/reference/steam-adapter/](../../research/reference/steam-adapter/).

## W3 — The independent temperature sensor, removed

**Removed from the plan on 2026-08-30, operator decision.** Through revision A
the build carried an independent outlet temperature subsystem: one PT1000
Class A surface-clamp probe per zone on a MAX31865 amplifier over SPI, wired
into the same `all-off` path as the valve fault flags, with a start refused
until the channel had spoken and a latch on over-temperature, fault-register,
starvation and divergence conditions.

**Why it went.** It was not load-bearing for the safety case. The stock system
runs with no independent measurement at all; the valve owns anti-scald, the
temperature envelope and fail-closed behaviour, and the replacement's safety
case rests on the setpoint clamps, the Phase 3 fail-off measurements and the
Therma K verification of every outlet at commissioning — none of which involve
the permanent sensor. Its authority was also weaker than it looked: it could
only command `all-off` through the same valve it was second-guessing, so in the
one scenario where a valve truly fails hot — a `WELDED` fault — it could do
nothing either. What it cost was real: ~$130 of probes and amplifiers, the only
soldering in the build, two enclosure glands, an SPI subsystem in software, and
a start path that refused without it.

The full implementation shipped and tested before removal — config, HAL
channel, safety events, supervisor sampling, e2e scenarios — and lives in git
history if it is ever wanted back.

**Would reopen it.** Evidence that the valve's thermistor self-report cannot be
trusted — for example a Phase 3 or Phase 4 Therma K measurement that disagrees
with the reported temperature beyond the sensor's tolerance.

## D12 — Like-for-like scope: no added equipment, no steam setup

**Operator decision, 2026-08-30. The project builds a controller that replaces
the K-99695 driving the two valves that exist today — nothing else. Do not
reintroduce any of the following into the plan, and do not re-raise them.**

Removed from the plan by this decision:

- **Manual valve-power disconnects** and every electrician work item — the
  GFCI survey, the nameplate-gated mains work, the "licensed review" language.
  The valves stay in their existing receptacles and circuits, untouched, the
  same as under the K-99695.
- **Posted emergency procedures**, the homeowner-visible instructions, and the
  enclosure-lid emergency card. The service still surfaces a `WELDED` fault
  (35) for what it is; that lives in software.
- **Instrument mandates** — the NIST-traceable Therma K requirement and the
  offset-characterisation ritual. A thermometer checks delivered water at
  commissioning; which thermometer is not a requirement.
- **The independent temperature sensor** — removed earlier the same day, [W3](#w3--the-independent-temperature-sensor-removed).
- **The whole steam setup**: the third converter, the 12 V rail, the adapter
  lead, Phase 5 commissioning, the generator/installer material, the kit
  pairing advice, and the in-enclosure interface deviation ([D9](#index), now
  moot). The house has no steam generator.

What this does **not** remove: the controller's own safety behaviour (the
clamps, boot-to-OFF, fail-off escalation, the transmit gate), the Phase 1–4
capture and commissioning measurements, and the dormant DTV+ steam code, which
was complete and tested before the descope and costs nothing to keep —
[HARDWARE.md § 12](HARDWARE.md). The K-1737-K1 reference record stays in
[STEAM-ADAPTER.md](STEAM-ADAPTER.md).

**Would reopen it.** A steam generator actually being installed reopens the
steam link, via [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md). Nothing else does.

**Amended 2026-10-24, operator decision.** The scope now covers the K-99693
wall interface as well as the K-99695: it is replaced by a new panel,
[D16](#d16--a-new-wall-panel-on-its-own-board-wired). Like the controller, the
panel replaces stock equipment and adds none.

## W4 — The sync-excluded Saturn checksum, superseded

Until 2026-10-24 `kdtv-proto` computed the Saturn checksum over `ADDRESS`
through `DATA` and left `SYNC1` and `SYNC2` out (`CHK-02` as first written).
The evidence was xagon0's response `AA 55 00 02 01 1E DF`, whose `0xDF` holds
only without the sync bytes. That frame is not a capture: the source's
"Annotated Packet Captures" are worked from its own stated formula, so it could
only ever agree with that formula.

**Superseded by measurement `[A]`.** Every frame captured on Valve 2's bus on
2026-10-24 — the K-99695's idle poll, the valve's replies, and the frames
around a start and a stop — closes only with the sync bytes in the sum
([research/diagnostics/2026-10-24-saturn-valve-bus/](../../research/diagnostics/2026-10-24-saturn-valve-bus/README.md)).
`0xAA + 0x55` is `0xFF`, so the old rule computed one less than the wire
carries on every frame, and a valve would have rejected every frame the
encoder built `[I]`. `CHK-01` is now the sync-inclusive rule, every fixture and
test vector carries the recomputed checksum, and the printed checksums in
xagon0's examples are not ported (`CHK-04`).

---

## D13 — In line through the K-99695, staged, no bypass

**Operator decision, 2026-10-24.** The controller sits in line with each valve
cable: one port faces the K-99695's valve jack, the other faces the valve. The
RS-485 pair (pins 3 and 4) is split at the controller; ground (pin 2) and the
valve's +12.7 V supply (pin 5) pass straight through. The controller is brought
into service in stages, set per valve in its configuration:

| Stage        | Master of the valve segment                                  | Master of the K-99695 segment                         | Wall interface                                                             |
| ------------ | ------------------------------------------------------------ | ----------------------------------------------------- | -------------------------------------------------------------------------- |
| Pass-through | The controller, retransmitting the K-99695's bytes unchanged | The K-99695; the controller retransmits valve replies | K-99693, through the K-99695                                               |
| Take-over    | The controller, from its own state machine                   | The K-99695; the controller answers it for the valve  | K-99693; its commands reach the valve under the controller's rails **[I]** |
| Cut-over     | The controller                                               | None; the K-99695 is powered down                     | The new panel, [D16](#d16--a-new-wall-panel-on-its-own-board-wired)        |

Every segment has exactly one master at every stage. That is what revision A's
"never connected to the same valve bus" protected, and it is the rule now.

**Alternatives:**

- The direct cable swap, [D1](#d1--direct-cable-swap-not-a-dual-controller-selector):
  each valve moves from the K-99695 to the controller in one power-off step,
  and the wall interface goes dark until a replacement exists.
- D1's rejected alternative, a dual-controller selector with bus relays.
- In line, plus a bypass relay that reconnects the K-99695 straight to the
  valve when the controller fails.

**Why in line wins:**

- The K-99695 and the K-99693 keep working through every stage until the new
  panel exists, so the shower stays in service through the build.
- Pass-through records both buses from inside the path, timestamped by the
  controller's own clock, with no parallel tap and no stub **[I]**.
- A stage change is a configuration setting, and a full rollback is moving two
  plugs per valve.
- Take-over removes the Phase 3 mixed-state condition, where the K-99695 runs
  with a valve missing, if the K-99695 accepts the controller's answers for the
  valve **[I]**. Phase 3 measures it.

**No bypass relay.** Operator decision: "Worst case I have to move the plugs."
The measurement behind it: the valve stopped the water about 2 s after the
K-99695's power was cut, with the line still held at a driven level, on the
silence alone **[A]**
([2026-10-24 bus capture § Fail-off](../../research/diagnostics/2026-10-24-saturn-valve-bus/README.md#fail-off-a)).
A dead controller is the same silence from the valve's side **[I]**, so its
failure leaves the water off. A bypass relay would be a part in the signal path
whose failure mode is two masters on one segment.

**What it costs.** From pass-through on, the controller is a single point of
failure for both valves: if it dies, both stop and the K-99695 loses them until
the plugs are moved **[I]**. The direct swap has the same exposure, from its
first day. Forwarding also adds about one byte time, ~1.04 ms, to each frame
on each hop **[I]**, against a reply that arrives 8.6–14.2 ms after the
request on the factory bus **[A]**. Phase 3 measures what the K-99695 tolerates.

**Would reopen it.** The K-99695 rejecting the forwarding delay in
pass-through, or the controller's answers in take-over. Either one returns that
stage to D1's direct swap, made at cut-over once the new panel is ready.

## D14 — RP2350 firmware in Rust, not a Linux computer

**Decision.** The controller is a WIZnet W5500-EVB-Pico2: an RP2350
microcontroller and a W5500 wired-Ethernet controller on one board, running
Rust firmware on [Embassy](https://embassy.dev). It replaces
[D2](#d2--raspberry-pi--rust-on-linux-not-a-bare-metal-mcu)'s Raspberry Pi 4.

**What changed.** D2 weighed a Pi against a bare microcontroller for a direct
swap with two links, and gave the Pi two criteria: Ethernet with a local API,
and stored logs for a seven-day soak. The in-line design needs five serial
ports (two per valve and one for the panel), and the operator asked for a
build smaller and cheaper than the Pi and its USB converters. Both of the Pi's
criteria now have microcontroller answers:

| Criterion           | Pi 4 + Linux (D2)                                                          | W5500-EVB-Pico2 + Embassy                                                                                              | Decides |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------- |
| Serial ports        | One USB converter per port, five for this design                           | 2 hardware UARTs and 12 PIO state machines **[V]**; `embassy-rp` ships a PIO UART                                      | **MCU** |
| Ethernet, local API | On-board                                                                   | W5500 on the board **[V]**; `embassy-net-wiznet`; `picoserve`, a `no_std` HTTP server with JSON and server-sent events | Neither |
| Logs for the soak   | journald on a filesystem                                                   | RAM ring buffer, drained over the LAN by the house's automation server; nothing written to flash **[I]**               | Neither |
| Boot to `OFF`       | Kernel, `systemd`, service start                                           | Firmware start, before the network is up **[I]**                                                                       | **MCU** |
| Trusted base        | Linux, `systemd`, the USB stack, an SD card                                | One firmware image                                                                                                     | **MCU** |
| Watchdog            | BCM2711 watchdog and `systemd`                                             | RP2350 hardware watchdog                                                                                               | Neither |
| Cost                | ~$420 in revision A for two links ([W5](#w5--revision-a-build-superseded)) | One £11.20 board **[V]**, five transceivers, a 5 V supply                                                              | **MCU** |

RP2350 figures: 520 kB SRAM, two Cortex-M33 cores at 150 MHz, three PIO blocks
**[V]** ([Adafruit](https://learn.adafruit.com/adafruit-feather-rp2350/overview)).
Board figures: 2 MB flash, W5500 on GPIO16–21 **[V]**
([The Pi Hut](https://thepihut.com/products/wiznet-w5500-evb-pico2)).

**ESP32, rejected.** The ESP32 has an Ethernet MAC, but the only `no_std` Rust
driver for it found on 2026-10-24 was `ph-esp32-mac` 0.1.x, first published in
February 2026, and the original ESP32's Xtensa cores build only with
Espressif's forked Rust toolchain (`espup`). An ESP32 with a W5500 works, and
is then this board with a radio the control path does not use.

**What it costs.** The Linux layer is rewritten for Embassy: `kdtv-hal`,
`kdtv-service`, `kdtv-api` and `kdtvd`, 14,374 lines with their tests. The
logic crates port to `no_std` with `alloc`. Today they are `std`: `kdtv-proto`
uses `serde_json`, `include_dir` and `sha2`; `kdtv-config` uses `toml`;
`kdtv-telemetry` uses `serde_json`, `tracing` and `jiff`. The
emulator, the e2e suite and `xtask` stay on the host.

**Would reopen it.** The ported logic not fitting 520 kB of SRAM and 2 MB of
flash, or the W5500 path failing the seven-day soak.

## D15 — One board for both valves and the panel, plain transceivers

**Decision.** One controller board carries all five RS-485 ports. Each port is
a TI THVD1410: half-duplex, with a driver-enable pin, not isolated. 120 Ω
termination at the controller's end of every segment.

**Alternatives:**

| Option                                                                                                                      | Why not                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| One board per valve                                                                                                         | Two supplies, two Ethernet drops, two firmware images, and a third place for the panel port. It separates the two zones' grounds only if the K-99695 does too, which is not known ([HARDWARE.md § 15](HARDWARE.md) item 8)                                                                                                                                                                                                                 |
| An isolated transceiver per port, D3's principle                                                                            | In line, pin 2 passes straight through, so each port is already tied to its cable's ground. A barrier between the port and that same ground isolates nothing                                                                                                                                                                                                                                                                               |
| ~~An auto-direction transceiver, TI THVD1406~~ — recommended in the design discussion on 2026-10-24, withdrawn the same day | Its datasheet: a low-to-high transition on D drives the bus high for 4–14 µs, then the driver turns off and the bus bias holds the line **[V]** ([THVD1406](https://www.ti.com/lit/gpn/THVD1406)). The K-99695 drives the full mark, +2.22 V differential **[A]**, and the valve's own bias is unsettled (+0.2 to +0.3 V, one short reading) **[A]**. A master that leaves its mark bits to a weak bias is not a like-for-like replacement |

**Zone to zone.** One board joins both valves' pin 2. If the K-99695 already
joins them, nothing changes. A power-off continuity check between pin 2 of its
two valve jacks settles it (item 8). If they are separate, one zone's two ports
take an isolated transceiver with its own isolated supply instead of the
THVD1410 **[I]**.

**Would reopen it.** Separate grounds in that check reopen isolation for one
zone. They do not reopen the board count.

## D16 — A new wall panel on its own board, wired

**Operator decision, 2026-10-24.** The K-99693 is not reused. A new panel is
built for the bathroom, on its own board with its own processor, wired to the
controller over the cable the K-99693 uses today. It must work with the
internet down.

- **Its own board.** "Keep the controller simple and focused, the UI is less
  critical" (operator). The controller owns the water; the panel is a client
  that sends the same constrained operations as the LAN API, under the same
  rails. A panel crash costs the panel, not the water.
- **Wired, over the existing in-wall cable.** Up to 25 ft of RJ45-terminated
  cable plus an in-line coupler **[K]**, carrying a 115200 baud UART, not
  Ethernet **[C]** ([wall-interface.md](../../docs/devices/wall-interface.md)).
  Both ends are new, so the pins are ours: RS-485 on one pair, power on two
  others ([HARDWARE.md § 16](HARDWARE.md)).
- **Offline by construction.** The panel talks only to the controller, on its
  own wire. Neither the internet nor the LAN is in its path.
- **Powered from the controller's end**, because there is mains power at the
  controller and none at the panel (operator). Passive DC from the panel's own
  adapter, not standard PoE: PoE needs power-sourcing and powered-device
  hardware at the two ends of one cable **[I]**.

**Alternatives:** reusing the K-99693, which would have the controller speak
the K-99695's Amulet protocol to a panel the operator doesn't want; the UI on
the controller board (operator); a wireless panel, which needs power at the
panel and puts a radio link back into its path **[I]**.

This amends [D12](#d12--like-for-like-scope-no-added-equipment-no-steam-setup).

**Would reopen it.** Nothing foreseeable.

## W5 — Revision A build, superseded

Revision A of [HARDWARE.md](HARDWARE.md), [SHOPPING-LIST.md](SHOPPING-LIST.md)
and [BUILD-ORDER.md](BUILD-ORDER.md) specified a direct cable swap (D1–D3) to a
Raspberry Pi 4 2 GB with two Waveshare `USB TO RS485/422` isolated converters
(SKU `23949`), a DS3231 RTC, two high-endurance microSD cards and an IP65
enclosure, about $420. It was never built. The two converters are owned and
are now the bench rig that plays the K-99695 and the valve
([HARDWARE.md § 13](HARDWARE.md)). Superseded 2026-10-24 by D13–D15. The full
revision A documents are in git at commit `e459eb0`.
