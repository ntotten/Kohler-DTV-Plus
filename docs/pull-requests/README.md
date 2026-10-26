# Pull requests #1–#14

On 2026-10-26 this repository left the fork network of
[aaronse/Kohler-DTV-Plus](https://github.com/aaronse/Kohler-DTV-Plus), so that
it could store photos and other binary files in Git LFS: GitHub refuses LFS
uploads to a public fork whose network doesn't already use LFS. GitHub's
documentation says a repository that leaves its fork network loses its pull
requests, so the descriptions of the first fourteen are kept here, as they
stood that day. None of them had reviews or comments. The commits themselves
are unaffected; each merged one is on `main` under the hash given.

These files are a record and are not edited; `.oxfmtrc.json` leaves them out
of formatting.

| PR            | Title                                                                                                        | State                              |
| ------------- | ------------------------------------------------------------------------------------------------------------ | ---------------------------------- |
| [#1](001.md)  | Set up oxfmt and format the repository                                                                       | merged 2026-08-29 as `fcd5944`     |
| [#2](002.md)  | controller: replacement master software in Rust, with an emulated test harness                               | merged 2026-08-30 as `1bc4d6e`     |
| [#3](003.md)  | fix(safety): constrain CGI parameters, not just endpoints                                                    | merged 2026-08-30 as `aa8c0da`     |
| [#4](004.md)  | Make the requirement-traceability convention adoptable, and adopt it for 87 requirements                     | merged 2026-08-30 as `425dfca`     |
| [#5](005.md)  | docs: retire the agents/ diagnostics tree, and salvage its Phase-1 parts                                     | merged 2026-08-30 as `fb7f935`     |
| [#6](006.md)  | Move the controller docs into controller/, add a build order, drop the independent temperature sensor        | merged 2026-08-30 as `ed6b389`     |
| [#7](007.md)  | Saturn valve buses: captures of both valves, shower runs, and the capture tools                              | merged 2026-10-24 as `edcc525`     |
| [#8](008.md)  | kdtv-proto: the Saturn checksum includes the sync bytes                                                      | merged 2026-10-24 as `925829a`     |
| [#9](009.md)  | Saturn valve bus: valve 2's start-up, timer refresh, water commands and fail-off; raw trimmed to key moments | merged 2026-10-24 as `a3fa20a`     |
| [#10](010.md) | Saturn bus: pin 5 is supplied by the valve                                                                   | merged 2026-10-24 as `e459eb0`     |
| [#11](011.md) | research: manifest carries the README's new size and hash                                                    | merged 2026-10-25 as `9987a61`     |
| [#12](012.md) | Controller revision B: one RP2350 board in line with the valve cables, and a new wall panel                  | merged 2026-10-25 as `c878a8e`     |
| [#13](013.md) | Correct the parts list: micro-USB supply, DigiKey part numbers, spares, valve leads                          | open on 2026-10-26, head `2893a30` |
| [#14](014.md) | Draw the in-line controller: schematic, cabling, block and stage diagrams                                    | open on 2026-10-26, head `902a2fc` |
