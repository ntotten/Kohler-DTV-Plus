# Kohler replacement controller — final parts list

Prices and stock checked **2026-08-29**. This is the purchasing companion to
[HARDWARE.md](HARDWARE.md), which specifies why each part was chosen and how it
is wired. When each group is bought is [BUILD-ORDER.md](BUILD-ORDER.md).

## Purchasing policy

Use manufacturer-direct stores and approved or authorized resellers. Avoid
marketplace inventory for the controller, storage, measurement tools, and valve
interface.

Raspberry Pi recommends its Approved Reseller network. The carts below use
PiShop US for Raspberry Pi hardware, B&H as an authorized SanDisk dealer,
Waveshare directly for the valve interfaces, Adafruit directly for the sensor
and clock boards, and ThermoWorks and Fluke directly for instruments.

Parts fall into six groups:

| Group | Meaning                                                                    |
| ----- | -------------------------------------------------------------------------- |
| **A** | Order now. Fully specified, nothing to measure first                       |
| **B** | Order after the Phase 0 survey. The measurement that closes each is named  |
| **C** | Instruments                                                                |
| **D** | Passive-capture equipment — select the method before buying anything       |
| **E** | Steam — the parts a professional supplies                                  |
| **F** | Firmware extraction — optional, and not on the replacement-controller path |

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
[HARDWARE.md § 4](HARDWARE.md). It mounts to the enclosure backplate
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
rejected in [HARDWARE.md § 5](HARDWARE.md): SKU `27646` does not
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
prohibited by [DESIGN.md § Hardware, "Not orderable from documents"](DESIGN.md).

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
|   1 | Bench DC supply with adjustable current limit, 0–30 V                  | Any lab-grade supply                                                                                 | Buy only if none is already available. Closes the §12 measurement below                      |
|   1 | Temperature-controlled soldering iron and thin rosin-core solder       | Any                                                                                                  | Buy only if none is already available. The MAX31865 boards ship with unsoldered headers      |

Borrow or rent the oscilloscope and isolated differential probe. Do not buy a
cheap marketplace probe for this job.

The current limit on the supply is the point, not the voltage range.
[HARDWARE.md § 12](HARDWARE.md) closes the DTV+ rail question by
applying 12 V to pin 4 with ground on pin 3 and reading `IC2` pin 8 — into an
adapter board whose working voltage is the thing being established. Set the
limit low and raise it.

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

### Wiring the tap

Whatever front end is chosen, four rules govern how it attaches. They are
properties of bridging a live, already-terminated bus, not of any one product.

- **Strap `RE` as well as `DE`.** Tying the driver enable inactive stops the
  transmitter; tying the receiver enable asserted is what makes the part a
  listener rather than a device that can be commanded into driving.
- **Add no termination at the tap.** The bus is already terminated by the
  controller and the valve. A third 120 Ω across the pair halves the load the
  drivers see.
- **Add no fail-safe bias at the tap.** The existing bias network sets the idle
  level; a parallel one shifts it. This is a separate rule from termination and
  is the easier of the two to leave populated by accident.
- **Keep the stub to inches, not feet.** A long spur off a terminated pair is an
  unterminated reflection path.

### On price, and what it does not buy

A commodity 8-channel 24 MHz USB analyzer — the FX2 class, `sigrok`/PulseView
supported, [SparkFun `TOL-18627`](https://www.sparkfun.com/usb-logic-analyzer-24mhz-8-channel.html),
$26.95, backordered as of 2026-08-30 — samples fast enough for 9600 baud by
three orders of magnitude, and it does satisfy the hardware-timestamping
requirement above, which a USB-serial front end cannot. It does **not** satisfy
the isolation requirement, and no sampling rate substitutes for that. It is a
candidate only behind an isolated front end, or on a bench rig where our own
emulator drives both ends and the K-99695 is not in the circuit. Priced here so
the tradeoff is explicit — a professional isolated analyzer is roughly 20× this
— not as a recommendation to buy it and strap it to a live valve bus.

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

## Group F — firmware extraction, optional

Getting Kohler's firmware off the K-99695 is a **separate track** from building
a replacement master. Nothing here is needed to ship the controller, and nothing
here is ordered until the board is photographed — see
[repair/firmware-extraction.md](../../docs/repair/firmware-extraction.md), which ranks
the two vectors and gates both on confirming the footprints against
[`Images/KohlerBoardOverall.webp`](../../Images/KohlerBoardOverall.webp).

| Qty | Item                                             | Source                                                                                                  | Buying note                                                                                                                        |
| --: | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
|   1 | USB-to-TTL serial cable, **3.3 V** logic         | [Adafruit 954](https://www.adafruit.com/product/954), $9.95                                             | The **J904** 4-pin console header, 115200 8N1. Try this first — it is the whole cost of finding out whether the shell is alive     |
|   1 | PEmicro Multilink Universal (`USB-ML-UNIVERSAL`) | [PEmicro direct](https://www.pemicro.com/products/product_viewDetails.cfm?product_id=15320180), $299.00 | ColdFire **V2/V3/V4** BDM over the **J201** 26-pin footprint. The 26-pin ColdFire ribbon is included — do not order one separately |

Three notes on the probe, all checked 2026-08-30:

- **The `-FX` variant is $599 and buys speed only.** Both support ColdFire
  V2/V3/V4; the MCF54416 is V4. For one dump, the $299 part is the correct
  choice. `USB-ML-ACP` is ARM-only and does **not** work here.
- **The 26-pin cable ships in the box.** Earlier drafts of this list carried a
  separate ribbon line item; it was redundant. The $50 synchronous-ColdFire
  adapter is for MCF5272/MCF5206(E) and is not needed either.
- **TBLCF** (Turbo BDM Light ColdFire) is the open-hardware alternative if the
  $299 is not worth it for a single read —
  [reference-links.md](../../research/reference-links.md).

Order neither part before the serial console has been tried and the J201
footprint confirmed populated. J904 is $10 and may answer the question outright.

---

## Do not buy

| Excluded                                              | Reason                                                                                                               |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Unipi Patron or another ~$500 PLC                     | Poor value for a two-link system                                                                                     |
| Waveshare `27646` or `2-CH RS485 HAT` `17221`         | One isolation barrier shared by both zones                                                                           |
| Generic `MAX485`, `MAX3485`, TTL-to-RS485 modules     | Mostly unisolated; some assert the transmitter during boot                                                           |
| Bidirectional USB-RS-485 adapters for passive capture | Automatic direction control is not physically receive-only                                                           |
| Hobby relays, smart plugs, inline cord switches       | Never in either valve's mains path                                                                                   |
| A UPS for the Pi                                      | Power loss must reach the valves; see [HARDWARE.md § 4](HARDWARE.md)                                                 |
| Additional protection components                      | The packaged converter already includes isolation and transient protection; add nothing without an electrical review |
| A PoE HAT                                             | Adds a second power path and a mains-adjacent module inside the enclosure                                            |
