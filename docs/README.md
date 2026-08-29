# docs/ — controller documentation

Reference documentation for the DTV+ system controller itself: the hardware, the
update pipeline, the CGI surface, and what extracting its firmware would take.

Findings here were confirmed against a production K-99695 running `0.0.3.89`,
read-only and rate-limited, except where a line says otherwise. Unverified
claims are marked as such in place — see [AGENT.md](../AGENT.md) rule 4.

For the app's own view of the protocol, see [PROTOCOL.md](../PROTOCOL.md); for
open questions and queued experiments, [INVESTIGATIONS.md](../INVESTIGATIONS.md).

## Contents

| Document                                                                              | Description                                                                                                                |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| [system architecture](system-architecture.md)                                         | Components, topology, the three-protocol trap, and the two independent error surfaces                                      |
| [system specification](system-specification.md)                                       | Full evidence-tagged specification of this installation — models, controller, buses, protocols, valves, encodings          |
| [controller hardware](controller-hardware.md)                                         | K-99695 core, NAND layout, buses and ports, software stack, power, LED patterns, factory debug access, boot                |
| [protocols/overview](protocols/overview.md)                                           | Saturn, DTV+ and Amulet CRC side by side, and the temperature-encoding footgun                                             |
| [system security](security.md)                                                        | No-auth API, plaintext-FTP update pipeline, unsigned uploads, CVE posture, owner mitigations                               |
| [security report](security-report.md)                                                 | The same weaknesses as a structured findings report, with an owner checklist                                               |
| [public record](public-record.md)                                                     | FCC exhibit map for the product family and US9777470B2 as architecture documentation                                       |
| [fcc filings](fcc-filings.md)                                                         | The filings that matter, and why the Konnect module is a Linux-class computer                                              |
| [patents](patents.md)                                                                 | US 9,777,470 B2 read as documentation, and the rest of the family                                                          |
| [hardware](hardware.md)                                                               | Board layout, and the factory debug/service footprints (J201 BDM, J904 console, SW101/SW201)                               |
| [firmware update](firmware-update.md)                                                 | The update flow, the full `ftp_status.cgi` matrix including valve flash/EEPROM, and the 2026 server decommissioning        |
| [firmware and updates](firmware-and-updates.md)                                       | The firmware inventory, what `dtvplus2_app_*.S19` is and isn't, the update matrix, boot flow                               |
| [repair/firmware extraction](repair/firmware-extraction.md)                           | What the controller `.S19` is, why HTTP cannot serve it, the service survey, ruled-out approaches, and the remaining paths |
| [repair/firmware extraction notes](repair/firmware-extraction-notes.md)               | The earlier extraction write-up — target, live-unit confirmations, vectors                                                 |
| [repair/extraction plan](repair/extraction-plan.md)                                   | Extraction plan with the 2026-08-25 post-reboot probe results                                                              |
| [repair/recovery](repair/recovery.md)                                                 | Known-good topology and the restore procedure after a factory reset                                                        |
| [troubleshooting/errors and known issues](troubleshooting/errors-and-known-issues.md) | The two error surfaces, valve codes, controller log codes, failure modes, the mid-shower shutoff                           |
| [web-interface/cgi endpoints](web-interface/cgi-endpoints.md)                         | Endpoint reference, real `files.cgi` / `files_available.cgi` responses, HTTP/0.9 and HEAD transport notes                  |
| [web-interface/api reference](web-interface/api-reference.md)                         | Local API reference — quick start, conventions, rate limiting, method support, response formats                            |
| [web-interface/openapi.yaml](web-interface/openapi.yaml)                              | Machine-readable spec for the CGI surface                                                                                  |
| [devices/touchscreen UI](devices/touchscreen-ui.md)                                   | The UI device, and how to tell V1 from V2 from the version string                                                          |
| [devices/wall interface](devices/wall-interface.md)                                   | K-99693 / K-99693-P variants, the link that is not Ethernet, teardown caution                                              |
| [devices/valve control](devices/valve-control.md)                                     | Valve behaviour, and which safety behaviour is owned by the valve rather than the controller                               |
| [control-logic/temperature system](control-logic/temperature-system.md)               | Q-format conversions and safety limits                                                                                     |
| [control-logic/temperature safety](control-logic/temperature-safety.md)               | Where the protection actually lives — valve-side, controller-side, and rules for any replacement master                    |
| [replacement-controller/](replacement-controller/)                                    | Design and parts for a direct replacement controller                                                                       |
| [state/](state/)                                                                      | Point-in-time assessments of the repository                                                                                |

