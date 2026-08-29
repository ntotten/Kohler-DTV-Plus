# Kohler DTV+ Hardware Specification

## Overview

The Kohler DTV+ system is a digital shower controller built around a Freescale ColdFire V4 microcontroller. It communicates with peripheral devices (steam generators, rain panels, light bridges, amplifiers, valve assemblies) over RS485 buses and provides a touchscreen UI interface. This document captures the hardware architecture as determined through reverse engineering.

---

## Main Processor

| Parameter | Value |
|---|---|
| CPU | Freescale MCF5441X ColdFire V4 |
| Architecture | 32-bit CISC (Motorola 68K lineage) |
| Clock Speed | ~250 MHz |
| Instruction Set | ColdFire ISA_A+ with hardware divide, MAC |
| Package | BGA |
| Core Count | 1 (single core) |

The MCF5441X integrates 8 UARTs, Ethernet MAC, FlexBus external bus, GPIO, timers, and DMA -- making it well-suited for a multi-port RS485 controller.

---

## Memory

| Type | Size | Notes |
|---|---|---|
| Internal SRAM | 64 KB | On-chip, zero-wait-state |
| External SRAM | 16 MB | Connected via FlexBus |
| NAND Flash | See below | HCC SafeFlash filesystem |

### NAND Flash Detail

The firmware references 512 MB NAND flash capacity, but physical inspection and repair documentation indicate a **Micron** part:

| Parameter | 16-bit Variant | 8-bit Variant |
|---|---|---|
| Part Number | MT29F2G16AABWP | MT29F2G08 |
| Chip ID | 0x2CCA | 0x2CDA |
| Capacity | 256 MB (2 Gbit) | 256 MB (2 Gbit) |
| Page Size | 2048 bytes + 64 bytes OOB | 2048 bytes + 64 bytes OOB |
| Block Size | 128 KB (64 pages per block) | 128 KB (64 pages per block) |
| Total Blocks | 2048 | 2048 |
| Bus Width | 16-bit | 8-bit |

> **Note:** The discrepancy between 512 MB (firmware references) and 256 MB (physical chip) may be due to firmware supporting multiple flash variants, or the 512 MB figure including raw capacity before ECC overhead.

### NAND Block Layout

```
Block 0    +-----------------------+
           |  Bootloader           |
           |  (primary + backup)   |
Block ~50  +-----------------------+
           |  Configuration data   |
           |  (device settings,    |
           |   calibration, keys)  |
Block ~200 +-----------------------+
           |  Reserved / spare     |
           |                       |
Block 499  +-----------------------+
Block 500  |  HCC SafeFAT          |
           |  Filesystem           |
           |  (/firmware, /config, |
           |   /web, /logs)        |
           |                       |
Block 2047 +-----------------------+
```

| Region | Blocks | Purpose |
|---|---|---|
| Reserved | 0 - 499 | Bootloader, configuration, spare blocks |
| Filesystem | 500 - 2047 | HCC SafeFAT managed filesystem |

---

## RTOS

| Parameter | Value |
|---|---|
| RTOS | MQX 3.8 |
| Vendor | Freescale / NXP |
| Kernel Features | Preemptive multitasking, message queues, semaphores, events, lightweight timers |
| TCP/IP Stack | RTCS (Real-Time Communication Suite) |
| File System | HCC SafeFlash + SafeFAT |
| Shell | MQX Shell (debug builds) |

MQX 3.8 is tightly integrated with the MCF5441X BSP (Board Support Package), providing drivers for all on-chip peripherals.

---

## RS485 Interfaces

The controller has two distinct RS485 bus systems:

### DTV+ Ports (8 ports)

| Parameter | Value |
|---|---|
| Count | 8 independent ports |
| Physical Layer | RS485 half-duplex |
| Baud Rate | 9600 |
| UART Mapping | Internal MCU UARTs 1-8 |
| Protocol | DTV+ protocol |
| Device Discovery | Auto-discovery with address assignment |
| Direction Control | GPIO-driven DE/RE pins per port |

Ports 1-8 map directly to the MCF5441X's on-chip UART peripherals. Each port has dedicated RS485 transceiver hardware with GPIO-controlled direction (transmit enable / receive enable).

### Valve Ports (2 ports)

| Parameter | Value |
|---|---|
| Count | 2 independent ports |
| Physical Layer | RS485 half-duplex |
| Baud Rate | 9600 |
| UART Hardware | External TL16C752C dual UART |
| Bus Interface | FlexBus (memory-mapped I/O) |
| Base Addresses | Channel A: 0xC0000000, Channel B: 0xC0010000 |
| Protocol | Saturn protocol |

