# Saturn valve bus: scope capture, 2026-10-24

The factory K-99695 talking to a valve over its Saturn RS-485 bus, captured differentially at the valve
connector: idle, and through one start and stop of an outlet. With it are the connector's pinout and the
meter readings of both valves' connectors. This answers three of the open items in
[controller/docs/HARDWARE.md](../../../controller/docs/HARDWARE.md) §15: the pin count from item 2, item 3
(which conductor is positive) and item 4 (factory termination and idle bias). The tools that took and
decoded it are in [research/tools/saturn-capture/](../../tools/saturn-capture/).

Tiers follow [docs/system-specification.md](../../../docs/system-specification.md): `[A]` measured here,
`[I]` inference, `[?]` unresolved.

## Setup `[A]`

| Item              | Value                                                                                       |
| ----------------- | ------------------------------------------------------------------------------------------- |
| Scope             | Rigol DHO804, firmware 00.01.05, driven over SCPI on TCP 5555                               |
| Probes            | CH1 tip on pin 4, CH2 tip on pin 3, both ground clips on pin 2, both probes 10x             |
| Vertical          | 1 V/div, offset −2.5 V, DC coupling, bandwidth limit off, probe ratio 10 on both            |
| Trigger           | Edge, CH1 falling through 1.8 V, normal sweep, single shot (roll mode for the long records) |
| Point of contact  | The valve connector, with the cable plugged into the controller                             |
| Which valve's bus | `[I]` Valve 2: the idle levels on the scope match valve 2's meter readings, not valve 1's   |

No HTTP request went to the controller before 18:38 UTC. After that there were six, one at a time and at
least 5 s apart: `values.cgi` at 18:38:31, the four of the shower run in
[shower-run/controller-calls.log](shower-run/controller-calls.log), and `system_info.cgi` at 18:42:26.

## Connector `[A]`

Six positions. Pins 1 and 6 are unpopulated; pins 2 to 5 are wired.

| Pin | Function                  | Basis                                                |
| --- | ------------------------- | ---------------------------------------------------- |
| 2   | Ground                    | 12.7 V from pin 2 to pin 5                           |
| 3   | RS-485, negative          | Scope; always the opposite of pin 4                  |
| 4   | RS-485, positive          | Scope; higher of the two in the mark state           |
| 5   | +12.7 V supply            | Meter, powered                                       |

Pin 4 is the higher of the two conductors in the mark state, both idle and driven. RS-485 vendors
disagree on whether to letter the positive line A or B, so wire adapters by the pin numbers, not the
letters.

## Meter readings `[A]`

Handheld meter. The resistances were taken after the operator switched the system off at 17:46 UTC.
Valve 1 readings before 17:58 are assigned by sequence. Every reading, including the invalid and
superseded ones, is in [session/meter-readings.csv](session/meter-readings.csv).

| Measurement, unpowered                      | Valve 1 | Valve 2 |
| ------------------------------------------- | ------- | ------- |
| Pin 2 to pin 3, plugged into the controller | 239.7 Ω | 234.9 Ω |
| Pin 2 to pin 4, plugged into the controller | 268.5 Ω | 261.7 Ω |
| Pin 3 to pin 4, plugged into the controller | 55.6 Ω  | 55.7 Ω  |
| Pin 3 to pin 4, unplugged, valve side       | 141.2 Ω | 141.2 Ω |

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

| Quantity                             | CH1 (pin 4) | CH2 (pin 3) | Differential (4 − 3) |
| ------------------------------------ | ----------- | ----------- | -------------------- |
| Idle, between frames                 | 2.37 V      | 2.02 V      | +0.36 V              |
| Controller's request, driven mark    | 3.64 V      | 1.42 V      | +2.22 V              |
| Controller's request, driven space   | 1.61 V      | 3.51 V      | −1.90 V              |
| Controller's request, 2nd–98th pct.  | 1.57–3.65 V | 1.40–3.55 V |                      |
| Valve's reply, 2nd–98th pct.         | 0.96–2.90 V | 0.85–2.83 V |                      |