## Provenance

Six of these files began as copies of
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) documents and
carry our own additions on top. They are **not** verbatim, which is why they live
here rather than in the vendored tree:

| This file                                                                  | Upstream original                          | What we added                                                                                         |
| -------------------------------------------------------------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| [hardware.md](hardware.md)                                                 | `docs/hardware.md`                         | _Factory Debug and Service Access_ — identifying J201/J904/J903/SW101/SW201 from the board photograph |
| [firmware-update.md](firmware-update.md)                                   | `docs/firmware-update.md`                  | Hardcoded `8.8.8.8` resolver, the full `ftp_status.cgi` update matrix, 2026 server decommissioning    |
| [devices/touchscreen-ui.md](devices/touchscreen-ui.md)                     | `docs/devices/touchscreen-ui.md`           | _Identifying the hardware version_ — V1 vs V2 from the version string                                 |
| [devices/valve-control.md](devices/valve-control.md)                       | `docs/devices/valve-control.md`            | _Safety Ownership_ — valve-side vs controller-side, and the rules that follow for replacement masters |
| [web-interface/cgi-endpoints.md](web-interface/cgi-endpoints.md)           | `docs/web-interface/cgi-endpoints.md`      | Real `files.cgi` / `files_available.cgi` responses, `ftp_status` pointer, transport notes             |
| [control-logic/temperature-system.md](control-logic/temperature-system.md) | `docs/control-logic/temperature-system.md` | Fixed a pre-existing broken link                                                                      |

The unmodified upstream copies of all six remain in
[research/xagon0/](../research/xagon0/) — see
[its PROVENANCE.md](../research/xagon0/PROVENANCE.md) for the licensing caveat,
which applies to the inherited portions of these files too. Cross-references
here into documents we did not copy point back into that vendored tree.

The remaining documents are original to this project. Fourteen of them were
written elsewhere and only reached this repository on 2026-08-29:

- From the retired `ntotten/kohler-dtv` repository (commit `354759d`, published
  CC BY 4.0), which was reduced to a redirect on 2026-08-25 claiming its content
  had moved here — only part of it had:
  [system-architecture.md](system-architecture.md),
  [controller-hardware.md](controller-hardware.md),
  [protocols/overview.md](protocols/overview.md),
  [security-report.md](security-report.md),
  [fcc-filings.md](fcc-filings.md),
  [patents.md](patents.md),
  [firmware-and-updates.md](firmware-and-updates.md),
  [repair/firmware-extraction-notes.md](repair/firmware-extraction-notes.md),
  [troubleshooting/errors-and-known-issues.md](troubleshooting/errors-and-known-issues.md),
  [web-interface/api-reference.md](web-interface/api-reference.md),
  [devices/wall-interface.md](devices/wall-interface.md) and
  [control-logic/temperature-safety.md](control-logic/temperature-safety.md).
  Its `openapi.yaml` was byte-identical to
  [web-interface/openapi.yaml](web-interface/openapi.yaml) and was not copied twice.
- From the `ntotten/agents` diagnostics working directory:
  [system-specification.md](system-specification.md),
  [repair/extraction-plan.md](repair/extraction-plan.md) and
  [repair/recovery.md](repair/recovery.md), alongside the capture sets now in
  [research/diagnostics/](../research/diagnostics/).

Several of these predate the hardware work recorded in
[INVESTIGATIONS.md](../INVESTIGATIONS.md) and cover the same ground from an
earlier vantage point. Where two documents disagree, the one cited from
[PROTOCOL.md](../PROTOCOL.md) or [INVESTIGATIONS.md](../INVESTIGATIONS.md) is the
one that was checked against the controller.
