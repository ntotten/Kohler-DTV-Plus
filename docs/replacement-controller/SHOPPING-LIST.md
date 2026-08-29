# Kohler replacement controller — final parts list

Prices and stock checked **2026-08-29**. This is the purchasing companion to
[HARDWARE-SPEC.md](HARDWARE-SPEC.md), which specifies why each part was chosen
and how it is wired.

## Purchasing policy

Use manufacturer-direct stores and approved or authorized resellers. Avoid
marketplace inventory for the controller, storage, measurement tools, and valve
interface.

Raspberry Pi recommends its Approved Reseller network. The carts below use
PiShop US for Raspberry Pi hardware, B&H as an authorized SanDisk dealer,
Waveshare directly for the valve interfaces, Adafruit directly for the sensor
and clock boards, and ThermoWorks and Fluke directly for instruments.

Parts fall into five groups:

| Group | Meaning                                                                   |
| ----- | ------------------------------------------------------------------------- |
| **A** | Order now. Fully specified, nothing to measure first                      |
| **B** | Order after the Phase 0 survey. The measurement that closes each is named |
| **C** | Instruments                                                               |
| **D** | Passive-capture equipment — select the method before buying anything      |
| **E** | Steam — the parts a professional supplies                                 |

---

## Group A — order now

### Cart 1: PiShop US

| Qty | Item                                     | SKU      |   Unit |  Total |
| --: | ---------------------------------------- | -------- | -----: | -----: |
|   1 | Raspberry Pi 4 Model B, 2 GB             | `SC0193` | $55.00 | $55.00 |
|   1 | Official Raspberry Pi 15 W USB-C PSU, US | `1660`   |  $8.80 |  $8.80 |
|   1 | PiShop Premium Metal Case for Pi 4       | `9013`   | $16.95 | $16.95 |

Subtotal **$80.75**. All three in stock when checked.

The metal case is a passive heat spreader — no fan, per
[HARDWARE-SPEC.md § 4](HARDWARE-SPEC.md). It mounts to the enclosure backplate
on standoffs with the GPIO header accessible.

