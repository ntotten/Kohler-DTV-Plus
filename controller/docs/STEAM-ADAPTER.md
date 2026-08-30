# Steam as a third link on the replacement controller

Reference material for the steam link specified in
[HARDWARE.md § 12](HARDWARE.md): what Kohler publishes about the
K-1737-K1 adapter, the DTV+ protocol it speaks, the limits it enforces, and
where the published sources contradict each other.

Nothing here has been tested against steam hardware. Everything is from Kohler
documents **[K]**, the controller's own shipped code **[B]**, and third-party
reverse engineering **[C]**.

---

## Safety position

The generator is a self-contained appliance. Kohler documents its protections
in the generator itself (§6), which is the same architecture the valve links
rely on: the device owns its safety, and the master sends setpoints.

[DESIGN.md](DESIGN.md) is acceptable because the valve
owns mixing, over-temperature trips, and fail-closed behaviour; the replacement
master sends a setpoint to a device that protects itself. That claim is sourced
and testable —
[valve-control.md § Safety Ownership](../../docs/devices/valve-control.md#safety-ownership).

The equivalent claim for steam is partly established. Kohler documents
low-water, tank-high-limit and room-over-temperature protections inside the
generator (§6) **[K]**. Three gaps remain:

| Gap                                                                                             | Status                                                             |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Heating-element behaviour when the data link goes silent                                        | Not stated in any source                                           |
| Which timer ends a session                                                                      | Contested — see §7                                                 |
| `WARNING`: "A user interface must be located within the steam enclosure" **[K]**, in two guides | **Operator decision 2026-08-29: accepted, not a blocker** — see §6 |

Hazard comparison against the existing water design:

|                          | Water (existing design)                                                                            | Steam (this document)                                                                                                                                                                          |
| ------------------------ | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hazard                   | Scalding at the fitting; the user can step out                                                     | Whole-room air at up to 125 °F / 51.7 °C with ~100 % humidity; heat stress and impaired egress                                                                                                 |
| Energy source            | The house's existing hot supply                                                                    | The generator's own supply, behind the adapter. Out of scope for this project                                                                                                                  |
| Fail-closed proven?      | Claimed by upstream firmware analysis, and **the design requires it be measured at commissioning** | **Partly.** Kohler documents low-water, tank high-limit and room over-temperature trips in the generator **[K]** — but says nothing about what the element does when the data link goes silent |
| Unattended cycles        | None                                                                                               | Power clean runs **up to 45 minutes** at "extremely high" room temperature, discharging through the steam head, with no clean abort **[K]**                                                    |
| Kohler's own requirement | Interface optional; the valve is the safety device                                                 | **"A user interface must be located within the steam enclosure"** — a `WARNING`, in two guides **[K]**. Accepted as a recorded deviation, operator decision 2026-08-29 — §6                    |

Two questions stay open and are queued on Kohler case **#07797183**: what the
generator does when the DTV+ link goes silent, and whether the 20-minute shutoff
runs in the generator or the controller. Neither needs Kohler to answer first —
pulling the link mid-session measures both, which is what
[HARDWARE.md § 12](HARDWARE.md) schedules for Phase 5.

---

## Evidence tiers

Same scheme as [system-specification.md](../../docs/system-specification.md).

| Tier                       | Meaning                                                                                                                                                           |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **[A] Ours**               | Measured on our controller at `192.168.4.80`, 2026-08-22, read-only. Source: [`2026-08-22-idle-baseline/`](../../research/diagnostics/2026-08-22-idle-baseline/). |
| **[B] Shipped code**       | Read out of the controller's own web UI, mirrored at [`research/controller-mirror/`](../../research/controller-mirror/). Same model and firmware as ours.         |
| **[K] Kohler**             | A Kohler primary document. Cited inline.                                                                                                                          |
| **[C] Reverse-engineered** | Third-party analysis, chiefly [`research/xagon0/`](../../research/xagon0/). Not verified against our unit or against any steam hardware.                          |
| **[?] Unresolved**         | Sources disagree, or nobody has checked.                                                                                                                          |
| **[I] Inference**          | Our reasoning on top of the above. Flagged as inference every time.                                                                                               |

No tier [A] evidence about a running steam generator exists in this project
yet. Tier [A] here is the controller's own reported steam configuration.

---

## 1. What the controller reports with no adapter attached **[A]**

Useful as the baseline a steam link starts from, and as the set of
configuration ranges the firmware carries. From the 2026-08-22 idle baseline,
`values.cgi`:

| Field                                             | Value                                                          |
| ------------------------------------------------- | -------------------------------------------------------------- |
| `steam_con_string`                                | `"not_seen"`                                                   |
| `steam_version_string`                            | `"not_seen"`                                                   |
| `steam_installed`                                 | `false`                                                        |
| `steam_running`                                   | `false`                                                        |
| `steam_select`                                    | `false`                                                        |
| `steam_max_temp_string`                           | `125`                                                          |
| `steam_default_string_temp` / `steam_temp_string` | `110`                                                          |
| `steam_def_time_string` / `steam_time_string`     | `10`                                                           |
| `steam_power_clean_string`                        | `20.0` **and** `580.0` — the key appears twice in one response |
| `max_steam_runtime_enable` / `max_steam_runtime`  | `0` / `0`                                                      |
| `steamTimeRemaining`                              | `"-9"`                                                         |

`system_info.cgi`: `steamStatus = ""`, `steamTempStatus = "110"`,
`steamTimeStatus = "-9:00"`, `ui_steam_running = false`.

`sim_dev_values.cgi`: `steam_status = 2`. DTV+ port 5 is the steam **simulation**
port when no physical device is attached **[C]**
([implementation-quirks.md](../../research/xagon0/docs/implementation-quirks.md)).

These are stored defaults for a device the controller has not met: useful as
configuration ranges, not as observations of a generator.

The `-9` / `-9:00` values are sentinels for "no steam session". Their exact
meaning is unverified **[?]**.

---

## 2. Physical and electrical layer

**Partial yes, with an unverified gap at the connector.**

The controller side is well described. Its eight DTV+ ports are **RS-485
half-duplex, 9600 8N1, no flow control**, GPIO-driven DE/RE, 2048-byte TX/RX
buffers, one independent bus per port **[C]**
([hardware.md](../../docs/hardware.md), [system-specification.md § 4](../../docs/system-specification.md)).
The documented RS-485 connector pinout is **1 = A+, 2 = B−, 3 = GND**, and the
same table covers both the DTV+ and the valve ports **[C]**.

That is electrically identical to what the two Waveshare `USB TO RS485/422`
converters already do for the valves. **[I]** So _if_ the steam link is a plain
DTV+ RS-485 port, a third identical converter is physically sufficient, and it
should be a third separately isolated one for the same reason the valve design
rejected the dual-channel part.

What is **not** established:

- **The connector housing and keying.** The 3-pin A+/B−/GND table is a signal
  assignment, not a part number. The valve design already refuses to buy mating
  connectors by assumption — [DESIGN.md § Hardware, "Not orderable from documents"](DESIGN.md)
  requires photographing both ends first. The same rule applies here, harder,
  because nobody in this project has ever seen a DTV+ peripheral port populated.
- **Whether the port carries device power.** [hardware.md](../../docs/hardware.md) says
  "each peripheral device is powered separately" and shows a `VCC` pin only on
  the _UI_ connector, not on the RS-485 connector **[C]**. **[I]** That reads as
  "DTV+ ports are signal-only", but it is inference from an omission, which is
  weak evidence.
- **Termination and idle bias on a DTV+ port.** Unknown. Same open question the
  valve design lists and defers to measurement.

**Verdict, from the controller side alone:** _probably_ yes, one more isolated
USB-RS-485 converter at 9600 8N1.

**But §5 revises this downward.** Kohler's own adapter documentation describes
the field cabling as 25 ft of telephone-style modular cable, not a terminal
block, and never names the electrical standard. Read §5 before treating a third
converter as the answer.

---

## 3. Implementation scope

**A whole second protocol stack. This is the largest single finding in this
document.**

The replacement controller currently implements **Saturn** only. DTV+ is a
different protocol on the same kind of wire at the same baud rate — the repo
already calls this the system's most confusing property
([system-specification.md § 5](../../docs/system-specification.md)), and upstream notes
that a DTV+ frame sent to a valve produces "no response or garbage" **[C]**.

New work required, all tier **[C]** from
[dtv-plus-protocol.md](../../research/xagon0/docs/protocols/dtv-plus-protocol.md):

| Layer          | What has to be built                                                                                                                                 | Notes                                                                                       |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Framing        | `SOF 0x88 · DEST · SRC · CMD · payload · CHECKSUM · EOF 0x55`                                                                                        | Saturn shares none of this                                                                  |
| Byte stuffing  | `0x88`, `0x55`, `0xAA` escaped by prefixing `0xAA`; SOF/EOF never escaped                                                                            | Saturn has no escaping documented here                                                      |
| Checksum       | 2's complement of `DEST+SRC+CMD+payload`, verified by summing to `0x00`                                                                              | Different from Saturn's                                                                     |
| Discovery      | 3-step: `DEV_ADDRESS_OPP 0x05` broadcast → `DEV_REQUEST_ADDR 0x06` carrying the device ID → `DEV_ASSIGN_ADDR 0x07`                                   | Address assignment, which the valve design permits only in `DISCOVERY` state with water off |
| Addressing     | Device ships at `0x00`; master assigns from `0x03–0x07`; `0xFF` broadcast                                                                            | Steam's `0x05` is a **device ID**, not a bus address — see the contradiction in §10         |
| Command set    | At minimum `GET_DEV_STATUS 0x30`, `STATUS_UPDATE 0x31`, `SET_DEV_PARAM 0x34`, `DEV_ACK 0x35`, `DEV_NAK 0x36`, `ERROR 0x37`, `CLEAR_FAULT_FLAGS 0x3A` | Everything else must be denied, per the valve design's allowlist rule                       |
| Timing         | Steam tick **150 ms** (vs 525 ms for valves), reply timeout 300 ms, echo window 150 ms                                                               | 3.5× the transaction rate of a valve bus                                                    |
| Half-duplex    | DE/RE handling and TX echo consumption                                                                                                               | The Waveshare converter does direction automatically; echo still has to be discarded        |
| Steam payloads | Status decode (actual temp, desired temp, op state, timer min/sec, error flags) and the `SET_DEV_PARAM` write shape                                  | [steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md)                 |
| Fx2 encoding   | A second temperature encoding alongside Cx2, with conversion at the boundary                                                                         | §7                                                                                          |

**[I]** Rough shape of the effort: the serial-port count goes 2 → 3, but the
protocol implementations go 1 → 2, the fixture/emulator work roughly doubles,
and the capture campaign in [DESIGN.md § Packet capture questions](DESIGN.md)
has to be repeated end to end on a bus type nobody has captured yet. Calling
this "one more port" would be wrong by a wide margin.

There is also no shortcut through the existing captures: **no DTV+ bus has ever
been captured in this project.** All eight of this controller's DTV+ peripheral
ports are empty — `steam`, `music_module`, `lighting`, `amp`, `watertile` and
`watertile2` all report `not_seen`, and `rain_installed`, `bridge_installed` and
`light1..3_installed` are all `false` **[A]**.

The one DTV+ speaker in the house is the **wall interface**, which is connected
(`ui1_con_string = "conn"`, `num_interface = 1`) since the repair recorded in
[INVESTIGATIONS.md § I2](../../INVESTIGATIONS.md). But it sits on the
controller's **separate UI UART**, not on one of the eight peripheral ports, and
that link is documented at **115200** 8N1 for Amulet CRC while DTV+ is **9600**
**[C]**. Discovery is said to happen over DTV+ before the switch to Amulet
([system-specification.md § 4](../../docs/system-specification.md)); the DTV+ command set
does contain `CHANGE_BAUD 0x18`. **[I]** So a capture of that link plausibly
contains real DTV+ discovery frames, but whether and when the baud changes is
**unverified**, and the capture rig would have to cope with it. See
[Cheapest next steps](#cheapest-next-steps) step 6.

---

## 4. The K-1737-K1 adapter

**A device. An active, generator-powered module with three status LEDs, a
terminated multi-drop port, and its own temperature sensor.** Not a cable.

Kohler publishes the installation mechanics in detail and publishes essentially
nothing about the electronics. The load-bearing document is **1235393-2-C,
"Installation and Care Guide — Steam Adapter Kit / Steam Head for DTV+"**,
covering K-1737-K1, K-1838-K1, K-5548-K1 and K-5549-K1
([resources.kohler.com](https://resources.kohler.com/onlinecatalog/pdf/1235393_2.pdf)).
Everything in this section is **[K]** unless marked otherwise.

### What is in the kit

Kohler publishes no verbatim parts list. The following is enumerated from the
installation steps of 1235393-2-C, so treat it as complete-ish rather than
authoritative:

| Item                          | Detail                                                                                                                                                                          |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Steam adapter module**      | Adhesive-backed: "Mount the adapter firmly to the side of the steam generator." Four ports — generator crossover, temperature sensor, data cable, and one marked **"Optional"** |
| **Steam head**                | With aromatherapy well, housing, gasket, ½" NPT                                                                                                                                 |
| **Remote temperature sensor** | On a long wire. Wires to the **adapter**, not to the generator                                                                                                                  |
| **Crossover cable**           | 10 in (254 mm) silver **or** 16 in (406 mm) white — adapter to generator                                                                                                        |
| **Data cable**                | **25 ft (7.6 m)** — adapter to the K-99695 controller                                                                                                                           |
| **Terminator**                | "Install the terminator into the 'Optional' port on the steam adapter"                                                                                                          |
| **Ferrite + cable tie**       | For the adapter end of the data cable                                                                                                                                           |
| **Test kit**                  | Referenced repeatedly in the troubleshooting section                                                                                                                            |

The adapter's dimensions, weight, enclosure, voltage and current draw are **not
published anywhere the research could reach**.

### Topology

```text
  remote temp sensor ──┐
                       ├── STEAM ADAPTER ──25 ft data cable── K-99695
  steam generator ─────┘   (adhesive-mounted           "Digital Interface(s) and
     via 10"/16"            to the generator)           Optional Components" ports
     crossover cable        + terminator in the
                            "Optional" port
```

The adapter plugs into the generator's **"Steam Generator Control Cable Input"**
— the same port the standard Kohler steam wall keypad uses. Kohler's product
copy says the kit "enables the DTV+ system to control the KOHLER steam
generator, **replacing the standard steam interface**". So the adapter is not
inserted alongside the native control; it takes its place.

### It is a device, and here is the evidence

- **Three status LEDs, one per interface**: `GEN TEST` (generator connection),
  `TEMP SENSOR`, `DATA LINK`. Each blinks green once per second in normal
  operation. Passive harnesses do not have a heartbeat.
- **It is powered, and powered from the generator — not from the controller.**
  1235393-2-C troubleshooting item 5, "Lights on the adapter are off", gives
  probable causes as "Inadequate power is supplied to the steam generator" and
  "The user interface is not receiving power from the steam generator". Power
  flows generator → adapter → interface.
- **It owns the room temperature sensor.** In a DTV installation the sensor
  wires to the adapter, so room temperature is measured by the adapter's
  circuit, not the controller's.
- **Its bus is terminated and multi-drop.** A 3-way straight coupler daisy-chains
  two generators in the dual kit; the "Optional" port takes a terminator, and
  the DTV II guide says "The terminator is factory installed and must remain
  installed if no additional equipment is to be connected".
- **Miswiring has a protocol-level symptom**, not a dead-short one: "Steam menu
  disappears after requesting steam. A. Cables between the steam generator(s)
  and adapter are installed incorrectly" — crossover versus straight-through.
- Error **0408 = "A communication error has occurred"**, remedied by a system
  reset from the user interface.

### Which generators — and a caution about this specific kit

Kohler's **Steam Specification Guide**, form 22-3187-0822
([resources.kohler.com](https://resources.kohler.com/webassets/kpna/brochures/KOHLER_SteamSpecGuide.pdf)),
maps the current Invigoration-series generators to **K-5548-K1** (single) and
**K-5549-K1** (dual) — **not** to K-1737-K1. The K-1737 family is the earlier
kit: the DTV II roughing-in sheet 1069333-1-C
([resources.kohler.com](https://resources.kohler.com/onlinecatalog/pdf/1069333_1.pdf))
lists it against K-1695 (5 kW), K-1708 (7 kW), K-1733 (9 kW), K-1734 (11 kW),
K-1696 (13 kW) and K-1713 (15 kW), and Kohler's product copy for the `-K1`
variant says "for use with Fast-Response steam generators between 5 and
15 kilowatts".

**[I]** So the kit the operator owns appears to target the **earlier 5–15 kW
single-generator line**, with K-5548-K1 as its Invigoration-era successor.
1235393-2-C covers both in one book but distinguishes them by sensor: the wall
hole is **⅜ in for K-1737-K1/K-1838-K1** and **7⁄16 in for
K-5548-K1/K-5549-K1**. Before buying a generator to pair with this kit, confirm
the pairing with Kohler — this is a compatibility inference from two documents,
not a statement Kohler makes about K-1737-K1 directly.

Kohler's current service instruction **1581267-2-B, "Digital Steam Adapter"
(2025-09)** ([techcomm.kohler.com](https://techcomm.kohler.com/techcomm/pdf/1581267-2.pdf))
shows the same adapter now serving **both DTV+ and Anthem+**, renames the cables
("controller harness", "Anthem harness (blue)", "daisy chain harness (blue)",
"16 in crossover cable (silver)"), and adds "Apply dielectric grease to all
connectors."

### What Kohler does not say

- **No document names the protocol.** Not RS-485, not Modbus, not anything. The
  research found no published statement.
- **No wiring-terminal diagram was obtainable.** The two Kohler Assist wiring
  PDFs are hosted behind a Salesforce login and returned a login page rather
  than a document.
- **No spec or roughing-in sheet numbered for K-1737-K1 exists** at the usual
  `resources.kohler.com` paths — both guessed URLs returned 404. Only the DTV II
  sheet 1069333-1-C was found.
- **No adapter voltage, current, connector type, or pin count.**
- **No UL 499 / UL 1951 / ASME listing statement** appeared in any fetchable
  Kohler PDF; those likely live on the physical rating label.
- **No third-party reverse engineering exists at all.** A GitHub code search for
  `steam_on.cgi`, `SteamOperationState`, `DT_W_Steam` and `powerclean_check.cgi`
  matched **exactly one repository — this project's own**. Nobody has published
  a teardown of the adapter, a capture of the adapter↔generator bus, or a
  photograph of its board. Every public artifact stops at the controller's HTTP
  surface.

---

## 5. Consequences for the electrical design

Kohler's adapter documentation constrains the field cabling in a way the
controller-side pinout table in §2 does not capture.

**The controller-side link is a 25 ft cable into the "Digital Interface(s) and
Optional Components" port group** — the same group the wall interface uses. And
the wall interface's cable is already documented here as **"up to 25 ft of
RJ45-terminated cable plus an in-line coupler… Nothing about it is Ethernet"**
([wall-interface.md](../../docs/devices/wall-interface.md)), which matches the
K-99693-P spec sheet's own bill of materials: "25′ (7.62 m) / RJ45 Coupler /
RJ45 Ethernet Cable" **[K]**
([`K-99693-P_spec.pdf`](../../research/reference/K-99693-P_spec.pdf)).

Kohler describes the steam side as **telephone-style modular cable**:
1235393-2-C §1 says "If the steam generator is not within 25′ (7.6 m) of the
controller, add a telephone-style extension cable", and the crossover-versus-
straight-through distinction (silver/white = crossover, black =
straight-through) is modular-telephone wiring convention.

> **Superseded 2026-08-29 by the teardown** —
> [`research/reference/steam-adapter/`](../../research/reference/steam-adapter/).
> The paragraphs below inferred that DTV+ peripheral ports are modular jacks and
> that a screw-terminal converter would not be sufficient. **Both are wrong.**
> The modular jack is the **generator** port; the DTV+ side is a **4-pin
> polarized header**, labelled `FROM DTV CONTROL` on the adapter's own lid, with
> a second identical header marked `TO NEXT DEVICE (OPTIONAL)` for the
> daisy-chain. Kept here as the record of a wrong turn, per
> [AGENT.md](../../AGENT.md) rule 5.

**[I]** ~~Putting those together: the DTV+ peripheral ports are almost certainly
modular jacks carrying a serial bus over patch cable, rather than a bare 3-pin
A+/B−/GND terminal, and some adapter from a modular jack to A/B/GND would be
needed.~~ Wrong — see above. What survives is the narrower point that Kohler
never names an electrical standard for the link in any published document.

**And the teardown settles the protocol.** `IC2` is an **`ADM4852`** **[A]** —
a half-duplex RS-485/RS-422 transceiver, ⅛ unit load, slew-rate limited. The
DTV+ peripheral link is two-wire RS-485, the three `PC900V` optocouplers are its
receiver output, driver input and tied enable, and a standard RS-485 converter
is the right part. **[?]** was the right tier for a year; it is **[A]** now.

## 6. Safety ownership

**Mostly the generator, per Kohler's own guides, with two exceptions.**

The primary source is **1230487-2-E, "Installation and Care Guide — Steam
Generator"**
([resources.kohler.com](https://resources.kohler.com/webassets/kpna/catalog/pdf/en/1230487_2.pdf)),
plus the native control-kit guides **1230489-2-C**
([resources.kohler.com](https://resources.kohler.com/onlinecatalog/pdf/1230489_2.pdf))
and **1045789-5-C**
([techcomm.kohler.com](https://techcomm.kohler.com/techcomm/pdf/1045789-5.pdf)).

### Generator-owned protections **[K]**

| Protection                 | Evidence                                                                                                                                                              |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Low water / dry fire**   | Error `0140-A`, "Tank water level is too low"                                                                                                                         |
| **Tank high-limit**        | Error `0140-B`, "Tank temperature limit has been exceeded"                                                                                                            |
| **Automatic fill shutoff** | "Steam generators are equipped with an automatic shut-off. The water will stop after the unit is full"                                                                |
| **Pressure relief valve**  | ¾″ NPT, mounted at the top; must not be plumbed into the steam line or directed into the enclosure                                                                    |
| **Room over-temperature**  | Error `0120`, "The temperature in the steam room has exceeded the maximum allowable level." Remedy: "Reset power to the steam generator and ventilate the steam room" |
| **Session auto-shutoff**   | Steam Spec Guide: "Auto Shutoff — Automatically shuts down after 20 minutes if not reactivated"                                                                       |

This closes unknowns that §11 originally listed as blocking. Dry-fire and
low-water protection **do** exist, in the generator, and Kohler documents them.

**[I]** The `0120` room-over-temperature path is the important one for a
replacement master: the room sensor wires to the **adapter**, and the error is
cleared by cycling generator power, not by a bus command. That is the shape of a
protection that does not depend on the controller — which is exactly the
property the valve design relies on. It is not proof that the trip is
independent of the bus, but it is the best evidence available.

### The two exceptions

**1. Kohler requires a user interface inside the steam enclosure.** This is a
`WARNING`, twice:

> "WARNING: Risk of personal injury. A user interface must be located within the
> steam enclosure to allow temperature regulation and control of the steam
> flow." — 1235393-2-C

> "Do not install the steam control user interface outside the steam enclosure.
> The user interface must be installed within the enclosure to allow the sensors
> to regulate the temperature and control the flow of steam." — 1230487-2-E

A Raspberry Pi in a service enclosure is not a user interface inside the steam
room, and on this system the in-enclosure interface is the K-99693, which the
replacement plan powers down
([DESIGN.md](DESIGN.md), Phase 4).

**Operator decision, 2026-08-29: accepted and not treated as a blocker.** The
reasoning is recorded here because it is a deliberate deviation from a
manufacturer `WARNING`, and because it partly answers itself.

Kohler states two purposes in the same sentence, and they separate:

| Purpose Kohler gives                                              | Who satisfies it in a DTV+ install                                                                                                                                                            |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "allow the **sensors** to regulate the temperature"               | **The K-1737-K1 kit's own remote temperature sensor**, which wires to the adapter, not to the controller or the interface (§4) **[K]**. Room temperature is measured by the adapter's circuit |
| "**control** of the steam flow" — a means to stop it, in the room | Unresolved by the kit. The operator's position is that removing power is the remedy they would actually use, and that a touchscreen is the wrong instrument in an emergency                   |

**[I]** The sensing half of the requirement therefore appears to be met by the
kit Kohler sells for this exact topology, independently of any wall interface.
That is inference from Kohler's own description of the kit, not a statement
Kohler makes.

For the control half the operator's position is that removing power is the
remedy they would actually use, and that a touchscreen is the wrong instrument
in an emergency. How the generator's supply is switched is the installer's
concern, not this project's.

The deviation is recorded here and in the commissioning report.

**2. Where the session timer runs is still unknown.** Kohler documents a
20-minute auto-shutoff as a generator feature **[K]**, while the
reverse-engineered notes describe `steamOnTicker` counting seconds against
`steamTimerSetTime` with `0` **disabling automatic shutoff entirely** **[C]**.
Those may be two independent timers, or the same one described from two sides.
**[?]** Until it is settled, a replacement master cannot claim that a crash
mid-session is safe.

### Side by side with the valve

The valve column is sourced from
[valve-control.md § Safety Ownership](../../docs/devices/valve-control.md#safety-ownership).

| Protection                   | Valve — who owns it                                                                                                                  | Steam — who owns it                                                                                                                                                                                                                                       |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Closed-loop regulation       | **Valve.** The controller sends a setpoint and reads a thermistor; there is no PID in the controller **[C]**                         | **Unknown.** [steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md) shows the controller writing a desired temp and polling actual temp, which is the same shape — but nobody has confirmed the loop closes in the generator **[?]** |
| Hard temperature envelope    | **Valve.** Setpoints outside 30–49 °C rejected with `RANGE_ERROR`; `MAX_WATER_TEMP` Cx2 98 is a hardware ceiling **[C]**             | **Generator.** "Maximum allowable temperature 125 °F (52 °C); minimum operating temperature 90 °F (32 °C)" **[K]**. Whether the firmware also clamps is unverified                                                                                        |
| Over-temperature trip        | **Valve.** `OVERTEMP_OUTLET_ERROR`, `OVERTEMP_CONTROL_ERROR` **[C]**                                                                 | **Generator.** Error `0120`, room temperature exceeded, cleared by cycling generator power **[K]**. Status also carries Overtemperature (bit 5, `0x20`) and Safety circuit (bit 6, `0x40`) flags **[C]**                                                  |
| Dry-fire / low water         | n/a                                                                                                                                  | **Generator.** Error `0140-A` low tank water, `0140-B` tank temperature limit, plus automatic fill shutoff and a ¾″ pressure relief valve **[K]**                                                                                                         |
| Fail-closed on comms loss    | **Valve.** Comms loss times out and closes the valve; power loss closes the solenoids. "The failure direction is always OFF" **[C]** | **Unknown.** The documented behaviour is that the _controller_ retries and then latches a permanent error **[C]**; error `0408` is "A communication error has occurred", remedied by a UI reset **[K]**. Neither says what the heating element does       |
| Session timeout              | Prompt 3 valve has `PROMPT3_TIMEOUT_MAX` 1800 s in the **valve** **[C]**                                                             | **Contested.** Kohler documents a generator "Auto Shutoff… after 20 minutes if not reactivated" **[K]**; upstream describes a `steamOnTicker`/`steamTimerSetTime` pair where `0` disables shutoff **[C]**. Same timer or two? **[?]**                     |
| In-room user interface       | n/a                                                                                                                                  | **Required by Kohler**, as a `WARNING`, in two separate guides **[K]** — see above                                                                                                                                                                        |
| Unattended maintenance cycle | n/a                                                                                                                                  | **Power clean, up to 45 minutes, "Do not enter the steam room"** **[K]** — §8                                                                                                                                                                             |

**[I]** Net assessment: the generator is documented as a self-protecting
appliance, which is the architecture the valve design depends on. Two gaps are
not closable by reading — what the heating element does when the bus goes
silent, and which timer actually stops a session — and one blocker is not
closable by measurement at all: Kohler requires a user interface in the
enclosure.

---

## 7. Temperature and runtime limits

### Temperature: Fx2, not Cx2

Steam uses **Fahrenheit × 2**; valves use **Celsius × 2**. Conversion at the
boundary is `Fx2 = ((Cx2 × 9) / 5) + 64` **[C]**
([temperature-system.md](../../docs/control-logic/temperature-system.md)).

This is already flagged as a footgun in
[system-specification.md § 8](../../docs/system-specification.md). Adding a real steam
link makes it a live one rather than a theoretical one, because the replacement
controller would then hold both encodings in the same process:

| Mistake                                       | Result                                                                                                                                               |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Cx2 for 43 °C (`86`) sent to steam as Fx2     | Asks for 43 °F. Harmless, just broken                                                                                                                |
| Fx2 for 110 °F (`220`) sent to a valve as Cx2 | Asks for **110 °C**. Rejected by `RANGE_ERROR` if the valve behaves as documented **[C]** — but it is a setpoint no code should ever be able to form |

**[I]** A useful structural defence: make the two encodings distinct types that
cannot be assigned to each other, and put the conversion in exactly one place.
Runtime range-checking alone is not enough, because the failure is a units
error, not a magnitude error.

### The limits

| Limit                                    | Value                                                                                                                                                          | Tier       | Source                                                                                                                                                                                          |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Target temperature range, wall interface | **90 °F (32 °C) – 125 °F (52 °C)**, 1 °F / 0.5 °C steps                                                                                                        | **[K]**    | User Guide 1241234-5-D p. 67, "Steam – Setup" ([`research/reference/`](../../research/reference/))                                                                                              |
| Factory default target                   | **110 °F (43 °C)**                                                                                                                                             | **[K]**    | Same, p. 67 — and matches `steam_temp_string = 110` **[A]**                                                                                                                                     |
| Configured max on this system            | **125 °F**                                                                                                                                                     | **[A]**    | `steam_max_temp_string = 125`                                                                                                                                                                   |
| Default duration                         | **10 minutes**                                                                                                                                                 | **[K][A]** | Guide p. 67; `steam_def_time_string = 10`                                                                                                                                                       |
| Maximum session duration                 | **20 minutes**                                                                                                                                                 | **[K][B]** | Guide p. 67 "The maximum set steam duration time is 20 minutes"; web UI input is `min="1" max="20"` and clamps on change                                                                        |
| Optional max-run-time cap                | No Limit / 20 / 25 / 30 / 35 minutes; **disabled here**                                                                                                        | **[B][A]** | `settings.html` dropdown; `max_steam_runtime_enable = 0`                                                                                                                                        |
| Firmware min setpoint                    | `MIN_STEAM_SETPOINT` given as Cx2 48 = 24 °C = 75 °F (Fx2 150)                                                                                                 | **[C]**    | [steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md)                                                                                                                     |
| Max pre-heat                             | 10 minutes **[C]** vs 20 minutes **[K]** — see §10                                                                                                             | **[?]**    |                                                                                                                                                                                                 |
| **Generator's own limits**               | Max allowable **125 °F (52 °C)**; min operating **90 °F (32 °C)**; min run time **10 min**; max time allowed **20 min**; ships preset to **113 °F for 15 min** | **[K]**    | 1230489-2-C ([resources.kohler.com](https://resources.kohler.com/onlinecatalog/pdf/1230489_2.pdf)), 1045789-5-C ([techcomm.kohler.com](https://techcomm.kohler.com/techcomm/pdf/1045789-5.pdf)) |

The generator's native limits and the DTV+ controller's limits are the **same
numbers** — 90–125 °F, 20-minute cap. **[I]** That is consistent with the
generator owning the envelope and the DTV+ path simply inheriting it, which is
the architecture this design would want. It is not proof, because a shared
number could equally be two independent implementations of one product
requirement.

Two constraints follow.

**125 °F is not a firmware constant, it is a settings field.** The controller's
own settings page renders Max Temperature as a bare `<input type="number">` with
no `min` or `max` attribute, written straight to `save_variable.cgi` index 58
**[B]**. That is the same trap as the water `max_temp` = 113 °F noted in
[DISCLAIMER.md](../../DISCLAIMER.md): an installer setting, not a safety
guarantee. Whether the generator or the firmware clamps above 125 °F is
**unknown**.

**125 °F is exactly the wall interface's rated ambient maximum.** The K-99693-P
spec sheet gives "Max. Ambient temp: 125 °F (51.7 °C)" and warns "Do not install
the digital interface above the steamhead of a steam unit" **[K]**
([`K-99693-P_spec.pdf`](../../research/reference/K-99693-P_spec.pdf); also
[wall-interface.md](../../docs/devices/wall-interface.md)). **[I]** That the steam
ceiling and the interface's ambient rating are the same number looks like a
system-integration limit rather than a coincidence — but that is inference, and
it would not be the binding constraint on a human in the room anyway.

**[I]** One encoding observation: Fx2 is documented as a single byte, so it
tops out at 255 = 127.5 °F. A 125 °F ceiling sits just under the encoding's own
range. That is consistent, not proof of anything, but it means an implementation
must not assume headroom above 127.5 °F exists in the wire format.

### The web path is less constrained than the touchscreen path **[B][I]**

`steam_on.cgi?temp=&time=` is fed directly from the two input boxes on
`control.html`. The **time** box carries `min="1" max="20"`; the **temp** box
carries no `min` at all, and gets only its `max` set at runtime from
`steam_max_temp_string`. So the CGI path will accept a steam temperature below
the 90 °F floor the wall interface enforces. Whether the firmware rejects it is
untested. **[I]** By analogy with `quick_shower.cgi`, the `temp` parameter is
whole degrees in the system's configured unit (°F here), not Fx2 — the Fx2
encoding lives below the web layer, on the wire. Unverified for steam
specifically.

---

## 8. The power-clean cycle

### What the cycle is **[K]**

From User Guide 1241234-5-D pp. 38 and 70 — this is Kohler's own text, and it is
the strongest safety evidence in this document:

- Power clean is "an automatic cleaning procedure for proper, safe maintenance
  of the steam generator".
- The system allows **600 minutes of cumulative steam usage** before a cleaning
  cycle is required. After 600 minutes the interface goes **directly** to the
  Power Clean screen, and "the cleaning cycle must be completed in order to
  reset the 600-minute counter and enable steam use". Steam is gated behind it.
- **"Power clean will run for 45 minutes. Once activated, you must remain out of
  the steam area until the cleaning cycle has completed."**
- **"CAUTION: Stay out of the steam area until the cleaning cycle is complete."**
- Starting it gives a 1-minute countdown, then the cycle runs for 45 minutes,
  with a modal showing rotating arrows. It can be cancelled with the power icon.

The generator-side guides go further, and they **confirm the elevated
temperature** that upstream only inferred — 1230489-2-C, 1230487-2-E and
1235393-2-C **[K]**:

- **Trigger:** cumulative runtime. "Users will be automatically reminded to use
  power clean after 600 minutes of steam generator use." The native keypad
  displays `run` `PCLn`.
- **Grace, then lockout:** "The steam generator may be used three times after
  the 'run' 'PCLn' message is delivered and then the steam generator will not
  operate until the power clean cycle has been completed." 1235393-2-C: "Failure
  to initiate power clean after the third reminder will result in the system
  locking."
- **What it does:** it flushes water out **through the steam head**. "The
  PowerClean function of the generator flushes through the steam head. An
  additional drain line is not required for this feature."
- **Elevated temperature, stated as a WARNING:** _"WARNING: Risk of personal
  injury. Steam room temperatures will be extremely high when the power clean
  function is activated. Do not enter the steam room while the power clean
  function is in progress."_
- **No clean abort:** "If electrical power to the steam generator is interrupted
  during the power clean function, the power clean cycle will need to be
  restarted", and "The power clean cycle must be entirely completed before
  normal steam generator operation can be resumed."
- Manual activation on the native keypad is Timer + Increase + Decrease held for
  5 seconds.

Upstream adds, tier **[C]**, that the cycle is entered over the bus by setting
the generator's operation state to `STEAM_POWER_CLEAN` (`0xCC`)
([steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md)).

So the cycle is: up to 45 minutes, at extremely high room temperature, **with
hot water discharging from the steam head**, with no safe interruption, and with
steam locked out until it completes.

**[I]** For the replacement controller this is the single most dangerous
sequence in the steam feature set: a 45-minute, elevated-temperature,
unattended cycle that the vendor explicitly tells humans to stay out of. Any
replacement master that can start it must also be able to prove nobody is in the
room, which is a problem software cannot solve. A defensible position is that
the replacement controller **never** initiates power clean, and steam is simply
locked out when the counter expires until someone runs it from the factory path.

### The 600-minute counter on this system **[A][?]**

`values.cgi` returns `steam_power_clean_string` **twice in one response**, as
`20.0` and `580.0`. A JSON parser takes the last, `580.0`. The settings page
labels this field "Time Remaining before Power Clean (minutes)" **[B]**, and
Kohler's own screenshot of that page in the guide shows **580** with Default
Time 10, Max Temperature 125, Default Temperature 110 — the identical set of
values our controller reports **[K][A]**.

`20 + 580 = 600`, which matches the documented interval. **[I]** That is
suggestive of a used/remaining pair emitted under one key, but it is a guess:
this system has never run steam, so 580 cannot be a live countdown, and Kohler's
own illustration also shows 580. Recorded as an unexplained duplicate key, not
as a decoded field.

### The `powerclean_check.cgi` rating is wrong **[B]** — see §10

The controller's own shipped `settings.js` uses `powerclean_check.cgi` as a pure
status read, polled once per second, and triggers the cycle through a completely
different endpoint. Details in the contradictions section, because this one
changes the repo's own safety table.

---

## 9. Steam is coupled to the valve buses **[K]**

The valve design treats each zone as an isolated bus with its own state
machine. Steam does not fit that model, because two documented features couple
steam to the water side:

- **Deluge.** User Guide p. 69: during a steam session, the deluge control
  "activates 10 seconds of water flow from the default showerhead". If water was
  already running, the deluge temperature matches the other active fittings on
  the same valve. So a steam session can command a **Saturn valve**.
- **Spa "Steam Coach".** One of the spa experiences is `steam coach`, and the
  system's spa scripts are listed with a "Steam Compatibility" line per shower
  configuration **[K]**. The controller's own `control.html` carries the
  interlock text "Cannot Run Steam While Spa Is Running" **[B]**.

**[I]** Consequence for the design: a steam-capable replacement controller
cannot keep steam in its own lane. It needs a cross-bus coordinator that can
open a valve outlet for 10 seconds while a steam session is running, plus a
mutual-exclusion rule against spa. That is state-machine work spanning both
protocols, and it is the kind of coupling that turns "add a third link" into a
redesign of the service's core. It also means the [DESIGN.md](DESIGN.md)
rule "never open a valve without explicit consent" now has a second caller.

If steam is ever added, the cheapest safe scope is almost certainly **steam
on/off/setpoint only, deluge and spa permanently denied** — matching how the
production encoder already denies calibration and firmware commands.

---

## 10. Contradictions found

Recorded per [AGENT.md](../../AGENT.md) rule 5. Two of these are ours.

### 10.1 `powerclean_check.cgi` is a read, not a trigger — **ours**

[PROTOCOL.md](../../PROTOCOL.md), [DISCLAIMER.md](../../DISCLAIMER.md) and
[`app/server/cgi-safety.mjs`](../../app/server/cgi-safety.mjs) all say
`powerclean_check.cgi` is 3/5 and blocked because it is "documented as able to
_trigger_ the steam power-clean cycle, not merely report it".

That traces to one ambiguous upstream sentence — "Check or trigger the steam
generator power-clean cycle"
([cgi-endpoints.md](../../research/xagon0/docs/web-interface/cgi-endpoints.md)) —
and the controller's own shipped code contradicts it **[B]**:

- `settings.js` `powerclean_check_load()` GETs `powerclean_check.cgi`, reads
  `data.powerclean` as `0` or `1`, and opens or closes a modal. It then
  **re-schedules itself every 1000 ms**, starting at page load. Kohler polls this
  endpoint once per second from its own settings page.
- The actual trigger is `run_power_clean()`, whose comment reads "now send
  message to start power clean", and whose body is
  `update_value(const_steam_start_clean, 1)` — i.e.
  **`save_variable.cgi?index=60&value=1`**.

**[I]** An endpoint the vendor polls at 1 Hz from its own UI is not a cycle
trigger. The 3/5 rating appears to derive from the ambiguous upstream wording.

The consequence is in the other direction. `save_variable.cgi` is rated 2/5 and
exposed as a `command` in `cgi-safety.mjs`, noted "indices 1-105. Only index 43
(volume) is used by this app" — but no code in `app/server/` constrains the
index. The endpoint this repo blocks is a status read; the endpoint that starts
a 45-minute unattended power-clean cycle is reachable through the proxy.

Inert on this system, because steam is `not_seen`. It becomes live if a
generator is installed.

**Verified 2026-08-29** against the mirrored shipped code:
[`values.js:59`](../../research/controller-mirror/js/values.js) defines
`const_steam_start_clean = 60`;
[`settings.js:1275`](../../research/controller-mirror/js/settings.js) defines
`run_power_clean()` calling `update_value(const_steam_start_clean, 1)`;
`settings.js:1304` polls `powerclean_check.cgi` and re-schedules itself at
1000 ms. The safety-table correction is tracked separately from this note.

### 10.2 Index 60 is an action, not a startup setting — **ours**

[save-variable-reference.md](../../research/xagon0/docs/web-interface/save-variable-reference.md)
describes index 60 `steam_start_clean` as "Power-clean **on startup**, 0 = off,
1 = on" — a persisted boolean. The shipped controller UI uses it as an immediate
imperative: the "Start Power Clean" button calls it with `1` and then re-polls
status 500 ms later **[B]**. We follow the shipped code, per the same precedent
as the massage-mode conflict in [PROTOCOL.md](../../PROTOCOL.md).

### 10.3 Steam device ID `0x05` versus the assignable address range — **upstream**

[dtv-plus-protocol.md](../../research/xagon0/docs/protocols/dtv-plus-protocol.md)
states devices ship at `0x00` and the master assigns addresses from
**`0x03–0x07`** during discovery, and that steam's **device ID** is `0x05`. Its
own annotated capture then shows "Get Steam Generator Status" addressed to
`DEST = 0x05`, described as "steam" — conflating the device ID with a bus
address. A lone steam device on its own port would be assigned `0x03`, not
`0x05`.

**[I]** Anyone implementing from that page will hard-code the wrong DEST. Note
that the same document's discovery example is internally consistent (`DEV_REQUEST_ADDR`
carries `0x05` as the device _ID_, then `DEV_ASSIGN_ADDR` grants `0x03`) — it is
only the status example that is wrong.

### 10.4 Pre-heat: 10 minutes or 20?

`MAX_STEAM_PRE_HEAT_TIME` is given as **10 min** **[C]**
([timing-constants.md](../../research/xagon0/docs/control-logic/timing-constants.md),
[steam-generator.md](../../research/xagon0/docs/devices/steam-generator.md)).
Kohler's guide says that if the target cannot be reached, "the pop-up window
will disappear after **20 minutes**, and the steam countdown clock begins
counting down" **[K]** (p. 68). These may be different things — a firmware
pre-heat cap versus a UI warm-up dialog timeout — but they are not reconciled.
**[?]**

### 10.5 Steam tick: 150 ms or 500 ms?

`STEAM_TICK_TIME` is **150 ms** **[C]** and the system overview agrees. The DTV+
protocol document's own timing table gives a generic "Port Tick — 500 ms — main
polling interval per port". Unreconciled **[?]**. This matters: at 150 ms a steam
port carries 3.5× the transactions of a valve port, which is a real load
question given [FIELD-NOTES.md § 1](../../research/FIELD-NOTES.md) on this
system's sensitivity to polling.

### 10.6 Devices per port: 1, 2, or 5?

[timing-constants.md](../../research/xagon0/docs/control-logic/timing-constants.md)
says max **1** non-valve device per port; [system-specification.md § 4](../../docs/system-specification.md)
says up to **2** devices per port; [dtv-plus-protocol.md](../../research/xagon0/docs/protocols/dtv-plus-protocol.md)
says addresses `0x03–0x07` allow **5** per port. Unreconciled **[?]**. Irrelevant
for one steam generator on a dedicated converter, but it would matter for any
attempt to share a bus.

### 10.7 Retries: 4 or 5?

Steam recovery is documented as "retries the failed command up to **4** times"
**[C]**, while `DEVICE_MAX_RETRIES` is **5** and the DTV+ protocol page says
"retried up to **5** times". Minor, but it is exactly the kind of off-by-one that
changes when a fault latches. **[?]**

### 10.8 `MIN_STEAM_SETPOINT` is below the documented UI floor

Upstream gives a firmware minimum equivalent to 75 °F **[C]**; Kohler documents
the adjustable range as starting at 90 °F **[K]**. Both can be true — a firmware
floor under a UI floor — but the web CGI path has no minimum at all (§7), so
which one actually binds a `steam_on.cgi` call is **unknown**.

---

## 11. What we do not know

Ordered by how much it would block the work.

Kohler's generator and adapter guides answer what the kit contains, what
connects to what, where the adapter draws power, the generator's mains
requirements, what power clean does, and that low-water and high-limit
protection exist. What they do not answer:

| #   | Unknown                                                                                                                                                                                                                  | Why it blocks                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | **What the heating element does when the data link goes silent.** Kohler documents error `0408` and a UI reset; nothing says the generator stops                                                                         | The fail-closed claim, which is the whole basis of the valve design's safety case                                                    |
| 2   | **Which timer actually ends a session** — the generator's documented 20-minute auto-shutoff **[K]**, or `steamTimerSetTime` where `0` disables shutoff **[C]**                                                           | Decides whether a crashed master leaves a boiler running                                                                             |
| 3   | ~~Whether the in-enclosure-interface WARNING can be satisfied at all~~ — **closed 2026-08-29 by operator decision.** Accepted as a recorded deviation; see §6                                                            | No longer blocking                                                                                                                   |
| 4   | ~~**The protocol on the adapter↔controller link.**~~ **Closed 2026-08-29 [A]** — `IC2` is an `ADM4852` half-duplex RS-485 transceiver. A standard converter is the right part                                            | Closed                                                                                                                               |
| 5   | **The modular connector's pin assignment**, pin count, termination and idle bias on a DTV+ peripheral port                                                                                                               | Cannot build a lead                                                                                                                  |
| 6   | **Whether the DTV+ port sources device power.** The adapter is generator-powered, so probably not — but the wall interface _is_ controller-powered on a similar cable                                                    | Decides isolation and cabling                                                                                                        |
| 7   | **The real on-wire steam frames** — exact `SET_DEV_PARAM` payload shape, status field order and widths, discovery sequence                                                                                               | Cannot write an encoder from prose                                                                                                   |
| 8   | **Whether the generator speaks DTV+ natively or the adapter translates.** The adapter substitutes for the native keypad, which suggests the generator side is the _generator's_ protocol and the adapter bridges **[I]** | Decides whether the master talks to the adapter or through it                                                                        |
| 9   | **Whether K-1737-K1 is the right kit for whichever generator is bought.** Kohler maps current Invigoration generators to K-5548-K1, not K-1737-K1                                                                        | Buying the wrong generator wastes the kit                                                                                            |
| 10  | **Whether the 125 °F ceiling is enforced below the settings layer**                                                                                                                                                      | An installer field with no `min`/`max` is not a limit                                                                                |
| 11  | **How deluge is commanded on the wire**, and whether it is controller-side orchestration or a generator-initiated request                                                                                                | The cross-bus coupling in §9                                                                                                         |
| 12  | **Everything in §10 marked [?]**                                                                                                                                                                                         | Implementation details that decide whether a link works at all                                                                       |
| 13  | **Listing status.** No UL 499 / UL 1951 / ASME statement appeared in any fetchable Kohler PDF; they likely live on the rating label                                                                                      | The listing argument in [temperature-safety.md](../../docs/control-logic/temperature-safety.md) applies here with much higher stakes |

Items 1, 2, 7, 8 and 11 need a capture of a working steam installation. Items 5,
6 and 13 need the parts in hand. Items 3 and 9 need a conversation with Kohler.

---

## Cheapest next steps

Ordered cheapest first. Every one of these is read-only or off-hardware.

| #   | Step                                                                                                                                                                                                                                                                                                                                                                                                      | Cost                            | Resolves                                                                                                                                                                                                                                 |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **Photograph and meter an unused DTV+ peripheral port on the K-99695** — powered down, continuity only — and photograph the connector body and the K-99693's plug                                                                                                                                                                                                                                         | An hour, no purchase            | Unknowns 5 and 6. Also settles whether the modular-jack reading in §5 is right                                                                                                                                                           |
| 2   | **Ask Kohler.** Support case **#07797183** is already open with Kohler engineering. Add four questions: is the K-1737-K1 link RS-485 and what is its pinout; what does the generator do when the data link goes silent; is the 20-minute shutoff in the generator or the controller; and can the in-enclosure-interface requirement be met other than with a K-99693                                      | Free; one email                 | Unknowns 1, 2, 3, 4 — the four that reading cannot close                                                                                                                                                                                 |
| 3   | **Confirm generator/kit pairing before buying anything.** Kohler maps current Invigoration generators to K-5548-K1, not K-1737-K1                                                                                                                                                                                                                                                                         | Free                            | Unknown 9, before money is spent                                                                                                                                                                                                         |
| 4   | **Retrieve the two Kohler Assist wiring diagrams.** They are the documents most likely to show terminals on both the adapter and the generator, and the research could not fetch them — they sit behind a Salesforce login at [assist.kohler.com](https://assist.kohler.com/en/valves-shower-bath/DTV-and-Steam-Generator-Wiring-Diagrams). A Kohler account, or asking in case #07797183, may reach them | Free                            | Unknowns 4 and 5, from a primary source                                                                                                                                                                                                  |
| 5   | **Read US 9,777,470 B2 FIG. 14–15 and their description**, the steam figures of the DTV+ system patent                                                                                                                                                                                                                                                                                                    | Free                            | May document the steam subsystem's architecture — the only first-party architecture text that exists ([patents.md](../../docs/patents.md))                                                                                               |
| 6   | **Reconcile §10.1 in the repo's safety table.** Decide whether `powerclean_check.cgi` should be re-rated as a read, and whether `save_variable.cgi` should be index-restricted                                                                                                                                                                                                                            | Half a day of code              | The live gap between what is blocked and what is reachable                                                                                                                                                                               |
| 7   | **Capture the wall-interface link at controller boot** with the physically receive-only front end the valve design already specifies. The K-99693 is the only DTV+ speaker in this house, and it is connected and healthy. Capture at 9600 and 115200; expect a baud change                                                                                                                               | Existing hardware; no new parts | If discovery really is DTV+, this validates framing, stuffing, checksum and the 3-step handshake against real Kohler traffic — the parts steam shares with every other DTV+ device. Also settles §10.5 and §10.6 for at least one device |
| 8   | **Write a DTV+ codec against the vendored spec, with fixtures, offline** — framing, stuffing, checksum, discovery, the steam status/param payloads, plus a steam emulator                                                                                                                                                                                                                                 | Days; no hardware               | Turns unknown 7 from "unwritten" into "written but unverified", and produces the decoder needed to read any future capture                                                                                                               |
| 9   | **Find someone with a working DTV+ steam installation** and ask for a receive-only capture, via the Tier 1 repos in [SOURCES.md](../../research/SOURCES.md) or the Homey/C4 threads. Odds are poor: no third-party work on this bus was found — see the GitHub code search result below                                                                                                                   | Free; low odds                  | Unknowns 1, 2, 7, 11 — the only route that does not require buying a generator                                                                                                                                                           |

**Not recommended:** buying a steam generator purely to reverse engineer it.
That inverts the project's economics.

**Also not recommended:** treating steps 1–9 as a path to shipping steam. Even
with all of them done, unknown 3 — Kohler's in-enclosure interface requirement —
is a decision for the operator and an electrician, not something this project
can engineer around.

---

## Sources

| Source                                                                                                                                                                                                                                                   | What it gave this document                                                                                                                                                               | Tier    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| [`2026-08-22-idle-baseline/`](../../research/diagnostics/2026-08-22-idle-baseline/)                                                                                                                                                                      | Every steam field this controller reports, and proof steam is `not_seen`                                                                                                                 | **[A]** |
| [`research/controller-mirror/`](../../research/controller-mirror/) — `js/control.js`, `js/settings.js`, `js/values.js`, `control.html`, `settings.html`                                                                                                  | The 1–20 minute duration clamp, the max-run-time options, the unbounded max-temp field, the power-clean trigger path, the spa/steam interlock                                            | **[B]** |
| Kohler User Guide **1241234-5-D**, [`research/reference/guide-text.txt`](../../research/reference/guide-text.txt) — pp. 38, 67–70                                                                                                                        | The 90–125 °F range, the 20-minute session cap, the 600-minute / 45-minute power-clean cycle and its "stay out" warnings, deluge                                                         | **[K]** |
| Kohler **K-99693-P** spec sheet, [`research/reference/K-99693-P_spec.pdf`](../../research/reference/K-99693-P_spec.pdf)                                                                                                                                  | 125 °F ambient rating; "do not install above the steamhead of a steam unit"; the 25 ft RJ45 cable + coupler                                                                              | **[K]** |
| Kohler **1235393-2-C**, Installation and Care Guide — Steam Adapter Kit / Steam Head for DTV+ ([PDF](https://resources.kohler.com/onlinecatalog/pdf/1235393_2.pdf))                                                                                      | The kit contents, topology, adhesive mounting, the three status LEDs, the terminator, generator-sourced power, the telephone-style cable, the in-enclosure interface WARNING, error 0408 | **[K]** |
| Kohler **1230487-2-E**, Installation and Care Guide — Steam Generator ([PDF](https://resources.kohler.com/webassets/kpna/catalog/pdf/en/1230487_2.pdf))                                                                                                  | Errors 0120 / 0140-A / 0140-B, automatic fill shutoff, pressure relief valve, power clean flushing through the steam head                                                                | **[K]** |
| Kohler **1230489-2-C** ([PDF](https://resources.kohler.com/onlinecatalog/pdf/1230489_2.pdf)) and **1045789-5-C** ([PDF](https://techcomm.kohler.com/techcomm/pdf/1045789-5.pdf)), Steam Control Kit guides                                               | The generator's own 90–125 °F / 10–20 minute limits, 113 °F × 15 min factory preset, the 600-minute reminder, three-session grace then lockout, the 45-minute cycle and its WARNING      | **[K]** |
| Kohler **Steam Specification Guide**, form 22-3187-0822 ([PDF](https://resources.kohler.com/webassets/kpna/brochures/KOHLER_SteamSpecGuide.pdf))                                                                                                         | Generator-to-control-kit mapping, "Auto Shutoff… after 20 minutes"                                                                                                                       | **[K]** |
| Kohler **1069333-1-C** (DTV II roughing-in, [PDF](https://resources.kohler.com/onlinecatalog/pdf/1069333_1.pdf)) and **1581267-2-B** (Digital Steam Adapter service instruction, 2025-09, [PDF](https://techcomm.kohler.com/techcomm/pdf/1581267-2.pdf)) | K-1737's generator compatibility list; the current harness naming and the DTV+/Anthem+ dual role                                                                                         | **[K]** |
| [xagon0 `steam-generator.md`](../../research/xagon0/docs/devices/steam-generator.md)                                                                                                                                                                     | Device ID `0x05`, operating states, Fx2, status/param payload shape, error bits, status codes, power-clean state `0xCC`                                                                  | **[C]** |
| [xagon0 `dtv-plus-protocol.md`](../../research/xagon0/docs/protocols/dtv-plus-protocol.md)                                                                                                                                                               | Framing, stuffing, checksum, discovery, addressing, command set, timing                                                                                                                  | **[C]** |
| [xagon0 `timing-constants.md`](../../research/xagon0/docs/control-logic/timing-constants.md)                                                                                                                                                             | 150 ms steam tick, retry counts, pre-heat cap, port limits                                                                                                                               | **[C]** |
| [`docs/hardware.md`](../../docs/hardware.md), [`system-specification.md`](../../docs/system-specification.md)                                                                                                                                            | DTV+ port electrical spec, RS-485 pinout, per-peripheral power                                                                                                                           | **[C]** |
| [`docs/devices/valve-control.md`](../../docs/devices/valve-control.md)                                                                                                                                                                                   | The valve safety-ownership table this document compares against                                                                                                                          | **[C]** |

Kohler documents and supports none of the protocol material. The vendored
xagon0 tree publishes **no license** — see
[PROVENANCE.md](../../research/xagon0/PROVENANCE.md); it is reference-only.

**Not obtainable**, recorded so nobody repeats the search:

- Any spec or roughing-in sheet numbered for **K-1737-K1**. The conventional
  `resources.kohler.com/webassets/kpna/catalog/pdf/en/K-1737-K1_spec_*.pdf` and
  `K-1737_spec_*` paths both return **404**. Only the DTV II sheet exists.
- The **kohler.com / us.kohler.com product pages** — two fetches timed out at
  60 s, and a direct request returned **HTTP 403**. The marketing copy quoted in
  §4 is search-index-sourced, not verified against the live page.
- The two **Kohler Assist wiring diagrams** — hosted on
  `globalkohler.my.salesforce.com`, both returned a Salesforce login page rather
  than a PDF.
- Adapter dimensions, weight, enclosure, voltage, current, connector type, pin
  count; any statement of the wire protocol; any UL 499 / UL 1951 / ASME listing
  statement; any IAPMO/UPC or ICC code text mandating steam-room timers or a
  maximum temperature. The 20-minute and 125 °F figures are **Kohler's own
  product limits**, and no primary code source was found tying them to a mandate.
- Any **third-party reverse engineering of this bus at all.** A GitHub code
  search for `steam_on.cgi`, `SteamOperationState`, `DT_W_Steam` and
  `powerclean_check.cgi` matched exactly one repository: this project's own.
- No Kohler product called a **"Steam Management Module" / SMM** appears in any
  document found, despite DTV+ device ID `0x06` being labelled that way upstream
  **[C]**. **[?]**
