# controller/hardware — the carrier: schematic, board and cabling

Revision B's carrier: a two-layer PCB that holds the Pico 2, the WIZ850io and
the five THVD1410 ports, and the cables between it, the K-99695, the valves and
the panel. What they draw is specified in [HARDWARE.md](../docs/HARDWARE.md);
where the two disagree, HARDWARE.md is right and the drawings are regenerated.

[![Carrier PCB, top](fab/carrier-top.svg)](fab/carrier-routing.svg)

Every file here is generated. The scripts in `tools/` are the source of the
schematic and of the board, and nobody edits either in KiCad: change a table at
the top of a script and run `tools/render.sh`, which regenerates everything and
checks it.

| File                                       | What it is                                                                                                |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| [carrier.svg](carrier.svg)                 | The schematic, rendered by KiCad                                                                          |
| [fab/](fab/README.md)                      | What to order and how to build it: Gerber and drill zip, BOM, pick-and-place, drawings                    |
| [cabling.svg](cabling.svg)                 | Every cable between the carrier, the K-99695, the valves and the panel                                    |
| [cabling-wires.csv](cabling-wires.csv)     | The same, one row per conductor, with the pin and signal at each end                                      |
| `carrier.kicad_sch`, `carrier.kicad_sym`   | The schematic and the project's own symbols: Pico 2, the WIZ850io's two rows, THVD1410                    |
| `carrier.kicad_pcb`                        | The board                                                                                                 |
| `carrier.kicad_pro`, `carrier.kicad_dru`   | The project: design rules, net classes and one custom DRC rule                                            |
| `carrier.pretty/`                          | The project's own footprints: the Pico 2 and the WIZ850io in their sockets, and the Würth valve jack      |
| `carrier.ses`                              | Freerouting's routing, kept so the board rebuilds without routing again                                   |
| `sym-lib-table`                            | Points KiCad at `carrier.kicad_sym`                                                                       |
| [cabling.wireviz.yml](cabling.wireviz.yml) | The cabling's source, for [WireViz](https://github.com/wireviz/WireViz) 0.4                               |
| `tools/carrier_schematic.py`               | Writes the schematic from its tables: GPIO map, jack pins, ports, parts and part numbers                  |
| `tools/check_netlist.py`                   | Checks the netlist KiCad reads back against those tables                                                  |
| `tools/carrier_footprints.py`              | Writes `carrier.pretty` from the Pico 2 datasheet, WIZnet's dimension drawing and Würth's jack data sheet |
| `tools/carrier_board.py`                   | Places, routes (replaying `carrier.ses`), pours ground and runs DRC, which must find nothing              |
| `tools/carrier_fab.py`                     | Writes `fab/`                                                                                             |
| `tools/wire_list.py`                       | Writes `cabling-wires.csv` and checks the cabling against the board's nets                                |
| `tools/render.sh`                          | Runs all of it in order, and checks every Mermaid diagram in the repository                               |

```bash
pip install wireviz==0.4.1           # needs Graphviz: brew install graphviz
controller/hardware/tools/render.sh  # KiCad 7; --check fails if anything committed is stale
```

`render.sh` needs `kicad-cli` and a Python that can import KiCad's `pcbnew`
(`KICAD_CLI` and `KICAD_PYTHON` override where it looks; on a Mac it finds
both inside KiCad.app). The committed files were made with KiCad 7.0.11, and
rerunning it with that version reproduces every one byte for byte; another
version makes the same board in different bytes.

Routing is the one step that isn't repeatable: Freerouting finds a different
route each run. So `carrier.ses` holds its result and every run replays it.
After a change to the placement or the netlist, route again with
`render.sh --reroute`, which needs Freerouting 2.4.1 (`FREEROUTING_JAR`) and
Java 25 (`JAVA`), and commit the new `carrier.ses` with the board.

[docs/diagrams.md](../../docs/diagrams.md) is the convention for every drawing
in this repository.
