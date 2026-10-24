# Saturn valve bus: scope capture, 2026-10-24

The factory K-99695 talking to a valve over its Saturn RS-485 bus, captured differentially at the valve
connector with the system idle. With it are the connector's pinout and unpowered resistances on both
valves, measured with a handheld meter. This answers three of the open items in
[controller/docs/HARDWARE.md](../../../controller/docs/HARDWARE.md) §15: the pin count from item 2, item 3
(which conductor is positive) and item 4 (factory termination and idle bias).

Tiers follow [docs/system-specification.md](../../../docs/system-specification.md): `[A]` measured here,
`[I]` inference, `[?]` unresolved.

## Setup `[A]`

| Item              | Value                                                                                   |
| ----------------- | --------------------------------------------------------------------------------------- |
| Scope             | Rigol DHO804, firmware 00.01.05, driven over SCPI on TCP 5555                           |
| Probes            | CH1 tip on pin 4, CH2 tip on pin 3, both ground clips on pin 2, both probes 10x         |
| Vertical          | 1 V/div, offset −2.5 V, DC coupling, bandwidth limit off, probe ratio 10 on both        |
| Trigger           | Edge, CH1 falling through 1.8 V, normal sweep, single shot                              |
| Memory            | 1 Mpt per channel                                                                       |
| Point of contact  | The valve connector, with the cable plugged into the controller                         |
| System state      | Idle: no shower running, and no CGI calls to the controller during or around the captures |
| Which valve's bus | `[?]` Probably valve 2, the connector metered last before the probes went on            |

## Connector `[A]`

Six positions. Pins 1 and 6 are unpopulated; pins 2 to 5 are wired.

| Pin | Function                    | Basis                                                                      |
| --- | --------------------------- | -------------------------------------------------------------------------- |
| 2   | Ground                      | 12.7 V from pin 2 to pin 5                                                 |
| 3   | RS-485 line, idles ~2.0 V   | Scope; always the opposite of pin 4                                        |
| 4   | RS-485 line, idles ~2.4 V   | Scope; the idle (mark) state holds pin 4 above pin 3                       |
| 5   | +12.7 V supply              | Meter, powered                                                             |

Pin 4 is the positive conductor: it is the higher of the two in the mark state, both idle and driven.
Pin 3 is the negative conductor. RS-485 vendors disagree on whether to letter the positive line A or B,
so wire adapters by the pin numbers, not the letters.

## Meter readings `[A]`

Handheld meter. The resistances were taken after the operator switched the system off at 17:46 UTC.
Valve 1 readings before 17:58 are assigned by sequence. Every reading, including the invalid and
superseded ones, is in [session/meter-readings.csv](session/meter-readings.csv).

| Measurement, unpowered                         | Valve 1  | Valve 2  |
| ---------------------------------------------- | -------- | -------- |
| Pin 2 to pin 3, plugged into the controller    | 239.7 Ω  | 234.9 Ω  |
| Pin 2 to pin 4, plugged into the controller    | 268.5 Ω  | 261.7 Ω  |
| Pin 3 to pin 4, plugged into the controller    | 55.6 Ω   | 55.7 Ω   |
| Pin 3 to pin 4, unplugged, valve side          | 141.2 Ω  | 141.2 Ω  |

| Measurement, powered, against pin 2 | Valve 1 | Valve 2 |
| ----------------------------------- | ------- | ------- |
| Pin 3                               | ~2.4 V  | 2.0 V   |
| Pin 4                               | ~2.7 V  | 2.4 V   |
| Pin 5                               | ~12 V   | 12.7 V  |

`[I]` Both ends load the pair. The valve side alone is 141.2 Ω; with the controller connected it is
55.6 Ω, which puts about 92 Ω across the pair on the controller side. A 120 Ω terminator alongside the
~500 Ω path through pin 2 accounts for that (120 ∥ 508 = 97 Ω, and 141.2 ∥ 97 = 57.5 Ω, against
55.6 Ω measured). The two valves measure the same within 3%.

## Electrical `[A]`

From [edge-detail](edge-detail.metadata.json) and [poll-1](poll-1.metadata.json):

| Quantity                                | CH1 (pin 4)  | CH2 (pin 3)  | Differential (4 − 3) |
| --------------------------------------- | ------------ | ------------ | -------------------- |
| Idle, between frames                    | 2.37 V       | 2.02 V       | +0.36 V              |
| Controller's request, driven mark       | 3.64 V       | 1.42 V       | +2.22 V              |
| Controller's request, driven space      | 1.61 V       | 3.51 V       | −1.90 V              |
| Controller's request, 2nd–98th pct.    | 1.57–3.65 V  | 1.40–3.55 V  |                      |
| Valve's reply, 2nd–98th pct.            | 0.96–2.90 V  | 0.85–2.83 V  |                      |

- Differential edges, 10–90%: 54 ns idle to space, 69 ns space to mark. Peak differential +2.53 V and −2.25 V.
- The two transmitters sit at different common modes, about 2.6 V for the controller and 1.9 V for the
  valve, which is how the request and the reply can be told apart on the screenshots.

## Protocol `[A]`

- 9600 baud, 8 data bits, no parity, 1 stop bit, least significant bit first. Mark (idle, logic 1) is
  pin 4 above pin 3.
- One exchange per cycle, the same in every capture:

