# Replacement controller — hardware specification

Status: **build specification, revision A.** Not yet built.

An open replacement master for the Kohler DTV+: two isolated Saturn valve
links driven from a Raspberry Pi. This specifies the unit described in
[DESIGN.md](DESIGN.md). That document owns the architecture, the safety rules
and the delivery phases; this one owns the hardware. Purchase links and prices
are in [SHOPPING-LIST.md](SHOPPING-LIST.md), and the order in which parts are
bought and phases run is [BUILD-ORDER.md](BUILD-ORDER.md). §12 documents the
dormant DTV+ steam stack the codebase carries; steam is otherwise out of
scope.

Evidence tiers follow [system-specification.md](../../docs/system-specification.md) —
**[A]** ours/measured, **[B]** shipped code, **[K]** Kohler primary, **[C]**
reverse-engineered, **[?]** unresolved, **[I]** inference. Component data cited
from a manufacturer's own published page is marked **[V]** and linked.

---

## 1. Links

| Link | Device            | Bus    | Protocol | Encoding | Subsystem |
| ---- | ----------------- | ------ | -------- | -------- | --------- |
| 1    | Valve, six-port   | RS-485 | Saturn   | Cx2      | §5        |
| 2    | Valve, three-port | RS-485 | Saturn   | Cx2      | §5        |

Each link gets its own converter, its own isolation barrier, and its own state
machine. Nothing is shared between them. The codebase also carries a dormant
DTV+ steam stack — §12 — which is out of scope of this plan and drives no
hardware.

### Reference configuration

The build is specified against a working installation, and every number in this
document that comes from hardware comes from it:

| Zone | Valve               | Firmware | Configured outlets | `max_temp` | Calibration |
| ---- | ------------------- | -------- | ------------------ | ---------- | ----------- |
| 1    | Six-port            | `0.12`   | 5                  | 113 °F     | `173`       |
| 2    | Three-port "Prompt" | `0.14`   | 3                  | 113 °F     | `160`       |

Valve models are most likely K-682-K and K-557-K1. Read the nameplates before
ordering connectors or doing mains work — §11.

---

## 2. Platform decision

**Raspberry Pi 4 Model B, 2 GB, running a Rust service on Linux. Not a
microcontroller.**

