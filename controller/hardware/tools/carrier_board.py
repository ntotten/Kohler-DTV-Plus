#!/usr/bin/env python3
"""Lay out the carrier PCB, carrier.kicad_pcb, from the schematic's netlist.

The schematic (carrier_schematic.py) says what connects to what and which
footprint each part takes; this script says where each part goes, draws the
board edge and the silkscreen, routes, pours ground and runs KiCad's DRC,
which must find nothing. Nobody edits the board in KiCad: change the tables
below and run render.sh.

Routing is Freerouting's, and Freerouting is not deterministic, so its result
is kept in carrier.ses and replayed. `--reroute` routes again (needs Java 25
and Freerouting 2.4.1, FREEROUTING_JAR) and replaces carrier.ses; do that
whenever placement or the netlist changes, and commit the two together.

Usage: carrier_board.py <netlist> [--reroute]

Coordinates are millimetres from the board's top-left corner, y down, with the
top edge against the Hammond 1591ESBK's wall: every cable enters through that
wall, so every jack's mouth and the Pico 2's USB socket sit on the top edge.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import pcbnew

sys.path.insert(0, str(Path(__file__).parent))
from carrier_schematic import PORTS  # noqa: E402
from sexpr import find, find1, parse  # noqa: E402

HERE = Path(__file__).resolve().parent.parent
PCB = HERE / "carrier.kicad_pcb"
SES = HERE / "carrier.ses"
FP_DIRS = [
    os.environ.get("KICAD_FOOTPRINT_DIR", ""),
    os.environ.get("KICAD7_FOOTPRINT_DIR", ""),
    "/usr/share/kicad/footprints",
    "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints",
]

# ---------------------------------------------------------------------------
# The board and the box
# ---------------------------------------------------------------------------

W, H = 160.0, 60.0

# Hammond 1591ESBK, inside: 183.52 x 103.53 mm, lid-screw bosses 7.66 mm in
# from each corner and two more 80.10 mm along each long wall from the corner
# bosses (Hammond drawing 1591ES, rev 07.01.2020) [V]. The bosses' diameter
# isn't dimensioned; 6.2 mm is scaled off the drawing [I]. Card-guide ribs
# stand 1.0 mm off every wall [V].
BOX_W, BOX_H = 183.52, 103.53
BOSS_IN, BOSS_MID, BOSS_R = 7.66, 80.10, 3.1
# Where the board sits in the box: 1.5 mm off the jack wall, clear of the
# ribs, and between the corner bosses.
BOARD_IN_BOX = (11.75, 1.5)

# The middle boss on the jack wall, cleared by a notch in the top edge.
NOTCH = (72.0, 80.0, 10.5)  # x from, x to, depth

# Mounting holes for the M2.5 nylon standoffs (SHOPPING-LIST.md).
HOLES = {"H1": (3.5, 56.5), "H2": (76.0, 15.0), "H3": (156.5, 4.5), "H4": (156.5, 56.5)}

# The jacks, turned 180° so their mouths face the wall. In KiCad's footprints
# the mouth is 16.77 mm (54601, RJ12) and 13.97 mm (54602, RJ45) from pin 1,
# so these rows put each mouth on the board edge (the RJ45's 0.43 mm back from
# it, so its silkscreen stays on the board).
RJ12_Y, RJ45_Y = 16.77, 14.4
JACK = {"K1": ("J1", 12.0), "V1": ("J2", 28.0), "K2": ("J3", 47.0), "V2": ("J4", 63.0),
        "P": ("J5", 96.0)}
# Where each footprint's body is centred left to right, from pin 1.
BODY_CENTRE = {"RJ12": -3.18, "RJ45": -4.445}

# The two modules, by their pin 1: the Pico 2's USB socket and the WIZ850io's
# RJ45 both reach the top edge (carrier_footprints.py has the offsets).
PICO = (104.11, 2.67)
WIZ_J1 = (128.34, 9.1)
WIZ_J2 = (WIZ_J1[0] + 20.32, WIZ_J1[1])

# Each port's parts, in a column under its jack. y of each row.
ROW = {"R_DE": 24.5, "R_TERM": 28.5, "R_UP": 32.5, "R_DN": 36.5, "U": 42.0, "C": 47.0,
       "TP": 51.0}
TP_PITCH = 4.0
J6_AT = (69.5, 54.0)
NT1_AT = (79.0, 54.0)

# The revision printed on the board and in every fabrication file's title block.
REV, REV_DATE = "B", "2026-10-26"

# Silkscreen labels: HARDWARE.md §10.
LABELS = {
    "K1": ["TO KOHLER VALVE 1"],
    "V1": ["VALVE 1 · 6-PORT"],
    "K2": ["TO KOHLER VALVE 2"],
    "V2": ["VALVE 2 · 3-PORT PROMPT"],
    "P": ["PANEL · NOT ETHERNET", "DC ON PAIRS"],
}

# KiCad's DRC checks each footprint against the libraries it came from; the
# board is built from them by this script, so it can't drift, and the check
# would only need KiCad's library tables set up wherever this runs.
SEVERITIES = {"lib_footprint_issues": "ignore", "lib_footprint_mismatch": "ignore"}

# Design rules, for any 2-layer, 1 oz board house.
RULES = {
    "min_clearance": 0.2,
    # Freerouting necks a track down to pass between the THVD1410's 0.65 mm
    # pins; 0.15 mm is still inside any board house's limits.
    "min_track_width": 0.15,
    "min_via_diameter": 0.6,
    "min_through_hole_diameter": 0.3,
    "min_copper_edge_clearance": 0.5,
}
# Custom rules, written to carrier.kicad_dru. A through-hole pad meets the
# ground pour on both layers, so one spoke on a layer where tracks crowd it
# still leaves it well connected.
DRU = """(version 1)
(rule "through-hole pads connect on both layers"
  (constraint min_resolved_spokes 1)
  (condition "A.Type == 'Pad' && A.Pad_Type == 'Through-hole'"))