```
request  AA 55 00 07 00 FA
reply    AA 55 01 01 0B 00 01 4A 00 CF 00 00 07 08 02 00 C9   exploratory captures, 18:23–18:26 UTC
reply    AA 55 01 01 0B 00 01 4A 00 D0 00 00 07 08 02 00 C8   after-poll-0.5s and poll-1 to poll-4, 18:30–18:31 UTC
```

- Byte pitch is 1146 µs in the request and 1158 µs in the reply, against 1042 µs for 10 bits at 9600 baud;
  each transmitter leaves its own inter-byte gap.
- Request start to reply start: 8.6, 9.4, 10.3 and 14.2 ms (poll-1 to poll-4).
- The next request starts 0.488 s and 0.490 s after the triggered one (after-poll-0.5s, after-poll-0.3s).
  Nothing else is on the pair between 0.1 s and 0.9 s after a trigger. xagon0 documents a 525 ms cycle.
- `[I]` Reply data byte 3 is a live value: it moved from `CF` to `D0` between 18:26 and 18:30 UTC with the
  system idle, and the checksum moved with it.

### Checksum

Every frame on this bus checks with the sync bytes **included**: the checksum is the 8-bit two's
complement of the sum of every byte from SYNC1 through the last data byte.

| Frame                | On the wire | Sync excluded | Sync included |
| -------------------- | ----------- | ------------- | ------------- |
| Request              | `FA`        | `F9`          | `FA`          |
| Reply, data[3] = CF  | `C9`        | `C8`          | `C9`          |
| Reply, data[3] = D0  | `C8`        | `C7`          | `C8`          |

`kdtv-proto` CHK-01 and CHK-02 (`controller/crates/kdtv-proto/src/saturn/frame.rs`) exclude the sync
bytes, following the xagon0 example `AA 55 00 02 01 1E DF`. On this bus that rule computes one less than
the wire carries, on every frame.

## Files

Eight captures. Each has three parts:

| File                        | Contents                                                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------- |
| `<capture>.channel1.raw`    | CH1 samples exactly as `:WAVeform:DATA?` returned them, BYTE format, full memory (`:WAVeform:MODE MAXimum`), read in 250 000-point chunks and concatenated with the block headers removed |
| `<capture>.channel2.raw`    | The same for CH2                                                                                  |
| `<capture>.metadata.json`   | Preamble, scope settings at capture time, timestamps, byte counts, SHA-256                        |
| `<capture>.png`             | The scope screen (`:DISPlay:DATA? PNG`) taken straight after the read                             |

| Capture            | Timebase   | Sample interval | Record covers, relative to the trigger | What it shows                         |
| ------------------ | ---------- | --------------- | -------------------------------------- | ------------------------------------- |
| `poll-1` … `poll-4` | 20 ms/div  | 400 ns          | −0.12 s to +0.28 s                     | One request and its reply             |
| `after-poll-0.3s`  | 20 ms/div  | 400 ns          | +0.10 s to +0.50 s                     | The next request                      |
| `after-poll-0.5s`  | 20 ms/div  | 400 ns          | +0.30 s to +0.70 s                     | The next request and reply            |
| `after-poll-0.7s`  | 20 ms/div  | 400 ns          | +0.50 s to +0.90 s                     | The end of that reply, then idle      |
| `edge-detail`      | 5 µs/div   | 1.6 ns          | −0.8 ms to +0.8 ms                     | The request's first edges at 625 MSa/s |

Convert with the preamble (`format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference`):

```python
import json, numpy as np

meta = json.load(open("poll-1.metadata.json"))
ch = meta["channels"]["CHANnel1"]
_, _, _, _, xinc, xorig, xref, yinc, yorig, yref = map(float, ch["preamble"].split(","))
raw = np.fromfile(ch["file"], dtype=np.uint8).astype(float)
volts = (raw - yorig - yref) * yinc            # probe ratio already applied
seconds = (np.arange(raw.size) - xref) * xinc + xorig   # 0 s is the trigger
```

`manifest.json` lists every file with its size and SHA-256.

### session/

The working session that led up to the captures, kept as recorded:

| File                                | Contents                                                                               |
| ----------------------------------- | -------------------------------------------------------------------------------------- |
| `meter-readings.csv`                | Every meter reading, with the invalid and superseded ones marked and why               |
| `scope-commands.csv`                | Every SCPI command sent to the scope and its reply, in order                           |
| `1815-ch2-probe-mismatch.png`       | CH2 at a tenth of its true level: its probe on 10x with the scope set to 1x            |
| `1815-first-edge.png`               | The first triggered edge: a valve reply bit at 10 µs/div                               |
| `1816-first-request-200us.png/.csv` | The first request at 200 µs/div; the CSV is the 1000-point screen export (volts)        |
| `1819-roll-mode-no-data.png`        | 50 ms/div switches the DHO804 to roll mode, which does not trigger                     |
| `1819-armed-waiting.png`            | Armed with CH1's probe moved to 10x and the scope still at 1x, so it never triggered   |
| `1821-both-probes-10x.png`          | Both channels corrected to 10x, idle levels matching the meter                         |
| `1823-request-and-reply-20ms.png`   | A request and reply at 20 ms/div, free-running                                         |
| `1808-before-setup.png`             | The scope as found                                                                     |

Between 18:15 and 18:21 UTC the scope's probe ratios did not always match the probe switches: CH2's probe
was on 10x with the scope at 1x until 18:15:55, and CH1's probe was moved to 10x at 18:17 with the scope
at 1x until 18:21. `scope-commands.csv` records each correction. Every capture in the top-level directory
was taken with both at 10x.
