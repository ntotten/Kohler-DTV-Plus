# docs/replacement-controller/ — the replacement master

## Document roles

| File                                         | Owns                                                           |
| -------------------------------------------- | -------------------------------------------------------------- |
| [CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md) | Architecture, safety rules, delivery phases                    |
| [HARDWARE-SPEC.md](HARDWARE-SPEC.md)         | The build: subsystems, GPIO map, power, enclosure, bench tests |
| [SHOPPING-LIST.md](SHOPPING-LIST.md)         | Parts, prices, purchase links, what cannot be ordered yet      |
| [STEAM-ADAPTER.md](STEAM-ADAPTER.md)         | Reference material on the K-1737-K1 and the DTV+ protocol      |

Measured hardware facts about the steam adapter live in
[research/reference/steam-adapter/](../../research/reference/steam-adapter/),
not here. This directory cites them.

## Three links, not two

Two Saturn valve buses and one DTV+ steam link. Steam is a first-class part of
the plan, not a future option. The generator behind the adapter is installed by
a professional and is **out of scope** — do not reintroduce its mains
requirements into these documents.

## Settled — do not re-litigate

Each of these was decided against a real alternative. Reopen only with new
evidence, and record why.

| Decision                                    | Reason                                                                                                               |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Raspberry Pi + Rust, not a bare-metal MCU   | Every binding deadline is ≥150 ms; the valve owns safety, so master determinism buys nothing the failure model needs |
| Three separate USB converters, not a HAT    | Waveshare's 2-CH HAT has one isolated supply and one digital isolator for both channels — one barrier, not two       |
| Our RS-485 termination stays **off**        | At 9600 baud over 25 ft, cable delay is ~1:2700 of the bit period. The adapter already terminates at 114 Ω           |
| Ground **connected** on the steam link only | Its `SMBJ28A` clamps are unidirectional, removing the negative common-mode range. Not optional there                 |
| Pin 4 of the DTV+ bus is driven by us       | The adapter's transceiver is bus-powered through `R16`; without it the link never comes up                           |
| Kohler's in-enclosure interface `WARNING`   | Accepted as a recorded deviation, operator decision 2026-08-29                                                       |

## I1 is closed

The replacement controller was **never** predicated on fixing I1, and I1 is now
resolved anyway — the cause was this project's own app. Do not describe the
K-99695 as unreliable; every lockup recorded here came from our HTTP clients
exceeding its documented limits.

## Writing here

Facts and specifications. Not a narrative of the research, not a record of
options considered. State the design, give the numbers, name a genuine unknown
once where it belongs, and move on. Safety facts belong in the safety sections
that already exist — do not sprinkle them through every other section.

Mark inference `[I]` every time. Evidence tiers follow
[system-specification.md](../system-specification.md): `[A]` ours/measured,
`[B]` shipped code, `[K]` Kohler primary, `[C]` reverse-engineered, `[?]`
unresolved, `[I]` inference.

Wrong turns stay in, per [AGENT.md](../../AGENT.md) rule 5 — struck through or
marked superseded, not deleted.