"""
NETCLASSES = [
    # name, track, clearance, via diameter, via drill, patterns
    ("Default", 0.25, 0.2, 0.8, 0.4, []),
    ("Power", 0.5, 0.2, 0.8, 0.4, ["+3V3", "GND"]),
    ("Bus", 0.3, 0.2, 0.8, 0.4, ["/*_A", "/*_B"]),
    # Pin 5's current is not measured (HARDWARE.md §5): 0.8 mm carries amps.
    ("Supply", 0.8, 0.25, 0.8, 0.4, ["/VALVE*_12V7", "/PANEL_DC*"]),
]


def mm(v: float) -> int:
    return pcbnew.FromMM(v)


def pt(x: float, y: float) -> pcbnew.VECTOR2I:
    return pcbnew.VECTOR2I(mm(x), mm(y))


# ---------------------------------------------------------------------------
# The netlist
# ---------------------------------------------------------------------------


class Netlist:
    def __init__(self, path: Path) -> None:
        tree = parse(path.read_text())[0]
        self.parts: dict[str, dict] = {}
        for comp in find(find1(tree, "components"), "comp"):
            ref = find1(comp, "ref")[1]
            fields = {}
            fs = find1(comp, "fields")
            for f in find(fs, "field") if fs else []:
                fields[find1(f, "name")[1]] = f[2] if len(f) > 2 else ""
            dnp = any(find1(p, "name") and find1(p, "name")[1] == "dnp"
                      for p in find(comp, "property"))
            self.parts[ref] = {
                "value": find1(comp, "value")[1],
                "footprint": find1(comp, "footprint")[1],
                "fields": fields,
                "dnp": dnp,
            }
        self.pin_net: dict[tuple[str, str], str] = {}
        for net in find(find1(tree, "nets"), "net"):
            name = find1(net, "name")[1]
            for node in find(net, "node"):
                ref, pin = find1(node, "ref")[1], find1(node, "pin")[1]
                if not ref.startswith("#"):
                    self.pin_net[(ref, pin)] = name

    def nets_of(self, ref: str) -> set[str]:
        return {n for (r, _p), n in self.pin_net.items() if r == ref}

    def find(self, prefix: str, nets: set[str]) -> str:
        hits = [r for r in self.parts if re.fullmatch(prefix + r"\d+", r) and self.nets_of(r) == nets]
        if len(hits) != 1:
            raise SystemExit(f"expected one {prefix} on {sorted(nets)}, found {hits}")
        return hits[0]


# ---------------------------------------------------------------------------
# Placement
# ---------------------------------------------------------------------------


def placement(nl: Netlist) -> dict[str, tuple[float, float, float]]:
    """ref -> (x, y, rotation) of the footprint's origin."""
    at: dict[str, tuple[float, float, float]] = {
        "A1": (*PICO, 0),
        "J10": (*WIZ_J1, 0),
        "J11": (*WIZ_J2, 0),
        "J6": (*J6_AT, 0),
        "NT1": (*NT1_AT, 0),
    }
    for k, spec in enumerate(PORTS, start=1):
        port = spec["port"]
        jack, x = JACK[port]
        kind = "RJ45" if port == "P" else "RJ12"
        at[jack] = (x, RJ45_Y if kind == "RJ45" else RJ12_Y, 180)
        cx = x + BODY_CENTRE[kind]
        u = nl.find("U", {f"/{port}_RX", f"/{port}_DE", f"/{port}_TX", f"/{port}_A",
                          f"/{port}_B", "GND", "+3V3"})
        at[u] = (cx, ROW["U"], 180)
        at[f"C{k}"] = (cx - 1.25, ROW["C"], 0)
        at[nl.find("R", {f"/{port}_DE", "GND"})] = (cx - 5.08, ROW["R_DE"], 0)
        at[nl.find("R", {f"/{port}_A", f"/{port}_B"})] = (cx - 5.08, ROW["R_TERM"], 0)
        if spec["kind"] in ("V", "P"):
            at[nl.find("R", {"+3V3", f"/{port}_A"})] = (cx - 5.08, ROW["R_UP"], 0)
            at[nl.find("R", {f"/{port}_B", "GND"})] = (cx - 5.08, ROW["R_DN"], 0)
        if spec["kind"] in ("K", "V"):
            for i, pin_no in enumerate((2, 3, 4)):
                tp = [r for r, p in nl.parts.items()
                      if r.startswith("TP") and p["value"] == f"{port}-{pin_no}"][0]
                at[tp] = (cx + TP_PITCH * (i - 1), ROW["TP"], 0)
    missing = sorted(set(nl.parts) - set(at))
    if missing:
        raise SystemExit(f"no placement for {missing}")
    return at


