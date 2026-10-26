# Diagrams

Every plan carries drawings a reviewer can check without reading the prose:
how the blocks connect, how anything physical is wired, and how data and
control move. The model is [OpenRX](https://github.com/OpenDrone-hw/OpenRX):
KiCad sources in the repository, rendered images in the documents, the power
tree as text, and part and pin tables.

## What a plan carries

| A plan that                       | Carries                                                                 |
| --------------------------------- | ----------------------------------------------------------------------- |
| Adds or changes hardware          | A block diagram, a schematic, a cabling drawing and a power tree        |
| Adds a circuit board to be made   | The board itself, its fabrication files, and a from-to wire list        |
| Changes how data or control moves | A sequence or flow diagram, and a state diagram for anything with modes |
| Changes a network                 | A topology diagram: addresses, and which side may open a connection     |

## Tools

| Drawing                               | Tool                                                                                             | Source in the repository                      | What GitHub shows                                                                  |
| ------------------------------------- | ------------------------------------------------------------------------------------------------ | --------------------------------------------- | ---------------------------------------------------------------------------------- |
| Block, flow, sequence, state, network | [Mermaid](https://mermaid.js.org)                                                                | A `mermaid` code block in the Markdown        | The diagram, in the file, the PR diff and a PR body                                |
| Schematic                             | [KiCad](https://www.kicad.org)                                                                   | `.kicad_sch` in a `hardware/` folder          | The SVG `kicad-cli` exports, committed beside the source                           |
| Circuit board                         | KiCad, laid out by a script, routed by [Freerouting](https://github.com/freerouting/freerouting) | `.kicad_pcb`, generated, beside the schematic | SVG renders, and a `fab/` folder: Gerber, drill, BOM, pick-and-place, assembly PDF |
| Cables, connectors, pinouts           | [WireViz](https://github.com/wireviz/WireViz)                                                    | `.wireviz.yml` beside the schematic           | The SVG it renders, committed beside the source                                    |
| Wire list                             | A script over the `.wireviz.yml`                                                                 | The `.wireviz.yml`                            | A CSV, one row per conductor                                                       |
| Power tree, pin map, part list        | Text                                                                                             | A `text` code block or a Markdown table       | As written                                                                         |

Mermaid is the default, because GitHub renders it: the source is the picture.
KiCad and WireViz need a render step, so their SVG is committed next to the
source and regenerated whenever the source changes. A board is never edited in
KiCad's editor: a script is its source, as it is the schematic's, so an agent
can change it and a reviewer can read the change. Nobody edits an SVG by
hand. No draw.io, Visio or screenshots of diagrams; photographs are for real
hardware.

## Rules

- **A drawing states nothing the text doesn't.** It shows the same facts, with
  the same evidence tiers: an inferred connection is marked `[I]` in the
  drawing too.
- **Render before pushing.** [`controller/hardware/tools/render.sh`](../controller/hardware/tools/render.sh)
  regenerates the schematic, the board, its fabrication files and the cabling,
  checks the schematic's netlist, the board's DRC and the wire list against the
  board, and fails if any Mermaid block in the repository doesn't parse. GitHub shows
  a broken Mermaid block as an error box, not a diagram.
- **Mermaid:** `flowchart`, `sequenceDiagram` and `stateDiagram-v2` only,
  because GitHub's renderer lags Mermaid's newest diagram types. Short labels,
  `<br/>` for a line break, a `subgraph` for a physical box or place.
- **A PR that changes a plan puts its main drawing in the PR description** as
  well, so the review starts from the picture.
