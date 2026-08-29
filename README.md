# Kohler DTV+

Analysis of the Kohler DTV+ system controller (K-99695-NA / K-99693-P-NA), and a
working replacement interface for it.

> ⚠️ **Read [DISCLAIMER.md](DISCLAIMER.md) before running anything here.** This
> drives water temperature and flow on undocumented, unauthenticated hardware.
> Water above **43 °C / 109 °F can scald**, and some controller endpoints can
> brick the unit. Not affiliated with, endorsed by, or supported by Kohler Co.

## Why

The K-99693 wall interface on this system failed. The controller still reports
every other component as healthy — valve, amplifier, controller all `conn` — and
simply has nothing left to command it:

```
num_interface        = 0
ui1_con_string       = not_seen      <- the failed touchscreen
valve_1_con_string   = conn
amp_con_string       = conn
controller_con_string = conn
```

The controller exposes an undocumented CGI API that its own web pages use. That
is the replacement input.

## What's here

|                                                            |                                                                                                                                                                                                                           |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [app/](app/)                                               | React + Vite interface styled after the K-99693. Runs on a dev machine, a LAN box, or a phone browser.                                                                                                                    |
| [viewer/](viewer/)                                         | Separate 3D parts viewer and STL exporter for modification work. No hardware surface — it cannot touch the valve.                                                                                                         |
| [PROTOCOL.md](PROTOCOL.md)                                 | The controller's CGI API — transport quirks, endpoints, payload fields, safety ratings.                                                                                                                                   |
| [DESIGN.md](DESIGN.md)                                     | Architecture, decisions, testing, and what the Android/Capacitor port needs.                                                                                                                                              |
| [DISCLAIMER.md](DISCLAIMER.md)                             | Safety warnings, CGI risk scale, and how this repo enforces it.                                                                                                                                                           |
| [LICENSE.md](LICENSE.md)                                   | Open Maker License, plus what it does and does not cover.                                                                                                                                                                 |
| [CONTRIBUTING.md](CONTRIBUTING.md) / [CLA.md](CLA.md)      | How to contribute safely, and the contributor agreement.                                                                                                                                                                  |
| [AGENT.md](AGENT.md) / [CLAUDE.md](CLAUDE.md)              | Contract for agents working here, including the story-log convention.                                                                                                                                                     |
| [STORY-LOG.md](STORY-LOG.md)                               | Significant events and reversals, newest first.                                                                                                                                                                           |
| [INVESTIGATIONS.md](INVESTIGATIONS.md)                     | What we are still trying to find out, and the experiments queued to find it out. Includes the open investigation into the shower stopping mid-use.                                                                        |
| [docs/](docs/)                                             | Controller reference — hardware, firmware update pipeline, CGI surface, security posture, what firmware extraction would take, and the [replacement-controller design](docs/replacement-controller/CONTROLLER-DESIGN.md). |
| [research/SOURCES.md](research/SOURCES.md)                 | Monitoring index — where to sweep for new community findings.                                                                                                                                                             |
| [research/FIELD-NOTES.md](research/FIELD-NOTES.md)         | What breaks when you automate a DTV+ — failure reports from the community, sourced, with what we changed in response.                                                                                                     |
| [research/diagnostics/](research/diagnostics/)             | Raw controller captures, dated — error logs, idle baselines, and the extraction probe.                                                                                                                                    |
| [research/reference-links.md](research/reference-links.md) | Bibliography — projects, integrations, Kohler documents, FCC exhibits, patents, platform sources.                                                                                                                         |
| [research/controller-mirror/](research/controller-mirror/) | Verbatim mirror of the controller's own web UI, plus live payload captures.                                                                                                                                               |
| [research/xagon0/](research/xagon0/)                       | Vendored third-party analysis — see [PROVENANCE.md](research/xagon0/PROVENANCE.md).                                                                                                                                       |
| [research/reference/](research/reference/)                 | Kohler's user guide, rendered for interface reference.                                                                                                                                                                    |

## Quick start

```bash
cd app
npm install
npm run dev            # http://localhost:5180, and on your LAN IP
```

Set `KOHLER_HOST` if your controller is not at `192.168.0.115`.

```bash
npm test               # unit tests, no hardware
npm run selftest       # live checks, strictly read-only — never opens a valve
```

See [app/README.md](app/README.md) for hosting, the API surface, and the safety
gate.

The parts viewer is a separate app with its own dependencies and no connection
to the controller:

```bash
cd viewer
npm install
npm run dev            # http://localhost:5181
npm run check          # typecheck + tests + export gate + build
```

See [viewer/README.md](viewer/README.md) for why manufacturer CAD needs its
units declared, and what the K-99693 model turned out to be missing.

Formatting is repo-wide and mechanical, run from the root:

```bash
npm install
npm run format         # Markdown, TypeScript, CSS, YAML, HTML, JSON
npm run format:check   # what CI would check
```

## Safety gate

The controller has no authentication and exposes endpoints that can wipe or
brick it. Every known endpoint is rated 0-5 in
[app/server/cgi-safety.mjs](app/server/cgi-safety.mjs), and the proxy refuses
anything above **2/5** before a packet is sent. Only a subset of the ~50 known
endpoints is reachable; `reset_factory.cgi`, `clear_dt.cgi`, `fileupload.cgi`,
`unpack_bin.cgi`, `edit_dt.cgi`, `rpc.cgi` and friends are permanently
unreachable.

The live surface is the authority, not this page — `GET /api/safety` returns the
risk ceiling and the exact set of exposed endpoints.

## Hardware

<img src="Images/KohlerBoardOverall.webp" alt="Kohler DTV+ system controller circuit board, overall view" style="width:100%;">

The DTV+ system controller board — photograph by
[xagon0](https://github.com/xagon0/Kohler-DTV-Plus/blob/master/Images/Images.md),
re-encoded from the upstream 11 MB PNG to a 962 KB WebP at full 3710×2242
resolution (see [research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md)).
The controller
speaks RS-485 to the valve and amplifier, and HTTP/0.9-flavoured CGI to
everything else; [PROTOCOL.md](PROTOCOL.md) covers the latter.

## Credits

This repository began as a fork of
[timelery/Kohler-DTV-Plus](https://github.com/timelery/Kohler-DTV-Plus) (2017) by
Tim Elery — the original CGI enumeration and controller notes, which are
preserved in [PROTOCOL.md](PROTOCOL.md). It was detached from that fork network
on 2026-07-26, having diverged into a different project; the full history,
including Tim's original commits, is kept here intact.

- [xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) — CGI
  safety ratings, RS-485 protocol analysis, hardware and repair documentation.
- [dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python) — endpoint
  and parameter reference.
- Kohler's _User Guide — Digital Interface and System Controller for DTV+_
  (1241234-5-D) for the interface design.

## License

Work original to this repository is under the
[Open Maker License](https://github.com/aaronse/OpenMakerLicense) — AGPL-3.0 with
a maker addendum. Personal, educational, repair, and small-shop use are
explicitly allowed; hosted-service and commercial redistribution need a licence.

The third-party material vendored here — xagon0's analysis, Kohler's guide and
controller UI, and the content inherited from `timelery` — is **not** covered and
is **not** ours to license. See [LICENSE.md](LICENSE.md) for the scope table.
