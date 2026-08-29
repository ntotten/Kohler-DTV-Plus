# research/reference/steam-adapter/ — probing this board

[README.md](README.md) holds the findings. This file holds the method, because
the same mistakes were made repeatedly on 2026-08-29.

## The board is unpowered and has never been powered

Bought four years ago, never plugged in. Nothing stored, nothing to discharge,
no risk to the meter. Probe freely.

## High resistance does not mean "not connected"

This was got wrong three times in one session. The board is full of
semiconductors in series and shunt positions, and a meter's resistance mode
biases them:

| Reading                | Was concluded           | Actually was                              |
| ---------------------- | ----------------------- | ----------------------------------------- |
| Conducts one way only  | "connected to `C13`"    | `SMBJ28A` clamp forward-biased            |
| 140 kΩ / 2.13 MΩ       | "pin 4 is not a supply" | Measuring through a reverse-biased `D9`   |
| Would have read 1.2 kΩ | (nearly missed)         | `R16` in series on the way to `IC2` pin 8 |

**Rules that follow:**

1. **Measure every pair in both probe polarities.** Copper reads the same both
   ways. A junction does not. This one test resolves most ambiguity here.
2. **Read the magnitude, not just the beep.** Under 1 Ω is copper. Hundreds of
   ohms to kΩ usually means a resistor or a junction in the path — which is
   still a real connection, just not a direct one.
3. **The continuity beeper alone is not enough.** It only sounds below a low
   threshold, so a genuine path through a series resistor stays silent.

## Confirm the part before theorising about it

`C13` was taken from a photograph to be the 330 µF electrolytic near the
connectors. It is an **unpopulated two-hole footprint beside `R27`**. Two
increasingly elaborate explanations were then built for a reading that was
never anomalous.

Tracing visible copper and reading part markings has beaten inference from
photographs every single time on this board. Prefer them.

## Photographs

Downscaled to 2000 px, quality 70 — 52 MB to 13 MB. Every marking transcribed
in the README was read at that resolution or lower. Originals are with the
operator and are not in the repository.

New photographs should be compressed the same way before committing.

## Still to do

One check, and it needs power rather than a meter: apply 12 V to connector
pin 4 from a **current-limited** supply with ground on pin 3, and measure `IC2`
pin 8. About 5 V confirms the rail and the supply chain; the current reading
sizes the permanent supply. That would be the first time this adapter has ever
been powered — set the limit low.