The TL16C752C is a Texas Instruments dual UART with 64-byte FIFOs, accessed as a memory-mapped peripheral through the ColdFire FlexBus interface.

```
MCF5441X FlexBus
       |
       +--- CS (Chip Select) at 0xC0000000
       |
  +----+----+
  | TL16C752C |
  | Dual UART |
  +----+----+
  | Ch A | Ch B |
  +------+------+
  Valve1  Valve2
```

---

## Network

### Ethernet

| Parameter | Value |
|---|---|
| Speed | 10/100 Mbps |
| MAC | On-chip MCF5441X FEC |
| PHY | External (board-specific) |
| Connector | Standard RJ45 |
| TCP/IP Stack | RTCS (MQX Real-Time Communication Suite) |
| Protocols | HTTP server, DHCP client, DNS, TCP/UDP |

### WiFi (Optional)

| Parameter | Value |
|---|---|
| Interface | SDIO or UART |
| Security | WPA2 |
| Usage | Alternative to Ethernet for configuration/control |

---

## UI Connection

| Parameter | Value |
|---|---|
| Interface | UART |
| Baud Rate | 115200 |
| Format | 8N1, no flow control |
| Protocol | Amulet CRC protocol (Modbus-like) |
| Touch Controller | Amulet display module |
| UI Processor | MCF52252 ColdFire (Amulet variant) |
| RX Timeout | 150 ms |

The UI connection uses a datatable-based memory model where the controller writes display variables and the touchscreen sends RPCs (remote procedure calls) back for user interactions.

---

## GPIO

| Function | Direction | Notes |
|---|---|---|
| RS485 DE/RE (x10) | Output | Direction control for each RS485 port (8 DTV+ + 2 valve) |
| Relay Output 1 | Output | Dry-contact relay |
| Relay Output 2 | Output | Dry-contact relay |
| Dry Contact Input 1 | Input | External trigger / sensor |
| Dry Contact Input 2 | Input | External trigger / sensor |

---

## Power

| Parameter | Value |
|---|---|
| Input Voltage | 24V AC or DC (typical) |
| Power Consumption | ~5-10W (controller only) |
| Device Power | Each peripheral device is powered separately |
| Regulation | On-board regulators for 3.3V, 5V logic rails |

---

## Connector Pinouts

### RS485 (DTV+ and Valve Ports)

| Pin | Signal | Description |
|---|---|---|
| 1 | A+ | Non-inverting (Data+) |
| 2 | B- | Inverting (Data-) |
| 3 | GND | Signal ground |

### Ethernet

Standard RJ45 connector, 10/100 Mbps, T568B pinout.

### UI Connector

| Pin | Signal | Description |
|---|---|---|
| 1 | TX | Controller transmit (to UI RX) |
| 2 | RX | Controller receive (from UI TX) |
| 3 | GND | Signal ground |
| 4 | VCC | Power supply to UI module |

---

## Flash Filesystem

### HCC SafeFlash

| Feature | Description |
|---|---|
| Filesystem | HCC SafeFAT over SafeFlash |
| Wear Leveling | Dynamic wear leveling across filesystem blocks |
| Error Correction | ECC per page (uses OOB area) |
| Power-Loss Safety | Journaled metadata for crash recovery |
| Block Range | Blocks 500-2047 (filesystem partition) |

### File System Layout

```
/
+-- firmware/          Firmware images (.S19, .rbin)
+-- config/            Device configuration files
|   +-- network.cfg    IP settings, DHCP/static
|   +-- devices.cfg    Paired device list
|   +-- presets.cfg    User presets / scenes
+-- web/               Embedded web server content
|   +-- index.html
|   +-- css/
|   +-- js/
+-- logs/              Diagnostic and error logs
```

---

## Development Tools

| Tool | Version | Purpose |
|---|---|---|
| CodeWarrior | 10.2 | IDE for ColdFire development |
| ColdFire C/C++ Compiler | v10.2 | Cross-compiler targeting MCF5441X |
| P&E Micro BDM Debugger | - | JTAG/BDM debug probe for ColdFire |
| MQX | 3.8 | RTOS and BSP |
| HCC Embedded | - | SafeFlash / SafeFAT filesystem libraries |

### Build Artifacts

| Extension | Format | Description |
|---|---|---|
| .S19 | Motorola S-Record | ASCII hex format, used for flash programming |
| .elf | ELF (Executable and Linkable Format) | Debug-enabled binary with symbols |
| .rbin | Raw Binary | Stripped binary image for direct flash writes |

---

## Factory Debug and Service Access

