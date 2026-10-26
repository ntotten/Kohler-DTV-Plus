# controller/hardware — the carrier's drawings

The schematic and cabling for revision B, the protoboard carrier that holds the
Pico 2, the WIZ850io and the five THVD1410 ports. What they draw is specified
in [HARDWARE.md](../docs/HARDWARE.md); where the two disagree, HARDWARE.md is
right and the drawing is regenerated.

| File                                       | What it is                                                                  |
| ------------------------------------------ | --------------------------------------------------------------------------- |
| [carrier.svg](carrier.svg)                 | The schematic, rendered by KiCad                                            |
| `carrier.kicad_pro`, `carrier.kicad_sch`   | The KiCad project and its one sheet                                         |
| `carrier.kicad_sym`, `sym-lib-table`       | The project's own symbols: Pico 2, the WIZ850io's two headers, THVD1410     |
| [cabling.svg](cabling.svg)                 | Every cable between the carrier, the K-99695, the valves and the panel      |
| [cabling.wireviz.yml](cabling.wireviz.yml) | Its source, for [WireViz](https://github.com/wireviz/WireViz) 0.4           |
| `tools/carrier_schematic.py`               | Writes the schematic from the tables at its top: GPIO map, jack pins, ports |
| `tools/check_netlist.py`                   | Checks the netlist KiCad reads back against those tables                    |
| `tools/render.sh`                          | Runs both, renders both SVGs, and checks every Mermaid diagram in the repo  |

While the carrier is a protoboard, `carrier_schematic.py` is the schematic's
source: change its tables, run `tools/render.sh`, and commit the `.kicad_sch`
and the SVG together. Every connection is a net label on a short stub, so each
pin's net is printed beside it. When a carrier PCB starts
([HARDWARE.md § 15](../docs/HARDWARE.md) item 12) the sheet is edited in KiCad
from then on and the script is deleted.

```bash
pip install wireviz==0.4.1           # needs Graphviz: brew install graphviz
controller/hardware/tools/render.sh  # KiCad 7 or later; KICAD_CLI=… if not on PATH
```

[docs/diagrams.md](../../docs/diagrams.md) is the convention for every drawing
in this repository.