- [Raspberry Pi 4 Model B 2GB](https://www.pishop.us/product/raspberry-pi-4-model-b-2gb/)
- [15 W USB-C power supply](https://www.pishop.us/product/raspberry-pi-15w-power-supply-us-white/)
- [Premium metal case](https://www.pishop.us/product/pishop-premium-metal-case-for-raspberry-pi-4/)

### Cart 2: B&H Photo

| Qty | Item                                 | Manufacturer number  |   Unit |  Total |
| --: | ------------------------------------ | -------------------- | -----: | -----: |
|   2 | SanDisk 64 GB High Endurance microSD | `SDSQQNR-064G-GN6IA` | $26.99 | $53.98 |

One installed, one imaged recovery spare held offline. The exact manufacturer
number matters — the High Endurance line is specified for continuous write.

[B&H, authorized dealer](https://www.bhphotovideo.com/c/product/1987067-REG/sandisk_sdsqqnr_064g_gn6ia_64gb_high_endurance_microsdhc.html/overview)

### Cart 3: Waveshare direct

The exact converter was not found at DigiKey, Mouser, or PiShop US. Buy from the
manufacturer rather than a marketplace seller.

| Qty | Item                                  | SKU     | Unit (qty 3) |  Total |
| --: | ------------------------------------- | ------- | -----------: | -----: |
|   3 | `USB TO RS485/422` isolated converter | `23949` |       $17.09 | $51.27 |

FT232R family + SP485EEN, galvanically isolated, 15 kV ESD, 600 W surge,
jumper-selectable 120 Ω termination, `PE`/`TA`/`TB`/`RA`/`RB` screw terminals,
35 mm DIN case, 81.9 × 54.0 × 32.0 mm. A USB-A-to-USB-B cable is included with
each unit — **order no separate USB cable**.

**One converter per link, never shared** — two valves and the steam adapter. Both dual-channel alternatives are
rejected in [HARDWARE-SPEC.md § 5](HARDWARE-SPEC.md): SKU `27646` does not
document channel-to-channel isolation, and the `2-CH RS485 HAT` (SKU `17221`)
carries a single `B0505LS` isolated supply and a single `π142M61` digital
isolator for both channels.

On arrival: confirm all three units report **distinct** USB serial numbers, and
record whether the fitted bridge is `FT232RL` or `FT232RNL` — Waveshare's own
page names both.

[Waveshare direct](https://www.waveshare.com/usb-to-rs485-422.htm) ·
[wiki](https://www.waveshare.com/wiki/USB_TO_RS485/422)

### Cart 4: Adafruit direct

| Qty | Item                                               | PID    |   Unit |  Total |
| --: | -------------------------------------------------- | ------ | -----: | -----: |
|   2 | PT1000 RTD Temperature Sensor Amplifier — MAX31865 | `3648` | $14.95 | $29.90 |
|   1 | DS3231 Precision RTC — STEMMA QT                   | `5188` | $13.95 | $13.95 |
|   1 | STEMMA QT 4-pin to premium male headers cable      | —      |  $0.95 |  $0.95 |
|   1 | Female-to-female jumper wire bundle                | —      |    ~$2 |    ~$2 |

Subtotal **≈ $46.80**. Both boards in stock when checked.

**Also required, any source:** one **CR1220** coin cell — the DS3231 board does
not include one, and without it the RTC does not survive a power removal.

**Assembly note.** The MAX31865 breakouts ship with unsoldered header and
terminal blocks; soldering is required, and each must be configured for 3-wire
operation. The RTC needs no soldering when the STEMMA QT-to-headers cable is
used.

Power the MAX31865 `VIN` from the Pi's **3V3** rail. Its level shifting follows
`VIN`, so a 5 V supply would drive 5 V into a 3.3 V GPIO.

- [PT1000 MAX31865 amplifier, PID 3648](https://www.adafruit.com/product/3648)
- [DS3231 Precision RTC — STEMMA QT, PID 5188](https://www.adafruit.com/product/5188)

### Group A total

| Cart      |   Subtotal |
| --------- | ---------: |
| PiShop US |     $80.75 |
| B&H       |     $53.98 |
| Waveshare |     $51.27 |
| Adafruit  |    ~$46.80 |
| **Total** | **≈ $233** |

Before shipping and tax, across four vendors.

---

## Group B — order after the Phase 0 survey

Each row names the measurement that closes it. Ordering by assumption is
prohibited by [CONTROLLER-DESIGN.md § Field-select](CONTROLLER-DESIGN.md).

| Item                                    | Specification                                                      | Closed by                                             | Rough |
| --------------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------- | ----: |
| 2 × PT1000 pipe-surface probe           | Class A, 3-wire, ≥ 2 m lead, ≥ 100 °C, clamp sized to the pipe     | Outlet pipe OD, Phase 0                               | ~$100 |
| Enclosure                               | IP65 / NEMA 4X, non-metallic, ≥ 300 × 200 × 150 mm, mounting plate | Bench layout                                          |  ~$80 |
| DIN rail                                | 35 × 7.5 mm, ≥ 200 mm usable — sized for three converters          | Bench layout                                          |  ~$10 |
| Cable glands                            | 7 minimum: PSU, Ethernet, 2 × valve, 1 × steam, 2 × RTD            | Cable ODs                                             |  ~$20 |
| DIN terminal blocks, end stops, markers | Field-side A/B/PE per zone                                         | Conductor gauge                                       |  ~$30 |
| Ferrules and crimp tool                 | Sized to the conductors                                            | Conductor gauge                                       |  ~$40 |
| Pi DIN carrier or standoff set          | GPIO header must stay accessible                                   | Bench layout                                          |  ~$15 |
| A/B test posts, 2 pairs                 | Insulated, labeled, meterable without unlanding a conductor        | Bench layout                                          |  ~$15 |
| Valve mating connectors or pigtails     | —                                                                  | **Photograph both ends; verify continuity unpowered** |     — |
| Adapter-lead cable                      | Shielded twisted pair, gauge and length to suit                    | Measure the installed run                             |     — |
| RS-485 termination / bias components    | —                                                                  | **Meter the unpowered bus; Phase 1 capture**          |     — |
| Manual valve-power disconnects          | Directly acting, labeled, accessible                               | **Electrician**, after both nameplates are read       |     — |

**Never cut the original Kohler cables.** They are the rollback path. Build
adapter leads instead.

---

## Group C — instruments

| Qty | Item                                                                   | Source                                                                                               | Buying note                                                                                  |
| --: | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
|   1 | Therma K meter and fast-response probe kit, NIST-traceable certificate | [ThermoWorks direct](https://www.thermoworks.com/products/therma-k-kit)                              | The reference instrument. Characterizes the surface probes' offset and verifies every outlet |
|   1 | Fluke 117 true-RMS multimeter                                          | [Fluke direct](https://www.fluke.com/en-us/product/electrical-testing/digital-multimeters/fluke-117) | Buy only if no suitable meter is already available. The electrician handles mains            |

Borrow or rent the oscilloscope and isolated differential probe. Do not buy a
cheap marketplace probe for this job.

The Therma K is not optional. The permanent PT1000 probes are surface-clamped
and read low with lag; their correction is established against this instrument
at commissioning, and every outlet is verified with it at Phase 4.

---

## Group D — passive-capture equipment

**Select the method before buying anything.**

Do not connect either bidirectional converter in parallel with the operating
K-99695. Hardware automatic direction control is not physically receive-only.

Requirements:

- A professional isolated analyzer with a hardware listen-only mode, or a
  temporary isolated receiver whose driver enable is physically strapped
  inactive.
- **Hardware timestamping.** A USB-serial front end quantizes arrival times at
  its latency-timer interval — 16 ms by default — which does not resolve jitter
  on a 525 ms tick or a 320 ms deadline. Use a logic analyzer on the
  differential pair, or an analyzer with a hardware listen-only mode.

Capture equipment is not part of the permanent controller.

---

## Group E — steam, installed by others

The generator and everything behind the adapter is installed by a professional
and is not this project's to buy. One note for whoever specifies it: Kohler maps
current Invigoration generators to K-5548-K1 rather than K-1737-K1 **[K]**, so
confirm the kit matches.

**New, pending one measurement:** the DTV+ bus carries **+V on pin 4** and the
adapter's RS-485 transceiver is powered from it **[A]**, so our master has to
supply that rail. The Pi's 5 V cannot — the adapter shunt-regulates from a
higher voltage, most likely **12 V**. A small 12 V supply, or a 5 V-to-12 V
converter, joins the parts list once `D7`'s zener value fixes the number.

The K-1737-K1 adapter kit is already owned and has been opened and photographed
— [`research/reference/steam-adapter/`](../../research/reference/steam-adapter/).
The third converter is in Group A. The only remaining part is the adapter-side
lead: a **4-pin polarized header**, not the modular jack earlier documents
assumed. Its mating connector is a Group B item, pending a continuity check on
the open board.

---

## Do not buy

| Excluded                                              | Reason                                                                                                               |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Unipi Patron or another ~$500 PLC                     | Poor value for a two-link system                                                                                     |
| Waveshare `27646` or `2-CH RS485 HAT` `17221`         | One isolation barrier shared by both zones                                                                           |
| Generic `MAX485`, `MAX3485`, TTL-to-RS485 modules     | Mostly unisolated; some assert the transmitter during boot                                                           |
| Bidirectional USB-RS-485 adapters for passive capture | Automatic direction control is not physically receive-only                                                           |
| Hobby relays, smart plugs, inline cord switches       | Never in either valve's mains path                                                                                   |
| A UPS for the Pi                                      | Power loss must reach the valves; see [HARDWARE-SPEC.md § 4](HARDWARE-SPEC.md)                                       |
| Additional protection components                      | The packaged converter already includes isolation and transient protection; add nothing without an electrical review |
| A PoE HAT                                             | Adds a second power path and a mains-adjacent module inside the enclosure                                            |
