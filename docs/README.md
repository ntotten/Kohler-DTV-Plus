# docs/ — controller documentation

Reference documentation for the DTV+ system controller itself: the hardware, the
update pipeline, the CGI surface, and what extracting its firmware would take.

Findings here were confirmed against a production K-99695 running `0.0.3.89`,
read-only and rate-limited, except where a line says otherwise. Unverified
claims are marked as such in place — see [AGENT.md](../AGENT.md) rule 4.

For the app's own view of the protocol, see [PROTOCOL.md](../PROTOCOL.md); for
open questions and queued experiments, [INVESTIGATIONS.md](../INVESTIGATIONS.md).

## Contents

| Document | Description |
| --- | --- |
| [system security](security.md) | No-auth API, plaintext-FTP update pipeline, unsigned uploads, CVE posture, owner mitigations |
| [public record](public-record.md) | FCC exhibit map for the product family and US9777470B2 as architecture documentation |
| [hardware](hardware.md) | Board layout, and the factory debug/service footprints (J201 BDM, J904 console, SW101/SW201) |
| [firmware update](firmware-update.md) | The update flow, the full `ftp_status.cgi` matrix including valve flash/EEPROM, and the 2026 server decommissioning |
| [repair/firmware extraction](repair/firmware-extraction.md) | What the controller `.S19` is, why HTTP cannot serve it, the service survey, ruled-out approaches, and the remaining paths |
| [web-interface/cgi endpoints](web-interface/cgi-endpoints.md) | Endpoint reference, real `files.cgi` / `files_available.cgi` responses, HTTP/0.9 and HEAD transport notes |
| [web-interface/openapi.yaml](web-interface/openapi.yaml) | Machine-readable spec for the CGI surface |
| [devices/touchscreen UI](devices/touchscreen-ui.md) | The UI device, and how to tell V1 from V2 from the version string |
| [devices/valve control](devices/valve-control.md) | Valve behaviour, and which safety behaviour is owned by the valve rather than the controller |
| [control-logic/temperature system](control-logic/temperature-system.md) | Q-format conversions and safety limits |
| [state/](state/) | Point-in-time assessments of the repository |

## Provenance

Six of these files began as copies of
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) documents and
carry our own additions on top. They are **not** verbatim, which is why they live
here rather than in the vendored tree:

| This file | Upstream original | What we added |
| --- | --- | --- |
| [hardware.md](hardware.md) | `docs/hardware.md` | *Factory Debug and Service Access* — identifying J201/J904/J903/SW101/SW201 from the board photograph |
| [firmware-update.md](firmware-update.md) | `docs/firmware-update.md` | Hardcoded `8.8.8.8` resolver, the full `ftp_status.cgi` update matrix, 2026 server decommissioning |
| [devices/touchscreen-ui.md](devices/touchscreen-ui.md) | `docs/devices/touchscreen-ui.md` | *Identifying the hardware version* — V1 vs V2 from the version string |
| [devices/valve-control.md](devices/valve-control.md) | `docs/devices/valve-control.md` | *Safety Ownership* — valve-side vs controller-side, and the rules that follow for replacement masters |
| [web-interface/cgi-endpoints.md](web-interface/cgi-endpoints.md) | `docs/web-interface/cgi-endpoints.md` | Real `files.cgi` / `files_available.cgi` responses, `ftp_status` pointer, transport notes |
| [control-logic/temperature-system.md](control-logic/temperature-system.md) | `docs/control-logic/temperature-system.md` | Fixed a pre-existing broken link |

The unmodified upstream copies of all six remain in
[research/xagon0/](../research/xagon0/) — see
[its PROVENANCE.md](../research/xagon0/PROVENANCE.md) for the licensing caveat,
which applies to the inherited portions of these files too. Cross-references
here into documents we did not copy point back into that vendored tree.

The remaining documents are original to this project.