- Differential edges, 10–90%: 54 ns idle to space, 69 ns space to mark. Peak differential +2.53 V and −2.25 V.
- The two transmitters sit at different common modes, about 2.6 V for the controller and 1.9 V for the
  valve, which is how the request and the reply can be told apart on the screenshots.

## Protocol `[A]`

- 9600 baud, 8 data bits, no parity, 1 stop bit, least significant bit first. Mark (idle, logic 1) is
  pin 4 above pin 3.
- One request and one reply per cycle. Request start to reply start: 8.6, 9.4, 10.3 and 14.2 ms
  (poll-1 to poll-4). Byte pitch is 1146 µs in the request and 1158 µs in the reply, against 1042 µs for
  10 bits at 9600 baud.
- The cycle averages 525 ms, the figure xagon0 documents. Before the re-address described below, the
  intervals alternate between about 0.55 s and 0.49 s (`idle-roll`; `after-poll-0.3s` and
  `after-poll-0.5s` caught the short one, 0.490 s and 0.488 s). After it they are a steady 0.525 s, with a
  poll slot given up to the `3A` broadcast now and then.
- Every frame decoded, 102 across the captures, carries a valid checksum; each capture's `.frames.txt`
  lists them.

### Checksum

Every frame on this bus checks with the sync bytes **included**: the checksum is the 8-bit two's
complement of the sum of every byte from SYNC1 through the last data byte. None of the 102 checks with
them excluded.

| Frame                                 | On the wire | Sync excluded | Sync included |
| ------------------------------------- | ----------- | ------------- | ------------- |
| Poll, `AA 55 00 07 00 …`              | `FA`        | `F9`          | `FA`          |
| Poll, `AA 55 13 07 00 …`              | `E7`        | `E6`          | `E7`          |
| Command, `AA 55 13 87 06 01 01 63 64 00 00 …` | `98` | `97`          | `98`          |
| Reply, `… 4A 00 CF 00 00 07 08 02 00 …` | `C9`      | `C8`          | `C9`          |

`kdtv-proto` CHK-01 and CHK-02 (`controller/crates/kdtv-proto/src/saturn/frame.rs`) exclude the sync
bytes, following the xagon0 example `AA 55 00 02 01 1E DF`. On this bus that rule computes one less than
the wire carries, on every frame.

### Idle

```
18:23–18:39 UTC
request  AA 55 00 07 00 FA
reply    AA 55 01 01 0B 00 01 4A 00 CF 00 00 07 08 02 00 C9      to 18:26
reply    AA 55 01 01 0B 00 01 4A 00 D0 00 00 07 08 02 00 C8      18:30–18:39 (once D1)

from 18:40 UTC
request  AA 55 13 07 00 E7
reply    AA 55 01 01 0B 00 01 4A 00 D0 00 00 07 08 03 00 C7
         AA 55 1F 3A 01 01 A6                                     every few seconds
```

Between the end of `idle-roll` (18:39:26) and the start of `shower-run/shower-start` (18:40:00) the
controller's poll changed address from `00` to `13`, the reply's data byte 9 changed from `02` to `03`,
and a broadcast `AA 55 1F 3A 01 01 A6` began. xagon0 lists control `3A` with data `01` as the
address enquiry for unaddressed valves. Nothing was sent to the controller in that interval, the operator
changed nothing on the system, and the idle levels on the scope are unchanged to the millivolt, so the
probes did not move. The poll was still at `13` at 18:54 (`later-poll`). `[?]` What prompted the
controller to re-address the valve.

### Shower run

Valve 2, outlet position 1 (showerhead), 96 °F, started with `quick_shower.cgi` at 18:40:02 and stopped
with `stop_shower.cgi` at 18:40:29 UTC. Both records are roll mode, 0 s at the moment the scope stopped.

