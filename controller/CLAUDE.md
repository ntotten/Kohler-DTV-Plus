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

## The scope is the swap

This project replaces the K-99695 driving the two valves that exist today —
nothing else. **Do not add installation equipment or requirements the stock
system does not have** (disconnects, sensors, posted procedures, instrument
mandates, electrician work items), and **do not raise steam**: the house has
no generator, steam setup is out of scope, and the dormant DTV+ code the
workspace carries is documented in [docs/HARDWARE.md § 12](docs/HARDWARE.md).
Operator decision 2026-08-30 — [docs/DECISIONS.md D12](docs/DECISIONS.md).

## Settled — do not re-litigate

Each of these was decided against a real alternative. The full records — the
alternatives, the evidence, and what would reopen each — are
[docs/DECISIONS.md](docs/DECISIONS.md). Reopen one only with new evidence, and
record why there.

| Decision                                     | Reason                                                                                                               |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Like-for-like scope; steam out of scope      | Operator decision 2026-08-30 — no added equipment, no steam setup. Do not re-raise. `D12`                            |
| Raspberry Pi + Rust, not a bare-metal MCU    | Every binding deadline is ≥150 ms; the valve owns safety, so master determinism buys nothing the failure model needs |
| Separate USB converters, never multi-channel | Every multi-channel candidate shares one isolation barrier between zones. `D3`                                       |

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