def port_centre(port: str) -> float:
    jack, x = JACK[port]
    return x + BODY_CENTRE["RJ45" if port == "P" else "RJ12"]


# ---------------------------------------------------------------------------
# Building the board
# ---------------------------------------------------------------------------


def lib_dir(lib: str) -> Path:
    if lib == "carrier":
        return HERE / "carrier.pretty"
    for d in FP_DIRS:
        if d and (Path(d) / f"{lib}.pretty").is_dir():
            return Path(d) / f"{lib}.pretty"
    raise SystemExit(f"footprint library {lib} not found; set KICAD_FOOTPRINT_DIR")


def load_fp(fpid: str) -> pcbnew.FOOTPRINT:
    lib, name = fpid.split(":", 1)
    fp = pcbnew.FootprintLoad(str(lib_dir(lib)), name)
    if fp is None:
        raise SystemExit(f"footprint {fpid} not found")
    fp.SetFPID(pcbnew.LIB_ID(lib, name))
    return fp


def segment(board, layer, x1, y1, x2, y2, width=0.1):
    s = pcbnew.PCB_SHAPE(board)
    s.SetShape(pcbnew.SHAPE_T_SEGMENT)
    s.SetStart(pt(x1, y1))
    s.SetEnd(pt(x2, y2))
    s.SetLayer(layer)
    s.SetWidth(mm(width))
    board.Add(s)


