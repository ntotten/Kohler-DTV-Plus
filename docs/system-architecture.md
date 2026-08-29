# System architecture

The DTV+ is a distributed embedded system: one controller, up to three wall
interfaces, up to four valves, and a family of single-purpose peripherals —
all on serial buses, with Ethernet only at the controller.

## Components

| Part                    | What it is                                                      | Notes                                                                                                              |
| ----------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| **K-99695(-NA)**        | System controller — the networked "brain"                       | Runs everything; the only Ethernet port. Also an **ECO** variant (K-99695-E, CALGreen) with its own firmware line. |
| **K-99693-P / K-99693** | Wall interface (touchscreen)                                    | Two hardware generations — see [wall-interface.md](devices/wall-interface.md). Up to 3 per system. Bracket: K-99694.       |
| **DTV 6-port valve**    | Thermostatic mixing valve, 6 outlets                            | Firmware type `0x06`.                                                                                              |
| **Prompt 2-port valve** | Thermostatic mixing valve                                       | Firmware type `0x17`.                                                                                              |
| **Prompt 3-port valve** | Thermostatic mixing valve, optional flow control                | Firmware type `0x1E` (flow-control variant `0xFF`).                                                                |
| K-99696                 | Bluetooth amplifier module                                      | DTV+ bus device; answers to two IDs (`0x40` or `0x07`) — a real quirk.                                             |
| K-97999                 | Konnect Wi-Fi module                                            | Optional cloud bridge; ARM/Linux-class board per [FCC research](fcc-filings.md).                       |
| —                       | Steam generator, rain panel (RGB), LightBridge lighting dimmers | DTV+ bus devices `0x05`, `0x03`, `0x08`.                                                                           |

The controller powers the wall interface(s) and talks to everything else over
RS-485. Peripherals have their own power.

## Topology

```
                        +----------------------+
                        |  K-99695 CONTROLLER  |
                        |  ColdFire / MQX 3.8  |
                        |                      |
                        |  HTTP (Ethernet)     |<-- LAN: unauthenticated CGI API
                        |  8x RS-485 (DTV+)    |
                        |  2x RS-485 (Saturn)  |     +-- valve UART A
                        |  1x UART (UI link)   |     +-- valve UART B
                        +---+------+--------+--+
                            |      |        |
              DTV+ bus -----+      |        +----- Saturn buses (valves only)
              9600 8N1             |               9600 8N1
              (steam, rain,        |
               lights, amp,        +----- UI link 115200 8N1
               UI discovery)             (Amulet CRC after discovery)
```

## The three-protocol trap

The single most confusing property of the system: **three protocols, two of
them at the same baud rate on the same kind of wire.**

| Protocol       | Speaks to                                               | Baud   | Origin                                        |
| -------------- | ------------------------------------------------------- | ------ | --------------------------------------------- |
| **DTV+**       | Steam, rain panel, LightBridge, amplifier, UI discovery | 9600   | Native to this product                        |
| **Saturn**     | All valves                                              | 9600   | Older — inherited from Mira valve controllers |
| **Amulet CRC** | Touchscreen only, after discovery                       | 115200 | Datatable sync + RPC                          |

Sending a DTV+ frame to a valve (or vice versa) yields no response or
garbage. If you tap a bus, identify the protocol before interpreting bytes.

Summaries of all three live in [protocols.md](protocols/overview.md); the full
frame-level documentation is in the upstream projects (see
[../reference/links.md](../research/reference-links.md)).

## Two independent error surfaces

Valve errors and controller errors live in **different places with different
retention** — valve faults are current-state flags that leave no history;
the controller keeps a persistent, timestamped log. Confusing the two
produces wrong diagnoses. Full treatment:
[errors-and-known-issues.md](troubleshooting/errors-and-known-issues.md).

## Where the intelligence is

- The **valves** run their own temperature regulation (mixing loop,
  thermistor, safety trips). The controller sends setpoints and reads
  results — it does not run a PID loop. This layering is what makes both
  repair and third-party controllers feasible; see
  [temperature-safety.md](control-logic/temperature-safety.md).
- The **controller** owns configuration, presets, scheduling of peripherals,
  the web UI, the update pipeline, and all bus mastering.
- The **wall interface** is a display/peer device: it synchronizes a shared
  _datatable_ with the controller and sends user actions back as RPCs — but
  it can start/stop the shower entirely on its own over the bus.
