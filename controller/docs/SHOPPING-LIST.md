# Kohler replacement controller — parts list

Revision B, 2026-10-24: one microcontroller board in line with both valve
cables. The revision A list — a Raspberry Pi 4, microSD cards, an RTC and USB
converters — is superseded,
[DECISIONS.md W5](DECISIONS.md#w5--revision-a-build-superseded). When each
order happens is [BUILD-ORDER.md](BUILD-ORDER.md); why each part was chosen is
[HARDWARE.md](HARDWARE.md).

Two orders now and a third later. Everything orderable goes in now, including
the 6P6C valve jacks, and the wall panel's parts wait for the panel's design.

Prices are marked with the date they were checked; "not checked" means
exactly that. Buy the transceivers from a distributor (TI, DigiKey, Mouser,
LCSC), not a marketplace seller.

---

## Order now

### The controller

|     Qty | Item                                                                | Part / SKU                                                                           | Price                                         | Note                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ------: | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|       2 | Raspberry Pi Pico 2                                                 | DigiKey `2648-SC1631CT-ND`                                                           | $6.12, DigiKey, 2026-10-24 **[V]**            | One installed, one spare. RP2350, not the original Pico. Micro-USB                                                                                                                                                                                                                                                                                                                                                                              |
|       2 | WIZnet WIZ850io W5500 Ethernet module                               | DigiKey `1278-1043-ND`                                                               | $24.52, DigiKey, 2026-10-24 **[V]**           | Wired to the Pico 2's GPIO16–21, the pins the W5500-EVB-Pico2 uses **[V]**                                                                                                                                                                                                                                                                                                                                                                      |
|       2 | 2×6 female header, 2.54 mm                                          | Sullins PPPC062LFBN-RC, DigiKey `S7109-ND`                                           | $0.78, DigiKey, 2026-10-25 **[V]**            | The WIZ850io plugs in rather than being soldered. Its pins are two 1×6 rows at 2.54 mm **[V]** ([WIZnet](https://docs.wiznet.io/Product/ioModule/wiz850io)); the gap between the rows is not checked, so the two extra 20-pin sockets can be cut into 1×6 pieces if the 2×6 doesn't fit **Doesn't fit the PCB:** the two rows are 20.32 mm apart (WIZnet drawing **[V]**), so the carrier takes two 1×6 pieces cut from a 20-pin socket instead |
|      10 | TI THVD1410 RS-485 transceiver, VSSOP-8                             | `THVD1410DGK` (Mouser `595-THVD1410DGK`)                                             | $3.74 each at 10, Mouser, 2026-10-25 **[V]**  | Five ports and five spares, since the 0.65 mm VSSOP is the easiest part to ruin by hand. Same part as the SOIC `THVD1410DR`, which DigiKey and Mouser had none of until mid-November. **Not the THVD1406** — §5                                                                                                                                                                                                                                 |
|       2 | Adafruit 1212 SOIC/MSOP/TSSOP-8 adapter, 6-pack                     | DigiKey `1528-1071-ND` (Mouser `485-1212`)                                           | $2.95, 2026-10-25 **[V]**                     | Takes the VSSOP-8 (0.65 mm pitch) Protoboard only: the carrier PCB takes the VSSOP directly                                                                                                                                                                                                                                                                                                                                                     |
|      10 | 120 Ω resistor                                                      | Yageo CFR-25JB-52-120R, DigiKey `120QBK-ND`                                          | $0.10, DigiKey, 2026-10-25 **[V]**            | Termination, one per port                                                                                                                                                                                                                                                                                                                                                                                                                       |
|      10 | 4.7 kΩ resistor                                                     | Yageo CFR-25JB-52-4K7, DigiKey `13-CFR-25JB-52-4K7-ND`                               | $0.10, DigiKey, 2026-10-25 **[V]**            | Pull-down on each transceiver's `DE`; anything up to 8.2 kΩ — §5                                                                                                                                                                                                                                                                                                                                                                                |
|      10 | 100 nF ceramic capacitor                                            | Vishay K104K15X7RF5TL2, DigiKey `BC1084CT-ND`                                        | $0.31, DigiKey, 2026-10-25 **[V]**            | Decoupling, one per transceiver                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 10 each | 220, 330, 470, 680 Ω and 1 kΩ resistors                             | Yageo CFR-25JB, DigiKey `220QBK-ND` `330QBK-ND` `470QBK-ND` `680QBK-ND` `1.0KQBK-ND` | $0.10–0.11, DigiKey, 2026-10-25 **[V]**       | Valve-segment bias candidates, value chosen in Phase 3 — §5                                                                                                                                                                                                                                                                                                                                                                                     |
|       2 | Adafruit Perma-Proto full-size protoboard; 20-pin female headers ×6 | DigiKey `1528-1100-ND`; Sullins PPPC201LFBN-RC, DigiKey `S7053-ND`                   | $10.95 and $1.23, DigiKey, 2026-10-25 **[V]** | The board plugs into the headers rather than being soldered in. A perfboard already owned can replace the Perma-Proto Superseded by the carrier PCB (below); its 20-pin sockets are still used, three per board                                                                                                                                                                                                                                 |
|       2 | RJ45 jack, through hole                                             | Amphenol 54602-908LF, DigiKey `609-1046-ND`                                          | $0.78, DigiKey, 2026-10-25 **[V]**            | Panel port, one spare. Pins are off the 0.1" grid: wire it in                                                                                                                                                                                                                                                                                                                                                                                   |
|       1 | DC jack, 2.1 mm, panel mount                                        | Switchcraft 721A, DigiKey `SC3508-ND`                                                | $5.06, DigiKey, 2026-10-25 **[V]**            | Panel adapter input, wired only to the panel jack — §16                                                                                                                                                                                                                                                                                                                                                                                         |
|       1 | Header pins for test points                                         | —                                                                                    | Already owned                                 | Pins 2, 3, 4 of each of the four valve segments — §10                                                                                                                                                                                                                                                                                                                                                                                           |
|       1 | Raspberry Pi 12.5 W micro-USB supply, US                            | Raspberry Pi SC0623, DigiKey `2648-SC0623-ND`                                        | $8.00, DigiKey, 2026-10-25 **[V]**            | The Pico 2 is micro-USB. Any isolated 5 V micro-USB supply of 1 A or more works                                                                                                                                                                                                                                                                                                                                                                 |
|       1 | Hammond 1591ESBK ABS box, 191 × 110 × 61 mm                         | Mouser `546-1591ES-BK`                                                               | $11.51, Mouser, 2026-10-25 **[V]**            | Dry location beside the K-99695 — §10. The 157 × 51 mm Perma-Proto **[V]** does not fit the 150 mm 1591DSBK chosen first; it fits this one with the jacks along one wall **[I]**. Mounts on M2.5 nylon standoffs, already owned The 160 × 60 mm carrier PCB fits between its corner bosses with every jack on one wall ([box-fit drawing](../hardware/fab/carrier-box-fit.svg))                                                                 |
|       6 | 6P6C modular jack, through hole                                     | Würth 615006138421, DigiKey `732-2113-ND`                                            | $0.60, DigiKey, 2026-10-25 **[V]**            | Valve segments K1, V1, K2, V2 and two spares, so the original cables plug in uncut. Pins are off the 0.1" grid: wire them in The carrier PCB takes the Amphenol 54601-906LF instead (below): KiCad's footprint for it is drawn from Amphenol's drawing, and no Würth drawing could be fetched                                                                                                                                                   |
|       1 | Ethernet patch cable                                                | —                                                                                    | Not checked                                   | To the house network                                                                                                                                                                                                                                                                                                                                                                                                                            |

- [Raspberry Pi Pico 2, DigiKey](https://www.digikey.com/en/products/detail/raspberry-pi/SC1631/24627142)
- [WIZnet WIZ850io, DigiKey](https://www.digikey.com/en/products/detail/wiznet/WIZ850IO/8789619)
- [TI THVD1410DGK, Mouser](https://www.mouser.com/ProductDetail/Texas-Instruments/THVD1410DGK?qs=chTDxNqvsyml7%252BOzp%2FJQSQ%3D%3D)
- [TI THVD1410 datasheet](https://www.ti.com/lit/gpn/THVD1410)
- [Raspberry Pi SC0623 micro-USB supply, DigiKey](https://www.digikey.com/en/products/detail/raspberry-pi/SC0623/15298153)

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

### The carrier PCB

Added 2026-10-26, after the order above went in. The PCB replaces the
protoboard; everything a board house needs is in
[`controller/hardware/fab/`](../hardware/fab/README.md).

|   Qty | Item                                                         | Part / SKU                                                                                                      | Price       | Note                                                                                                            |
| ----: | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------------------------- |
| 1 lot | Carrier PCB: 2 layers, 160 × 60 mm, 1.6 mm FR-4, 1 oz copper | `fab/carrier-gerbers.zip`, any board house                                                                      | Not checked | The smallest lot the house sells. One board is installed; a second makes the bench build                        |
|     6 | 6P6C modular jack, right angle, through hole                 | Amphenol 54601-906LF, [DigiKey](https://www.digikey.com/en/products/detail/amphenol-cs-fci/54601-906LF/7589414) | Not checked | `K1`, `V1`, `K2`, `V2` and two spares. The board's footprint is KiCad's, drawn from Amphenol drawing C-BMJ-0082 |

Everything else on the board is in the order above: the THVD1410s, the
resistors and capacitors, the RJ45 jack, the 20-pin sockets (two for the
Pico 2, one cut into the WIZ850io's two 1×6 rows) and the test-point pins.
`fab/carrier-bom.csv` is the full list.

### Optional

| Qty | Item                     | Note                                                                                                                                                             |
| --: | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|   1 | Raspberry Pi Debug Probe | SWD flashing and `defmt` logs. Price not checked. A spare Pico running the probe firmware does the same job **[V]**, and micro-USB flashing works without either |

### Already owned

| Item                                          | Use                                                                                                                                                                                                           |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2 × Waveshare `USB TO RS485/422`, SKU `23949` | The bench rig: plays the K-99695 and the valve — §13                                                                                                                                                          |
| Rigol DHO804 oscilloscope                     | Bench checks 1 and 7; the valve-segment bias in Phase 3                                                                                                                                                       |
| Logic analyzer                                | Timing on the bench                                                                                                                                                                                           |
| 2 × straight-through 6P (RJ12) cords          | The leads from `K1` and `K2` to the K-99695's valve jacks. The Kohler cable's phone plug moves from the K-99695 to `V1`/`V2`; its valve end is a different connector and stays put (Nate, 2026-10-25) **[A]** |
| Multimeter                                    | Continuity, termination, the K-99695 ground check                                                                                                                                                             |

---

## Order later, only if needed

The part that waits on a measurement
([HARDWARE.md § 11](HARDWARE.md)):

| Item                                                 | Qty               | Closed by                                                                                                   |
| ---------------------------------------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------- |
| An isolated transceiver with its own isolated supply | 2, only if needed | The K-99695 ground check, [HARDWARE.md § 15](HARDWARE.md) item 8: only if its two jacks' pin 2 are separate |

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
