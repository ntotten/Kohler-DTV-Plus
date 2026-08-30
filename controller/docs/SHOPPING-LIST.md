# Kohler replacement controller — parts list

Two orders, and the second one is small. The list optimizes for wall-clock
time: shipping is the slow part, so everything orderable goes in one sitting
today, and the only items that wait are the ones that physically cannot be
ordered yet — nobody knows the valve connector part until the plugs are
photographed. When each order happens is [BUILD-ORDER.md](BUILD-ORDER.md); why
each part was chosen is [HARDWARE.md](HARDWARE.md).

Prices and stock checked **2026-08-29** except where marked as estimates. Use
manufacturer-direct stores and approved or authorized resellers; avoid
marketplace inventory for the controller, storage, and valve interface.

---

## Order now — one sitting

### Cart 1: PiShop US

| Qty | Item                                     | SKU      |   Unit |  Total |
| --: | ---------------------------------------- | -------- | -----: | -----: |
|   1 | Raspberry Pi 4 Model B, 2 GB             | `SC0193` | $55.00 | $55.00 |
|   1 | Official Raspberry Pi 15 W USB-C PSU, US | `1660`   |  $8.80 |  $8.80 |
|   1 | PiShop Premium Metal Case for Pi 4       | `9013`   | $16.95 | $16.95 |

The metal case is a passive heat spreader — no fan, per
[HARDWARE.md § 4](HARDWARE.md). It mounts to the enclosure backplate on
standoffs with the GPIO header accessible.

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

| Qty | Item                                  | SKU     | Unit | Total |
| --: | ------------------------------------- | ------- | ---: | ----: |
|   2 | `USB TO RS485/422` isolated converter | `23949` | ~$18 |  ~$36 |

One per valve, never shared. The $17.09 unit price on the page is the
three-unit break; two units land slightly higher. FT232R family + SP485EEN,
galvanically isolated, 15 kV ESD, 600 W surge, jumper-selectable 120 Ω
termination, `PE`/`TA`/`TB`/`RA`/`RB` screw terminals, 35 mm DIN case. A
USB-A-to-USB-B cable is included with each unit — **order no separate USB
cable**.

Every multi-channel alternative is rejected — one isolation barrier shared
between the zones — see [DECISIONS.md D3](DECISIONS.md).

On arrival: confirm both units report **distinct** USB serial numbers, and
record whether the fitted bridge is `FT232RL` or `FT232RNL` — Waveshare's own
page names both.