```
shower-start, -4.20 s   the quick_shower call
request  AA 55 13 87 06 00 01 63 00 00 00 FD
reply    AA 55 01 01 0C 80 00 01 63 00 D0 00 00 07 08 03 00 2D
request  AA 55 13 07 00 E7
reply    AA 55 01 01 0B 00 01 63 00 D0 00 00 07 08 03 00 AE
request  AA 55 13 87 06 01 01 63 64 00 00 98                      sent twice
reply    AA 55 01 01 0C 01 01 01 63 00 D0 64 00 07 08 03 00 47
then     AA 55 01 01 0B 01 01 63 00 CE 64 00 07 07 03 00 4C       each poll while running

shower-stop, -3.80 s    the stop_shower call
request  AA 55 13 87 06 03 01 63 00 00 00 FA                      sent twice
reply    AA 55 01 01 0C 01 03 01 63 01 63 00 00 07 08 03 00 15
reply    AA 55 01 01 0C 80 03 01 63 01 64 00 00 07 08 03 00 95
then     AA 55 01 01 0B 03 01 63 01 64 00 00 07 08 03 00 16       each poll while stopped
```

`[I]` Field reading, consistent across all of the above:

| Frame                   | Bytes                                       | Reading                                                                                |
| ----------------------- | ------------------------------------------- | -------------------------------------------------------------------------------------- |
| Command, control `87`   | `mode`, `setpoint` (2), `flow`, `00`, `00`  | mode 00 set up, 01 run, 03 stop; setpoint 0x0163 = 355 = 35.5 °C = 96 °F; flow 0x64 = 100 |
| Status reply, 11 bytes  | `state`, `setpoint` (2), `temperature` (2), `flow`, `00`, `timer` (2), `03`, `00` | state 00 idle, 01 running, 03 stopped; temperature in 0.1 °C; timer counts down 1 per second from 0x0708 = 1800 |
| Command reply, 12 bytes | `01` or `80`, then the 11-byte status       | `[?]` What separates 01 from 80                                                        |

The temperature field read 0x00D0 (20.8 °C) idle, 0x00C9–0x00D5 in the first seconds of flow and
0x015B–0x016A (34.7–36.2 °C) 20–27 s in. The timer resets to 1800 on both start and stop and keeps
counting down after the stop: `after-run-poll` reads `03 01 63 01 20 00 00 05 79 03 00` at 18:47:10,
timer 0x0579 = 1401, 399 s after the stop, with the temperature down to 0x0120 = 28.8 °C; `later-poll`
reads 0x03BD = 957 at 18:54:34, 843 s after the stop, and 27.0 °C. While the water ran, `system_info`
reported both valves `On` (valve 1 95 °F, valve 2 99 °F) although only valve 2 was given an outlet;
afterwards both read `Paused` with `ui_shower_on` and `devices_running` false.

## Files

