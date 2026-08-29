# K-99695 controller hardware

The system controller (PCB marking **PCB-0424-00-R04**) is a 2012-era
embedded board built around a Freescale ColdFire. Everything below is from
upstream board analysis and public documentation, confirmed against a
production unit where noted.

## Core

|              |                                                                                                                                           |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| CPU          | Freescale **MCF54416** ColdFire V4 (marking `MCF54416CMJ256`), ~250 MHz, single core, BGA                                                 |
| Architecture | 32-bit CISC, Motorola 68K lineage, ISA_A+ with hardware divide and MAC                                                                    |
| Internal     | 64 KB zero-wait-state SRAM; 256 KB internal flash (bootloader)                                                                            |
| External RAM | 16 MB SRAM over FlexBus — the application **runs from RAM**, loaded at addresses ≥ `0x40500000`                                           |
| NAND flash   | Micron **MT29F2G16AABWP** (16-bit, chip ID `0x2CCA`) or **MT29F2G08** (8-bit, `0x2CDA`) — 2 Gbit / 256 MB, 2048+64 B pages, 128 KB blocks |

The MCF5441X integrates 8 UARTs, an Ethernet MAC, FlexBus, GPIO, timers and
DMA — most of what a ten-port RS-485 controller needs on one die.

> Upstream notes a mismatch: firmware references a 512 MB NAND while the
> physical part is 256 MB. Unexplained (multi-variant support or
> raw-vs-ECC capacity).

## NAND layout

| Region     | Blocks   | Contents                                                                                                       |
| ---------- | -------- | -------------------------------------------------------------------------------------------------------------- |
| Reserved   | 0–499    | Bootloader (primary + backup) around block 0; configuration/calibration/keys around block 50; spare thereafter |
| Filesystem | 500–2047 | HCC SafeFAT — `/images` (firmware staging), `/backup`, config tables, logs                                     |

## Buses and ports

| Interface      | Electrical                              | Notes                                                                                                                                                                                                |
| -------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DTV+ ports ×8  | RS-485 half-duplex, 9600 8N1            | MCU's own UARTs; GPIO-driven DE/RE per port; point-to-point per port (up to 2 devices); 2048 B TX/RX buffers                                                                                         |
| Valve ports ×2 | RS-485 half-duplex, 9600                | External **TI TL16C752C** dual UART (64 B FIFOs), FlexBus-mapped at `0xC0000000` / `0xC0010000`; behind a semaphore with a **750 ms timeout** — long holds fail subsequent operations until released |
| UI link        | UART **115200** 8N1                     | 150 ms RX timeout; DTV+ discovery first, then Amulet CRC                                                                                                                                             |
| Ethernet       | On-chip FEC, 10/100, external PHY, RJ45 | The only network interface                                                                                                                                                                           |

RS-485 connector pinout (all ports): pin 1 = A+ (non-inverting), pin 2 = B−
(inverting), pin 3 = signal ground.

## Software stack

|            |                                                                                             |
| ---------- | ------------------------------------------------------------------------------------------- |
| RTOS       | Freescale **MQX 3.8** (preemptive; message queues, semaphores, lightweight timers)          |
| TCP/IP     | RTCS                                                                                        |
| Filesystem | HCC SafeFlash + SafeFAT (dynamic wear levelling, per-page ECC, journalled metadata)         |
| Web server | MQX HTTP server (see [api-reference.md](web-interface/api-reference.md) for behavior)                     |
| Toolchain  | CodeWarrior 10.2 / ColdFire C/C++ 10.2 / P&E Micro BDM; artifacts `.S19` / `.elf` / `.rbin` |

MQX runs 15+ tasks; the notable ones: `MAIN_TASK` (priority 17);
`SHOWER_TASK` / `STEAM_TASK` / `RAIN_PANEL_TASK` / `LIGHT_BRIDGE_TASK` /
`RELAY_TASK` (15); `VALVE_TASK` / `VALVE2_TASK` and the eight
`DTV_PLUS_PORT` handlers (14); `UI_TASK` (13). Ports 5–8 double as
simulation ports (steam, rain, LightBridge, amplifier) when no physical
device is attached.

## Power and environmental

- 24 V AC/DC in from an external brick (72″ cord), roughly 5–10 W for the
  controller alone; on-board regulators produce 3.3 V / 5 V.
- Mains side: dedicated circuit protected by a Class A GFCI; UL 1951 /
  CSA C22.2 listed.
- Ambient limit **40 °C / 104 °F**; mount **higher than the digital valve**
  (Kohler's own caution, to prevent water damage).
- Enclosure 9-1/2″ × 5-3/8″ × 1-3/4″; shelf (rubber feet) or wall mount;
  fits a 2×4 stud cavity with an access panel.

## LED patterns (no network needed)

| Pattern         | Timing                 | Meaning                                           |
| --------------- | ---------------------- | ------------------------------------------------- |
| Double blink    | 200 ms on/off, 1 s off | General error — read `cerror_logs.cgi`            |
| Slow blink      | 2 s on, 4 s off        | Firmware failed CRC32; controller will not run it |
| Very slow blink | 2 s on, 8 s off        | NAND failure (critical)                           |
| Solid           | continuous             | Normal / booting                                  |
| None            | —                      | No power to the board                             |

## Factory debug access (important for repair)

The board photograph published upstream
([xagon0 repo, Images](https://github.com/xagon0/Kohler-DTV-Plus)) shows
unpopulated factory debug footprints:

| Marker    | Footprint     | Almost certainly                                                                                                                                                                            |
| --------- | ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **J201**  | 2×13 (26-pin) | **ColdFire BDM** — the standard Freescale 26-pin debug layout. A P&E Micro Multilink/Cyclone (or TBLCF) here can halt the CPU and dump RAM/flash — the definitive firmware-extraction path. |
| **J904**  | 4-pin         | **Serial console** (start at 115200 8N1). If the MQX shell or boot logging is enabled, this is interactive filesystem access with a $10 cable.                                              |
| **J903**  | 2×8           | Adjacent debug/expansion header — uncharacterized                                                                                                                                           |
| **SW201** | pushbutton    | Reset / boot-mode                                                                                                                                                                           |
| **SW101** | slide switch  | Boot-mode / service selection — uncharacterized                                                                                                                                             |

There is also a coin-cell (RTC) and extensive labelled test points.
[research/firmware-extraction.md](repair/firmware-extraction-notes.md) uses
these as the primary extraction paths.

## Boot overview

```
Power on → internal-flash bootloader → NAND reset + chip-ID check
  (mismatch = infinite hang, no bypass)
→ mount SafeFAT (formats if blank)
→ find a:/images/dtvplus2_app_v*.*.*.*.S19
→ validate: per-line checksums + CRC32 from the S0 header + all S3
  addresses ≥ 0x40500000
→ bad CRC = slow-blink LED, no boot
→ load to RAM, jump
```

No valid image on NAND → the bootloader falls back to its built-in TFS
recovery app, which serves a minimal firmware-upload web page. Full detail:
[firmware-and-updates.md](firmware-and-updates.md).