[Waveshare direct](https://www.waveshare.com/usb-to-rs485-422.htm) ·
[wiki](https://www.waveshare.com/wiki/USB_TO_RS485/422)

### Cart 4: Adafruit direct

| Qty | Item                                          | PID    |   Unit |  Total |
| --: | --------------------------------------------- | ------ | -----: | -----: |
|   1 | DS3231 Precision RTC — STEMMA QT              | `5188` | $13.95 | $13.95 |
|   1 | STEMMA QT 4-pin to premium male headers cable | —      |  $0.95 |  $0.95 |
|   1 | Female-to-female jumper wire bundle           | —      |    ~$2 |    ~$2 |

**Also required, any source:** one **CR1220** coin cell — the DS3231 board does
not include one, and without it the RTC does not survive a power removal.

No soldering: the RTC connects with the STEMMA QT-to-headers cable.

[DS3231 Precision RTC — STEMMA QT, PID 5188](https://www.adafruit.com/product/5188)

### Cart 5: build hardware, any electrical supplier

Commodity parts, sized generously instead of waiting to measure. Worst case a
guess is wrong and ~$50 of it is re-bought; that beats a second
order-and-shipping cycle.

| Item                                    | Specification                                                      | Rough |
| --------------------------------------- | ------------------------------------------------------------------ | ----: |
| Enclosure                               | IP65 / NEMA 4X, non-metallic, ≥ 300 × 200 × 150 mm, mounting plate |  ~$80 |
| DIN rail                                | 35 × 7.5 mm, ≥ 200 mm usable                                       |  ~$10 |
| Cable gland assortment                  | Covers PSU, Ethernet, 2 × valve cable, plus spares                 |  ~$20 |
| DIN terminal blocks, end stops, markers | Field-side A/B/PE per zone                                         |  ~$30 |
| Ferrule kit and crimp tool              | Assorted sizes                                                     |  ~$40 |
| Pi DIN carrier or standoff set          | GPIO header must stay accessible                                   |  ~$15 |
| A/B test posts, 2 pairs                 | Insulated, labeled, meterable without unlanding a conductor        |  ~$15 |
| Shielded twisted-pair cable, one spool  | For the adapter leads; cut to length after measuring the runs      |  ~$25 |

### Capture equipment — pick the method first

Needed for Phase 1. Do not connect a bidirectional converter in parallel with
the operating K-99695: hardware automatic direction control is not physically
receive-only.

Requirements:

- A professional isolated analyzer with a hardware listen-only mode, or a
  temporary isolated receiver whose driver enable is physically strapped
  inactive.
- **Hardware timestamping.** A USB-serial front end quantizes arrival times at
  its latency-timer interval — 16 ms by default — which does not resolve jitter
  on a 525 ms tick or a 320 ms deadline. Use a logic analyzer on the
  differential pair, or an analyzer with a hardware listen-only mode.

Wiring the tap, whatever front end is chosen:

- **Strap `RE` as well as `DE`.** Tying the driver enable inactive stops the
  transmitter; tying the receiver enable asserted is what makes the part a
  listener rather than a device that can be commanded into driving.
- **Add no termination at the tap.** The bus is already terminated by the
  controller and the valve.
- **Add no fail-safe bias at the tap.** The existing bias network sets the idle
  level; a parallel one shifts it.
- **Keep the stub to inches, not feet.** A long spur off a terminated pair is an
  unterminated reflection path.

On price: a commodity 8-channel 24 MHz USB analyzer — the FX2 class,
`sigrok`/PulseView supported,
[SparkFun `TOL-18627`](https://www.sparkfun.com/usb-logic-analyzer-24mhz-8-channel.html),
$26.95 — samples fast enough for 9600 baud a thousand times over and satisfies
the timestamping requirement, but **not** the isolation requirement. It is a
candidate only behind an isolated front end, or on a bench rig where our own
emulator drives both ends. A professional isolated analyzer is roughly 20×
this. Capture equipment is not part of the permanent controller.

### Tools — only if not already owned

| Item                                      | Note                                                                          |
| ----------------------------------------- | ----------------------------------------------------------------------------- |
| Thermometer                               | Any accurate one; used to check delivered water at every outlet in Phases 3–4 |
| True-RMS multimeter                       | Continuity and bus checks                                                     |
| Bench DC supply, adjustable current limit | General bench work                                                            |

Borrow or rent an oscilloscope and isolated differential probe if one is ever
needed; do not buy a cheap marketplace probe.

---

## Order after the Phase 0 photographs

The one follow-up order — the parts that cannot be known from documents:

| Item                                 | Closed by                                                       |
| ------------------------------------ | --------------------------------------------------------------- |
| Valve mating connectors or pigtails  | Photograph both ends; verify A/B/ground continuity unpowered    |
| RS-485 termination / bias components | Meter the unpowered bus; Phase 1 capture — most likely **none** |

**Never cut the original Kohler cables.** They are the rollback path. Build
adapter leads instead.

---

## Do not buy

| Excluded                                              | Reason                                                                                                               |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Unipi Patron or another ~$500 PLC                     | Poor value for a two-link system                                                                                     |
| Any multi-channel RS-485 converter or HAT             | One isolation barrier shared between zones — [DECISIONS.md D3](DECISIONS.md)                                         |
| Generic `MAX485`, `MAX3485`, TTL-to-RS485 modules     | Mostly unisolated; some assert the transmitter during boot                                                           |
| Bidirectional USB-RS-485 adapters for passive capture | Automatic direction control is not physically receive-only                                                           |
| Hobby relays, smart plugs, inline cord switches       | Never in either valve's mains path                                                                                   |
| A UPS for the Pi                                      | Power loss must reach the valves; see [HARDWARE.md § 4](HARDWARE.md)                                                 |
| Additional protection components                      | The packaged converter already includes isolation and transient protection; add nothing without an electrical review |
| A PoE HAT                                             | Adds a second power path and a mains-adjacent module inside the enclosure                                            |
