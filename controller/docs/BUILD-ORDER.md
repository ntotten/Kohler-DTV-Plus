# Build order

What happens in what order. The phase definitions and their gates are owned by
[DESIGN.md § Delivery phases](DESIGN.md); the parts by
[SHOPPING-LIST.md](SHOPPING-LIST.md); the wiring and bench tests by
[HARDWARE.md](HARDWARE.md). This document owns the sequencing. Revision B,
2026-10-24: the controller goes in line with the valve cables,
[DECISIONS.md D13](DECISIONS.md#d13--in-line-through-the-k-99695-staged-no-bypass).

## What this optimizes for

**Wall-clock time, with the shower in service throughout.** Everything
orderable is ordered in one sitting, and the software port starts the same day,
because it needs no hardware. The in-line stages keep the K-99695 and the
K-99693 working until the new panel is ready, so the only time the shower is
out of service is the few minutes of each power-off plug move **[I]**.

The risk revision A carried — a fail-off over 30 s rejecting the architecture —
is mostly retired: the valve stopped the water about 2 s after the K-99695's
power was cut **[A]**
([2026-10-24 bus capture § Fail-off](../../research/diagnostics/2026-10-24-saturn-valve-bus/README.md#fail-off-a)).
What Phase 3 still has to show is the same result for the controller's own
fault paths. The hardware at stake is now under $100 **[I]**.

## The sequence

### 1. Today

- Place the order: everything in
  [SHOPPING-LIST.md § Order now](SHOPPING-LIST.md).
- Start the software port, step 2. It runs in parallel with everything up to
  step 5.

### 2. Port the software

The workspace moves from a Linux service to RP2350 firmware —
[DECISIONS.md D14](DECISIONS.md#d14--rp2350-firmware-in-rust-not-a-linux-computer).

- The logic crates (`kdtv-units`, `kdtv-proto`, `kdtv-safety`, `kdtv-engine`,
  `kdtv-config`, `kdtv-telemetry`) become `no_std` with `alloc`, still tested
  on the host.
- A firmware crate on Embassy replaces `kdtv-hal`, `kdtv-service`, `kdtv-api`
  and `kdtvd`: the five ports, pass-through, the per-valve state machines, the
  LAN API, the log ring buffer, the watchdog.
- The transmit gate gains a forwarding-only scope **[I]**: real ports open,
  every byte sent is a byte just received on the paired port, and no encoder
  exists. The encoder stays gated until fixtures are tier `[A]`; the
  2026-10-24 captures are the evidence that promotes them.
- [requirements.toml](../requirements.toml) is re-extracted from the revision B
  documents. Its Pi and USB entries (`SER`, `USB`, `SYSD`, `TIME`, `GPIO`,
  `PWR`, `TEST`, `EXCL`) describe revision A until then; where an entry and its
  source document disagree, the document is right, as the register's own
  header says.

Gate: the host test and e2e suites pass against the emulator, and the firmware
fits the board ([HARDWARE.md § 15](HARDWARE.md) item 10).

### 3. While the boxes ship — Phase 0, survey and recovery

No purchases needed: photographs, cable labels, the recovery baseline and the
timed restore drill, plus three looks that close orders and open items:

- a photograph of the plug on a valve cable, which closes the connector order;
- with both valves' power off and their cables out of the K-99695,
  continuity between pin 2 of its two valve jacks (item 8);
- the plug of the in-wall panel cable at the controller end, to count its
  wired contacts (item 13).

Gate: the factory topology restores from the labels in under five minutes.

### 4. Order the connectors

The one follow-up order, from the photograph: jacks for the valve cables, and
the two short leads to the K-99695. Add the isolated transceivers for one zone
only if item 8 found the two jacks' grounds separate.

The carrier PCB goes in with it: `carrier-gerbers.zip` from
[`controller/hardware/fab/`](../hardware/fab/README.md) to any board house
~~, and the four RJ12 jacks its footprints take (SHOPPING-LIST.md, The carrier
PCB)~~. Its valve jacks are the Würth 615006138421s already ordered, and it
waits until its footprints are theirs (SHOPPING-LIST.md, The carrier PCB).

### 5. Boxes arrive — bench build, then Phase 2

The board on its carrier with the Waveshare bench rig: bench checks 1–13 of
[HARDWARE.md § 13](HARDWARE.md), ending in the seven-day soak against the
emulator. Gate: every injected failure ends in `OFF` without an unallowlisted
write, and checks 1 and 4 pass.

### 6. Phase 1 — passive capture, finished from inside the path

Most of it is done. The 2026-10-24 scope sessions captured valve 2 through
start-up, idle, every water operation but massage, the runtime refresh and the
fail-off, and valve 1 through start-up, idle and its commands **[A]**. The rest
— valve 1's remaining operations, and the long runs — is recorded by the
controller itself in step 7's pass-through, timestamped by its own clock.
Gate: repeated captures decode without checksum errors and produce the same
state transitions.

### 7. Phase 3 — in line: pass-through, then one valve taken over

**3a, pass-through, both valves.** A power-off plug move puts the controller in
line. The valve-segment bias is set against the factory idle level
([HARDWARE.md § 5](HARDWARE.md)). The controller forwards and logs and
originates nothing, while the house uses the shower as normal through the
K-99693. Measured: the delay added, what reply delay the K-99695 tolerates
(item 6), and anything the K-99695 logs.

**3b, take-over, one valve.** The controller becomes that valve's master and
answers the K-99695 for it. Every fault path is measured against the acceptance
thresholds in [DESIGN.md § Acceptance thresholds](DESIGN.md), the K-99695's
response to being answered is recorded (item 9), and the rollback drill is run,
timed. A failure here rejects the stage; it does not get patched.

### 8. Phase 4 — second valve and every outlet

Take over the other valve, verify every outlet with a thermometer, run one zone
and then both, and run the rollback drill. The K-99695 stays powered: it is
still how the K-99693 reaches the controller.

### 9. Phase 6 — local-only soak, then integration

One week local-only, then explicit Homebridge/Worker commands through the LAN
API, with cache-only status reads. Gate: the commissioning record for the
in-line stages is complete.

### 10. The wall panel — in parallel, from step 5

Design and build the new panel
([DECISIONS.md D16](DECISIONS.md#d16--a-new-wall-panel-on-its-own-board-wired)),
then its bench checks P1–P4 against the controller on the bench
([HARDWARE.md § 16](HARDWARE.md)). It gates step 11 and nothing before it.

### 11. Phase 7 — cut-over

The panel goes onto the in-wall cable, the `K` leads come out and are stored
with the K-99695's labels, and the K-99695 and K-99693 are powered down. Gate:
P1–P4 pass in place, and the commissioning record is complete.

Phase 5 was the steam link; steam is out of scope —
[DECISIONS.md D12](DECISIONS.md#d12--like-for-like-scope-no-added-equipment-no-steam-setup).