The board photograph in [Images/](../research/xagon0/Images/Images.md) shows several unpopulated factory debug/service footprints. Identification from board photography; pinouts unverified — confirm with a multimeter before attaching anything.

| Marker | Footprint | Assessment |
|---|---|---|
| J201 | 2x13 (26-pin) | **ColdFire BDM debug** — matches the standard Freescale 26-pin ColdFire BDM layout. A P&E Micro Multilink/Cyclone (or TBLCF) here can halt the CPU and dump RAM/flash. |
| J904 | 4-pin | **Serial console** — consistent with a 3.3 V UART (start at 115200 8N1). If the MQX shell or boot logging was left enabled, this is interactive access. |
| J903 | 2x8 | Adjacent debug/expansion header — uncharacterized |
| SW201 | pushbutton | Reset / boot-mode |
| SW101 | slide switch | Boot-mode / service selection — uncharacterized |

The board also has a coin cell (RTC backup) and extensive labelled test points (TP###).

> **Why this matters:** these footprints are the practical firmware-extraction surface. See [repair/firmware-extraction.md](repair/firmware-extraction.md).

---

## Touchscreen Variants

### Amulet (ColdFire-based)

| Parameter | Value |
|---|---|
| Processor | MCF52252 ColdFire |
| Architecture | ColdFire V2 (32-bit) |
| Firmware Format | `ui_amulet_v*.S19` |
| Communication | UART 115200, Amulet CRC protocol |
| Display | Capacitive touch LCD |

### Linux (ARM-based)

| Parameter | Value |
|---|---|
| Processor | ARM-based SoC |
| OS | Embedded Linux |
| Firmware Format | `dtvplus2_uiapp_v*.pack.tar` |
| Communication | Same UART/protocol interface to controller |
| Display | Higher resolution capacitive touch LCD |

---

## Replacement Hardware Recommendations

For building an open-source replacement controller, the following modern components are recommended:

### MCU Options

| Component | Option 1 | Option 2 | Option 3 |
|---|---|---|---|
| MCU | STM32H7 | i.MX RT1060 | ESP32-S3 |
| Core | Cortex-M7 @ 480MHz | Cortex-M7 @ 600MHz | Xtensa LX7 dual @ 240MHz |
| RAM | 1 MB internal | 1 MB internal | 512 KB internal |
| Flash | 2 MB internal | External (FlexSPI) | 16 MB external (typical) |
| UARTs | Up to 8 | Up to 8 | 3 (needs expansion) |
| Ethernet | Yes (some variants) | Yes (100M + optional PHY) | No (WiFi only) |
| Notes | Best balance | Highest performance | WiFi built-in, fewer UARTs |

### External UART Options

| Component | Channels | Interface | Notes |
|---|---|---|---|
| SC16IS752 | 2 | SPI or I2C | Drop-in for TL16C752C replacement |
| MAX3107 | 1 | SPI or I2C | Single channel, very compact |

### RS485 Transceiver Options

| Component | Features | Notes |
|---|---|---|
| MAX485 | Basic, half-duplex | Classic choice, needs DE/RE GPIO |
| SP485 | Basic, half-duplex | Pin-compatible with MAX485 |
| MAX13487 | Auto-direction | No DE/RE GPIO needed |
| SN65HVD75 | 3.3V, auto-direction | Low power, modern choice |

### Software Stack Replacements

| Original | Replacement | Notes |
|---|---|---|
| MQX RTOS | FreeRTOS | Open-source, widely supported |
| RTCS (TCP/IP) | lwIP or FreeRTOS+TCP | Open-source TCP/IP stacks |
| HCC SafeFlash | LittleFS or FatFS | Open-source flash filesystems |
| CodeWarrior | STM32CubeIDE / PlatformIO | Free IDE options |

### Minimum Specifications for Replacement

| Parameter | Minimum | Recommended |
|---|---|---|
| CPU | ARM Cortex-M4 @ 120 MHz | ARM Cortex-M7 @ 400+ MHz |
| RAM | 128 KB | 256 KB |
| Flash | 1 MB | 2 MB |
| UARTs | 11 total (2 valve + 8 DTV+ + 1 UI) | 11+ with DMA support |
| Ethernet | 10/100 Mbps | 10/100 Mbps with hardware MAC |
| GPIO | 12+ (RS485 DE/RE, relays, inputs) | 20+ for expansion |
| SPI/I2C | At least 1 bus for UART expanders | Multiple buses |

> **Note on UART count:** 11 UARTs is a hard requirement. Most MCUs top out at 6-8, so external UART expanders (SC16IS752 or similar) are typically needed for the valve ports or some DTV+ ports.
