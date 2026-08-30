# Decision record

Settled decisions, the alternatives they beat, and what would reopen them. The
plan documents state each decision once and cite this file for the reasoning —
the plan stays readable, and the record stays complete, per
[AGENT.md](../../AGENT.md) rule 5.

Each of these was decided against a real alternative. Reopen one only with new
evidence, and record why here.

## Index

| #                                                                  | Decision                                                                                      | Record                                    |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- | ----------------------------------------- |
| [D1](#d1--direct-cable-swap-not-a-dual-controller-selector)        | Direct cable swap, no dual-controller selector                                                | Below                                     |
| [D2](#d2--raspberry-pi--rust-on-linux-not-a-bare-metal-mcu)        | Raspberry Pi + Rust on Linux, not an MCU                                                      | Below                                     |
| [D3](#d3--three-single-channel-converters-not-a-dual-channel-part) | Three single-channel converters, no shared HAT                                                | Below                                     |
| [D4](#d4--no-industrial-plc)                                       | No industrial PLC                                                                             | Below                                     |
| [D5](#d5--steam-through-the-adapter-not-direct-to-the-generator)   | Steam through the adapter, not direct _(moot — steam out of scope, D12)_                      | Below                                     |
| D6                                                                 | Our RS-485 termination stays **off** (steam)                                                  | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D7                                                                 | Signal ground **connected** on the steam link                                                 | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D8                                                                 | Pin 4 of the DTV+ bus is driven by us                                                         | [STEAM-ADAPTER.md § 12](STEAM-ADAPTER.md) |
| D9                                                                 | Kohler's in-enclosure interface `WARNING` — accepted deviation, operator decision 2026-08-29  | [STEAM-ADAPTER.md § 6](STEAM-ADAPTER.md)  |
| D10                                                                | No UPS on the Pi — power loss must reach the valves                                           | [HARDWARE.md § 4](HARDWARE.md)            |
| D11                                                                | No relay, contactor, smart plug or cord switch in valve mains, and the rest of the exclusions | [HARDWARE.md § 14](HARDWARE.md)           |
| [W1](#w1--proposed-repository-layout-superseded)                   | Proposed repository layout — superseded                                                       | Below                                     |
| [W2](#w2--the-modular-jack-inference-superseded)                   | The modular-jack inference — superseded                                                       | [STEAM-ADAPTER.md § 5](STEAM-ADAPTER.md)  |
| [W3](#w3--the-independent-temperature-sensor-removed)              | The independent temperature sensor — removed 2026-08-30                                       | Below                                     |
| [D12](#d12--like-for-like-scope-no-added-equipment-no-steam-setup) | Like-for-like scope: no added equipment, no steam setup — operator decision 2026-08-30        | Below                                     |

D6–D11 are specifications with their evidence attached, so their records live
in the spec; this index exists so nothing settled has to be rediscovered by
re-reading everything.

---

## D1 — Direct cable swap, not a dual-controller selector

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