def circle(board, layer, x, y, r, width=0.1):
    s = pcbnew.PCB_SHAPE(board)
    s.SetShape(pcbnew.SHAPE_T_CIRCLE)
    s.SetCenter(pt(x, y))
    s.SetEnd(pt(x + r, y))
    s.SetLayer(layer)
    s.SetWidth(mm(width))
    board.Add(s)


def text(board, s, x, y, size=1.0, layer=pcbnew.F_SilkS, bold=False, align="center"):
    t = pcbnew.PCB_TEXT(board)
    t.SetText(s)
    t.SetPosition(pt(x, y))
    t.SetLayer(layer)
    t.SetTextSize(pcbnew.VECTOR2I(mm(size), mm(size)))
    t.SetTextThickness(mm(size * (0.2 if bold else 0.15)))
    t.SetHorizJustify({"center": pcbnew.GR_TEXT_H_ALIGN_CENTER,
                       "left": pcbnew.GR_TEXT_H_ALIGN_LEFT,
                       "right": pcbnew.GR_TEXT_H_ALIGN_RIGHT}[align])
    board.Add(t)
    return t


def outline(board) -> None:
    x0, x1, d = NOTCH
    pts = [(0, 0), (x0, 0), (x0, d), (x1, d), (x1, 0), (W, 0), (W, H), (0, H), (0, 0)]
    for (ax, ay), (bx, by) in zip(pts, pts[1:]):
        segment(board, pcbnew.Edge_Cuts, ax, ay, bx, by, 0.05)


def box_drawing(board) -> None:
    """The box's inside wall and lid bosses on User.Drawings, for the fit check."""
    ox, oy = BOARD_IN_BOX
    corners = [(-ox, -oy), (BOX_W - ox, -oy), (BOX_W - ox, BOX_H - oy), (-ox, BOX_H - oy)]
    for (ax, ay), (bx, by) in zip(corners, corners[1:] + corners[:1]):
        segment(board, pcbnew.Dwgs_User, ax, ay, bx, by, 0.15)
    for bx in (BOSS_IN, BOSS_IN + BOSS_MID, BOX_W - BOSS_IN):
        for by in (BOSS_IN, BOX_H - BOSS_IN):
            circle(board, pcbnew.Dwgs_User, bx - ox, by - oy, BOSS_R, 0.15)
    text(board, "Hammond 1591ESBK, inside wall; circles: lid-screw bosses",
         BOX_W / 2 - ox, BOX_H - oy - 4, 1.5, pcbnew.Dwgs_User)
    text(board, "Every cable enters through this wall", W / 2, -oy - 3, 1.5, pcbnew.Dwgs_User)


def silkscreen(board, nl: Netlist) -> None:
    for spec in PORTS:
        port = spec["port"]
        cx = port_centre(port)
        jack = JACK[port][0]
        lines = LABELS[port]
        top = 19.9 if len(lines) == 1 else 19.6
        text(board, f"{port} · {jack}", cx, top, 1.5, bold=True)
        for i, line in enumerate(lines):
            text(board, line, cx, top + 2.0 + 1.0 * i, 0.8)
        if spec["kind"] in ("K", "V"):
            for i, pin_no in enumerate((2, 3, 4)):
                text(board, f"{port}-{pin_no}", cx + TP_PITCH * (i - 1), ROW["TP"] + 2.4, 0.8)
    jx, jy = J6_AT
    text(board, "+", jx, jy - 2.4, 1.2, bold=True)
    text(board, "−", jx + 4.8, jy - 2.4, 1.2, bold=True)
    text(board, "J6 PANEL DC IN", jx + 2.4, jy + 2.6, 0.8)
    text(board, "NT1 · DC− = GND", NT1_AT[0] + 1.5, NT1_AT[1], 0.8, align="left")
    text(board, "A1 · Pico 2 · 5 V on its USB", PICO[0] + 8.89, 55.0, 0.8)
    text(board, "J10 · J11 · WIZ850io", WIZ_J1[0] + 10.16, 30.0, 0.8)
    for i, line in enumerate([
        "Kohler DTV+ in-line controller",
        f"carrier rev {REV} · {REV_DATE}",
        "github.com/ntotten/kohler-dtv-plus",
        "generated by controller/hardware/",
        "tools/carrier_board.py",
    ]):
        text(board, line, 125.0, 37.0 + 1.6 * i, 0.9, align="left")


