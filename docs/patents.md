# The DTV+ patent as documentation

Patents are public record, and the DTV+ system's core patent is unusually
informative: it describes the architecture Kohler actually built, in the
builder's own words, years before any community teardown.

## US 9,777,470 B2 — "Shower control system with network features"

- Assignee: **Kohler Co.** Filed 2015-07-20; priority chain back to
  2010-02-01 (via US 9,085,881 B2). Granted 2017-10-03.
- Full text: <https://patents.google.com/patent/US9777470B2/en>

What it documents, mapped to what we find in the field:

| Patent content                                                                                                                       | Observed system                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Central controller + one or more control panels + water/steam/audio/lighting/aromatherapy subsystems                                 | K-99695 controller + K-99693(-P) interfaces + valves + steam/rain/amp/LightBridge peripherals                                                     |
| Mixing valves with multiple outlet ports, independently controlled temperature zones                                                 | The DTV 6-port / Prompt 2-port / Prompt 3-port valves; two valve buses on the controller                                                          |
| Control panel with electronic display + capacitive touch, waterproof housing                                                         | The K-99693 wall interface                                                                                                                        |
| **Update data from "an Internet file server"** — firmware, control parameters, configuration, user interfaces, and _spa experiences_ | The FTP update pipeline (see [../docs/firmware-and-updates.md](firmware-and-updates.md)) — including the staged UI packs and hospitality channels |
| **Usage information collected by the controller and reported to the remote system**                                                  | The update/telemetry channel; also explains the field naming around upload status in `ftp_status.cgi`                                             |
| Spa experiences as stored multi-stage temperature/flow _profiles_                                                                    | The spa/therapy programs and multidimensional temperature profiles in the UI                                                                      |

The figures are a UI reference in their own right: the control-panel
cross-section (FIG. 3–4), the outlet-icon shower control screens
(FIG. 7–12), warm-up/purge (FIG. 13), steam (FIG. 14–15), lighting
(FIG. 16–18), audio (FIG. 19–20), spa (FIG. 21–22), user profiles
(FIG. 23), feature deactivation (FIG. 24), the shower-layout programming
screens (FIG. 25–28), and temperature-zone/therapy modules (FIG. 29–33).

## Why keep a copy

- **It is the only first-party architecture documentation that exists.**
  Kohler publishes nothing else at this depth.
- It settles terminology disputes (e.g. what "spa experiences" and
  temperature-zone control were meant to be).
- It is useful prior art context for anyone building compatible controllers
  or UIs — read it to understand the _claims_ before assuming a feature is
  free to reimplement. (Informational, not legal advice.)

## Related family

- [US 9,085,881 B2](https://patents.google.com/patent/US9085881B2/en) — the
  priority parent, same system lineage.
- Kohler Mira's digital-shower patents (e.g. USD758548S1 and family) cover
  the earlier Mira-era designs that the Saturn protocol descends from.
