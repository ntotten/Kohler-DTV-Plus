# K-1737-K1 steam adapter — teardown photographs

Nine photographs of a Kohler DTV Steam Adapter, opened, taken 2026-08-29 by the
operator. **Tier [A] — our own hardware.** This is the first direct evidence
about steam hardware in this project; everything prior was Kohler documents
**[K]** or third-party analysis **[C]**.

| File           | Shows                                                                                   |
| -------------- | --------------------------------------------------------------------------------------- |
| `IMG_0772-3`   | Board in the housing, component side                                                    |
| `IMG_0774`     | Solder side, whole board — MCU, IC2, CN3, LEDs                                          |
| `IMG_0775-6`   | Component side out of the housing                                                       |
| `IMG_0777`     | Close-up, the three PC900V optocouplers                                                 |
| `IMG_0778`     | Connector edge, straight on                                                             |
| `IMG_0779`     | Lid label                                                                               |
| `IMG_0780`     | **Lid label and connector edge in one frame**                                           |
| `IMG_0786-796` | Close-ups added later the same day — `CN3`/`U2`, the optocouplers, `JACK1`, `CN1`/`CN2` |

`IMG_0780` is the load-bearing photograph: it fixes each label to its connector
with no inference.

Images are downscaled to 2000 px on the long edge and re-encoded at quality 70,
taking the set from 52 MB to 13 MB. Every part marking transcribed below was
read at that resolution or lower, so nothing legible was lost. Originals are
with the operator.

## Board identity

| Field        | Value                                                              |
| ------------ | ------------------------------------------------------------------ |
| Silkscreen   | `STEAM GENERATOR DTV` `20140212_REV_06`                            |
| Label        | `STEAM DTV (ED99)` · `DTV H` · `0106`                              |
| Part number  | `4104-04032` + suffix, struck through and overwritten on the label |
| Date code    | `2020.11.25`                                                       |
| Flammability | UL 94V-0 (`JRC-02V0`)                                              |

## Ports — observed, not inferred

Left to right on the connector edge, each label's `>` pointing at its connector:

| #   | Connector                         | Label on lid                  |
| --- | --------------------------------- | ----------------------------- |
| 1   | 6-position modular jack (RJ-type) | `TO STEAM GENERATOR >`        |
| 2   | Barrel jack, DC-style             | `ROOM TEMP SENSOR >`          |
| 3   | **4-pin polarized header**        | `FROM DTV CONTROL >`          |
| 4   | **4-pin polarized header**        | `TO NEXT DEVICE > (OPTIONAL)` |

Ports 3 and 4 are `CN1` and `CN2` on the silkscreen; which is which is not yet
determined.

**This corrects [STEAM-ADAPTER.md § 5](../../../controller/docs/STEAM-ADAPTER.md).**
That section inferred, marked **[I]**, that DTV+ peripheral ports are modular
jacks carrying a serial bus over patch cable, and concluded a screw-terminal
converter "is not, on its own, plausibly sufficient". The modular jack is the
**generator** port. The DTV+ side is a 4-pin polarized header.

## Indicators

Five, not the three the installation guide describes:

`GEN TEST` · `TEMP SENSOR` · `DATA LINK` · `CHK SYS` (red) · `OK` (green)

## Silicon

| Ref               | Observation                                                                                                                                           |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| MCU               | LQFP-64, marking reads `D78F0_52` — Renesas/NEC µPD78F0xxx, 78K0/78K0R family                                                                         |
| `IC2`             | **`ADM4852ARZ`**, date code `#948` — Analog Devices RS-485/RS-422 transceiver, **half duplex**, ⅛ unit load, slew-rate limited, 2.5 Mbps, 8-lead SOIC |
| `U2`              | **`ATMLH920 16CM`** — Atmel **AT24C16** 2 KB I²C EEPROM, SOIC-8                                                                                       |
| `JACK1`           | 6-position modular jack — the generator port                                                                                                          |
| `C13`             | **Unpopulated** two-hole footprint beside `R27`, one pad on `A`, one on `B`                                                                           |
| (unlabelled)      | 330 µF 16 V electrolytic near `CN1`/`CN2`. Designator not read — it is **not** `C13`                                                                  |
| `PT1` `PT2` `PT3` | Three Sharp **`PC900V`** high-speed logic-output optocouplers                                                                                         |
| `OSC1`            | 20.000 MHz crystal                                                                                                                                    |
| `IC3`             | TO-252 regulator                                                                                                                                      |
| `CN3`             | 6-pin header, silkscreened `TXD` `RXD` `RESET` `GND` `VCC` `CLK` — programming/debug                                                                  |
| `P/CL`            | 2-pin jumper or test point. **[I]** Expands plausibly to "power clean"                                                                                |
| `LED1-3`          | On board, driving the lid indicators                                                                                                                  |

