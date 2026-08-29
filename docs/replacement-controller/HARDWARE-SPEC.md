# Replacement controller — hardware specification

Status: **build specification, revision A.** Not yet built.

An open replacement master for the Kohler DTV+: three isolated serial links —
two Saturn valve buses and one DTV+ steam link — driven from a Raspberry Pi.
This specifies the unit described in
[CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md). That document owns the
architecture, the safety rules and the delivery phases; this one owns the
hardware. Purchase links and prices are in
[SHOPPING-LIST.md](SHOPPING-LIST.md). Steam is scoped in
[STEAM-ADAPTER.md](STEAM-ADAPTER.md); §12 here carries only the hardware
consequences.

Evidence tiers follow [system-specification.md](../system-specification.md) —
**[A]** ours/measured, **[B]** shipped code, **[K]** Kohler primary, **[C]**
reverse-engineered, **[?]** unresolved, **[I]** inference. Component data cited
from a manufacturer's own published page is marked **[V]** and linked.

---

## 1. Links

| Link | Device            | Bus    | Protocol | Encoding | Subsystem |
| ---- | ----------------- | ------ | -------- | -------- | --------- |
| 1    | Valve, six-port   | RS-485 | Saturn   | Cx2      | §5        |
| 2    | Valve, three-port | RS-485 | Saturn   | Cx2      | §5        |
| 3    | Steam adapter     | RS-485 | DTV+     | Fx2      | §12       |

Each link gets its own converter, its own isolation barrier, and its own state
machine. Nothing is shared between them.

The DTV+ controller supports two valve ports and eight peripheral ports; this
design implements two valve links and one peripheral link, which covers the
common single-steam installation. Adding a fourth link is another converter and
another instance of the DTV+ stack.

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

The alternative considered was a bare-metal MCU (RP2040/STM32, Rust via
`embassy` or RTIC) driving RS-485 transceivers directly.

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
(§6). At 9600 baud a byte takes ~1.04 ms and a maximum 20-byte Saturn frame
takes ~21 ms to clock out; the wire, not the scheduler, dominates. The only
sub-100 ms number in the protocol is the 20 ms echo timeout, and
[CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md) already establishes it does not
apply to this build.