Every deadline that binds is ≥ 150 ms (§6), the wire rather than the scheduler
dominates at 9600 baud, and the valve — not the master — owns the safety
envelope, so an MCU would buy determinism the failure model does not need. The
full comparison against a bare-metal MCU, and the reasoning, are
[DECISIONS.md D2](DECISIONS.md#d2--raspberry-pi--rust-on-linux-not-a-bare-metal-mcu).

**[I]** An MCU becomes the right answer only if Phase 3 measures that a valve
does **not** close on communication loss. In that case the acceptance
thresholds in [DESIGN.md](DESIGN.md) reject this
architecture outright, and a redesign — not a platform swap — is required.

**Pi 4 over Pi 5:** the enclosure is sealed and passively cooled (§10). The Pi 4
2 GB dissipates ~3 W at this workload; the Pi 5 draws more and its documentation
calls for active cooling under sustained load. Compute is not a constraint —
this service moves at most a few hundred bytes per second.

---

## 3. Block diagram

```text
                    wired Ethernet (control path; Wi-Fi disabled)
                                      |
  5.1 V 3 A USB-C ──────┐   +---------v-----------------------------+
  (PSU outside the      └──>|  Raspberry Pi 4 Model B, 2 GB         |
   enclosure)               |   Rust service · local API · journald |
                            |   BCM2711 hardware watchdog           |
                            +---+-------+-------+---+
                            USB3|   USB3|   I2C1|
                                |       |       |
                                |       |       +--> DS3231 RTC
                                |       |            + CR1220 cell
                                |       |
          +---------------------+       |
          |                             |
 +--------v--------+           +--------v--------+
 | Waveshare 23949 |           | Waveshare 23949 |
 | FT232R+SP485EEN |           | (same part)     |
 | isolated        |           | isolated        |
 +--------+--------+           +--------+--------+
          | RS-485                      | RS-485
  +-------v-------+             +-------v-------+
  | Valve link 1  |             | Valve link 2  |
  | 6-port        |             | 3-port Prompt |
  | Saturn · Cx2  |             | Saturn · Cx2  |
  +---------------+             +---------------+

  K-99695 ports:     disconnected, capped, labeled
  K-99695 + K-99693: powered down after Phase 1 capture
```

Line power to both valves stays in the original listed receptacles and wiring.
No mains conductor enters this enclosure (§9).

---

## 4. Subsystem A — compute

| Item     | Specification                                                           |
| -------- | ----------------------------------------------------------------------- |
| Board    | Raspberry Pi 4 Model B, 2 GB LPDDR4                                     |
| Ambient  | 0–50 °C **[V]**; enclosure interior held ≤ 40 °C, verified in soak      |
| Cooling  | Passive metal case or heatsink. No fan — no moving part in a sealed box |
| Power in | Official Raspberry Pi 15 W USB-C supply, 5.1 V / 3 A                    |
| Network  | On-board Gigabit Ethernet. **Wi-Fi and Bluetooth disabled in firmware** |
| Storage  | 64 GB high-endurance microSD, plus one imaged spare held offline        |
| Watchdog | BCM2711 hardware watchdog enabled; `systemd` `RuntimeWatchdogSec`       |

**Storage policy.** Root filesystem mounted with journaling. Frame logs and
session logs go to a separate partition with bounded rotation. The spare card
is imaged at commissioning and re-imaged after any configuration change, so a
card failure is a swap, not a rebuild.

**No UPS.** Deliberate. A house power loss removes valve power, which closes
the valves — the correct outcome. Keeping the master alive through that event
adds a failure mode and buys nothing, because the master cannot act on valves
that have no power. Loss of Pi power is one of the fault paths Phase 3 measures.

---

## 5. Subsystem B — valve serial links

One converter per valve. Never shared.

### Selected part

Waveshare `USB TO RS485/422`, SKU `23949`. All rows **[V]**, from the
[manufacturer's product page](https://www.waveshare.com/usb-to-rs485-422.htm).

| Parameter         | Value                                                        |
| ----------------- | ------------------------------------------------------------ |
| USB bridge        | FTDI FT232R family                                           |
| Transceiver       | SP485EEN                                                     |
| Isolation         | Unibody power-supply isolation + unibody digital isolation   |
| Protection        | 15 kV ESD, 600 W surge/lightning, TVS, self-recovering fuse  |
| Host connector    | USB-B, 5 V, 200 mA self-recovering fuse                      |
| Field terminals   | Screw: `PE`, `TA`, `TB`, `RA`, `RB`                          |
| RS-485 assignment | `TA` = A+, `TB` = B−, `PE` = signal ground, `RA`/`RB` unused |
| Direction control | Hardware automatic                                           |
| Termination       | On-board 120 Ω, **disabled by default**, jumper-selectable   |
| Environment       | −15 – 70 °C, 5–95 % RH                                       |
| Mechanical        | ABS case for 35 mm DIN rail; 81.9 × 54.0 × 32.0 mm           |
| Included          | USB-A to USB-B cable, ≈1.2 m                                 |

### Why not a dual-channel part

Both dual-channel candidates — Waveshare SKU `27646` and the `2-CH RS485 HAT`
SKU `17221` — put both zones behind a single isolation barrier, which is the
one property this design refuses. The evidence and the full comparison are
[DECISIONS.md D3](DECISIONS.md#d3--three-single-channel-converters-not-a-dual-channel-part).

### The three USB failure modes and their mitigations

| Failure mode                          | Mitigation                                                                                                                          |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| FTDI 16 ms latency-timer quantum      | Set `latency_timer` to 1 ms and `ASYNC_LOW_LATENCY` at service start. Refuse to start if the value does not read back               |
| Duplicate or blank USB serial numbers | Bind zones by physical USB port path, not `/dev/ttyUSB*` order. Refuse to start unless both interfaces are present and **distinct** |
| Enumeration loss mid-session          | Treated as a link fault: `all-off` on that zone, close the port, latch the zone unavailable. Rule 5 of the design                   |

`latency_timer` is FTDI-specific. If either unit arrives with a non-FTDI
bridge, the equivalent low-latency setting for that driver must be established
and the start-up read-back adjusted before the unit is used.

### Configuration at assembly

1. Two-wire RS-485: `TA` → A+, `TB` → B−, `RA`/`RB` unused.
2. **Termination jumper OFF on both units** until the factory bus is captured
   and measured. Add termination only if it is present in the proven Kohler
   topology.
3. `PE` connected only if the captured wiring and the electrical review show a
   reference conductor is required. **The two field-side `PE` terminals are
   never joined to each other** — joining them defeats the per-valve barrier.
4. Verify the bridge chip on arrival. The manufacturer's page names `FT232RL`
   in its title and `FT232RNL` in its description **[?]**; record which is
   fitted.
5. Read both USB serial numbers before installing either unit. On collision,
   bind by port path and label the physical ports.

---

## 6. Protocol parameters the hardware must satisfy

Restated here because they set the timing budget. All **[C]**, unverified
against this hardware, to be confirmed from the Phase 1 capture.

| Parameter        | Saturn (valves) | DTV+ (steam, §12) |
| ---------------- | --------------- | ----------------- |
| Line             | 9600 8N1        | 9600 8N1          |
| Tick             | 525 ms          | 150 ms            |
| Response timeout | 400 ms          | 300 ms            |
| Message timeout  | 320 ms          | —                 |
| Echo timeout     | 20 ms — n/a     | 150 ms — n/a      |
| Retries          | 3               | 4 or 5 **[?]**    |
| Max frame        | 20 bytes        | variable          |

Byte time at 9600 8N1 is ~1.04 ms; a 20-byte frame occupies the wire ~21 ms.

---

## 7. Removed — independent temperature measurement

A PT1000/MAX31865 outlet temperature subsystem occupied this section through
revision A and was **removed from the plan on 2026-08-30**, operator decision —
[DECISIONS.md W3](DECISIONS.md#w3--the-independent-temperature-sensor-removed).
The section number is kept so every §8–§15 citation stays stable.

---

## 8. Subsystem D — timekeeping, and the GPIO map

The Pi 4 has no RTC. Timestamps before the first NTP sync are wrong, which
makes an unsynced boot's frame log unusable for correlating a fault.

| Item    | Part                                                   |
| ------- | ------------------------------------------------------ |
| RTC     | Adafruit DS3231 Precision RTC — STEMMA QT, PID `5188`  |
| Battery | CR1220 coin cell — **not included with the board [V]** |
| Bus     | I2C1                                                   |

Logs still record NTP sync state with every wall-clock stamp. The RTC narrows
the pre-sync window; it does not remove the requirement.

### Pi 40-pin header assignments

| Header pin | GPIO  | Function | Destination  |
| ---------: | ----- | -------- | ------------ |
|          3 | GPIO2 | I2C1 SDA | DS3231 `SDA` |
|          5 | GPIO3 | I2C1 SCL | DS3231 `SCL` |
|          6 | —     | GND      | DS3231 `GND` |
|         17 | —     | 3V3      | DS3231 `VIN` |

No GPIO drives a relay, a contactor, or anything in a mains path.

---

## 9. Power, isolation and grounding

### Budget

| Load                | Worst case  | Note                                     |
| ------------------- | ----------- | ---------------------------------------- |
| Raspberry Pi 4 2 GB | ~3.5 W      | This workload; idle ~2.7 W               |
| Converter × 3       | 3 × 1.0 W   | 200 mA fused each; actual draw is lower  |
| DS3231              | < 0.1 W     | On 3V3                                   |
| **Total**           | **≈ 6.6 W** | Supply is 15.3 W — better than 2× margin |

Worst-case downstream USB draw is 400 mA across two converters, inside the
Pi 4's documented total USB budget **[V]**.

### Isolation policy

1. One complete galvanic barrier per valve bus. **Field-side grounds are never
   joined.**
2. The Pi's USB ground is the common host-side reference. That is on the
   non-isolated side of every barrier and never reaches a valve.
3. Field-side `PE` is connected per-zone only, and only if measurement shows
   it is required.
4. Termination is added only where the factory topology has it.

### Mains policy

**No mains conductor enters this enclosure.** The Pi's USB-C supply plugs into
an existing receptacle outside the box; only its low-voltage output cable
passes through a gland. The build is entirely low-voltage, and this project
does no mains work anywhere: the valves stay in their existing receptacles and
circuits, untouched.

---

## 10. Enclosure, mechanical and environment

| Requirement       | Specification                                                                 |
| ----------------- | ----------------------------------------------------------------------------- |
| Location          | Dry, serviceable, ventilated. **Not inside the bathroom wet zone**            |
| Rating            | IP65 / NEMA 4X wall-mount, hinged or screw lid, non-metallic                  |
| Internal size     | ≥ 300 × 200 × 150 mm, with a mounting plate                                   |
| Rail              | 35 mm × 7.5 mm DIN, ≥ 200 mm usable length                                    |
| Rail budget       | 2 × converter at 81.9 × 54.0 × 32.0 mm, plus terminal blocks                  |
| Pi mounting       | DIN-rail carrier or plate standoffs, GPIO header accessible with the lid open |
| Breakout mounting | DS3231 on plate standoffs, not loose                                          |
| Glands            | 1 × Pi USB-C power, 1 × Ethernet, 2 × valve field cable                       |
| Interior ambient  | ≤ 40 °C, logged from the Pi's own thermal sensor through the seven-day soak   |
| Strain relief     | Every cable, at the gland. No conductor takes tension at a screw terminal     |

Size for one spare gland and ~55 mm of spare rail beyond this. Re-drilling a
sealed enclosure in service is worse than buying a larger one now.

### Labelling and test points

| Label                    | Applied to                                                  |
| ------------------------ | ----------------------------------------------------------- |
| `ZONE 1 · 6-PORT`        | Converter 1, its USB port, its field cable, both cable ends |
| `ZONE 2 · 3-PORT PROMPT` | Converter 2, its USB port, its field cable, both cable ends |
| `A+` / `B−` / `PE`       | Each field terminal                                         |
| `OEM — DO NOT CUT`       | Both original Kohler valve cables, at both ends             |

Bring A/B test points out per zone as labelled, insulated posts so the bus can
be metered without unlanding a conductor.

---

## 11. Field cabling — specified, not yet orderable

These items cannot be finalised from documents. Each row names the measurement
that closes it. Ordering by assumption is prohibited by
[DESIGN.md § Hardware, "Not orderable from documents"](DESIGN.md).

| Item                                  | What is unknown                               | Measurement that closes it                                                                  |
| ------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Valve mating connector / pigtail      | Housing, keying, pin count                    | Photograph both ends; record keying; verify A/B/ground continuity with everything unpowered |
| Adapter lead conductor gauge, length  | Run length, gauge                             | Measure the installed run at Phase 0                                                        |
| RS-485 termination and bias           | Whether the factory bus terminates, and where | Meter the unpowered bus; capture the original waveform in Phase 1                           |
| Cable polarity                        | Which conductor is A+                         | Phase 1 capture. The `TA`/`TB` labels are the converter's convention, not Kohler's          |
| Ferrules, glands, blocks, rail length | Final conductor gauges and layout             | Bench layout, after the above                                                               |

**Adapter leads, not modified OEM cables.** The original Kohler cables are the
rollback path and are never cut.

---

## 12. The dormant DTV+ steam stack

**Steam is out of scope of this plan** — operator decision 2026-08-30,
recorded in [DECISIONS.md](DECISIONS.md#d12--like-for-like-scope-no-added-equipment-no-steam-setup).
The house has no steam generator and nothing here is bought, wired or
commissioned for one.

What remains is code that existed before the descope: a complete DTV+ codec,
steam engine and emulator, exercised by the test suite and **disabled in the
deployed configuration** (`steam.enabled = false`). This section documents what
that dormant code enforces, because the requirement register cites it. The
K-1737-K1 adapter reference material — including everything measured on the
opened board — is [STEAM-ADAPTER.md](STEAM-ADAPTER.md).

### Protocol — DTV+, not Saturn

The valve links speak Saturn; steam speaks DTV+ on the same kind of wire at the
same baud rate. All **[C]**, from
[dtv-plus-protocol.md](../../research/xagon0/docs/protocols/dtv-plus-protocol.md)
and [steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md).

| Layer         | DTV+                                                                                                                                      |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Framing       | `SOF 0x88 · DEST · SRC · CMD · payload · CHECKSUM · EOF 0x55`                                                                             |
| Byte stuffing | `0x88`, `0x55`, `0xAA` escaped by a leading `0xAA`; SOF/EOF never escaped                                                                 |
| Checksum      | 2's complement of `DEST+SRC+CMD+payload`; verified by summing to `0x00`                                                                   |
| Discovery     | `DEV_ADDRESS_OPP 0x05` broadcast → `DEV_REQUEST_ADDR 0x06` with device ID → `DEV_ASSIGN_ADDR 0x07`                                        |
| Addressing    | Device ships at `0x00`; master assigns `0x03`–`0x07`; `0xFF` broadcast. Steam device ID is `0x05`                                         |
| Commands      | `GET_DEV_STATUS 0x30`, `STATUS_UPDATE 0x31`, `SET_DEV_PARAM 0x34`, `DEV_ACK 0x35`, `DEV_NAK 0x36`, `ERROR 0x37`, `CLEAR_FAULT_FLAGS 0x3A` |
| Tick          | 150 ms; reply timeout 300 ms                                                                                                              |
| Temperature   | **Fx2 — Fahrenheit × 2.** Valves use Cx2                                                                                                  |

**The encoding split is the one real implementation hazard.** `Fx2 = ((Cx2 × 9) / 5) + 64`.
Make Cx2 and Fx2 distinct types that cannot be assigned to each other, and put
the conversion in exactly one place. A units error here is not caught by range
checking: Fx2 for 110 °F is `220`, which read as Cx2 asks a valve for 110 °C.

### Limits

| Limit                    | Value                                                   | Tier       |
| ------------------------ | ------------------------------------------------------- | ---------- |
| Setpoint range           | 90 °F (32 °C) – 125 °F (52 °C), 1 °F steps              | **[K]**    |
| Factory default          | 110 °F                                                  | **[K][A]** |
| Session duration         | 1–20 minutes; default 10                                | **[K][B]** |
| Generator's own envelope | Max 125 °F, min operating 90 °F, min run 10 min, max 20 | **[K]**    |

The controller's limits and the generator's are the same numbers, which is
consistent with the generator owning the envelope and the DTV+ path inheriting
it **[I]**.

Clamp in our own code at both ends before transmission, the same way the valve
setpoints are clamped. Note that `steam_max_temp` is an installer settings field
with no `min`/`max` in the shipped web UI **[B]** — treat it as configuration,
not as a guarantee, exactly as the water `max_temp` is treated.

### Command allowlist

Same rule as the valve encoder: only captured, tested, operational frames ship.

**Exposed:** `steam_start(temperature_f, duration_minutes)`,
`steam_set_temperature`, `steam_set_duration`, `steam_stop`, `get_cached_state`.

**Permanently denied:**

| Denied                                   | Reason                                                                                                 |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Power clean (`STEAM_POWER_CLEAN` `0xCC`) | 45-minute unattended cycle discharging through the steam head. Run it from the generator's own control |
| Deluge                                   | Opens a Saturn valve from a steam session. Cross-bus actuation stays out of the first implementation   |
| Spa "Steam Coach"                        | Mutually exclusive with steam; the shipped UI carries the interlock **[B]**                            |
| Address clear outside `DISCOVERY`        | Same rule as the valve links                                                                           |
| Anything not in the captured frame set   | Same rule as the valve links                                                                           |

Power clean being denied on our side does not disable it — the generator
tracks its own 600-minute cumulative counter and reminds through its own
control **[K]**.

### Losing the DTV+ link

Three links exist and only two are ours: Pi ↔ valve (Saturn), Pi ↔ adapter
(DTV+), and adapter ↔ generator (Kohler's crossover cable). This is about the
second.

Two cases, and they behave differently:

| Case                                                                        | What the service does                                                                           |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| **Degraded but alive** — timeouts, NAKs, checksum failures, port still open | Transmit still works: command `steam_stop`, require acknowledgement, latch the link unavailable |
| **Hard loss** — USB gone, cable pulled, service dead, Pi unpowered          | Nothing can be sent. Steam is entirely on the generator's own behaviour                         |

The service enforces its own session limit for the degraded case.

For the hard case it assumes nothing: a dead process enforces no limits, and
what a generator does when this link goes silent has never been measured.

---

## 13. Bench acceptance — before anything is connected

Run with both converters on the Pi and **nothing attached to the field
side**. This is Phase 2 of
[DESIGN.md](DESIGN.md); these are the hardware checks
inside it.

| #   | Check                                                          | Pass criterion                                                              |
| --- | -------------------------------------------------------------- | --------------------------------------------------------------------------- |
| 1   | Both converters enumerate with **distinct** USB serial numbers | Two distinct IDs, or documented port-path binding in use                    |
| 2   | `latency_timer` reads back 1 ms on both                        | Service refuses to start otherwise                                          |
| 3   | Bridge chip identified and recorded                            | Matches an FTDI part, or the driver's low-latency equivalent is established |
| 4   | Termination jumpers OFF, verified visually and by resistance   | No 120 Ω across A/B                                                         |
| 5   | Field-side `PE` terminals not joined                           | Open circuit between the two zones' `PE`                                    |
| 6   | Loopback A↔B per converter, both directions                    | Frames decode without checksum error                                        |
| 7   | Zone-to-zone isolation                                         | No continuity between zone 1 and zone 2 field terminals                     |
| 8   | RTC holds time across a full power removal                     | Time correct on the next boot before NTP                                    |
| 9   | Hardware watchdog fires on a forced service hang               | Pi resets; boots to `READY_OFF`, no state restored                          |
| 10  | Enclosure interior ≤ 40 °C after 7 days sealed                 | Logged from the Pi's thermal sensor                                         |
| 11  | Every label present and correct                                | Visual                                                                      |

Checks 6 and 7 catch a wiring error capable of bridging two buses. Neither may
be skipped.

Steam adds two more, run against the DTV+ emulator: a Cx2 value must be
rejected by the steam encoder, and an Fx2 value must be rejected by the valve
encoder. The type split in §12 should make both uncompilable; the checks confirm
it.

---

## 14. Deliberately excluded

| Excluded                                                       | Reason                                                                     |
| -------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Any relay, contactor, smart plug or cord switch in valve mains | Design rule. A failing stop latency is not solved with an unreviewed relay |
| Mains wiring inside the enclosure                              | Keeps the build low-voltage; §9                                            |
| UPS on the Pi                                                  | Power loss must reach the valves; §4                                       |
| Dual-channel RS-485 converter or HAT                           | One isolation barrier shared by both zones; §5                             |
| Bidirectional USB-RS-485 adapter used as a capture tap         | Hardware automatic direction control is not physically receive-only        |
| Generic `MAX485` / `MAX3485` / TTL-to-RS485 modules            | Mostly unisolated; some assert the transmitter during boot                 |
| Wi-Fi in the control path                                      | Wired Ethernet only; radios disabled                                       |
| A custom PCB                                                   | Nothing in this build requires one                                         |
| An industrial PLC (~$500)                                      | Poor value for two links                                                   |

---

## 15. Open items

| #   | Item                                                        | Closed by                                                                                    | Blocks                  |
| --- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ----------------------- |
| 1   | Valve model numbers and nameplate data                      | Phase 0 photograph                                                                           | Connectors, mains work  |
| 2   | Valve connector housing, keying, pin count                  | Phase 0 photograph and continuity check                                                      | Adapter leads           |
| 3   | Cable polarity — which conductor is A+                      | Phase 1 capture                                                                              | First transmission      |
| 4   | Factory termination and idle bias                           | Phase 1 capture and unpowered measurement                                                    | Termination jumpers     |
| 5   | Which FTDI part is fitted — `FT232RL` or `FT232RNL` **[?]** | Inspection on arrival                                                                        | Nothing; record only    |
| 6   | Saturn response timeout: 320 ms or 400 ms                   | Phase 1 capture — [I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory) | Decoder deadlines       |
| 7   | Whether automatic purge is on                               | [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on) — one read-only call                 | Stop-latency definition |
