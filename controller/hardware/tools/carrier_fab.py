#!/usr/bin/env python3
"""Write the carrier's fabrication and reference files, fab/, from
carrier.kicad_pcb and the schematic's netlist.

  carrier-gerbers.zip   Gerber X2 for every layer, Excellon drill files and
                        the Gerber job file: what a board house asks for
  carrier-bom.csv       Bill of materials, one row per part number
  carrier-pos.csv       Pick-and-place (centroid) file, mm, top side
  carrier-assembly.pdf  Assembly drawing: outline, references, silkscreen
  carrier-top.svg       The top as it will be made: copper, pour, silkscreen
  carrier-bottom.svg    The bottom, mirrored, as seen from below
  carrier-routing.svg   Both copper layers without the ground pour, to read
                        the tracks
  carrier-box-fit.svg   The board in the Hammond box: walls, lid bosses,
                        part outlines and courtyards
  carrier.step          The bare board (outline, notch, holes) for mechanical
                        CAD; KiCad has no 3D models for the jacks or modules
  README.md             What to order, and these files

Every file is byte-for-byte reproducible with the same KiCad: export dates are
taken out, and the zip's entries carry a fixed time.

Usage: carrier_fab.py <netlist>
"""

from __future__ import annotations

import csv
import io
import re
import subprocess
import sys
import tempfile
import zipfile
from collections import Counter
from pathlib import Path

import pcbnew

sys.path.insert(0, str(Path(__file__).parent))
from carrier_board import (  # noqa: E402
    BOARD_IN_BOX, BOX_H, BOX_W, H, HOLES, NOTCH, PCB, REV, REV_DATE, RULES, W, Netlist,
)
from sexpr import dump, parse  # noqa: E402

HERE = Path(__file__).resolve().parent.parent
FAB = HERE / "fab"
KICAD_CLI = None  # set by main()

LAYERS = ["F.Cu", "B.Cu", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask",
          "Edge.Cuts"]

# Parts in the netlist that aren't bought on their own.
SAME_PART = {"J11": "J10"}  # the WIZ850io's second row of pins
NOT_PARTS = {"NT1": "copper on the board, joining DC− to ground in one place"}
# Parts whose value is a label, not a value: one BOM line for all of them.
LABEL_VALUES = {"TP": "Test point"}

# Hammond 1591ESBK inside height, from its drawing (SECTION A-A) [V].
BOX_DEPTH = 56.60


def cli(*args: str) -> None:
    subprocess.run([KICAD_CLI, *args], check=True, stdout=subprocess.DEVNULL)


# ---------------------------------------------------------------------------
# Taking the dates out
# ---------------------------------------------------------------------------

DATED = [
    (re.compile(r"^G04 Created by KiCad \(([^)]*)\) date .*\*$", re.M), r"G04 Created by KiCad (\1)*"),
    (re.compile(r"^%TF\.CreationDate,.*\*%\n", re.M), ""),
    (re.compile(r"^; DRILL file \{([^}]*)\} date .*$", re.M), r"; DRILL file {\1}"),
    (re.compile(r"^; #@! TF\.CreationDate,.*\n", re.M), ""),
    (re.compile(r'"CreationDate":\s*"[^"]*"'), f'"CreationDate": "{REV_DATE}T00:00:00Z"'),
    (re.compile(r"^<title>SVG Image created as (\S+) date .*</title>$", re.M), r"<title>\1</title>"),
    (re.compile(r"(FILE_NAME\('[^']*',')\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d"), rf"\g<1>{REV_DATE}T00:00:00"),
]


def undate(path: Path) -> None:
    text = path.read_text()
    for pattern, repl in DATED:
        text = pattern.sub(repl, text)
    path.write_text(text)


def undate_pdf(path: Path) -> None:
    """Same-length replacement, so the PDF's cross-reference offsets still hold."""
    data = path.read_bytes()
    stamp = ("D:" + REV_DATE.replace("-", "") + "000000").encode()
    data = re.sub(rb"D:\d{14}", lambda m: stamp[: len(m.group())], data)
    path.write_bytes(data)