**Why determinism is not a safety argument here.** The master is a protocol
translator, not a safety controller.
[valve-control.md § Safety Ownership](../devices/valve-control.md#safety-ownership)
places mixing, the temperature envelope, over-temperature trips and
fail-closed-on-comms-loss inside the valve **[C]**. A late frame produces a
valve timeout and closure — the designed-for outcome, and the outcome Phase 3
measures. An MCU would buy determinism the failure model does not need.

**[I]** An MCU becomes the right answer only if Phase 3 measures that a valve
does **not** close on communication loss. In that case the acceptance
thresholds in [CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md) reject this
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
                            +---+-------+-------+-------+-------+---+
                            USB3|   USB3|   USB2|   SPI0|   I2C1|
                                |       |       |       |       |
                                |       |       |       |       +--> DS3231 RTC
                                |       |       |       |            + CR1220 cell
                                |       |       |       |
                                |       |       |       +--> MAX31865 x 2
                                |       |       |            +--> PT1000 Class A,
                                |       |       |                 3-wire, 1 per zone,
                                |       |       |                 clamped to outlet pipe
                                |       |       |
          +---------------------+       |       +-----------------------+
          |             +---------------+                               |
          |             |                                               |
 +--------v--------+ +--v--------------+                     +----------v------+
 | Waveshare 23949 | | Waveshare 23949 |                     | Waveshare 23949 |
 | FT232R+SP485EEN | | (same part)     |                     | (same part)     |
 | isolated        | | isolated        |                     | isolated        |
 +--------+--------+ +--------+--------+                     +--------+--------+
          | RS-485            | RS-485                                | RS-485
  +-------v-------+   +-------v-------+                       +-------v--------+
  | Valve link 1  |   | Valve link 2  |                       | Steam adapter  |
  | 6-port        |   | 3-port Prompt |                       | K-1737-K1      |
  | Saturn · Cx2  |   | Saturn · Cx2  |                       | DTV+ · Fx2     |
  +---------------+   +---------------+                       +-------+--------+
                                                                      |
                                                              +-------v--------+
                                                              | Steam generator|
                                                              | self-contained,|
                                                              | out of scope   |
                                                              +----------------+

  K-99695 ports:     disconnected, capped, labeled
  K-99695 + K-99693: powered down after Phase 1 capture
  Generator mains:   the installer's work, never this enclosure's
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

Both dual-channel candidates were rejected for the same reason, now with
evidence:

| Part                                                              | Isolation architecture                                                                                           | Verdict      |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------ |
| Waveshare SKU `27646`, dual-channel USB                           | Manufacturer does not document channel-to-channel galvanic isolation                                             | Rejected     |
| Waveshare `2-CH RS485 HAT`, SKU `17221` (SPI, SC16IS752 + SP3485) | Board carries **one** `B0505LS` isolated supply and **one** `π142M61` digital isolator for both channels **[V]** | Rejected     |
| 2 × Waveshare SKU `23949`                                         | One complete isolation barrier per valve                                                                         | **Selected** |

**[I]** The 2-CH HAT would otherwise be attractive — it removes USB entirely and
mirrors Kohler's own architecture, which drives both valve ports from a
TL16C752C SPI/FlexBus dual UART **[C]** ([hardware.md](../hardware.md)). It is
rejected because a single barrier puts both zones on one isolated ground,
which is the exact property that disqualified SKU `27646`.

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

## 7. Subsystem C — independent temperature measurement

Required by [CONTROLLER-DESIGN.md § Independent temperature measurement](CONTROLLER-DESIGN.md).
It was also the instrument for [I1](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use)
E5, which closed unrun when I1 resolved on 2026-08-29. The sensor is required
regardless: it is the only independent temperature measurement in the system.

Every other temperature in this system is the valve's own thermistor
self-report. Per [DISCLAIMER.md](../../DISCLAIMER.md) that is not a
measurement.

### Chain

| Stage     | Part                                                                    | Specification                                               |
| --------- | ----------------------------------------------------------------------- | ----------------------------------------------------------- |
| Element   | PT1000, Class A, 3-wire, pipe-surface clamp, ≥ 2 m lead, ≥ 100 °C rated | Class A at 40 °C is ±0.23 °C                                |
| Amplifier | Adafruit PT1000 RTD amplifier, MAX31865, PID `3648`                     | 4300 Ω 0.1 % reference resistor; 2/3/4-wire capable **[V]** |
| Bus       | SPI0, one chip select per channel                                       | See §8                                                      |
| Channels  | 2 fitted (one per zone). Electronics support 4 (SPI1 `CE0`/`CE1`)       |                                                             |

**Power the MAX31865 `VIN` from the Pi's 3V3 rail.** The breakout accepts
3–5 VDC and its level shifting follows `VIN`, so `SDO` drives at the `VIN`
level **[V]**. Powering from 5 V would put 5 V onto a 3.3 V Pi GPIO. Configure
each breakout for 3-wire per its own documentation.

### Placement and its limitation

One probe per zone, clamped to the supply pipe of that zone's **default
outlet**, as close to the valve as accessible pipe allows. Exact location and
pipe OD are set during the Phase 0 survey (§11).

**A surface clamp is not an immersion measurement.** It reads pipe wall, lags
by seconds, and reads low. Two consequences, both mandatory:

1. **Characterise the offset at commissioning** against the Therma K immersion
   probe, across the working range, and apply that correction before any
   threshold is evaluated. The raw reading is logged alongside the corrected
   one.
2. **The interlock covers only the instrumented outlet.** When a non-instrumented
   outlet is active there is no independent continuous measurement. Every outlet
   is still verified individually with the immersion probe at Phase 4, and the
   setpoint clamp and valve fault monitoring still apply — but continuous
   independent coverage is limited to the default outlet until further channels
   are fitted. This limitation is recorded in the commissioning report.

### Alarm logic

The sensor has **no authority to open an outlet**. It can only contribute to
`all-off`.

| Condition                                                                | Action                                                 |
| ------------------------------------------------------------------------ | ------------------------------------------------------ |
| Corrected outlet temperature > 45.0 °C for > 2 s, instrumented outlet on | `all-off` that zone, latch unavailable                 |
| Raw reading > 50.0 °C, regardless of correction                          | `all-off` that zone, latch unavailable                 |
| MAX31865 fault register non-zero (RTD open/short, over/under-voltage)    | `all-off` that zone, latch unavailable                 |
| No RTD sample for > 5 s                                                  | `all-off` that zone, latch unavailable                 |
| Corrected reading vs valve-reported differ by > 5 °C for > 10 s          | `all-off` that zone, latch, record as I5-class finding |

45.0 °C sits above the 42.5 °C setpoint ceiling and above the 43 °C scald
threshold, with margin for sensor lag. It is a fault threshold, not a comfort
limit.

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

| Header pin | GPIO   | Function  | Destination                   |
| ---------: | ------ | --------- | ----------------------------- |
|          1 | —      | 3V3       | MAX31865 #1 `VIN`, #2 `VIN`   |
|          3 | GPIO2  | I2C1 SDA  | DS3231 `SDA`                  |
|          5 | GPIO3  | I2C1 SCL  | DS3231 `SCL`                  |
|          6 | —      | GND       | DS3231 `GND`                  |
|          9 | —      | GND       | MAX31865 #1 `GND`             |
|         14 | —      | GND       | MAX31865 #2 `GND`             |
|         17 | —      | 3V3       | DS3231 `VIN`                  |
|         19 | GPIO10 | SPI0 MOSI | MAX31865 #1 and #2 `SDI`      |
|         21 | GPIO9  | SPI0 MISO | MAX31865 #1 and #2 `SDO`      |
|         23 | GPIO11 | SPI0 SCLK | MAX31865 #1 and #2 `SCK`      |
|         24 | GPIO8  | SPI0 CE0  | MAX31865 #1 `CS` — **zone 1** |
|         26 | GPIO7  | SPI0 CE1  | MAX31865 #2 `CS` — **zone 2** |

Expansion channels 3 and 4 use SPI1 `CE0` (GPIO18, pin 12) and `CE1` (GPIO17,
pin 11). No GPIO drives a relay, a contactor, or anything in a mains path.

---

## 9. Power, isolation and grounding

### Budget

| Load                 | Worst case  | Note                                     |
| -------------------- | ----------- | ---------------------------------------- |
| Raspberry Pi 4 2 GB  | ~3.5 W      | This workload; idle ~2.7 W               |
| Converter × 3        | 3 × 1.0 W   | 200 mA fused each; actual draw is lower  |
| MAX31865 × 2, DS3231 | < 0.1 W     | On 3V3                                   |
| **Total**            | **≈ 6.6 W** | Supply is 15.3 W — better than 2× margin |

Worst-case downstream USB draw is 600 mA across three converters, inside the
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
passes through a gland. This keeps the build entirely low-voltage and removes
any need for an electrician to sign off on the enclosure's internals.

Mains work in this project is limited to what an electrician does outside the
box: identify valve voltage, receptacles, branch circuits and GFCI
arrangement, and fit the labelled manual valve-power disconnects that
[CONTROLLER-DESIGN.md § Safety boundary](CONTROLLER-DESIGN.md) requires.

---

## 10. Enclosure, mechanical and environment

| Requirement       | Specification                                                                                |
| ----------------- | -------------------------------------------------------------------------------------------- |
| Location          | Dry, serviceable, ventilated. **Not inside the bathroom wet zone**                           |
| Rating            | IP65 / NEMA 4X wall-mount, hinged or screw lid, non-metallic                                 |
| Internal size     | ≥ 300 × 200 × 150 mm, with a mounting plate                                                  |
| Rail              | 35 mm × 7.5 mm DIN, ≥ 200 mm usable length                                                   |
| Rail budget       | 3 × converter at 81.9 × 54.0 × 32.0 mm, plus terminal blocks                                 |
| Pi mounting       | DIN-rail carrier or plate standoffs, GPIO header accessible with the lid open                |
| Breakout mounting | MAX31865 and DS3231 on plate standoffs, not loose                                            |
| Glands            | 1 × Pi USB-C power, 1 × Ethernet, 2 × valve field cable, 1 × steam field cable, 2 × RTD lead |
| Interior ambient  | ≤ 40 °C, logged from the Pi's own thermal sensor through the seven-day soak                  |
| Strain relief     | Every cable, at the gland. No conductor takes tension at a screw terminal                    |

Size for one spare gland and ~55 mm of spare rail beyond this. Re-drilling a
sealed enclosure in service is worse than buying a larger one now.

### Labelling and test points

| Label                             | Applied to                                                      |
| --------------------------------- | --------------------------------------------------------------- |
| `ZONE 1 · 6-PORT`                 | Converter 1, its USB port, its field cable, both cable ends     |
| `ZONE 2 · 3-PORT PROMPT`          | Converter 2, its USB port, its field cable, both cable ends     |
| `STEAM · DTV+ · NOT COMMISSIONED` | Reserved USB port and gland                                     |
| `A+` / `B−` / `PE`                | Each field terminal                                             |
| `OEM — DO NOT CUT`                | Both original Kohler valve cables, at both ends                 |
| Emergency card                    | Inside the lid: rollback steps, and the `WELDED` (35) procedure |

Bring A/B test points out per zone as labelled, insulated posts so the bus can
be metered without unlanding a conductor.

The lid card must state that a `WELDED` fault (35) is a mechanically stuck
mixing valve that **no controller can close** — the only remedy is removing
valve power and closing the hot and cold service shutoffs.

---

## 11. Field cabling — specified, not yet orderable

These items cannot be finalised from documents. Each row names the measurement
that closes it. Ordering by assumption is prohibited by
[CONTROLLER-DESIGN.md § Field-select](CONTROLLER-DESIGN.md).

| Item                                  | What is unknown                               | Measurement that closes it                                                                  |
| ------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Valve mating connector / pigtail      | Housing, keying, pin count                    | Photograph both ends; record keying; verify A/B/ground continuity with everything unpowered |
| Adapter lead conductor gauge, length  | Run length, gauge                             | Measure the installed run at Phase 0                                                        |
| RS-485 termination and bias           | Whether the factory bus terminates, and where | Meter the unpowered bus; capture the original waveform in Phase 1                           |
| Cable polarity                        | Which conductor is A+                         | Phase 1 capture. The `TA`/`TB` labels are the converter's convention, not Kohler's          |
| RTD clamp size                        | Outlet pipe OD                                | Phase 0 survey                                                                              |
| Valve-power disconnect                | Valve voltage, receptacles, circuits, GFCI    | Electrician, after both nameplates are read                                                 |
| Ferrules, glands, blocks, rail length | Final conductor gauges and layout             | Bench layout, after the above                                                               |

**Adapter leads, not modified OEM cables.** The original Kohler cables are the
rollback path and are never cut.

---

## 12. Subsystem E — the steam link

A third serial link, built to the same pattern as the two valve links: one
dedicated isolated converter, one protocol stack, one state machine.

The generator is a self-contained appliance and protects itself: low water /
dry fire (`0140-A`), tank high-limit (`0140-B`), automatic fill shutoff, a ¾″
pressure relief valve, room over-temperature (`0120`), and a session
auto-shutoff — all Kohler-documented **[K]**. The K-1737-K1 adapter replaces the
native keypad as the control path. Same architecture as the valve links: the
device owns its own safety, and this controller sends it setpoints.

### Hardware

| Item                | Specification                                                                                 |
| ------------------- | --------------------------------------------------------------------------------------------- |
| Converter           | 3rd Waveshare `USB TO RS485/422`, SKU `23949` — identical to the two valve links              |
| Host port           | Pi USB 2.0; the remaining port stays spare                                                    |
| Field cable         | Adapter to Pi. Kohler ships a 25 ft cable adapter-to-K-99695; ours replaces that run          |
| Connector           | **4-pin polarized header**, labelled `FROM DTV CONTROL` **[A]**. Pin assignment to be metered |
| Enclosure provision | Gland used, not blanked; ~55 mm of rail                                                       |
| Power               | 1.0 W, budgeted in §9                                                                         |
| Isolation           | Its own barrier. Its field-side `PE` is not joined to either valve's                          |

Physically this is one more of a part already in the build. The work is the
protocol, not the hardware.

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

### The in-enclosure interface requirement

Kohler `WARNING`, in two guides: "A user interface must be located within the
steam enclosure to allow temperature regulation and control of the steam flow"
**[K]**. This design powers the K-99693 down at Phase 4.

**Operator decision, 2026-08-29: accepted as a recorded deviation.** Kohler's
stated purpose splits, and the halves land differently:

- **Sensing** — "to allow the sensors to regulate the temperature" is served by
  the K-1737-K1 kit's **own remote temperature sensor**, which wires to the
  adapter rather than to the interface **[K]**. **[I]** Inference from Kohler's
  description of the kit, not a statement Kohler makes.
- **Control** — an in-room means to stop steam. The operator's position is that
  removing power is the remedy they would actually use, and that a touchscreen
  is the wrong instrument in an emergency.

Recorded in the commissioning report. Full citations in
[STEAM-ADAPTER.md § 6](STEAM-ADAPTER.md).

### Why through the adapter and not straight to the generator

Both are possible. The adapter wins on protocol, and it is not close.

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

**Decision: through the adapter.**

Two things that would reopen it:

- The adapter fails or becomes unobtainable.
- The generator's installation guide, once a model is chosen, shows a simple
  interface. Worth ten minutes of reading at that point, not before.

**[I]** And if the native protocol is ever wanted, the cheapest route to it is
the adapter itself: put the receive-only analyser on the modular cable between
adapter and generator and record the adapter talking. That is a far better
starting point than probing the generator blind — but it needs a generator to
exist first.

### Out of scope

The generator and everything behind the adapter — its supply, its plumbing, its
own controls — are installed by a professional and are not this project's
concern. We connect to the adapter.

Worth one line to the installer: Kohler maps current Invigoration generators to
K-5548-K1 rather than K-1737-K1 **[K]**, so confirm the kit matches the
generator chosen.

### Losing the DTV+ link

Three links exist and only two are ours: Pi ↔ valve (Saturn), Pi ↔ adapter
(DTV+), and adapter ↔ generator (Kohler's crossover cable). This is about the
second.

Two cases, and they behave differently:

| Case                                                                        | What the service does                                                                           |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| **Degraded but alive** — timeouts, NAKs, checksum failures, port still open | Transmit still works: command `steam_stop`, require acknowledgement, latch the link unavailable |
| **Hard loss** — USB gone, cable pulled, service dead, Pi unpowered          | Nothing can be sent. Steam is entirely on the generator's own behaviour                         |

The hard-loss case is where steam differs from the valves:

|                       | Valve links                                                                         | Steam link                    |
| --------------------- | ----------------------------------------------------------------------------------- | ----------------------------- |
| Backstop on hard loss | The valve's own communication-loss shutdown                                         | The generator's own behaviour |
| Measured?             | **Yes** — Phase 3 tests every fault path against a pre-registered latency threshold | **Not yet**                   |

A session limit inside the service is worth nothing in the hard-loss case,
because a dead process enforces no limits. The generator's documented 20-minute
auto-shutoff is the likely backstop, but sources disagree on whether that timer
lives in the generator or the controller **[?]**.

**So measure it, the same way the valve side is measured.** With an adapter and
generator in place, pull the DTV+ link mid-session and record what happens. That
is a Phase 5 commissioning number, not a question for a document. Kohler case
**#07797183** carries the same question in parallel; whichever answers first,
the measurement is what the commissioning report records.

Until then the service assumes the worst on both counts: its own session limit
for the degraded case, and no assumption at all about the hard case.

### Before the connector can be built

The adapter-side link is a **4-pin polarized header**, read directly off the
adapter's own lid label **[A]** —
[`research/reference/steam-adapter/`](../../research/reference/steam-adapter/).
The adapter carries two identical headers, `FROM DTV CONTROL` and `TO NEXT
DEVICE (OPTIONAL)`, so the bus is multi-drop with a daisy-chain out.

**The link is RS-485 — settled.** The adapter's transceiver is an **`ADM4852`**
**[A]**: half-duplex RS-485/RS-422, ⅛ unit load, slew-rate limited, 8-lead SOIC
([Analog Devices](https://www.analog.com/en/products/adm4852.html)). Two wires,
A and B. A standard converter is the correct part, and the three `PC900V`
optocouplers map onto its receiver output, driver input and tied enable — the
textbook isolated half-duplex node.

⅛ unit load means up to 256 transceivers on the bus, and the driver is
deliberately slew-limited. Both are the signature of a long multi-drop
daisy-chain, which matches the adapter's own `TO NEXT DEVICE` header.

**The connector pinout is measured [A]**, `CN1` and `CN2` in parallel:

| Pin | Position                  | `IC2` pin | Signal                                     |
| --- | ------------------------- | --------- | ------------------------------------------ |
| 1   | Furthest from barrel jack | 7         | **`B`**                                    |
| 2   |                           | 6         | **`A`**                                    |
| 3   |                           | 5         | **`GND`**                                  |
| 4   | Nearest the barrel jack   | —         | Not connected to `IC2`; not yet identified |

Pin 1 is anchored physically: it is the end furthest from the barrel jack and
nearest `IC2`. Both headers carry the same orientation.

`B` before `A` — the reverse of the obvious guess, which is why it was metered.
Either header can be the bus input, so the daisy-chain is plain multi-drop.

The lead is three conductors: connector `B`/`A`/`GND` to the converter's
`TB`/`TA`/`PE`, **plus pin 4 to a +V rail** — four conductors, not three.

**Settled at the bench, board open and unpowered [A]:**

| Item        | Finding                                                                                                                                                                                                                                                               |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pin 4       | **+V supplied by the master** **[A]** — enters `D9` (`M7`, 1N4007) on the anode, then `R16` (1.2 kΩ, measured) to `IC2` pin 8, with `D7` shunting the rail. The adapter's transceiver is bus-powered, so **our master must drive pin 4**; the lead is four conductors |
| Termination | 114 Ω across pins 1 and 2, identical both polarities. `R27` is across the pair; the adapter is terminated. **Our converter's jumper stays off** — see below                                                                                                           |
| Ground      | **Required.** `SMBJ28A` clamps are unidirectional, conducting from ~0.7 V forward, which removes the negative half of the ADM4852's −7 V…+12 V common-mode range at the adapter. Pin 3 must tie to the converter's `PE`                                               |

Ground being mandatory is a departure from the valve links, where `PE` is
connected only if measurement shows a reference conductor is needed. The
converter is isolated, so this creates a shared reference without a loop back
through the Pi, and this link's field ground is still never joined to another
link's.

**Carry forward:** if a second DTV+ peripheral is ever daisy-chained onto this
link, it may expect power on pin 4, which our master would then have to supply.

**The DTV+ side is galvanically isolated — closed.** The transceiver draws its
supply from the bus rather than from the board it sits on, which only makes
sense across an isolation barrier; the three `PC900V` optocouplers bridge to the
generator-powered MCU domain.

**New build requirement: a 12 V rail in the enclosure.** The `ADM4852` is a 5 V
part, `R16` is 1.2 kΩ in series, and the 330 µF capacitor's 16 V rating caps the
bus — which leaves **12 V** as the only standard rail that fits **[I]**. `D7`
carries no legible marking and is not needed to reach that.

The Pi's 5 V USB-C supply cannot provide it, so a 12 V source joins the parts
list. Confirm before committing to it: apply 12 V to pin 4 from a
current-limited supply with ground on pin 3, and measure `IC2` pin 8. About 5 V
confirms both the rail and the supply chain, and the current reading sizes the
permanent supply.

**Why our termination stays off.** At 9600 baud the bit period is 104 µs while
25 ft of cable is ~38 ns one way — a ratio near 1:2700, so reflections settle
thousands of times over before a bit is sampled. The adapter's 120 Ω already
supplies the DC load and damping; a second one halves the bus to 60 Ω for no
gain, and a chained second adapter would reach 40 Ω, below the 54 Ω RS-485
drivers are specified against.

Procedure and results in
[`research/reference/steam-adapter/README.md`](../../research/reference/steam-adapter/README.md).

This replaces the earlier plan of metering an unused DTV+ port on the K-99695.

---

## 13. Bench acceptance — before anything is connected

Run with all three converters on the Pi and **nothing attached to the field
side**. This is Phase 2 of
[CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md); these are the hardware checks
inside it.

| #   | Check                                                                            | Pass criterion                                                              |
| --- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| 1   | Both converters enumerate with **distinct** USB serial numbers                   | Two distinct IDs, or documented port-path binding in use                    |
| 2   | `latency_timer` reads back 1 ms on both                                          | Service refuses to start otherwise                                          |
| 3   | Bridge chip identified and recorded                                              | Matches an FTDI part, or the driver's low-latency equivalent is established |
| 4   | Termination jumpers OFF, verified visually and by resistance                     | No 120 Ω across A/B                                                         |
| 5   | Field-side `PE` terminals not joined                                             | Open circuit between the two zones' `PE`                                    |
| 6   | Loopback A↔B per converter, both directions                                      | Frames decode without checksum error                                        |
| 7   | Zone-to-zone isolation                                                           | No continuity between zone 1 and zone 2 field terminals                     |
| 8   | RTD channels read ambient, and read a known reference against the Therma K probe | Within Class A tolerance plus the characterised offset                      |
| 9   | RTD open-circuit and short-circuit injected                                      | MAX31865 fault register set; service commands `all-off`                     |
| 10  | RTC holds time across a full power removal                                       | Time correct on the next boot before NTP                                    |
| 11  | Hardware watchdog fires on a forced service hang                                 | Pi resets; boots to `READY_OFF`, no state restored                          |
| 12  | Enclosure interior ≤ 40 °C after 7 days sealed                                   | Logged from the Pi's thermal sensor                                         |
| 13  | Every label present and correct; emergency card in the lid                       | Visual                                                                      |

Checks 6 and 7 catch a wiring error capable of bridging two buses. Neither may
be skipped.

Steam adds two more, run against the DTV+ emulator: a Cx2 value must be
rejected by the steam encoder, and an Fx2 value must be rejected by the valve
encoder. The type split in §12 should make both uncompilable; the checks confirm
it.

---

## 14. Deliberately excluded

| Excluded                                                       | Reason                                                                                  |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Any relay, contactor, smart plug or cord switch in valve mains | Design rule. A failing stop latency is not solved with an unreviewed relay              |
| Mains wiring inside the enclosure                              | Keeps the build low-voltage; §9                                                         |
| UPS on the Pi                                                  | Power loss must reach the valves; §4                                                    |
| Dual-channel RS-485 converter or HAT                           | One isolation barrier shared by both zones; §5                                          |
| Bidirectional USB-RS-485 adapter used as a capture tap         | Hardware automatic direction control is not physically receive-only                     |
| Generic `MAX485` / `MAX3485` / TTL-to-RS485 modules            | Mostly unisolated; some assert the transmitter during boot                              |
| Wi-Fi in the control path                                      | Wired Ethernet only; radios disabled                                                    |
| A custom PCB                                                   | Nothing in this build requires one                                                      |
| An industrial PLC (~$500)                                      | Poor value for two links                                                                |
| A second temperature sensor on the same element                | A redundant sensor is not a redundant measurement; the immersion probe is the reference |

---

## 15. Open items

| #   | Item                                                         | Closed by                                                                                    | Blocks                      |
| --- | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------- | --------------------------- |
| 1   | Valve model numbers and nameplate data                       | Phase 0 photograph                                                                           | Connectors, mains work      |
| 2   | Valve connector housing, keying, pin count                   | Phase 0 photograph and continuity check                                                      | Adapter leads               |
| 3   | Cable polarity — which conductor is A+                       | Phase 1 capture                                                                              | First transmission          |
| 4   | Factory termination and idle bias                            | Phase 1 capture and unpowered measurement                                                    | Termination jumpers         |
| 5   | Outlet pipe OD and accessible sensor location                | Phase 0 survey                                                                               | RTD clamp order             |
| 6   | Valve mains voltage, receptacles, circuits, GFCI             | Electrician                                                                                  | Disconnect selection        |
| 7   | Which FTDI part is fitted — `FT232RL` or `FT232RNL` **[?]**  | Inspection on arrival                                                                        | Nothing; record only        |
| 8   | Saturn response timeout: 320 ms or 400 ms                    | Phase 1 capture — [I5](../../INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory) | Decoder deadlines           |
| 9   | Whether automatic purge is on                                | [I4](../../INVESTIGATIONS.md#i4--is-automatic-purge-on) — one read-only call                 | Stop-latency definition     |
| 10  | DTV+ connector pinout on the peripheral port                 | Meter a powered-down port — §12                                                              | Steam adapter lead          |
| 11  | What the generator does when the DTV+ link drops mid-session | **Measure it at Phase 5** — §12. Kohler case #07797183 in parallel                           | Nothing; worst case assumed |