def text_keepouts(board) -> None:
    """Keep vias out from under the board's own silkscreen text, so the router
    leaves every label readable. Tracks may still pass under it."""
    for item in list(board.GetDrawings()):
        if not isinstance(item, pcbnew.PCB_TEXT) or item.GetLayer() != pcbnew.F_SilkS:
            continue
        box = item.GetBoundingBox()
        box.Inflate(mm(0.4))
        z = pcbnew.ZONE(board)
        z.SetIsRuleArea(True)
        z.SetDoNotAllowVias(True)
        z.SetDoNotAllowTracks(False)
        z.SetDoNotAllowCopperPour(False)
        z.SetDoNotAllowPads(False)
        z.SetDoNotAllowFootprints(False)
        z.SetLayerSet(pcbnew.LSET(pcbnew.F_Cu).AddLayer(pcbnew.B_Cu))
        z.SetZoneName(f"no vias under: {item.GetText()}")
        poly = z.Outline()
        poly.NewOutline()
        for x, y in ((box.GetLeft(), box.GetTop()), (box.GetRight(), box.GetTop()),
                     (box.GetRight(), box.GetBottom()), (box.GetLeft(), box.GetBottom())):
            poly.Append(x, y)
        board.Add(z)


def title_block(board) -> None:
    tb = board.GetTitleBlock()
    tb.SetTitle("Kohler DTV+ in-line controller: carrier")
    tb.SetRevision(REV)
    tb.SetDate(REV_DATE)
    tb.SetCompany("github.com/ntotten/kohler-dtv-plus")
    tb.SetComment(0, "Generated by controller/hardware/tools/carrier_board.py; not edited by hand")
    tb.SetComment(1, "2 layers, 1.6 mm FR-4, 1 oz copper; see controller/hardware/fab/README.md")


def write_project(path: Path) -> None:
    """The .kicad_pro: KiCad 7's defaults with these rules and net classes."""
    import json

    with tempfile.TemporaryDirectory() as tmp:
        b = pcbnew.CreateEmptyBoard()
        pcbnew.SaveBoard(str(Path(tmp) / "t.kicad_pcb"), b)
        pro = json.loads((Path(tmp) / "t.kicad_pro").read_text())
    pro["meta"]["filename"] = path.name
    pro["board"]["design_settings"]["rules"].update(RULES)
    pro["board"]["design_settings"]["rule_severities"].update(SEVERITIES)
    classes, patterns = [], []
    template = pro["net_settings"]["classes"][0]
    for name, track, clearance, via_d, via_drill, pats in NETCLASSES:
        c = dict(template)
        c.update(name=name, track_width=track, clearance=clearance, via_diameter=via_d,
                 via_drill=via_drill)
        classes.append(c)
        patterns += [{"netclass": name, "pattern": p} for p in pats]
    pro["net_settings"]["classes"] = classes
    pro["net_settings"]["netclass_patterns"] = patterns
    path.write_text(json.dumps(pro, indent=2, sort_keys=True) + "\n")


