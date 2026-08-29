# Protocols: Saturn, DTV+, and Amulet CRC

Three serial protocols share the DTV+ wiring. This page is the orientation
map; for byte-level detail, see the upstream protocol documents linked from
[../reference/links.md](../../research/reference-links.md).

> **Verify against your own bus before building on this.** The frame formats
> below are consistent across two independent upstream analyses, but the
> Saturn command set in particular is worth confirming with a passive RS-485
> capture against your valve generation before you write a master.

## DTV+ — peripherals and UI discovery (9600 8N1)

Native protocol for everything except valves: steam (`0x05`), rain panel
(`0x03`), LightBridge (`0x08`), amplifier (`0x40`, sometimes `0x07`), UI
discovery (`0x30`/`0x31`), bootloader (`0xF0`), test fixture (`0x09`).

```
SOF(0x88) DEST SRC CMD [payload] CHECKSUM EOF(0x55)
```

- 2's-complement checksum over the frame body.
- `0x88`, `0x55`, `0xAA` are reserved inside the frame and byte-stuffed by
  prefixing `0xAA`.
- The controller is always the master (`0x00`); peripherals are polled at
  50–350 ms depending on device.

## Saturn — the valves (9600 8N1)

Older protocol inherited from Mira valve controllers. Speaks only to valves,
on the two dedicated valve UARTs (not the DTV+ ports).

```
SYNC1(0xAA) SYNC2(0x55) ADDRESS CONTROL DATA_LEN [DATA] CHECKSUM
```

- Checksum: 2's complement of (ADDRESS + CONTROL + DATA_LEN + DATA),
  i.e. the full frame sums to `0x00` mod 256.
- Max packet ~20 bytes; valve tick 525 ms; response timeout ~320 ms; a
  handful of retries per command.
- **Master address depends on valve family** — get it wrong and the valve
  silently ignores you:

  | Valve                             | Master address |
  | --------------------------------- | -------------- |
  | DTV 6-port                        | `0x00`         |
  | Prompt 2-port, not networked      | `0x00`         |
  | Prompt 2-port / 3-port, networked | `0x10`         |

- Valve firmware type IDs reported at discovery: `0x06` DTV 6-port, `0x17`
  Prompt 2-port, `0x1E` Prompt 3-port, `0xFF` Prompt 3 with flow control.
- Command families: address management (`0x3A` subcommands: clear / enquire /
  allocate), reads (firmware version `0x01`, type `0x02`, outlets `0x07`,
  temperature `0x0B`, flow `0x0C`, faults `0x0F`, calibration `0x10`, serial
  `0x11`, config `0x15`, extended status `0x40`, diagnostics `0x54`), writes
  (outlets `0x87`, target temperature `0x8B`, target flow `0x8C`, config
  `0x95`, pause `0x99`, calibration `0xC0`), and system commands (factory
  reset `0xF4`, bootloader `0xF6`, flow-sensor calibrate `0xF7`).
- Responses echo the request control byte; `0x80` = error with code;
  `0xFF` = NAK.
- Outlet bitmaps differ by family: DTV 6-port uses bits 0–5; Prompt 3 uses
  `0x04 0x08 0x10 0x20 0x40 0x80` for outlets 1–6.
- **Prompt 3 30-minute runtime timer** (`1800 s`): the valve shuts off when
  it expires. The reset is only accepted once ≥ 900 s have elapsed — naive
  constant polling does **not** hold it off; a replacement master must manage
  it deliberately (see [temperature-safety.md](../control-logic/temperature-safety.md)).

## Amulet CRC — the wall interface (115200 8N1)

After DTV+ discovery, the UI link switches to the Amulet CRC protocol: a
shared-datatable model with CRC-16 framing.

- Controller → panel: `SET_BYTE_VAR` / `SET_WORD_VAR` / `SET_STRING_VAR`
  (the panel mirrors the datatable in real time).
- Panel → controller: `INVOKE_RPC` (user actions — this is how a touch
  starts the shower).
- V2 panels add file transfer: `SET_FILE_TRANSFER (0x20)` →
  `WRITE_LARGE_DATA` chunks → `FLUSH_MD5 (0x21)` → `FILE_COMPLETE (0x22)`.
- The controller re-sends critical variables periodically to survive lost
  frames.

## Temperature encodings — the genuine footgun

| Consumer        | Encoding                 |
| --------------- | ------------------------ |
| Valves          | **Cx2** — Celsius × 2    |
| Steam generator | **Fx2** — Fahrenheit × 2 |

Conversion across the boundary: `Fx2 = ((Cx2 × 9) / 5) + 64`.

| Cx2 | °C   | °F    | Fx2 |
| --- | ---- | ----- | --- |
| 60  | 30.0 | 86.0  | 172 |
| 80  | 40.0 | 104.0 | 208 |
| 90  | 45.0 | 113.0 | 226 |
| 98  | 49.0 | 120.2 | 240 |

`MIN_SYS_VALVE_TEMP` = Cx2 60 (below it the valve raises Full Cold — it
cannot reach the setpoint, usually because hot supply is missing).
`MAX_WATER_TEMP` = Cx2 98 (49 °C).

**Sending an Fx2 value to a valve asks for 104 °C. Sending a Cx2 value to
steam asks for 4 °C. Always convert.**

None of this is what the HTTP layer takes: `quick_shower.cgi` and friends
want whole degrees in the system's configured unit. Cx2/Fx2 live below the
web interface, on the wire.
