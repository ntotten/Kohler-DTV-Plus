# CLAUDE.md

@AGENT.md

## Quick orientation

This repository drives a **Kohler DTV+ shower** whose K-99693 wall interface is
disconnected. It controls real water at real temperatures.

Before anything else, read [DISCLAIMER.md](DISCLAIMER.md). The short version:

- **Never issue a CGI endpoint rated above 2/5.** The gate in
  `app/server/cgi-safety.mjs` enforces it — do not weaken it.
- **Never open a valve without asking first**, every time. Read-only work is
  free; moving water is not.
- **Keep polling at 15 s idle / 5 s active, two HTTP sessions maximum.** Faster
  locks the controller up. **This is what caused [I1](INVESTIGATIONS.md#i1--the-shower-stops-mid-use)** —
  a month of investigation into a phantom valve fault that was our own app. See
  `research/FIELD-NOTES.md` §1 and [app/CLAUDE.md](app/CLAUDE.md).
- Water above **43 °C / 109 °F scalds**. The controller's configured maximum
  (113 °F here) is above that, so it is not a safe bound.

## How to write here

Facts and specifications, not a narrative of the research and not a record of
options considered. State the design, give the numbers, name a genuine unknown
once where it belongs, and move on.

- **This is an open project**, not a survey of one house. A specific
  installation is a reference configuration, not the subject. Do not frame
  documents around what happens to be installed.
- **Do not hedge or pad with risk warnings.** Safety facts belong in the safety
  sections that already exist. Do not gate requested work behind caveats.
- **Mark inference `[I]` every time.** Tiers are defined in
  [docs/system-specification.md](docs/system-specification.md): `[A]`
  ours/measured, `[B]` shipped code, `[K]` Kohler primary, `[C]`
  reverse-engineered, `[?]` unresolved, `[I]` inference.
- **Wrong turns stay in**, struck through or marked superseded rather than
  deleted — [AGENT.md](AGENT.md) rule 5.
- **`oxfmt` formats Markdown too**, including table alignment. Run
  `npm run format` from the repo root after editing any document.

## Directory guides

| Directory                                                                       | Covers                                                                               |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| [app/](app/CLAUDE.md)                                                           | The HTTP client, its hard limits, the CGI safety gate                                |
| [controller/](controller/CLAUDE.md)                                             | The replacement master — Rust workspace, design docs, build order, settled decisions |
| [research/reference/steam-adapter/](research/reference/steam-adapter/CLAUDE.md) | How to probe the adapter board without repeating mistakes                            |

## Where things stand

- **The replacement controller covers three links**: two Saturn valve buses and
  one DTV+ steam link. Steam is in scope; the generator behind the adapter is
  not.
- **[I3](INVESTIGATIONS.md#i3--valuescgi-intermittently-drops-a-healthy-valve),
  [I4](INVESTIGATIONS.md#i4--is-automatic-purge-on) and
  [I5](INVESTIGATIONS.md#i5--the-saturn-register-map-is-contradictory)** remain
  open.

## Story log

Append significant events to [STORY-LOG.md](STORY-LOG.md) — this work is being
documented for YouTube ([@azab2c](https://www.youtube.com/@azab2c)) and possibly
for Kohler technical support. See the Story log section of
[AGENT.md](AGENT.md).

## Commands

```bash
npm install && npm run format                     # repo root: format everything
npm run format:check                              # ...or just check

cd app
npm run dev                                       # http://localhost:5180
npm run check                                     # typecheck + unit tests + build
npm run selftest -- --api http://127.0.0.1:5180   # live, strictly read-only
```