def build(nl: Netlist) -> pcbnew.BOARD:
    board = pcbnew.CreateEmptyBoard()
    board.SetCopperLayerCount(2)
    nets: dict[str, pcbnew.NETINFO_ITEM] = {}

    def net(name: str) -> pcbnew.NETINFO_ITEM:
        if name not in nets:
            nets[name] = pcbnew.NETINFO_ITEM(board, name)
            board.Add(nets[name])
        return nets[name]

    for ref, (x, y, rot) in sorted(placement(nl).items()):
        part = nl.parts[ref]
        fp = load_fp(part["footprint"])
        fp.SetReference(ref)
        fp.SetValue(part["value"])
        fp.SetPosition(pt(x, y))
        fp.SetOrientationDegrees(rot)
        if part["dnp"]:
            fp.SetAttributes(fp.GetAttributes() | pcbnew.FP_EXCLUDE_FROM_BOM | pcbnew.FP_EXCLUDE_FROM_POS_FILES)
        for name, value in sorted(part["fields"].items()):
            fp.SetProperty(name, value)
        board.Add(fp)
        for pad in fp.Pads():
            n = nl.pin_net.get((ref, pad.GetNumber()))
            if n:
                pad.SetNet(net(n))
        # Values stay on the fabrication layer. References go where they can
        # be read once the board is built; the jacks, test points, J6 and NT1
        # are named by the labels silkscreen() prints instead.
        fp.Value().SetVisible(False)
        r = fp.Reference()
        prefix = re.match(r"[A-Z]+", ref).group()
        r.SetTextSize(pcbnew.VECTOR2I(mm(0.8), mm(0.8)))
        r.SetTextThickness(mm(0.12))
        if prefix == "R":
            r.SetPosition(pt(x + 5.08, y))  # inside the body's outline
        elif prefix == "C":
            r.SetPosition(pt(x + 5.0, y))
        elif prefix == "U":
            r.SetPosition(pt(x - 4.6, y))
        elif ref not in ("A1", "J10", "J11"):
            r.SetVisible(False)
    for ref, (x, y) in HOLES.items():
        fp = load_fp("MountingHole:MountingHole_2.7mm_M2.5")
        fp.SetReference(ref)
        fp.SetValue("M2.5")
        fp.SetPosition(pt(x, y))
        fp.Reference().SetVisible(False)
        board.Add(fp)
    outline(board)
    box_drawing(board)
    silkscreen(board, nl)
    text_keepouts(board)
    title_block(board)
    return board


# ---------------------------------------------------------------------------
# Routing
# ---------------------------------------------------------------------------


def route(dsn: Path) -> None:
    jar = os.environ.get("FREEROUTING_JAR")
    java = os.environ.get("JAVA", "java")
    if not jar:
        raise SystemExit("--reroute needs FREEROUTING_JAR (Freerouting 2.4.1) and Java 25 (JAVA)")
    subprocess.run(
        [java, "-jar", jar, "-de", str(dsn), "-do", str(SES), "-mp", "100", "-mt", "1",
         "--gui.enabled=false", "--usage_and_diagnostic_data.disable_analytics=true"],
        check=True,
    )


def apply_session(board, ses: Path) -> int:
    """Add Freerouting's wires and vias. Specctra's y axis points up."""
    tree = parse(ses.read_text())[0]
    routes = find1(tree, "routes")
    res = find1(routes, "resolution")
    scale = {"um": 1000, "mm": 1_000_000, "mil": 25400}[str(res[1])] / float(res[2])
    nets = {n.GetNetname(): n for n in board.GetNetsByName().values()}
    layers = {"F.Cu": pcbnew.F_Cu, "B.Cu": pcbnew.B_Cu}
    vias = {}
    for ps in find(find1(routes, "library_out") or [], "padstack"):
        m = re.search(r"_(\d+):(\d+)_um", ps[1])
        vias[ps[1]] = (int(m.group(1)) * 1000, int(m.group(2)) * 1000)
    count = 0
    for n in find(find1(routes, "network_out"), "net"):
        net = nets[n[1]]
        for wire in find(n, "wire"):
            path = find1(wire, "path")
            layer, width = layers[path[1]], int(float(path[2]) * scale)
            xy = [float(v) for v in path[3:]]
            points = [(int(xy[i] * scale), -int(xy[i + 1] * scale)) for i in range(0, len(xy), 2)]
            for a, b in zip(points, points[1:]):
                t = pcbnew.PCB_TRACK(board)
                t.SetStart(pcbnew.VECTOR2I(*a))
                t.SetEnd(pcbnew.VECTOR2I(*b))
                t.SetWidth(width)
                t.SetLayer(layer)
                t.SetNet(net)
                board.Add(t)
                count += 1
        for v in find(n, "via"):
            d, drill = vias[v[1]]
            via = pcbnew.PCB_VIA(board)
            via.SetPosition(pcbnew.VECTOR2I(int(float(v[2]) * scale), -int(float(v[3]) * scale)))
            via.SetViaType(pcbnew.VIATYPE_THROUGH)
            via.SetLayerPair(pcbnew.F_Cu, pcbnew.B_Cu)
            via.SetWidth(d)
            via.SetDrill(drill)
            via.SetNet(net)
            board.Add(via)
            count += 1
    return count


