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

| Qty | Item                                        | Part / SKU      | Price                                  | Note                                                                                                        |
| --: | ------------------------------------------- | --------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
|   2 | WIZnet W5500-EVB-Pico2                      | Pi Hut `106079` | £11.20, The Pi Hut, 2026-10-24 **[V]** | One installed, one spare. The **Pico2** (RP2350) board, not the RP2040 W5500-EVB-Pico. US price not checked |
|   6 | TI THVD1410 RS-485 transceiver, SOIC-8      | `THVD1410DR`    | Not checked                            | Five ports and a spare. **Not the THVD1406** — §5                                                           |
|   6 | SOIC-8 to DIP-8 adapter board               | —               | Not checked                            | For the protoboard                                                                                          |
|   6 | 120 Ω resistor                              | —               | Not checked                            | Termination, one per port                                                                                   |
|   6 | 4.7 kΩ resistor                             | —               | Not checked                            | Pull-down on each transceiver's `DE`; anything up to 8.2 kΩ — §5                                            |
|   6 | 100 nF ceramic capacitor                    | —               | Not checked                            | Decoupling, one per transceiver                                                                             |
|   1 | Resistor assortment                         | —               | Not checked                            | Valve-segment bias, values chosen in Phase 3 — §5                                                           |
|   1 | Protoboard, two 20-pin female header strips | —               | Not checked                            | The board plugs into the headers rather than being soldered in                                              |
|   1 | RJ45 jack, board mount                      | —               | Not checked                            | Panel port                                                                                                  |
|   1 | DC jack, 2.1 mm                             | —               | Not checked                            | Panel adapter input, wired only to the panel jack — §16                                                     |
|   1 | Header pins for test points                 | —               | Not checked                            | Pins 2, 3, 4 of each of the four valve segments — §10                                                       |
|   1 | Official Raspberry Pi 15 W USB-C supply, US | PiShop `1660`   | $8.80, PiShop, 2026-08-29              | Any isolated 5 V USB-C supply of 1 A or more works                                                          |
|   1 | Small non-metallic box                      | —               | Not checked                            | Dry location beside the K-99695 — §10                                                                       |
|   1 | Ethernet patch cable                        | —               | Not checked                            | To the house network                                                                                        |

- [WIZnet W5500-EVB-Pico2, The Pi Hut](https://thepihut.com/products/wiznet-w5500-evb-pico2)
- [TI THVD1410 datasheet](https://www.ti.com/lit/gpn/THVD1410)
- [15 W USB-C power supply, PiShop](https://www.pishop.us/product/raspberry-pi-15w-power-supply-us-white/)

**A Pico already owned works instead of the EVB.** The W5500-EVB-Pico2 is a
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
