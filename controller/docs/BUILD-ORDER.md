# Build order

What happens in what order, what is bought when, and why. The phase definitions
and their gates are owned by [DESIGN.md § Delivery phases](DESIGN.md); parts,
prices and purchase links by [SHOPPING-LIST.md](SHOPPING-LIST.md); the wiring
and bench tests by [HARDWARE.md](HARDWARE.md). This document owns the
sequencing.

## The rule that orders everything

**Money follows measurement.** Nothing whose specification depends on the
installation is ordered until the named measurement closes it — that is all of
[SHOPPING-LIST.md](SHOPPING-LIST.md) Group B: connectors wait for photographs
and continuity checks, probe clamps wait for the pipe OD, cable wait for the
measured run, disconnects wait for the electrician. What is ordered up front —
Group A — is the set of parts fully specified from manufacturer data alone: the
Pi, the three converters, storage, the sensor and clock boards. No Phase 0 or
Phase 1 measurement feeds into any of them.

## Why compute hardware is ordered before the measurements

Group A is not ahead of the measurements; it is on a different track. The only
strict dependency it has is _Group A in hand before Phase 2_, and Phase 2 —
bench acceptance plus a seven-day continuous soak against emulated devices —
never touches the shower. Ordering Group A first lets the bench track run in
parallel with the survey and capture track, and the soak is the longest fixed
delay in the whole plan.

What the early order puts at risk, stated plainly:

| Risk                                                                                           | Exposure                |
| ---------------------------------------------------------------------------------------------- | ----------------------- |
| Phase 3 measures a fail-off latency over 30 s, which rejects this architecture outright        | The ~$233 Group A spend |
| The valve buses are RS-485 at tier `[C]` until Phase 1 confirms them (the steam link is `[A]`) | Two converters, ~$34    |

Waiting would not shrink either risk — resolving them takes the same capture
and pilot phases whichever order the parts arrive in. Waiting only serializes
the two tracks.

## The two tracks

```text
      Bench track                              Installation track
      -----------                              ------------------
 order Group A + instruments             Phase 0 — survey and recovery
 select the capture method                (labels, photographs, continuity,
 (Group D gates on that choice)            baseline; no purchases needed)
        |                                       |
        v                                       +--> closes most of Group B:
 bench layout with parts in hand                |    probes, glands, leads,
        |                                       |    connectors, blocks;
        +--> closes the layout rows of          |    electrician closes the
        |    Group B: enclosure, rail,          |    valve-power disconnects
        |    carrier, test posts                v
        v                                 Phase 1 — passive capture
 Phase 2 — bench acceptance               (Group D front end, receive-only)
 (HARDWARE.md §13) and the                      |
 seven-day soak, emulator only                  +--> fixtures promoted [C]→[A],
        |                                       |    opening the transmit gate;
        |                                       |    closes termination/bias
        +------------------+--------------------+
                           v
             Phase 3 — one-valve manual pilot
                           v
             Phase 4 — second valve, every outlet;
             K-99695 and K-99693 powered down
                           v
             Phase 5 — steam link
             (generator installed by others first)
                           v
             Phase 6 — local soak → signed report
```

The two tracks join at Phase 3, which needs both: a decoder proven against real
captures, and a bench-proven unit to connect.

## The sequence

### 1. Now, with nothing in hand

- The software is built and tested against emulated devices, and the transmit
  gate stays closed until fixtures are tier `[A]` —
  [controller/README.md](../README.md).
- **Select the passive-capture method** — Group D says select before buying.
  The front end must be physically receive-only and hardware-timestamped;
  the candidates and the wiring rules are in
  [SHOPPING-LIST.md § Group D](SHOPPING-LIST.md).
- Kohler case **#07797183** carries the open steam questions in parallel.

### 2. Order Group A and the instruments

Group A (~$233, four carts) plus the Therma K reference thermometer from
Group C — bench acceptance check 8 needs it, not just commissioning. The
Fluke, bench supply and soldering iron are bought only if not already owned.

### 3. Phase 0 — survey and recovery

No purchases required; a camera, labels, and the electrician's visit. Produces
the measurements that close most of Group B, and the recovery baseline.
Gate: the factory topology restores from the labels in under five minutes.

### 4. Order Group B as its rows close

Three closers, three moments:

| Rows                                                               | Closed by                             |
| ------------------------------------------------------------------ | ------------------------------------- |
| Enclosure, DIN rail, Pi carrier, test posts                        | Bench layout, once Group A is in hand |
| Probes, glands, terminal blocks, ferrules, cable, valve connectors | The Phase 0 survey                    |
| Termination and bias components                                    | The Phase 1 capture — likely **none** |
| Manual valve-power disconnects                                     | The electrician, after the nameplates |

### 5. Phase 1 — passive capture

The Group D front end, attached receive-only to one original bus at a time,
answers the twelve capture questions in
[DESIGN.md § Packet capture questions](DESIGN.md) and produces the golden
frames. Promoting fixtures from `[C]` to `[A]` is what allows the encoder to
ever open a real serial port. Gate: repeated captures decode without checksum
errors and produce the same state transitions.

### 6. Phase 2 — bench acceptance and the soak

All three converters on the Pi, nothing on the field side: the thirteen checks
of [HARDWARE.md § 13](HARDWARE.md), then seven days of continuous run against
the emulator, in the assembled enclosure. The bench work starts as soon as
Group A arrives; the gate is judged with the capture-derived fixtures in
place. Gate: every injected failure ends in `OFF` without an unallowlisted
write.

### 7. Phase 3 — one-valve manual pilot

The first water. Needs both tracks complete, the adapter leads built, the
disconnects fitted, and the Therma K on the outlet. Every fault path is
measured against the acceptance thresholds in
[DESIGN.md § Acceptance thresholds](DESIGN.md); a failure here rejects the
architecture, it does not get patched.

### 8. Phase 4 — second valve and every outlet

Repeat for the other valve, verify every outlet with the immersion probe,
run the rollback drill, power down the K-99695 and wall interface.

### 9. Phase 5 — steam

Waits on a generator and adapter installed by a professional (Group E is not
this project's to buy). Before committing to the permanent 12 V supply, the
bench measurement in [HARDWARE.md § 12](HARDWARE.md) confirms the rail on the
owned adapter with a current-limited supply. The phase ends by measuring the
hard case: pull the DTV+ link mid-session and record what the generator does.

### 10. Phase 6 — local-only soak, then integration

One week local-only, then explicit Homebridge/Worker commands, cache-only
status reads. Gate: a signed commissioning report and homeowner-visible
emergency instructions.
