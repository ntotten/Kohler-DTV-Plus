# Monitoring index

Places worth re-checking for DTV+ knowledge. Maintained so a periodic sweep is
mechanical rather than a fresh search each time.

Last full sweep: **2026-07-26**. Findings from that sweep are in
[FIELD-NOTES.md](FIELD-NOTES.md).

## How to sweep

```bash
# Activity since the last sweep, across every tracked repo.
for r in niemyjski/homeassistant-kohler dcmeglio/kohler-python \
         dcmeglio/hubitat-kohlerdtv xagon0/Kohler-DTV-Plus \
         timelery/Kohler-DTV-Plus yon/ha-kohler-anthem \
         kenyonj/kohler-konnect-ha; do
  echo "=== $r"
  gh api "repos/$r" --jq '"pushed=\(.pushed_at) open_issues=\(.open_issues_count)"'
  gh api "repos/$r/issues?state=all&since=2026-07-26&per_page=20" \
    --jq '.[] | "  #\(.number) [\(.state)] \(.title)"'
done
```

Then re-run the discovery searches in the last section to catch new projects.

---

## Tier 1 — check every sweep

The projects that talk to a **DTV+ over the local CGI API**. Same hardware, same
protocol, so their bugs are our bugs.

| Source                                                                              | Why                                                                                                                    | Watch for                                                                                                                      |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| [niemyjski/homeassistant-kohler](https://github.com/niemyjski/homeassistant-kohler) | The most active DTV+ integration, and the best bug tracker for this hardware. Actively maintained.                     | Lockups, polling values, outlet mapping, new state fields. `custom_components/kohler/coordinator.py` is the file that matters. |
| [dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python)                 | The Python library underneath it.                                                                                      | Endpoint or parameter changes.                                                                                                 |
| [dcmeglio/hubitat-kohlerdtv](https://github.com/dcmeglio/hubitat-kohlerdtv)         | Independent implementation by the same author. Quiet since 2020 but the Groovy driver is a useful second opinion.      | Divergence from the HA integration on protocol details.                                                                        |
| [xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus)                 | Deepest protocol and hardware analysis: RS-485, datatable, error codes, NAND recovery. Vendored at [xagon0/](xagon0/). | New docs; the board photo; anything on the Amulet UI protocol. Re-vendor if it moves.                                          |
| [timelery/Kohler-DTV-Plus](https://github.com/timelery/Kohler-DTV-Plus)             | This repo's origin. Low traffic but the issue tracker still collects owner reports.                                    | New reports of systems left stuck.                                                                                             |

## Tier 2 — check occasionally

**Cloud API** (Anthem / Konnect). Different product, different protocol — nothing
here can break a local DTV+ setup. Useful mainly as a contrast, and if a Konnect
module ever enters the picture.

| Source                                                                    | Notes                                                                       |
| ------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| [yon/ha-kohler-anthem](https://github.com/yon/ha-kohler-anthem)           | Azure AD B2C auth, Kohler backend RBAC changes, credential-capture tooling. |
| [yon/kohler-anthem](https://github.com/yon/kohler-anthem)                 | The library, with written postmortems under `working/findings/`.            |
| [kenyonj/kohler-konnect-ha](https://github.com/kenyonj/kohler-konnect-ha) | Good worked examples of diagnosing against live hardware.                   |

## Tier 3 — forums

Low signal-to-noise and mostly not machine-readable, but occasionally carry
owner reports that never reach GitHub.

| Source                                                                                                                                                                        | Notes                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| [Home Assistant community — "Anyone interface to Kohler DTV+?"](https://community.home-assistant.io/t/anyone-interface-to-kohler-dtv/38591)                                   | Thin as of the last sweep.                                                                                        |
| [Homey — DTV+ app thread](https://community.homey.app/t/kohler-dtv-smart-showering-system-steam-audio-lights-and-more/150733)                                                 | A local-API Homey app exists; no protocol detail published. Worth asking the author.                              |
| [HomeSeer — Kohler DTV thread](https://forums.homeseer.com/forum/general-home-automation/general-home-automation-hardware-discussion/29203-kohler-s-dtv-digital-shower/page2) | **403s to automated fetches** — open in a browser.                                                                |
| [C4 Forums — Kohler DTV Plus](https://www.c4forums.com/forums/topic/27930-kohler-dtv-plus/)                                                                                   | Control4 integrators; occasionally has install-side detail.                                                       |
| Home Depot / Build.com reviews for K-99693                                                                                                                                    | Owner reliability reports. **403s to automated fetches.** Weak evidence, but relevant to interface failure rates. |

## Kohler primary sources

| Source                                                                                                      | Notes                                                                       |
| ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| [User Guide 1241234-5-D](https://techcomm.kohler.com/techcomm/pdf/1241234-5.pdf)                            | The interface screens. Vendored at [reference/](reference/).                |
| [techcomm.kohler.com](https://techcomm.kohler.com/)                                                         | Service literature. Check for revisions to the guide and installation docs. |
| [DTV+ Konnect FAQ](https://resources.kohler.com/webassets/kpna/brochures/DTV+%20Konnect%20FAQ_EXTERNAL.pdf) | What the K-97999 module does and does not change.                           |

Worth periodically checking whether Kohler has published a **firmware update**
for the K-99695 — ours is `0.0.3.89`. Note that `check_updates.cgi` is rated 3/5
and blocked here; firmware work is a deliberate, separate exercise.

## Discovery searches

Re-run these to catch projects that did not exist at the last sweep:

- `Kohler DTV+ Home Assistant integration`
- `Kohler DTV+ CGI reverse engineering`
- `Kohler K-99695 controller API`
- `quick_shower.cgi` / `system_info.cgi` / `values.cgi` **(these endpoint names are the highest-signal search terms — they appear almost nowhere else)**
- GitHub code search: `quick_shower.cgi`, `valve1_outlet`, `kohler dtv`
- GitHub topic/keyword: `kohler`, sorted by recently updated

```bash
gh search code 'quick_shower.cgi' --limit 30
gh search repos 'kohler dtv' --sort updated --limit 30
```

## Our own telemetry

The most relevant source for this system is this system. See
[FIELD-NOTES.md](FIELD-NOTES.md) §6 for behaviour observed here and nowhere
else — a periodic scan should include our own captured traces, not just other
people's repositories.