def deterministic_zip(out: Path, files: list[Path]) -> None:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for f in sorted(files, key=lambda p: p.name):
            info = zipfile.ZipInfo(f.name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            z.writestr(info, f.read_bytes())
    out.write_bytes(buf.getvalue())


# ---------------------------------------------------------------------------
# The files
# ---------------------------------------------------------------------------


def gerbers(tmp: Path) -> list[Path]:
    out = tmp / "gerbers"
    out.mkdir()
    cli("pcb", "export", "gerbers", "--layers", ",".join(LAYERS), "--subtract-soldermask",
        "-o", f"{out}/", str(PCB))
    cli("pcb", "export", "drill", "--format", "excellon", "--excellon-units", "mm",
        "--excellon-separate-th", "--generate-map", "--map-format", "gerberx2",
        "-o", f"{out}/", str(PCB))
    files = sorted(p for p in out.iterdir() if p.is_file())
    for f in files:
        undate(f)
    deterministic_zip(FAB / "carrier-gerbers.zip", files)
    return files


def pos(tmp: Path) -> None:
    out = FAB / "carrier-pos.csv"
    cli("pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "both",
        "-o", str(out), str(PCB))


def bom(nl: Netlist) -> list[dict]:
    rows: dict[tuple, dict] = {}
    for ref, part in nl.parts.items():
        if ref in SAME_PART or ref in NOT_PARTS:
            continue
        f = part["fields"]
        refs = [ref] + [r for r, of in SAME_PART.items() if of == ref]
        value = LABEL_VALUES.get(re.match(r"[A-Z]+", ref).group(), part["value"])
        key = (part["dnp"], value, part["footprint"], f.get("MPN", ""))
        row = rows.setdefault(key, {
            "References": [], "Qty": 0, "Value": value,
            "Footprint": part["footprint"].split(":", 1)[1],
            "Manufacturer": f.get("Manufacturer", ""), "MPN": f.get("MPN", ""),
            "Supplier": f.get("Supplier", ""),
            "Fitted": "no (DNP)" if part["dnp"] else "yes",
            "Notes": "; ".join(v for k, v in sorted(f.items()) if k in ("Socket", "Mounting")),
        })
        row["References"] += refs
        row["Qty"] += 0 if part["dnp"] else 1

    def natural(r: str):
        m = re.match(r"([A-Z]+)(\d+)", r)
        return (m.group(1), int(m.group(2))) if m else (r, 0)

    out = []
    for row in rows.values():
        row["References"] = sorted(row["References"], key=natural)
        out.append(row)
    out.sort(key=lambda r: (r["Fitted"] != "yes", natural(r["References"][0])))
    for r in out:
        r["References"] = " ".join(r["References"])
    return out


def write_bom(rows: list[dict]) -> None:
    cols = ["Item", "Qty", "References", "Value", "Footprint", "Manufacturer", "MPN", "Supplier",
            "Fitted", "Notes"]
    buf = io.StringIO()
    w = csv.DictWriter(buf, fieldnames=cols, lineterminator="\n")
    w.writeheader()
    for i, r in enumerate(rows, start=1):
        w.writerow({"Item": i, **r})
    (FAB / "carrier-bom.csv").write_text(buf.getvalue())


def drawings(tmp: Path) -> None:
    def svg(name: str, layers: str, board: Path = PCB, mirror: bool = False) -> None:
        out = FAB / name
        args = ["pcb", "export", "svg", "--layers", layers, "--exclude-drawing-sheet",
                "--page-size-mode", "2", "-o", str(out)]
        cli(*args, *(["--mirror"] if mirror else []), str(board))
        undate(out)

    svg("carrier-top.svg", "F.Cu,F.SilkS,Edge.Cuts")
    svg("carrier-bottom.svg", "B.Cu,B.SilkS,Edge.Cuts", mirror=True)
    svg("carrier-box-fit.svg", "Edge.Cuts,Dwgs.User,F.Fab,F.CrtYd")
    # The routing view: the same board with its pours taken away.
    tree = parse(PCB.read_text())[0]
    bare = tmp / "carrier.kicad_pcb"
    bare.write_text(dump([x for x in tree if not (isinstance(x, list) and x and x[0] == "zone")]))
    svg("carrier-routing.svg", "F.Cu,B.Cu,F.SilkS,Edge.Cuts", bare)
    pdf = FAB / "carrier-assembly.pdf"
    cli("pcb", "export", "pdf", "--layers", "F.Fab,F.SilkS,Edge.Cuts", "--include-border-title",
        "--exclude-value", "-o", str(pdf), str(PCB))
    undate_pdf(pdf)
    step = FAB / "carrier.step"
    cli("pcb", "export", "step", "--board-only", "--force", "-o", str(step), str(PCB))
    undate(step)


# ---------------------------------------------------------------------------
# The README
# ---------------------------------------------------------------------------


def measure() -> dict:
    board = pcbnew.LoadBoard(str(PCB))
    tracks = [t for t in board.GetTracks() if t.GetClass() == "PCB_TRACK"]
    vias = [t for t in board.GetTracks() if t.GetClass() == "PCB_VIA"]
    pads = [p for fp in board.GetFootprints() for p in fp.Pads()]
    drills = Counter()
    for p in pads:
        if p.GetDrillSize().x:
            plated = p.GetAttribute() == pcbnew.PAD_ATTRIB_PTH
            drills[(round(pcbnew.ToMM(p.GetDrillSize().x), 2), plated)] += 1
    for v in vias:
        drills[(round(pcbnew.ToMM(v.GetDrillValue()), 2), True)] += 1
    smd = sorted({fp.GetReference() for fp in board.GetFootprints()
                  if fp.GetAttributes() & pcbnew.FP_SMD and fp.GetReference() not in NOT_PARTS})
    return {
        "min_track": min(pcbnew.ToMM(t.GetWidth()) for t in tracks),
        "min_drill": min(d for d, _ in drills),
        "drills": drills,
        "vias": len(vias),
        "smd": smd,
        "thickness": pcbnew.ToMM(board.GetDesignSettings().GetBoardThickness()),
    }


def readme(rows: list[dict], gerber_files: list[Path]) -> None:
    m = measure()
    plated = sum(n for (_d, p), n in m["drills"].items() if p)
    unplated = sum(n for (_d, p), n in m["drills"].items() if not p)
    sizes = ", ".join(f"{d:g}" for d in sorted({d for d, _ in m["drills"]}))
    fitted = sum(r["Qty"] for r in rows)
    lines = [
        f"# fab — carrier rev {REV}, ordering and reference files",
        "",
        "Generated by [`tools/carrier_fab.py`](../tools/carrier_fab.py) from",
        "[`carrier.kicad_pcb`](../carrier.kicad_pcb); `tools/render.sh` rewrites",
        "this whole folder. Change the scripts, never these files.",
        "",
        "## Ordering the board",
        "",
        "Upload `carrier-gerbers.zip` to any board house. It holds every layer as",
        "Gerber X2, the plated and unplated Excellon drill files, and the Gerber job",
        "file, which states the stack-up below for houses that read it.",
        "",
        "| Setting | Value |",
        "| --- | --- |",
        f"| Size | {W:g} × {H:g} mm, with a {NOTCH[1] - NOTCH[0]:g} × {NOTCH[2]:g} mm notch in the top edge for the box's middle lid boss |",
        "| Layers | 2 |",
        f"| Thickness | {m['thickness']:g} mm FR-4 |",
        "| Copper | 1 oz (35 µm) |",
        f"| Smallest track | {m['min_track']:.2f} mm (design rule {RULES['min_track_width']:g} mm) |",
        f"| Smallest clearance | {RULES['min_clearance']:g} mm |",
        f"| Holes | {plated} plated, {unplated} unplated; drills {sizes} mm |",
        "| Surface finish, mask, silkscreen | Any: lead-free HASL, any mask colour, silkscreen on top |",
        "",
        "Those limits are well inside the cheapest standard 2-layer service at the",
        "usual board houses.",
        "",
        "## Assembling it",
        "",
        f"`carrier-bom.csv` lists {len(rows)} lines, {fitted} parts fitted, with",
        "manufacturer part numbers and where each was priced. The `no (DNP)` row is",
        "the bias resistors, left empty until their values are chosen",
        "([HARDWARE.md, Termination and bias](../../docs/HARDWARE.md#termination-and-bias)).",
        f"Everything is through-hole except {', '.join(m['smd'])}, the THVD1410s",
        "(VSSOP-8, 0.65 mm pitch), which hand-solder with flux and a fine tip.",
        "`carrier-pos.csv` has every part's centre and rotation for a board house that",
        "assembles; `carrier-assembly.pdf` shows where each reference goes.",
        "",
        "The Pico 2 and the WIZ850io sit in sockets, not soldered to the board",
        "(the BOM's Notes column names the sockets). The panel's DC jack, J6, mounts",
        "in the box wall and reaches the board on two wires.",
        "",
        "## Fitting the box",
        "",
        f"The Hammond 1591ESBK is {BOX_W:g} × {BOX_H:g} mm inside and {BOX_DEPTH:g} mm deep",
        "(Hammond drawing 1591ES) **[V]**. The board sits "
        f"{BOARD_IN_BOX[1]:g} mm off the wall every cable",
        "comes through, with all five jack mouths and the Pico 2's USB socket on that",
        "edge, between the corner bosses and notched around the middle one.",
        "`carrier-box-fit.svg` draws the walls, the six lid bosses and every part's",
        "outline and courtyard; the bosses' diameter is scaled off the drawing, not",
        "dimensioned **[I]**. Four M2.5 holes "
        f"({', '.join(sorted(HOLES))}) take the nylon standoffs.",
        "",
        "## The files",
        "",
        "| File | What it is |",
        "| --- | --- |",
        "| `carrier-gerbers.zip` | " + ", ".join(f"`{f.name}`" for f in gerber_files) + " |",
        "| `carrier-bom.csv` | Bill of materials |",
        "| `carrier-pos.csv` | Pick-and-place: reference, value, footprint, x and y (mm), rotation, side |",
        "| `carrier-assembly.pdf` | Assembly drawing: outline, references, silkscreen, title block |",
        "| [`carrier-top.svg`](carrier-top.svg) | The top as made: copper with the ground pour, silkscreen |",
        "| [`carrier-bottom.svg`](carrier-bottom.svg) | The bottom, mirrored, as seen from below |",
        "| [`carrier-routing.svg`](carrier-routing.svg) | Both copper layers without the pour (top red, bottom blue) |",
        "| [`carrier-box-fit.svg`](carrier-box-fit.svg) | The board inside the box |",
        "| `carrier.step` | The bare board for mechanical CAD; KiCad's library has no models for the jacks or modules |",
        "",
    ]
    (FAB / "README.md").write_text("\n".join(lines))


def main() -> int:
    global KICAD_CLI
    import os
    import shutil

    KICAD_CLI = os.environ.get("KICAD_CLI") or shutil.which("kicad-cli")
    if not KICAD_CLI:
        raise SystemExit("kicad-cli not found; set KICAD_CLI")
    nl = Netlist(Path(sys.argv[1]))
    FAB.mkdir(exist_ok=True)
    for old in FAB.iterdir():
        old.unlink()
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        files = gerbers(tmp)
        pos(tmp)
        rows = bom(nl)
        write_bom(rows)
        drawings(tmp)
        readme(rows, files)
    print(f"fab: {len(files)} files in carrier-gerbers.zip, {len(rows)} BOM lines")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
