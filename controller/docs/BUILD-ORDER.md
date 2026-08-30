# Build order

What happens in what order. The phase definitions and their gates are owned by
[DESIGN.md § Delivery phases](DESIGN.md); the parts by
[SHOPPING-LIST.md](SHOPPING-LIST.md); the wiring and bench tests by
[HARDWARE.md](HARDWARE.md). This document owns the sequencing.

## What this optimizes for

**Wall-clock time.** Shipping is the slow part of this project, so everything
orderable is ordered in one sitting, today. The only purchases that wait are
the two that physically cannot be made yet: the valve mating connectors — an
unknown part until the plugs are photographed — and any termination or bias
components, unknown until the bus is metered and most likely none. Nothing else
blocks on a measurement; commodity build hardware is bought generously sized
instead of measured first, accepting ~$50 of possible re-buys to skip a whole
order-and-wait cycle.

One risk is carried knowingly: if Phase 3 measures a fail-off latency over
30 s, the architecture is rejected and roughly $420 of hardware is sunk.
Waiting would not shrink that risk — resolving it takes the same phases in the
same order.

## The sequence

### 1. Today

- Place the order: everything in
  [SHOPPING-LIST.md § Order now](SHOPPING-LIST.md). Pick the capture method
  first — its line items depend on that choice.
- The software is already built and tested against emulated devices, and the
  transmit gate stays closed until fixtures are tier `[A]` —
  [controller/README.md](../README.md).

### 2. While the boxes ship — Phase 0, survey and recovery

No purchases needed: photographs, cable labels, the recovery baseline,
and the timed restore drill. Produces the connector photographs that close
the follow-up order. Gate: the factory topology restores from the labels in
under five minutes.

### 3. Order the connectors

The one follow-up order: valve mating connectors or pigtails, per the
photographs and an unpowered continuity check.

### 4. Boxes arrive — bench build, then Phase 2

Both converters on the Pi: bench checks 1–9 of [HARDWARE.md § 13](HARDWARE.md),
the enclosure build, then the seven-day soak against the emulator (checks
10–11). Gate: every injected failure ends in `OFF` without an unallowlisted
write.

### 5. Phase 1 — passive capture

The receive-only front end, attached to one original bus at a time, answers
the capture questions in [DESIGN.md § Packet capture questions](DESIGN.md) and
produces the golden frames. Promoting fixtures from `[C]` to `[A]` is what
ever allows a real serial port to open. Runs in parallel with the soak; both
must be complete before Phase 3. Gate: repeated captures decode without
checksum errors and produce the same state transitions.

### 6. Phase 3 — one-valve manual pilot

The first water: adapter leads built from the spool and connectors, a
thermometer on the outlet, every fault path measured against the acceptance
thresholds in [DESIGN.md § Acceptance thresholds](DESIGN.md). A failure here
rejects the architecture; it does not get patched.

### 7. Phase 4 — second valve and every outlet

Repeat for the other valve, verify every outlet with a thermometer, run the
rollback drill, power down the K-99695 and wall interface.

### 8. Phase 6 — local-only soak, then integration

One week local-only, then explicit Homebridge/Worker commands, cache-only
status reads. Gate: the commissioning record is complete.

Phase 5 was the steam link; steam is out of scope —
[DECISIONS.md D12](DECISIONS.md#d12--like-for-like-scope-no-added-equipment-no-steam-setup).