## The link is RS-485 — settled

`IC2` is an **ADM4852**
([Analog Devices](https://www.analog.com/en/products/adm4852.html)). That is a
half-duplex RS-485/RS-422 transceiver, so the DTV+ peripheral link is **two-wire
differential RS-485**, and a standard converter is the correct part. Three
properties of the specific part chosen are informative:

| Property            | Value                 | What it implies                                                                                              |
| ------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------ |
| Duplex              | **Half**              | Two wires, A and B — not a four-wire RS-422 pair                                                             |
| Receiver input load | **⅛ unit load**       | Up to **256** transceivers on one bus. Built for a long daisy-chain, not a point link                        |
| Driver              | **Slew-rate limited** | Deliberately slow edges for EMI and reflection tolerance on long cable. 9600 baud is far inside its envelope |

**[I]** The three `PC900V` optocouplers now map cleanly onto the standard
8-pin transceiver: one for the receiver output, one for the driver input, one
for the tied driver/receiver enable. That is the textbook isolated half-duplex
RS-485 node, and it explains why there are exactly three.

## Architecture — inference **[I]**

The solder side shows a routing gap separating the board into two domains, with
the three optocouplers bridging it.

**Three optocouplers is the signature of an isolated half-duplex RS-485 node:**
driver input, receiver output, driver enable. Together with an 8-pin SOIC whose
marking begins `ADM`, the reading is an **opto-isolated RS-485 interface**.

The optocouplers and `IC2` sit on the `CN1`/`CN2` side of the board, and the
modular jack sits with the MCU and crystal. **[I]** So the isolated RS-485
interface serves the **DTV+ link**, and the generator link is on the MCU's
non-isolated side. That is the sensible arrangement: the DTV+ bus runs 25 ft
across a building and daisy-chains to other devices; the generator crossover
cable is 10–16 in.

All of this is inference from layout. It is not a traced schematic.

## The DTV+ connector pinout — measured 2026-08-29 **[A]**

Continuity from `CN1` and `CN2` to `IC2` (`ADM4852`), board open and unpowered.
**`CN1` and `CN2` are wired in parallel** — the same three nets reach both, so
the daisy-chain is a plain multi-drop bus and either header can be the input.

| Pin | `IC2` pin | Signal                                     |
| --- | --------- | ------------------------------------------ |
| 1   | 7         | **`B`**                                    |
| 2   | 6         | **`A`**                                    |
| 3   | 5         | **`GND`**                                  |
| 4   | —         | No connection to `IC2`. Not yet identified |

`IC2` pin 8 (`VCC`) reaches **no** connector pin.

**Note the ordering: `B` comes before `A`.** The reverse of what a guess would
produce, and the reason this was measured rather than assumed.

### Pin 1, anchored physically

**Pin 1 is the pin furthest from the barrel jack and nearest `IC2`. Pin 4 is
the pin nearest the barrel jack.** Both headers carry the same orientation.

Along the connector edge, barrel jack to the left, reading left to right:

```text
   [JACK1]   [barrel]   |  CN1              |  CN2              |
   generator  room temp | 4    3    2    1  | 4    3    2    1  |
                        | ?   GND    A    B | ?   GND    A    B |
                                                          ^
                                              pin 1 end, nearest IC2
```

Per the lid label, the header nearer the barrel jack is `FROM DTV CONTROL` and
the far one is `TO NEXT DEVICE (OPTIONAL)`. They are wired in parallel, so the
distinction is documentation rather than electrical.

Before crimping a cable, confirm the same orientation against the mating
housing's polarizing ramp — geometry plus a photograph is enough to get it
right, but the key is what the connector actually enforces.

### Bus protection and termination **[A]**

Traced visually on the solder side at the connector entry:

| Ref   | Marking    | Reading                                                   |
| ----- | ---------- | --------------------------------------------------------- |
| `D10` | `SMBJ 28A` | 600 W TVS, 28 V standoff, SMB package                     |
| `D11` | `SMBJ 28A` | Second TVS, matched pair                                  |
| `R27` | `121`      | **120 Ω** — sits between `D10` and `D11` at the bus entry |

`CN1` pin 1 (`B`) runs into `D10`. `A` and `B` therefore each have their own
clamp, which is the normal arrangement.

**[I]** 120 Ω between the two TVS diodes at an RS-485 connector entry is the
characteristic-impedance **bus termination**, giving the textbook network: a
termination resistor across the differential pair and one TVS per line to
ground. Not yet confirmed by measurement.

Three signs the bus is built for a real field run: ⅛-unit-load receiver,
slew-rate-limited driver, and per-line TVS clamping.

**A TVS is a diode, so meter readings across it are polarity-dependent.** A
meter's resistance mode forward-biases the clamp, and a path blocked one way
conducts the other. Take measurements touching `A` or `B` in both probe
polarities; copper reads the same either way, a junction does not.

### Termination — measured **[A]**

**114 Ω between connector pins 1 and 2, identical in both probe polarities.**
Symmetry rules out a semiconductor junction, so this is `R27` across the
differential pair and the adapter is permanently terminated. 114 Ω sits inside the 5 % band
for a 120 Ω part; the ADM4852's ⅛-unit-load input (≥ 96 kΩ) contributes nothing
in parallel.

**Our end stays unterminated.** At 9600 baud a bit lasts 104 µs while 25 ft of
cable is about 38 ns one way, so reflections settle thousands of times over
before the bit is sampled. The adapter's resistor already provides the DC load
and damping. Adding a second halves the bus to 60 Ω for no benefit, and a
chained second adapter would bring it to 40 Ω — below the 54 Ω that RS-485
drivers are specified against.

**[?]** A fixed terminator in every adapter still sits oddly with the
⅛-unit-load transceiver and the `TO NEXT DEVICE` header, which together imply a
long daisy-chain. Either Kohler intends this device to sit at a bus end, or the
few-device case was simply accepted. It does not affect a single-adapter link.

### `SMBJ28A` is unidirectional — confirmed **[A]**

Not the `CA` bidirectional variant. In the reverse direction it stands off 28 V;
in the forward direction it is an ordinary diode conducting from about 0.7 V.

**Consequence: this node has almost no negative common-mode headroom.** The
ADM4852 itself is specified for bus common mode from −7 V to +12 V, but a
unidirectional clamp to ground removes the negative part of that range at the
adapter. Any bus line driven more than ~0.7 V below the adapter's local ground
forward-biases the TVS and draws current.

**So the ground conductor is required, not optional.** Connector pin 3 (`GND`)
must be connected to our converter's `PE`, giving the two ends a shared
reference so the differential pair never sits far below the adapter's ground.
This is a departure from the valve links, where
[DESIGN.md](../../../controller/docs/DESIGN.md)
connects `PE` only if measurement shows a reference conductor is needed. Here
the reason is positive and known in advance.

Our converter is galvanically isolated, so tying its field-side `PE` to the
adapter's ground creates the shared reference without a loop back through the
Pi. As with the valve links, this link's field ground is never joined to any
other link's.

### `C13` — an unpopulated footprint, not a fitted capacitor

Pads labelled `C13` sit beside `R27`, one on the `A` net and one on the `B` net.
Probing pins 1 and 2 to them reads through, and across them reads the same
114 Ω as the pins themselves, because they are simply another point on those two
nets.

**[I]** A capacitor designator on unfitted pads across the differential pair is
a filter position Kohler provisioned and did not populate. Nothing is fitted
there — which is as it must be, since a capacitor of any size across `A`/`B`
would degrade a 9600-baud differential signal.

Recorded because an earlier round of this investigation mistook these pads for
the board's 330 µF electrolytic and spent effort explaining a reading that was
never anomalous. Confirm the pads are empty.

### Still open

1. **Pin 4 is a DC supply input. Whether we must drive it is open.**

   Established **[A]**: `CN1` pin 4 ties to `CN2` pin 4, so the bus carries four
   conductors. Pin 4 runs to `D9`, marked `M7` — a 1N4007 1 A rectifier — and
   **lands on `D9`'s non-band pad, the anode**, confirmed directly. The band
   faces away from the connectors, so current flows from the bus into the
   board.

   A series rectifier passing current inward is reverse-polarity protection on a
   **supply input**. Pin 4 is not a spare and not a shield.

   `D9` does not reach either terminal of the 330 µF electrolytic, so it feeds
   something else — a regulator, or another rail.

   **The supply chain is confirmed [A].** `R16` to `IC2` pin 8 measures
   1.2 kΩ — the resistor's own value — so `R16` sits in series between the
   `D9` node and the transceiver's `VCC`:

   ```text
   CN pin 4 ──> D9 (M7, anode) ──> R16 (1.2 kΩ) ──> IC2 pin 8 (VCC)
                                        └──> D7 (shunt, clamps the rail)
   ```

   **Three consequences:**

   1. **Our master must supply pin 4.** The lead is **four** conductors. Without
      it the `ADM4852` has no rail and the link never comes up — a failure that
      would present as a protocol problem, not a power one.
   2. **The DTV+ side is galvanically isolated — question closed.** A
      transceiver powered from the bus rather than from the board it sits on
      only makes sense across an isolation barrier, which is what the three
      `PC900V` optocouplers implied from the start.
   3. **The rail is well above 5 V.** 1.2 kΩ in series cannot feed a 5 V part
      from 5 V; the drop alone would brown it out. `D7` must shunt-regulate a
      higher bus rail down. The 330 µF capacitor's 16 V rating points at
      **12 V** **[I]**.

   **The bus rail is 12 V — derived, not read [I].** `D7` is a glass MELF with
   a cathode band and no marking legible under a phone macro, but its value is
   not needed. The `ADM4852` is a 5 V part, so `IC2` pin 8 sits at 5 V. `R16` is
   1.2 kΩ in series, and the 330 µF capacitor's 16 V rating caps the bus below
   16 V. A rail that clears 5 V by enough to drop usefully across 1.2 kΩ while
   staying under 16 V leaves 12 V as the only standard candidate.

   **Confirm on the bench before the link is built into anything.** Apply 12 V
   to pin 4 from a current-limited supply, ground to pin 3, and measure `IC2`
   pin 8. Around 5 V confirms the chain and the rail; the supply's current
   reading gives the adapter's draw, which sizes the permanent supply. Nothing
   else on the board is energised by this, and it is the first time this adapter
   would ever be powered.

   Two earlier conclusions here — "not a supply, leave unlanded", then "very
   likely bus power" — were drawn ahead of the evidence and are kept rather than
   deleted, per [AGENT.md](../../../AGENT.md) rule 5.

2. **Is the DTV+ side actually isolated?** Now genuinely open again. The
   earlier reading — three `PC900V` optocouplers implying an isolated
   transceiver — is **not** settled by these measurements, because three optos
   would equally suit an isolated link to the generator. One test decides it:
   **is `IC2` pin 5 (`GND`) the same net as the MCU-side ground?** Continuity
   from `IC2` pin 5 to `C16`'s negative terminal, or to the MCU ground, answers
   it. Same net means `IC2` sits in the MCU's domain and the optocouplers
   isolate the generator link instead.
3. **Is the adapter terminated?** Resistance directly between connector pins 1
   and 2 (`B` and `A`). Roughly 120 Ω means a termination resistor is fitted;
   several kΩ or open means not. This decides whether our end terminates.
4. **What connector family?** Measure centre of pin 1 to centre of pin 4:
   7.5 mm is 2.5 mm pitch (JST XH), 7.62 mm is 2.54 mm (Molex KK). Needed to
   buy a mating housing.

### What can be built now

Three of four conductors are known, which is enough for the lead: `B`, `A` and
`GND` to the converter's `TB`, `TA` and `PE`. Pin 4 only has to be resolved
before anything is plugged in, in case it is a live supply rail.

**Not determined from the photographs:** which physical domain `IC2` sits in.
The solder-side silkscreen is mirrored relative to the component side and the
two could not be registered reliably by eye. The continuity check answers it
anyway.