| File                     | Contents                                                                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<capture>.channel1.raw` | CH1 samples exactly as `:WAVeform:DATA?` returned them, BYTE format, full memory (`:WAVeform:MODE MAXimum`), read in 250 000-point chunks and concatenated with the block headers removed |
| `<capture>.channel2.raw` | The same for CH2                                                                                                                                                                 |
| `….raw.gz`               | The roll-mode records, gzipped; `byte_count` and `sha256` in the metadata are of the decompressed samples                                                                        |
| `<capture>.metadata.json`| Preamble, scope settings at capture time, timestamps, byte counts, SHA-256                                                                                                       |
| `<capture>.png`          | The scope screen (`:DISPlay:DATA? PNG`) taken straight after the read                                                                                                            |
| `<capture>.frames.txt`   | `saturn_decode.py` output: time, checksum rule matched, frame bytes                                                                                                              |

| Capture                    | Timebase  | Sample interval | Record covers         | What it shows                                   |
| -------------------------- | --------- | --------------- | --------------------- | ----------------------------------------------- |
| `poll-1` … `poll-4`        | 20 ms/div | 400 ns          | −0.12 s to +0.28 s    | One request and its reply                       |
| `after-poll-0.3s`          | 20 ms/div | 400 ns          | +0.10 s to +0.50 s    | The next request                                |
| `after-poll-0.5s`          | 20 ms/div | 400 ns          | +0.30 s to +0.70 s    | The next request and reply                      |
| `after-poll-0.7s`          | 20 ms/div | 400 ns          | +0.50 s to +0.90 s    | The end of that reply, then idle                |
| `edge-detail`              | 5 µs/div  | 1.6 ns          | −0.8 ms to +0.8 ms    | The request's first edges at 625 MSa/s          |
| `idle-roll`                | roll      | 800 ns          | 6.4 s before 18:39:26 | Twelve idle cycles, the last before re-address  |
| `after-run-poll`           | 20 ms/div | 400 ns          | −0.12 s to +0.28 s    | A poll 6.7 minutes after the stop               |
| `later-poll`               | 20 ms/div | 400 ns          | −0.12 s to +0.28 s    | A poll 14 minutes after the stop, still at `13` |
| `shower-run/shower-start`  | roll      | 800 ns          | 6.5 s to 18:40:06     | Idle, the start commands, the first seconds of flow |
| `shower-run/shower-stop`   | roll      | 800 ns          | 8.0 s to 18:40:33     | Flow, the stop commands, stopped                |

Times in the triggered captures are relative to the trigger; in the roll records, to the moment the scope
stopped. Convert with the preamble (`format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference`):

```python
import gzip, json, numpy as np

meta = json.load(open("poll-1.metadata.json"))
ch = meta["channels"]["CHANnel1"]
_, _, _, _, xinc, xorig, xref, yinc, yorig, yref = map(float, ch["preamble"].split(","))
data = open(ch["file"], "rb").read()
if ch["file"].endswith(".gz"):
    data = gzip.decompress(data)
raw = np.frombuffer(data, dtype=np.uint8).astype(float)
volts = (raw - yorig - yref) * yinc                      # probe ratio already applied
seconds = (np.arange(raw.size) - xref) * xinc + xorig
```

`manifest.json` lists every file with its size and SHA-256.

### session/

The working session that led up to the captures, kept as recorded:

| File                                | Contents                                                                              |
| ----------------------------------- | ------------------------------------------------------------------------------------- |
| `meter-readings.csv`                | Every meter reading, with the invalid and superseded ones marked and why              |
| `scope-commands.csv`                | The settings commands and queries sent to the scope, with replies, in order; trigger polling and waveform reads are not in it |
| `1815-ch2-probe-mismatch.png`       | CH2 at a tenth of its true level: its probe on 10x with the scope set to 1x           |
| `1815-first-edge.png`               | The first triggered edge: a valve reply bit at 10 µs/div                              |
| `1816-first-request-200us.png/.csv` | The first request at 200 µs/div; the CSV is the 1000-point screen export (volts)       |
| `1819-roll-mode-no-data.png`        | 50 ms/div switches the DHO804 to roll mode, which does not trigger                    |
| `1819-armed-waiting.png`            | Armed with CH1's probe moved to 10x and the scope still at 1x, so it never triggered  |
| `1821-both-probes-10x.png`          | Both channels corrected to 10x, idle levels matching the meter                        |
| `1823-request-and-reply-20ms.png`   | A request and reply at 20 ms/div, free-running                                        |
| `1808-before-setup.png`             | The scope as found                                                                    |

Between 18:15 and 18:21 UTC the scope's probe ratios did not always match the probe switches: CH2's probe
was on 10x with the scope at 1x until 18:15:55, and CH1's probe was moved to 10x at 18:17 with the scope
at 1x until 18:21. `scope-commands.csv` records each correction. Every capture in the top-level directory
was taken with both at 10x.
