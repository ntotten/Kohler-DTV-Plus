# Kohler replacement controller — parts list

Revision B, 2026-10-24: one microcontroller board in line with both valve
cables. The revision A list — a Raspberry Pi 4, microSD cards, an RTC and USB
converters — is superseded,
[DECISIONS.md W5](DECISIONS.md#w5--revision-a-build-superseded). When each
order happens is [BUILD-ORDER.md](BUILD-ORDER.md); why each part was chosen is
[HARDWARE.md](HARDWARE.md).

Two orders now and a third later. Everything orderable goes in now. The valve
connectors wait for a photograph of the plug, and the wall panel's parts wait
for the panel's design.

Prices are marked with the date they were checked; "not checked" means
exactly that. Buy the transceivers from a distributor (TI, DigiKey, Mouser,
LCSC), not a marketplace seller.

---

## Order now

### The controller

|     Qty | Item                                            | Part / SKU                               | Price                             | Note                                                                                                                                           |
| ------: | ----------------------------------------------- | ---------------------------------------- | --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
|       2 | Raspberry Pi Pico 2                             | DigiKey `SC1631`                         | Not checked                       | One installed, one spare. RP2350, not the original Pico                                                                                        |
|       2 | WIZnet WIZ850io W5500 Ethernet module           | `WIZ850IO`                               | Not checked                       | Wired to the Pico 2's GPIO16–21, the pins the W5500-EVB-Pico2 uses **[V]**                                                                     |
|       2 | 2×6 female header, 2.54 mm                      | —                                        | Not checked                       | The WIZ850io plugs in rather than being soldered                                                                                               |
|       6 | TI THVD1410 RS-485 transceiver, VSSOP-8         | `THVD1410DGK` (Mouser `595-THVD1410DGK`) | $4.96, Mouser, 2026-10-24 **[V]** | Five ports and a spare. Same part as the SOIC `THVD1410DR`, which DigiKey and Mouser had none of until mid-November. **Not the THVD1406** — §5 |
|       1 | Adafruit 1212 SOIC/MSOP/TSSOP-8 adapter, 6-pack | Mouser `485-1212`                        | $2.95, Mouser, 2026-10-24 **[V]** | Takes the VSSOP-8 (0.65 mm pitch)                                                                                                              |
|       6 | 120 Ω resistor                                  | —                                        | Not checked                       | Termination, one per port                                                                                                                      |
|       6 | 4.7 kΩ resistor                                 | —                                        | Not checked                       | Pull-down on each transceiver's `DE`; anything up to 8.2 kΩ — §5                                                                               |
|       6 | 100 nF ceramic capacitor                        | —                                        | Not checked                       | Decoupling, one per transceiver                                                                                                                |
| 10 each | 220, 330, 470, 680 Ω and 1 kΩ resistors         | Yageo CFR-25JB                           | Not checked                       | Valve-segment bias candidates, value chosen in Phase 3 — §5                                                                                    |
|       1 | Protoboard, two 20-pin female header strips     | —                                        | Not checked                       | The board plugs into the headers rather than being soldered in                                                                                 |
|       1 | RJ45 jack, board mount                          | —                                        | Not checked                       | Panel port                                                                                                                                     |
|       1 | DC jack, 2.1 mm                                 | —                                        | Not checked                       | Panel adapter input, wired only to the panel jack — §16                                                                                        |
|       1 | Header pins for test points                     | —                                        | Not checked                       | Pins 2, 3, 4 of each of the four valve segments — §10                                                                                          |
|       1 | Official Raspberry Pi 15 W USB-C supply, US     | PiShop `1660`                            | $8.80, PiShop, 2026-08-29         | Any isolated 5 V USB-C supply of 1 A or more works                                                                                             |
|       1 | Hammond 1591DSBK ABS box, 150 × 80 × 50 mm      | `1591DSBK`                               | Not checked                       | Dry location beside the K-99695 — §10. Inside fit of the protoboard not checked; trim it to fit                                                |
|       1 | Ethernet patch cable                            | —                                        | Not checked                       | To the house network                                                                                                                           |

- [Raspberry Pi Pico 2, DigiKey](https://www.digikey.com/en/products/detail/raspberry-pi/SC1631/24627142)
- [WIZnet WIZ850io, DigiKey](https://www.digikey.com/en/products/detail/wiznet/WIZ850IO/8789619)
- [TI THVD1410DGK, Mouser](https://www.mouser.com/ProductDetail/Texas-Instruments/THVD1410DGK?qs=chTDxNqvsyml7%252BOzp%2FJQSQ%3D%3D)
- [TI THVD1410 datasheet](https://www.ti.com/lit/gpn/THVD1410)
- [15 W USB-C power supply, PiShop](https://www.pishop.us/product/raspberry-pi-15w-power-supply-us-white/)

**Why not the W5500-EVB-Pico2.** It is the same board in one piece, but on
2026-10-24 Mouser had none until about 2026-11-20 **[V]**, DigiKey didn't list it,
and The Pi Hut no longer ships to the US. Either build is the same
to the firmware **[I]**; the EVB can replace the pair later.

**A Pico already owned works too.** The W5500-EVB-Pico2 is a
Pico 2 with a W5500 on GPIO16–21 **[V]**, so a Pico 2 with a W5500 Ethernet
module wired to those six pins is the same board to the firmware **[I]**. Swap
the EVB line for one W5500 module (WIZnet's WIZ850io, or a generic W5500 board;
price not checked). An original Pico (RP2040) works the same way with 264 kB of
SRAM instead of 520 kB **[V]**
([CNX Software](https://www.cnx-software.com/2023/02/21/raspberry-pi-debug-probe-eases-bare-metal-development/)),
which tightens [HARDWARE.md § 15](HARDWARE.md) item 10; the bus log buffer is
the largest user.

### Optional

| Qty | Item                     | Note                                                                                                                                                         |
| --: | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
|   1 | Raspberry Pi Debug Probe | SWD flashing and `defmt` logs. Price not checked. A spare Pico running the probe firmware does the same job **[V]**, and USB-C flashing works without either |

### Already owned

| Item                                          | Use                                                     |
| --------------------------------------------- | ------------------------------------------------------- |
| 2 × Waveshare `USB TO RS485/422`, SKU `23949` | The bench rig: plays the K-99695 and the valve — §13    |
| Rigol DHO804 oscilloscope                     | Bench checks 1 and 7; the valve-segment bias in Phase 3 |
| Logic analyzer                                | Timing on the bench                                     |
| Multimeter                                    | Continuity, termination, the K-99695 ground check       |

---

## Order after the Phase 0 photograph

The parts that cannot be known from documents
([HARDWARE.md § 11](HARDWARE.md)):

| Item                                                  | Qty               | Closed by                                                                                                   |
| ----------------------------------------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------- |
| Jacks matching the valve cable's six-position plug    | 2, plus a spare   | A photograph of the plug: housing and part number                                                           |
| Leads from `K1` and `K2` to the K-99695's valve jacks | 2                 | The same photograph, and the length at the controller                                                       |
| An isolated transceiver with its own isolated supply  | 2, only if needed | The K-99695 ground check, [HARDWARE.md § 15](HARDWARE.md) item 8: only if its two jacks' pin 2 are separate |

**Never cut the original Kohler cables.** They are the rollback path.

---

## Later — the wall panel

Chosen with the panel's design ([HARDWARE.md § 15](HARDWARE.md) item 11): its
board and display, its own THVD1410 and 120 Ω, and its DC adapter, whose
voltage the panel hardware sets ([HARDWARE.md § 16](HARDWARE.md)).

---

## Tools — only if not already owned

| Item        | Note                                                                          |
| ----------- | ----------------------------------------------------------------------------- |
| Thermometer | Any accurate one; used to check delivered water at every outlet in Phases 3–4 |

Any further parallel capture on a live factory bus follows the tap rules in
[DESIGN.md § Packet capture questions](DESIGN.md): a physically receive-only
front end, no termination, no bias, a short stub.

---

## Do not buy

| Excluded                                                        | Reason                                                                                                |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| A Raspberry Pi, microSD cards, an RTC, more USB converters      | Revision A parts — [DECISIONS.md W5](DECISIONS.md#w5--revision-a-build-superseded)                    |
| Auto-direction transceivers, including the THVD1406             | They drive a mark for microseconds and leave it to the bias — [DECISIONS.md D15](DECISIONS.md)        |
| Generic `MAX485` / TTL-to-RS485 modules                         | No controlled driver enable; some assert the transmitter during boot                                  |
| An ESP32 board                                                  | [DECISIONS.md D14](DECISIONS.md)                                                                      |
| A PoE injector, splitter or HAT                                 | The panel takes passive DC — [DECISIONS.md D16](DECISIONS.md)                                         |
| Hobby relays, smart plugs, inline cord switches, a bypass relay | Never in a valve's mains path, and no bypass around the controller — [DECISIONS.md D13](DECISIONS.md) |
| A UPS                                                           | Power loss must reach the valves — [HARDWARE.md § 4](HARDWARE.md)                                     |
| Unipi Patron or another ~$500 PLC                               | Poor value for a two-link system                                                                      |
| Bidirectional USB-RS-485 adapters for passive capture           | Automatic direction control is not physically receive-only                                            |