def pour_ground(board) -> None:
    gnd = board.FindNet("GND")
    for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
        z = pcbnew.ZONE(board)
        z.SetLayer(layer)
        z.SetNet(gnd)
        z.SetLocalClearance(mm(0.3))
        z.SetMinThickness(mm(0.25))
        z.SetPadConnection(pcbnew.ZONE_CONNECTION_THERMAL)
        z.SetThermalReliefGap(mm(0.5))
        z.SetThermalReliefSpokeWidth(mm(0.5))
        z.SetIslandRemovalMode(pcbnew.ISLAND_REMOVAL_MODE_ALWAYS)
        z.SetZoneName(f"GND {'top' if layer == pcbnew.F_Cu else 'bottom'}")
        poly = z.Outline()
        poly.NewOutline()
        x0, x1, d = NOTCH
        for x, y in [(0, 0), (x0, 0), (x0, d), (x1, d), (x1, 0), (W, 0), (W, H), (0, H)]:
            poly.Append(mm(x), mm(y))
        board.Add(z)
    # NT1's copper bridge has no net of its own, so the pour keeps off it.
    keep = pcbnew.ZONE(board)
    keep.SetIsRuleArea(True)
    keep.SetDoNotAllowCopperPour(True)
    keep.SetDoNotAllowTracks(False)
    keep.SetDoNotAllowVias(False)
    keep.SetDoNotAllowPads(False)
    keep.SetDoNotAllowFootprints(False)
    keep.SetLayerSet(pcbnew.LSET(pcbnew.F_Cu))
    keep.SetZoneName("NT1 keep-out")
    poly = keep.Outline()
    poly.NewOutline()
    x, y = NT1_AT
    for dx, dy in [(-1.3, -0.8), (1.3, -0.8), (1.3, 0.8), (-1.3, 0.8)]:
        poly.Append(mm(x + dx), mm(y + dy))
    board.Add(keep)
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())


def drc(board, report: Path) -> tuple[int, int, str]:
    pcbnew.WriteDRCReport(board, str(report), pcbnew.EDA_UNITS_MILLIMETRES, True)
    text_ = report.read_text()
    found = {v: int(k) for k, v in re.findall(r"\*\* Found (\d+) (DRC violations|unconnected pads)", text_)}
    return found.get("DRC violations", -1), found.get("unconnected pads", -1), text_


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    reroute = "--reroute" in sys.argv
    # KiCad gives every new item a random UUID and saves the board in UUID
    # order; a seeded generator makes the same board in the same bytes.
    pcbnew.KIID.SeedGenerator(1591)
    nl = Netlist(Path(args[0]))
    write_project(HERE / "carrier.kicad_pro")
    (HERE / "carrier.kicad_dru").write_text(DRU)
    board = build(nl)
    with tempfile.TemporaryDirectory() as tmp:
        stage = Path(tmp) / "carrier.kicad_pcb"
        pcbnew.SaveBoard(str(stage), board, True)
        (Path(tmp) / "carrier.kicad_pro").write_text((HERE / "carrier.kicad_pro").read_text())
        (Path(tmp) / "carrier.kicad_dru").write_text(DRU)
        board = pcbnew.LoadBoard(str(stage))
        if reroute or not SES.exists():
            dsn = Path(tmp) / "carrier.dsn"
            if not pcbnew.ExportSpecctraDSN(board, str(dsn)):
                raise SystemExit("Specctra export failed")
            route(dsn)
        items = apply_session(board, SES)
        pour_ground(board)
        violations, unconnected, report = drc(board, Path(tmp) / "drc.rpt")
        pcbnew.SaveBoard(str(PCB), board, True)
    print(f"board: {len(nl.parts)} parts, {items} tracks and vias, "
          f"{violations} DRC violations, {unconnected} unconnected")
    if violations or unconnected:
        print(report)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
