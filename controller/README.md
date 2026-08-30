# controller — the replacement master

The Rust service that replaces the K-99695 as the master of this DTV+ system. It
runs on a Raspberry Pi 4 and drives three isolated serial links: two Saturn valve
buses and one DTV+ link to a K-1737-K1 steam adapter.

The design it implements is [docs/DESIGN.md](docs/DESIGN.md); the build it runs
on is [docs/HARDWARE.md](docs/HARDWARE.md); the order the work happens in is
[docs/BUILD-ORDER.md](docs/BUILD-ORDER.md). Read
[DISCLAIMER.md](../DISCLAIMER.md) first — this controls real water at real
temperatures.

## Status

**No valve traffic and no water actuation have been performed.** Every Saturn and
DTV+ frame here is tier `[C]`: derived from third-party reverse engineering and
unverified against this installation.

The service is built so that this is a property of the code rather than a note in
a README. See [the transmit gate](#the-transmit-gate) below.

## Quick start

Nothing here needs hardware.

```bash
./scripts/test.sh            # format, lint, tests, docs — what CI runs
./scripts/emulate.sh         # the whole system against emulated devices
./scripts/e2e.sh             # the end-to-end suite
./scripts/e2e.sh --docker    # the same, in the harness container
./scripts/e2e.sh --pi-sim    # the same, with the ARM64 binary under qemu
./scripts/build.sh --pi      # cross-compile for the Pi
./scripts/deploy.sh pi@host  # stage, validate on the target, then install
```

## The transmit gate

The gate is the single most important thing in this workspace, so it is worth
being precise about what it does.

Every wire fixture in `fixtures/` carries its provenance as a tier:

| Tier               | Meaning                                                       | Can open the gate |
| ------------------ | ------------------------------------------------------------- | ----------------- |
| `[A]` `Captured`   | Measured on this hardware, during Phase 1 packet capture      | Yes               |
| `[C]` `Documented` | From the vendored xagon0 reverse engineering, unverified here | No                |

Today every fixture is `[C]`. The gate is therefore **closed**, and it is enforced
at two boundaries:

1. `Encoder::new` requires a `TransmitAuthority`. No authority, no encoder, and
   therefore no frames at all.
2. `LinkFactory::open` refuses a real serial port unless that authority is
   `RealBusAttested`. Emulator and PTY backends open under either scope.

The second boundary is the one that matters. Gating only the encoder leaves a real
port open with a real `SerialStream` behind it, relying on nothing ever writing
bytes from another source.

So the daemon builds, boots, runs the entire emulated suite, and **cannot open a
real `/dev/ttyUSB*`**. Opening it needs a configuration change _and_ fixtures
promoted to `[A]`, both of which change the fixture-set hash — a dated,
reviewable act rather than a flag someone flips. CI asserts the gate is still
closed on every run.

**What a green suite does not prove.** The emulator is built from the same `[C]`
documents the encoder is. Agreement between them is internal consistency, not
evidence that either matches the valve. Phase 1 capture, Phase 2 bench and Phase 3
measurement are what close that gap.

## Layout

```text
controller/
  crates/
    kdtv-units       encodings and safety bounds — Cx2 vs Fx2, the clamps
    kdtv-proto       Saturn and DTV+ codecs, the allowlist, fixtures, the gate
    kdtv-config      typed configuration: parse, validate, refuse
    kdtv-telemetry   log schema, redaction, frame and session records
    kdtv-hal         I/O traits and their Linux implementations
    kdtv-safety      where water is authorised, and where it is stopped
    kdtv-engine      the sans-IO state machines, one per link
    kdtv-service     the tokio runtime and composition root
    kdtv-api         the local authenticated API and event stream
    kdtvd            the daemon binary
    kdtv-emulator    device models, wire simulator, e2e rig — never shipped
    xtask            repository automation
  docs/              the design documents — build order, design, hardware,
                     shopping list, decisions, steam reference
  fixtures/          golden frames, each with its provenance tier
  deploy/            the systemd unit and the two configurations
  docker/            the harness image
  commissioning/     test scripts and signed reports
```

`kdtv-emulator` and `xtask` are not dependencies of anything that ships. That is
asserted by a dependency-graph audit in CI, not by convention: the emulator is the
only crate that can build arbitrary or malformed frames, and keeping it out of the
daemon's graph is what stops that capability existing in production at all.

## Design notes worth knowing before reading the code

**Cx2 and Fx2 are unrelated types.** Valves speak Celsius times two; the steam
generator speaks Fahrenheit times two. There is no `From`, no `Deref` and no
shared trait between them, and exactly one conversion function, which `clippy.toml`
forbids calling outside the steam encoder. The hazard is concrete: Fx2 220 is
110 °F, and the same byte read as Cx2 asks a valve for 110 °C. Range checking does
not catch it, because both are in range for their own encoding.

**Denial is by absence, not by check.** An operation that must never happen has no
variant to spell it. Calibration writes, factory reset, bootloader, firmware
update, EEPROM access and flow calibration are not rejected at runtime — they
cannot be constructed. The one exception is documented where it lives: steam power
clean is `0xCC` in the _payload_ of an allowlisted command, so it is denied by the
operation-state enum having only `Off` and `On`, plus a byte-level test.

**Every failure path ends OFF.** Boot state is OFF and no water state is ever
persisted, so restart and watchdog reset cannot resume a session. Loss of a valid
response attempts all-off on the affected zone, closes that port and latches the
zone unavailable — and because the link is owned by value and `latch()` consumes
it, a partial escalation is not representable.

**A link fault takes one zone down.** Only a shared fault — the service, the
watchdog, the USB controller, a failed configuration check — takes both. The
scoping is an exhaustive match, so a new fault variant does not compile until its
scope is decided.

## Contradictions carried deliberately

The sources disagree in places, and this repository's rule is that inference is
marked rather than resolved quietly. Where the disagreement affects the wire, both
values are plumbed and neither is declared correct:

| Item                    | Positions                                                              | Handling                                                                    |
| ----------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Prompt 3 master address | `0x00` (DTV+) vs `0x10` (Prompt)                                       | configurable per zone, default `0x00`; investigation I5, capture question 1 |
| Saturn retries          | 3 vs 5                                                                 | configurable, default 3                                                     |
| DTV+ tick               | 150 ms vs 500 ms                                                       | configurable, marked `[?]`                                                  |
| DTV+ retries            | 4 vs 5                                                                 | configurable, marked `[?]`                                                  |
| Response timeout        | 400 ms vs 320 ms message timeout                                       | both plumbed                                                                |
| Prompt 3 timer refresh  | "any valid command resets it" vs "only a deliberate refresh"           | never send one; the valve timer is not counted as a backstop                |
| Saturn error codes      | two tables give codes 0, 1, 3, 7, 35, 36, 60, 71 incompatible meanings | the raw byte is carried; meaning requires naming a table                    |

There is deliberately **no echo timeout**. The stock master waits 20 ms for its own
transmission to return on the half-duplex bus; a converter with automatic direction
control presents no local echo, so there is no equivalent signal here. Direction is
inferred from address and content.

## Testing

| Ring | What runs                                                         | Where              |
| ---- | ----------------------------------------------------------------- | ------------------ |
| 1    | Pure state machines, deterministic, time as a parameter           | `cargo test`       |
| 2    | The real supervisor against simulated I/O and fixture sysfs trees | `cargo test`       |
| 3    | The real daemon binary over PTY pairs against emulated devices    | `./scripts/e2e.sh` |

End-to-end assertions run against the **transcript** — the bytes the daemon
actually transmitted — not against its own reported state. A service that believes
it is off while transmitting an open frame passes a state assertion and fails this
one.

Ring 3 opens valves, and that is the point: they are models in the test process,
behind pseudo-terminals. What no test can do is open a **real** one — the
transmit gate refuses a serial port while every fixture is tier `[C]`, and ring 3
asserts at the end of each run that the daemon opened pseudo-terminals and
nothing else. That is the same rule `npm test` follows in [app/](../app/),
stated as what it is rather than as "no test opens a valve", which stopped being
true the day the emulator gained device models.

The end-to-end suite skips, loudly, when `KDTV_E2E_DAEMON` is unset, so
`cargo test --workspace` stays green on a machine that has not built the daemon.
`./scripts/e2e.sh` sets it.
