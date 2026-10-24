# Saturn valve bus: scope capture, 2026-10-24

The factory K-99695 talking to both of its valves over their Saturn RS-485 buses, captured differentially
at the valve connectors: valve 2 idle and through a start and stop, then valve 1 through a controller
restart and two starts and stops. With them are the connector's pinout and the meter readings of both
valves' connectors. The two valves speak different command sets on the same framing; see
[Valve 1](#valve-1). This answers three of the open items in
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
| Which valve's bus | Valve 2 for the top level and `shower-run/`, valve 1 for `valve-1/`; the operator moved the probes at 18:57 UTC, and the idle levels match each valve's meter readings |

No HTTP request went to the controller before 18:38 UTC. After that there were fifteen, one at a time and
at least 5 s apart: `values.cgi` at 18:38:31; the shower run's four in
[shower-run/controller-calls.log](shower-run/controller-calls.log); `system_info.cgi` at 18:42:26 and at
19:01:58, after the restart; and the four of each valve-1 run in their own `controller-calls.log`.

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

## Protocol: valve 2 `[A]`

- 9600 baud, 8 data bits, no parity, 1 stop bit, least significant bit first. Mark (idle, logic 1) is
  pin 4 above pin 3.
- One request and one reply per cycle. Request start to reply start: 8.6, 9.4, 10.3 and 14.2 ms
  (poll-1 to poll-4). Byte pitch is 1146 µs in the request and 1158 µs in the reply, against 1042 µs for
  10 bits at 9600 baud.
- The cycle averages 525 ms, the figure xagon0 documents. Before the re-address described below, the
  intervals alternate between about 0.55 s and 0.49 s (`idle-roll`; `after-poll-0.3s` and
  `after-poll-0.5s` caught the short one, 0.490 s and 0.488 s). After it they are a steady 0.525 s, with a
  poll slot given up to the `3A` broadcast now and then.
- Every frame decoded, 834 across both buses (102 on valve 2, 732 on valve 1), carries a valid checksum;
  each capture's `.frames.txt` lists them.

### Checksum

Every frame on this bus checks with the sync bytes **included**: the checksum is the 8-bit two's
complement of the sum of every byte from SYNC1 through the last data byte. None of the 834 checks with
them excluded, on either bus.

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
afterwards both read `Paused` with `ui_shower_on` and `devices_running` false. `[I]` Valve 1 ran too: the
same call repeated with the probes on valve 1's bus (`valve-1/run-valve-2`) opens valve 1's outlet position 1.

## Valve 1

The probes moved to valve 1's connector at 18:57 UTC, same pins, while the controller restarted; it
dropped off the network and came back at 19:00:22.

### Electrical `[A]`

| Quantity                  | CH1 (pin 4) | CH2 (pin 3) | Differential (4 − 3) |
| ------------------------- | ----------- | ----------- | -------------------- |
| Idle                      | 2.67 V      | 2.27 V      | +0.40 V              |
| Driven mark, 2nd–98th pct.  | 3.62–3.69 V | 1.02–1.47 V |                      |
| Driven space, 2nd–98th pct. | 1.13–1.67 V | 3.45–3.52 V |                      |

The idle levels match valve 1's meter readings (2.7 V and 2.4 V) within 0.15 V, not valve 2's.

### Start-up `[A]`

`idle-roll-1` caught the controller bringing the bus up, at about 18:58:01, two minutes before it was back
on the network. Times are from that record:

```
-59.82 s  AA 55 1F 3A 01 01 A6                          four times, 3.87 s apart, no reply
-45.97 s  AA 55 00 54 00 AD                             -> AA 55 01 01 04 00 06 00 0C E9
-45.94 s  AA 55 00 40 00 C1                             -> AA 55 01 01 04 00 02 26 BF 14
-45.44 s  AA 55 00 54 00 AD                             -> AA 55 01 01 04 00 06 00 0C E9
-44.92 s  AA 55 00 16 00 EB                             -> AA 55 01 01 04 03 09 00 04 EB
-44.39 s  AA 55 00 0B 00 F6                             -> AA 55 01 01 0A 01 02 00 64 00 C8 01 2C 01 2C 6C
-43.87 s  AA 55 00 0C 00 F5                             -> AA 55 01 01 0B 02 01 00 3F 00 02 76 00 00 01 5C DD
-43.34 s  AA 55 00 15 00 EC                             -> AA 55 01 01 01 AD 51
-41.77 s  steady polling begins
```

xagon0 names control `3A` the address enquiry, `40` read extended status and `16` read generic outlet
status. The enquiry broadcasts stop once start-up is done. On valve 2's bus they have recurred every few
seconds since its poll address changed at 18:39 (see [Idle](#idle)). `[I]` The controller has not
finished setting valve 2 up since then; compare
[I3](../../../INVESTIGATIONS.md#i3--valuescgi-intermittently-drops-a-healthy-valve).

### Steady state `[A]`

Valve 1 stays at address `00` and is polled with two requests, alternating every 0.525 s:

```
AA 55 00 01 00 00    -> AA 55 01 01 07 <state> 00 00 00 <b4> 00 00 <chk>     idle: state 00, b4 A1-AA
AA 55 00 02 00 FF    -> AA 55 01 01 04 <state> 00 <outlets> 00 <chk>         idle: 00 00 00 00
```

Valve 2 at `13` (earlier `00`) gets one request, control `07`, with an 11-byte status reply. The framing,
baud rate, polarity and checksum are the same on both buses.

### Commands `[A]`

Valve 1 is commanded with controls `81` and `A4`, not valve 2's `87`, and answers every command with
`AA 55 01 01 01 01 FD`.

```
run-valve-1, start (valve 1 position 1, 96 °F)
AA 55 00 81 03 00 01 26 56      mode 00
AA 55 00 81 03 01 01 26 55      mode 01
AA 55 00 A4 01 00 5C            outlets 00
AA 55 00 A4 01 04 58            outlets 04
then status   AA 55 01 01 07 01 01 26 00 A1 00 00 2F   and   AA 55 01 01 04 01 00 04 00 F6

run-valve-1, stop
AA 55 00 A4 01 00 5C            outlets 00
AA 55 00 81 03 03 01 26 53      mode 03
then status   AA 55 01 01 07 03 01 26 01 33 00 00 9A   and   AA 55 01 01 04 00 00 00 00 FB
```

| Field                                  | Reading                                                                                                         |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `81` data byte 0, status byte 0        | Mode: 00 idle or set up, 01 running, 03 stopped, the same codes as valve 2                                      |
| `81` data bytes 1–2, status bytes 1–2  | 0x0126 = 294 in every command and status frame, idle and running. `[?]` Not the 96 °F setpoint in valve 2's tenths-of-a-degree-C encoding (0x0163) |
| `A4` data, outlet reply byte 2         | Outlet bitmask. Position 1 went out as `04`, physical outlet 3, which is what `values.cgi` maps it to (`valve1_outlet1_func` id 3) |
| Status bytes 3–4                       | `[I]` Water temperature, 0.1 °C: 0x00A1–0x00AA (16.1–17.0 °C) idle, up to 0x0159 (34.5 °C) during flow; `system_info` said 94–95 °F     |

### A valve 2 start also starts valve 1 `[A]`

`run-valve-2` repeats the valve 2 run (`quick_shower.cgi?valve_num=2&valve1_outlet=&…&valve2_outlet=1`)
with the probes on valve 1's bus. The controller sends valve 1 `81` mode 03, then mode 01, then `A4 04`:
valve 1's outlet position 1 opens, and its status reports running and outlet 3 open until the stop.
`system_info` reported valve 1 `On` at 94 °F alongside valve 2. The reverse does not happen: in
`run-valve-1` (`valve_num=1`, `valve2_outlet` empty) valve 2 stayed `Paused`. An empty `valve1_outlet`
is therefore not "no outlet" when the call is for valve 2. `[?]` Which outlet valve 1 gets: position 1
was both the outlet of the run just before (`run-valve-1`) and possibly valve 1's default
(`valve1_def_outlet = 1` in the 2026-08-22 `values.raw`, whose indexing is not established), so this
capture cannot separate "last selection" from "default".

## Files

| File                     | Contents                                                                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<capture>.channel1.raw` | CH1 samples exactly as `:WAVeform:DATA?` returned them, BYTE format, full memory (`:WAVeform:MODE MAXimum`), read in 250 000-point chunks and concatenated with the block headers removed |
| `<capture>.channel2.raw` | The same for CH2                                                                                                                                                                 |
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
| `valve-1/idle-roll-1`      | roll      | 8 µs            | 71 s to 18:59:01      | Valve 1: the controller's start-up, then idle   |
| `valve-1/idle-roll-2`, `-3` | roll     | 8 µs            | 55 s to 19:00:12, 19:01:20 | Valve 1 idle                               |
| `valve-1/run-valve-1/…`    | roll      | 800 ns          | 6.7 s and 8.0 s       | Valve 1 started and stopped                     |
| `valve-1/run-valve-2/…`    | roll      | 800 ns          | 6.9 s and 8.0 s       | Valve 1's bus while valve 2 starts and stops    |
| `shower-run/shower-start`  | roll      | 800 ns          | 6.5 s to 18:40:06     | Idle, the start commands, the first seconds of flow |
| `shower-run/shower-stop`   | roll      | 800 ns          | 8.0 s to 18:40:33     | Flow, the stop commands, stopped                |

Times in the triggered captures are relative to the trigger; in the roll records, to the moment the scope
stopped. Convert with the preamble (`format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference`):

```python
import json, numpy as np

meta = json.load(open("poll-1.metadata.json"))
ch = meta["channels"]["CHANnel1"]
_, _, _, _, xinc, xorig, xref, yinc, yorig, yref = map(float, ch["preamble"].split(","))
raw = np.fromfile(ch["file"], dtype=np.uint8).astype(float)
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
