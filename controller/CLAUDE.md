# controller/ — the replacement master

The Rust workspace and its documentation. Everything about designing, building
and commissioning the replacement controller lives in this directory. Raw
evidence stays in [research/](../research/) and is cited, never copied.

## Document roles

| File                                           | Owns                                                                                                                          |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| [README.md](README.md)                         | The Rust workspace: crates, the transmit gate, testing rings                                                                  |
| [docs/BUILD-ORDER.md](docs/BUILD-ORDER.md)     | What happens in what order, and what is bought when                                                                           |
| [docs/DESIGN.md](docs/DESIGN.md)               | Architecture, safety rules, delivery phases                                                                                   |
| [docs/HARDWARE.md](docs/HARDWARE.md)           | The build: subsystems, GPIO map, power, enclosure, bench tests                                                                |
| [docs/SHOPPING-LIST.md](docs/SHOPPING-LIST.md) | Parts, prices, purchase links, what cannot be ordered yet                                                                     |
| [docs/DECISIONS.md](docs/DECISIONS.md)         | Settled decisions, rejected alternatives, superseded wrong turns                                                              |
| [docs/STEAM-ADAPTER.md](docs/STEAM-ADAPTER.md) | Reference material on the K-1737-K1 and the DTV+ protocol                                                                     |
| [requirements.toml](requirements.toml)         | The requirement register; [commissioning/CHECKLIST.md](commissioning/CHECKLIST.md) is generated from it by `cargo xtask reqs` |

Measured hardware facts about the steam adapter live in
[research/reference/steam-adapter/](../research/reference/steam-adapter/),
not here. This directory cites them.

## Three links, not two

Two Saturn valve buses and one DTV+ steam link. Steam is a first-class part of
the plan, not a future option. The generator behind the adapter is installed by
a professional and is **out of scope** — do not reintroduce its mains
requirements into these documents.

## Settled — do not re-litigate

Each of these was decided against a real alternative. The full records — the
alternatives, the evidence, and what would reopen each — are
[docs/DECISIONS.md](docs/DECISIONS.md). Reopen one only with new evidence, and
record why there.

| Decision                                    | Reason                                                                                                               |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Raspberry Pi + Rust, not a bare-metal MCU   | Every binding deadline is ≥150 ms; the valve owns safety, so master determinism buys nothing the failure model needs |
| Three separate USB converters, not a HAT    | Waveshare's 2-CH HAT has one isolated supply and one digital isolator for both channels — one barrier, not two       |
| Our RS-485 termination stays **off**        | At 9600 baud over 25 ft, cable delay is ~1:2700 of the bit period. The adapter already terminates at 114 Ω           |
| Ground **connected** on the steam link only | Its `SMBJ28A` clamps are unidirectional, removing the negative common-mode range. Not optional there                 |
| Pin 4 of the DTV+ bus is driven by us       | The adapter's transceiver is bus-powered through `R16`; without it the link never comes up                           |
| Kohler's in-enclosure interface `WARNING`   | Accepted as a recorded deviation, operator decision 2026-08-29                                                       |

## Writing here

Facts and specifications. Not a narrative of the research, not a record of
options considered — rejected alternatives and superseded turns go to
[docs/DECISIONS.md](docs/DECISIONS.md), so the plan documents state each
decision once and stay readable. Safety facts belong in the safety sections
that already exist; do not sprinkle them through every other section.

Mark inference `[I]` every time. Evidence tiers follow
[system-specification.md](../docs/system-specification.md): `[A]` ours/measured,
`[B]` shipped code, `[K]` Kohler primary, `[C]` reverse-engineered, `[?]`
unresolved, `[I]` inference.

Wrong turns stay in, per [AGENT.md](../AGENT.md) rule 5 — struck through or
marked superseded, not deleted.

Editing [requirements.toml](requirements.toml) means regenerating
[commissioning/CHECKLIST.md](commissioning/CHECKLIST.md) with
`cargo xtask reqs --checklist commissioning/CHECKLIST.md`; never edit the
checklist by hand.
