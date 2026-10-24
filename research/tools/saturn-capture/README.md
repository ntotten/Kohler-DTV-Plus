# Saturn bus capture tools

Capture and decode the Saturn RS-485 bus between a K-99695 and its valves with a Rigol DHO800/DHO900
scope over the network. These took and decoded
[research/diagnostics/2026-10-24-saturn-valve-bus/](../../diagnostics/2026-10-24-saturn-valve-bus/).
Python 3.10+; `saturn_decode.py` needs numpy. Nothing else.

| Script             | Does                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------- |
| `dho.py`           | SCPI to the scope: queries, commands, full-memory two-channel captures, screenshots                     |
| `saturn_decode.py` | UART and Saturn framing from a capture, with each frame's checksum checked both ways                    |
| `kohler_once.py`   | One request to the controller: `values`, `system_info`, `quick_shower` or `stop_shower` only, 5 s apart |
| `shower_run.py`    | Records the bus through one outlet start and stop. Runs water                                           |

## Wiring

At the valve connector, with the cable plugged into the controller:

| Probe             | Pin |
| ----------------- | --- |
| CH1 tip           | 4   |
| CH2 tip           | 3   |
| Both ground clips | 2   |

Keep the ground clips off pin 5, the 12.7 V supply. Set each probe's ratio on the scope to match its 1x/10x
switch: the scope cannot read the switch, and a mismatch shows as a channel at a tenth (or ten times) of its
real level. Pin 2 against pin 5 should read about 12.7 V on a meter before the clips go on.

## Capture

```bash
export DHO_HOST=<scope address>
python3 dho.py send ":RUN" ":CHANnel1:DISPlay ON" ":CHANnel2:DISPlay ON" \
  ":CHANnel1:PROBe 10" ":CHANnel2:PROBe 10" ":CHANnel1:SCALe 1" ":CHANnel2:SCALe 1" \
  ":CHANnel1:OFFSet -2.5" ":CHANnel2:OFFSet -2.5" ":ACQuire:MDEPth 1M" \
  ":TIMebase:MAIN:SCALe 2e-2" ":TIMebase:MAIN:OFFSet 8e-2" \
  ":TRIGger:EDGE:SOURce CHANnel1" ":TRIGger:EDGE:SLOPe NEGative" ":TRIGger:EDGE:LEVel 1.8" \
  ":TRIGger:SWEep NORMal"
python3 dho.py send ":SINGle"
python3 dho.py query ":TRIGger:STATus?"     # STOP once it has triggered, within a second
python3 dho.py grab --out captures --name poll-1
python3 dho.py png --out captures --name poll-1
python3 saturn_decode.py captures/poll-1.metadata.json
```

Set the probe ratios before the trigger level: changing a ratio rescales the level with it. The falling
edge on CH1 through 1.8 V catches the start bit of every frame, the controller's and the valve's.

For several seconds in one record, use roll mode: `:TIMebase:MAIN:SCALe 0.5` with `:ACQuire:MDEPth 10M`
gives about 6.5 s at 1.25 MSa/s. Run, wait, `:STOP`, then `grab`. Roll mode never triggers.

## Decode

```
$ python3 saturn_decode.py poll-1.metadata.json
  0.000001  incl  AA 55 00 07 00 FA
  0.008568  incl  AA 55 01 01 0B 00 01 4A 00 D0 00 00 07 08 02 00 C8
```

Time is seconds from the trigger (or from the stop, in roll mode). The middle column says which checksum
rule the frame satisfies: `incl` with the sync bytes in the sum, `excl` without, `BAD` neither. `--bytes`
prints the UART bytes instead of frames, and `--invert` swaps the conductors if CH2 is on the positive
one. A `t_s,CH1,CH2` CSV works as input too.

## Shower run

```bash
python3 shower_run.py --scope <scope> --controller <controller> --out run1 --valve 2 --outlet 1 --temp 96
```

Starts one outlet with `quick_shower.cgi`, records the bus, reads `system_info.cgi`, stops it with
`stop_shower.cgi`, records again and reads `system_info.cgi`. The stop is sent whatever happens once the
scope is running, including a failed start. `--temp` is in the controller's unit and capped at 100 (°F);
see [DISCLAIMER.md](../../../DISCLAIMER.md) for why the controller's own maximum is not a safe bound.
Every controller call goes through `kohler_once.py`, so it counts against the two-session limit and keeps
its 5 s spacing; close any browser tab on the controller first ([app/CLAUDE.md](../../../app/CLAUDE.md)).

## Scope behaviour

Found on DHO804 firmware 00.01.05, and handled in `dho.py`:

- `:WAVeform:MODE RAW` returns nothing; `MAXimum` returns the full memory once the scope is stopped.
- The preamble's point count follows `:WAVeform:STARt`/`STOP`. A window left past the end of a shorter
  record makes both refuse to change; switching to `NORMal` and back resets it.
- Timebase changes are refused while stopped, because the record is fixed. Send `:RUN` first.
- 50 ms/div and slower is roll mode.
- `:MEASure:ITEM? <item>,<source>` adds the item to the on-screen results panel.
