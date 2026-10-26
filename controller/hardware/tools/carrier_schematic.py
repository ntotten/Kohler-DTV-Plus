#!/usr/bin/env python3
"""Generate the carrier schematic, carrier.kicad_sch, and its symbol library.

The carrier is the board the Pico 2, the WIZ850io and the five THVD1410 ports
sit on (HARDWARE.md §3-§10). This script is the schematic's source, and the
board's: carrier_board.py lays the PCB out from the netlist KiCad reads back
from what this writes. Change the tables below and run `render.sh`, which
regenerates the schematic, the board, the fabrication files and every
drawing; nobody edits the KiCad files by hand.

Connections are made with net labels at short wire stubs, so every pin's net
is written next to it. `render.sh` checks the netlist KiCad reads back against
`EXPECTED` below.

Run with KiCad's symbol libraries available: KICAD_SYMBOL_DIR, or the Linux
or macOS default install path. Written against KiCad 7; newer KiCad opens the
file and upgrades it on save.
"""

from __future__ import annotations

import os
import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from sexpr import Sym, dump, find, find1, lib_symbol  # noqa: E402

HERE = Path(__file__).resolve().parent.parent
PROJECT = "carrier"
VERSION = 20230121  # KiCad 7 schematic format

SYMBOL_DIRS = [
    os.environ.get("KICAD_SYMBOL_DIR", ""),
    "/usr/share/kicad/symbols",
    "/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols",
]

# Fixed UUIDs keep regenerated files diff-stable.
NS = uuid.UUID("6b1c3d4e-2a51-4c7e-9a1d-0c0ffee0ca71")


def uid(*parts: object) -> str:
    return str(uuid.uuid5(NS, "/".join(str(p) for p in parts)))


ROOT_UUID = uid("root")

# ---------------------------------------------------------------------------
# What the board is. Everything below the layout section follows from these.
# ---------------------------------------------------------------------------

# HARDWARE.md §8. Pico 2 physical pin -> net. Pin numbers per Raspberry Pi's
# Pico pinout, which the Pico 2 keeps (KiCad's RaspberryPi_Pico symbol).
PICO_PINS = {
    1: ("GPIO0", "V1_TX"),
    2: ("GPIO1", "V1_RX"),
    3: ("GND", "GND"),
    4: ("GPIO2", "V1_DE"),
    5: ("GPIO3", None),
    6: ("GPIO4", "V2_TX"),
    7: ("GPIO5", "V2_RX"),
    8: ("GND", "GND"),
    9: ("GPIO6", "V2_DE"),
    10: ("GPIO7", "K1_TX"),
    11: ("GPIO8", "K1_RX"),
    12: ("GPIO9", "K1_DE"),
    13: ("GND", "GND"),
    14: ("GPIO10", "K2_TX"),
    15: ("GPIO11", "K2_RX"),
    16: ("GPIO12", "K2_DE"),
    17: ("GPIO13", "P_TX"),
    18: ("GND", "GND"),
    19: ("GPIO14", "P_RX"),
    20: ("GPIO15", "P_DE"),
    21: ("GPIO16", "ETH_MISO"),
    22: ("GPIO17", "ETH_CS"),
    23: ("GND", "GND"),
    24: ("GPIO18", "ETH_SCK"),
    25: ("GPIO19", "ETH_MOSI"),
    26: ("GPIO20", "ETH_RST"),
    27: ("GPIO21", "ETH_INT"),
    28: ("GND", "GND"),
    29: ("GPIO22", None),
    30: ("RUN", None),
    31: ("GPIO26_ADC0", None),
    32: ("GPIO27_ADC1", None),
    33: ("AGND", None),
    34: ("GPIO28_ADC2", None),
    35: ("ADC_VREF", None),
    36: ("3V3", "+3V3"),
    37: ("3V3_EN", None),
    38: ("GND", "GND"),
    39: ("VSYS", None),
    40: ("VBUS", None),
}

# WIZnet's WIZ850io pin table (docs.wiznet.io, Pin Information). Its two
# 1x6 rows plug into two sockets on the carrier.
WIZ_J1 = [
    ("GND", "GND"),
    ("GND", "GND"),
    ("MOSI", "ETH_MOSI"),
    ("SCLK", "ETH_SCK"),
    ("SCSn", "ETH_CS"),
    ("INTn", "ETH_INT"),
]
WIZ_J2 = [
    ("GND", "GND"),
    ("3V3D", "+3V3"),
    ("3V3D", "+3V3"),
    ("NC", None),
    ("RSTn", "ETH_RST"),
    ("MISO", "ETH_MISO"),
]

# The five ports, HARDWARE.md §1, §5, §8, §16. kind: "K" faces the K-99695,
# "V" a valve, "P" the panel. bias: whether the controller end biases it.
PORTS = [
    {"port": "K1", "zone": 1, "kind": "K", "title": "K1 · to Kohler valve 1 jack"},
    {"port": "V1", "zone": 1, "kind": "V", "title": "V1 · valve 1, six-port"},
    {"port": "K2", "zone": 2, "kind": "K", "title": "K2 · to Kohler valve 2 jack"},
    {"port": "V2", "zone": 2, "kind": "V", "title": "V2 · valve 2, three-port Prompt"},
    {"port": "P", "zone": None, "kind": "P", "title": "P · wall panel, in-wall RJ45 cable"},
]

# Six-position modular jack on a valve segment: pins 2-5 wired, pin 4
# positive, pin 3 negative, pin 2 ground, pin 5 the valve's +12.7 V
# (HARDWARE.md §5, §11).
def valve_jack_pins(port: str, zone: int) -> dict[int, str | None]:
    return {
        1: None,
        2: "GND",
        3: f"{port}_B",
        4: f"{port}_A",
        5: f"VALVE{zone}_12V7",
        6: None,
    }


# HARDWARE.md §16, eight-conductor pin plan.
PANEL_JACK_PINS = {
    1: "P_A",
    2: "P_B",
    3: None,
    4: "PANEL_DC+",
    5: "PANEL_DC+",
    6: None,
    7: "PANEL_DC-",
    8: "PANEL_DC-",
}

# Footprints, by reference prefix or reference. KiCad's own library except
# the three in carrier.pretty (carrier_footprints.py). The RJ12 jacks are
# Amphenol 54601-906LF, whose footprint KiCad draws from Amphenol drawing
# C-BMJ-0082; the Würth 615006138421 bought for the bench has no KiCad
# footprint and its drawing could not be downloaded.
FP = {
    "A1": "carrier:Pico2_Socketed",
    "J10": "carrier:WIZ850io_J1",
    "J11": "carrier:WIZ850io_J2",
    "U": "Package_SO:VSSOP-8_3.0x3.0mm_P0.65mm",
    "C": "Capacitor_THT:C_Rect_L4.0mm_W2.5mm_P2.50mm",
    "R": "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
    "TP": "Connector_PinHeader_2.54mm:PinHeader_1x01_P2.54mm_Vertical",
    "RJ12": "Connector_RJ:RJ12_Amphenol_54601-x06_Horizontal",
    "J5": "Connector_RJ:RJ45_Amphenol_54602-x08_Horizontal",
    "J6": "Connector_Wire:SolderWire-0.5sqmm_1x02_P4.8mm_D0.9mm_OD2.3mm",
    "NT1": "NetTie:NetTie-2_SMD_Pad0.5mm",
}

# What the bill of materials says about each part, by value.
PARTS = {
    "120": {"MPN": "CFR-25JB-52-120R", "Manufacturer": "Yageo", "Supplier": "DigiKey 120QBK-ND"},
    "4k7": {"MPN": "CFR-25JB-52-4K7", "Manufacturer": "Yageo",
            "Supplier": "DigiKey 13-CFR-25JB-52-4K7-ND"},
    "100n": {"MPN": "K104K15X7RF5TL2", "Manufacturer": "Vishay", "Supplier": "DigiKey BC1084CT-ND"},
}

# ---------------------------------------------------------------------------
# Library symbols
# ---------------------------------------------------------------------------


def kicad_lib(name: str) -> Path:
    for d in SYMBOL_DIRS:
        if d and (Path(d) / name).exists():
            return Path(d) / name
    raise SystemExit(f"KiCad symbol library {name} not found; set KICAD_SYMBOL_DIR")


def std_symbol(lib: str, name: str) -> list:
    s = lib_symbol(kicad_lib(f"{lib}.kicad_sym"), name)
    if s is None:
        raise SystemExit(f"{lib}:{name} not in KiCad's library")
    if find1(s, "extends"):
        raise SystemExit(f"{lib}:{name} extends another symbol; use the base")
    s = list(s)
    s[1] = f"{lib}:{name}"
    return s


def prop(name, value, x=0.0, y=0.0, hide=False, justify=None, angle=0):
    effects = [Sym("effects"), [Sym("font"), [Sym("size"), 1.27, 1.27]]]
    if justify:
        effects.append([Sym("justify"), Sym(justify)])
    if hide:
        effects.append(Sym("hide"))
    return [Sym("property"), name, value, [Sym("at"), x, y, angle], effects]


def pin(kind, x, y, angle, length, name, number):
    return [
        Sym("pin"),
        Sym(kind),
        Sym("line"),
        [Sym("at"), x, y, angle],
        [Sym("length"), length],
        [Sym("name"), name, [Sym("effects"), [Sym("font"), [Sym("size"), 1.27, 1.27]]]],
        [Sym("number"), number, [Sym("effects"), [Sym("font"), [Sym("size"), 1.27, 1.27]]]],
    ]


def rect(x1, y1, x2, y2, fill="background"):
    return [
        Sym("rectangle"),
        [Sym("start"), x1, y1],
        [Sym("end"), x2, y2],
        [Sym("stroke"), [Sym("width"), 0.254], [Sym("type"), Sym("default")]],
        [Sym("fill"), [Sym("type"), Sym(fill)]],
    ]


def custom_symbol(name, ref, value, description, datasheet, footprint, body, pins):
    return [
        Sym("symbol"),
        name,
        [Sym("in_bom"), Sym("yes")],
        [Sym("on_board"), Sym("yes")],
        prop("Reference", ref, 0, body[1] + 1.27, justify=None),
        prop("Value", value, 0, body[3] - 1.27),
        prop("Footprint", footprint, 0, 0, hide=True),
        prop("Datasheet", datasheet, 0, 0, hide=True),
        prop("ki_description", description, 0, 0, hide=True),
        [Sym("symbol"), f"{name.split(':')[-1]}_0_1", rect(*body)],
        [Sym("symbol"), f"{name.split(':')[-1]}_1_1", *pins],
    ]


def thvd1410() -> list:
    """KiCad's MAX481E drawing, which its THVD1400D symbol extends: TI's
    THVD14x0 parts share the 8-pin half-duplex pinout. Pin names follow TI."""
    base = lib_symbol(kicad_lib("Interface_UART.kicad_sym"), "MAX481E")
    names = {"1": "R", "2": "~{RE}", "3": "DE", "4": "D"}
    out = [Sym("symbol"), "carrier:THVD1410"]
    for item in base[2:]:
        if isinstance(item, list) and item[0] == "property":
            key = item[1]
            item = list(item)
            if key == "Value":
                item[2] = "THVD1410"
            elif key == "Datasheet":
                item[2] = "https://www.ti.com/lit/gpn/THVD1410"
            elif key == "Footprint":
                item[2] = FP["U"]
            elif key == "ki_description":
                item[2] = (
                    "Half-duplex RS-485, 500 kbps, 3-5.5 V, fail-safe receiver, "
                    "driver and receiver enable"
                )
            elif key in ("ki_fp_filters", "ki_keywords"):
                continue
        elif isinstance(item, list) and item[0] == "symbol":
            unit = list(item)
            unit[1] = unit[1].replace("MAX481E", "THVD1410")
            body = []
            for el in unit[2:]:
                if isinstance(el, list) and el[0] == "pin":
                    el = [list(x) if isinstance(x, list) else x for x in el]
                    num = find1(el, "number")[1]
                    if num in names:
                        find1(el, "name")[1] = names[num]
                body.append(el)
            item = unit[:2] + body
        out.append(item)
    return out


def pico2() -> list:
    pins = []
    for n, (name, net) in PICO_PINS.items():
        kind = "bidirectional"
        if name in ("GND", "AGND"):
            kind = "power_in"
        elif name == "3V3":
            kind = "power_out"
        elif name in ("VSYS", "VBUS", "ADC_VREF"):
            kind = "power_in"
        elif name in ("RUN", "3V3_EN"):
            kind = "input"
        if n <= 20:
            pins.append(pin(kind, -15.24, 24.13 - 2.54 * (n - 1), 0, 5.08, name, str(n)))
        else:
            pins.append(pin(kind, 15.24, 24.13 - 2.54 * (40 - n), 180, 5.08, name, str(n)))
    return custom_symbol(
        "carrier:Pico2",
        "A",
        "Raspberry Pi Pico 2",
        "RP2350 module on two 20-pin sockets; power in on its own micro-USB",
        "https://datasheets.raspberrypi.com/pico/pico-2-datasheet.pdf",
        "",
        (-10.16, 26.67, 10.16, -26.67),
        pins,
    )


def wiz_row(row: str, table) -> list:
    pins = []
    for i, (name, _net) in enumerate(table):
        kind = "passive"
        if name == "GND":
            kind = "power_in"
        elif name == "3V3D":
            kind = "power_in"
        elif name == "NC":
            kind = "no_connect"
        elif name in ("INTn", "MISO"):
            kind = "output"
        else:
            kind = "input"
        pins.append(pin(kind, -10.16, 6.35 - 2.54 * i, 0, 2.54, name, str(i + 1)))
    return custom_symbol(
        f"carrier:WIZ850io_{row}",
        "J",
        f"WIZ850io {row}",
        f"WIZnet WIZ850io W5500 module, header {row}, on a 1x6 socket",
        "https://docs.wiznet.io/Product/ioModule/wiz850io",
        "",
        (-7.62, 8.89, 7.62, -8.89),
        pins,
    )


# ---------------------------------------------------------------------------
# Placement helpers
# ---------------------------------------------------------------------------


class Sheet:
    def __init__(self) -> None:
        self.items: list = []
        self.lib: dict[str, list] = {}
        self.pins: dict[str, dict[str, tuple]] = {}
        self.power_n = 0

    def use(self, sym: list) -> str:
        self.lib.setdefault(sym[1], sym)
        return sym[1]

    def lib_pins(self, lib_id: str):
        out = {}

        def walk(node):
            for x in node:
                if isinstance(x, list) and x and x[0] == "pin":
                    at = find1(x, "at")
                    out[find1(x, "number")[1]] = (float(at[1]), float(at[2]), float(at[3]))
                elif isinstance(x, list) and x and x[0] == "symbol":
                    walk(x)

        walk(self.lib[lib_id])
        return out

    def place(self, lib_id, ref, value, x, y, rot=0, mirror=None, fields=None,
              dnp=False, show_value=True, ref_at=None, value_at=None,
              value_justify=None, value_angle=0):
        """Place a symbol; return {pin number: (screen x, y, outward dx, dy)}."""
        import math

        pins = {}
        for num, (px, py, ang) in self.lib_pins(lib_id).items():
            if mirror == "y":
                px, ang = -px, (180 - ang) % 360
            t = math.radians(rot)
            rx = px * math.cos(t) - py * math.sin(t)
            ry = px * math.sin(t) + py * math.cos(t)
            out = math.radians(ang + 180 + rot)
            dx, dy = round(math.cos(out)), round(math.sin(out))
            pins[num] = (round(x + rx, 3), round(y - ry, 3), dx, -dy)
        power = ref.startswith("#")
        rx, ry = ref_at if ref_at else (x + 2.54, y - 1.27)
        vx, vy = value_at if value_at else (x + 2.54, y + 1.27)
        props = [
            prop("Reference", ref, rx, ry, hide=power, justify=None if power else "left"),
            prop("Value", value, vx, vy, hide=not show_value,
                 justify=value_justify or (None if power else "left"), angle=value_angle),
            prop("Footprint", (fields or {}).pop("Footprint", ""), x, y, hide=True),
            prop("Datasheet", (fields or {}).pop("Datasheet", ""), x, y, hide=True),
        ]
        for k, v in (fields or {}).items():
            props.append(prop(k, v, x, y, hide=True))
        node = [
            Sym("symbol"),
            [Sym("lib_id"), lib_id],
            [Sym("at"), x, y, rot],
        ]
        if mirror:
            node.append([Sym("mirror"), Sym(mirror)])
        node += [
            [Sym("unit"), 1],
            [Sym("in_bom"), Sym("no" if power else "yes")],
            [Sym("on_board"), Sym("no" if power else "yes")],
            [Sym("dnp"), Sym("yes" if dnp else "no")],
            [Sym("uuid"), uid("sym", ref)],
            *props,
            *[[Sym("pin"), n, [Sym("uuid"), uid("pin", ref, n)]] for n in sorted(pins)],
            [
                Sym("instances"),
                [
                    Sym("project"),
                    PROJECT,
                    [Sym("path"), f"/{ROOT_UUID}", [Sym("reference"), ref], [Sym("unit"), 1]],
                ],
            ],
        ]
        self.items.append(node)
        self.pins[ref] = pins
        return pins

    def wire(self, x1, y1, x2, y2):
        self.items.append([
            Sym("wire"),
            [Sym("pts"), [Sym("xy"), x1, y1], [Sym("xy"), x2, y2]],
            [Sym("stroke"), [Sym("width"), 0], [Sym("type"), Sym("default")]],
            [Sym("uuid"), uid("wire", x1, y1, x2, y2)],
        ])

    def label(self, name, x, y, dx, dy):
        angle = {(1, 0): 0, (0, -1): 90, (-1, 0): 180, (0, 1): 270}[(dx, dy)]
        justify = [Sym("left"), Sym("bottom")] if angle in (0, 90) else [Sym("right"), Sym("bottom")]
        self.items.append([
            Sym("label"),
            name,
            [Sym("at"), x, y, angle],
            [Sym("fields_autoplaced")],
            [Sym("effects"), [Sym("font"), [Sym("size"), 1.27, 1.27]], [Sym("justify"), *justify]],
            [Sym("uuid"), uid("label", name, x, y)],
        ])

    def power(self, net, x, y, dx, dy):
        """A power symbol at (x, y) with its body pointing (dx, dy)."""
        self.power_n += 1
        if net == "GND":
            rot = {(0, 1): 0, (1, 0): 90, (0, -1): 180, (-1, 0): 270}[(dx, dy)]
            lib = "power:GND"
        else:
            rot = {(0, -1): 0, (-1, 0): 90, (0, 1): 180, (1, 0): 270}[(dx, dy)]
            lib = "power:+3V3"
        # A sideways ground glyph needs no name; everything else shows it
        # just past the symbol.
        sideways = dy == 0
        self.place(lib, f"#PWR{self.power_n:03d}", net, x, y, rot=rot,
                   show_value=not (sideways and net == "GND"),
                   value_at=(x + 3.81 * dx, y + 4.45 * dy),
                   # KiCad turns field text with the symbol and flips text
                   # that would read upside down, justification included.
                   value_justify="left" if sideways else None,
                   value_angle=90 if sideways else 0)

    def no_connect(self, x, y):
        self.items.append([Sym("no_connect"), [Sym("at"), x, y], [Sym("uuid"), uid("nc", x, y)]])

    def connect(self, ref, num, net, stub=2.54):
        """Stub out of a pin and name it: a label, a power symbol, or NC."""
        x, y, dx, dy = self.pins[ref][str(num)]
        if net is None:
            self.no_connect(x, y)
            return
        ex, ey = round(x + dx * stub, 3), round(y + dy * stub, 3)
        self.wire(x, y, ex, ey)
        if net in ("GND", "+3V3"):
            self.power(net, ex, ey, dx, dy)
        else:
            self.label(net, ex, ey, dx, dy)

    def text(self, s, x, y, size=1.27, bold=False):
        font = [Sym("font"), [Sym("size"), size, size]]
        if bold:
            font.append(Sym("bold"))
        self.items.append([
            Sym("text"),
            s,
            [Sym("at"), x, y, 0],
            [Sym("effects"), font, [Sym("justify"), Sym("left"), Sym("bottom")]],
            [Sym("uuid"), uid("text", s[:40], x, y)],
        ])

    def box(self, x1, y1, x2, y2, title):
        self.items.append([
            Sym("rectangle"),
            [Sym("start"), x1, y1],
            [Sym("end"), x2, y2],
            [Sym("stroke"), [Sym("width"), 0], [Sym("type"), Sym("dash")]],
            [Sym("fill"), [Sym("type"), Sym("none")]],
            [Sym("uuid"), uid("box", x1, y1)],
        ])
        self.text(title, x1 + 2.54, y1 + 5.08, size=2.0, bold=True)


# ---------------------------------------------------------------------------
# Layout
# ---------------------------------------------------------------------------


def build() -> tuple[list, list]:
    sh = Sheet()
    for lib, name in [
        ("Device", "R"),
        ("Device", "C"),
        ("Device", "NetTie_2"),
        ("Connector", "6P6C"),
        ("Connector", "8P8C"),
        ("Connector", "Barrel_Jack_Switch"),
        ("Connector", "TestPoint"),
        ("power", "GND"),
        ("power", "+3V3"),
        ("power", "PWR_FLAG"),
    ]:
        sh.use(std_symbol(lib, name))
    custom = [thvd1410(), pico2(), wiz_row("J1", WIZ_J1), wiz_row("J2", WIZ_J2)]
    for s in custom:
        sh.use(s)

    # Compute: the Pico 2 and the WIZ850io's two sockets.
    sh.box(12.7, 12.7, 114.3, 177.8, "Compute · Pico 2 + WIZ850io (W5500)")
    sh.place("carrier:Pico2", "A1", "Raspberry Pi Pico 2", 63.5, 66.04,
             ref_at=(55.88, 38.1), value_at=(55.88, 95.25),
             fields={"Footprint": FP["A1"], "MPN": "SC1631", "Manufacturer": "Raspberry Pi",
                     "Supplier": "DigiKey 2648-SC1631CT-ND",
                     "Socket": "2 x 1x20 female header, Sullins PPPC201LFBN-RC (DigiKey S7053-ND)"})
    for n, (_name, net) in PICO_PINS.items():
        sh.connect("A1", n, net)
    sh.text("Pin numbers: Raspberry Pi Pico pinout (top view).", 22.86, 101.6)
    sh.text("5 V comes in on the Pico 2's own micro-USB.", 22.86, 104.14)
    sh.text("RP2350 SWD and the status LED (GPIO25) are on the Pico 2.", 22.86, 106.68)

    sh.place("carrier:WIZ850io_J1", "J10", "WIZ850io J1 (SPI)", 45.72, 132.08,
             ref_at=(38.1, 121.92), value_at=(38.1, 142.24),
             fields={"Footprint": FP["J10"], "MPN": "WIZ850IO", "Manufacturer": "WIZnet",
                     "Supplier": "DigiKey 1278-1043-ND",
                     "Socket": "1x6 female header, 2.54 mm, cut from a PPPC201LFBN-RC"})
    for i, (_n, net) in enumerate(WIZ_J1, start=1):
        sh.connect("J10", i, net)
    sh.place("carrier:WIZ850io_J2", "J11", "WIZ850io J2 (power, reset)", 90.17, 132.08,
             ref_at=(82.55, 121.92), value_at=(82.55, 142.24),
             fields={"Footprint": FP["J11"],
                     "Socket": "1x6 female header, 2.54 mm, cut from a PPPC201LFBN-RC"})
    for i, (_n, net) in enumerate(WIZ_J2, start=1):
        sh.connect("J11", i, net)
    sh.text("W5500 on GPIO16-21, the pins the W5500-EVB-Pico2 uses.", 22.86, 152.4)
    sh.text("WIZ850io: 141 mA typical from 3.3 V (WIZnet).", 22.86, 154.94)

    # Ground flag: the board's ground comes from the valves' pin 2 and the
    # module, and ERC needs one driver.
    flag = sh.place("power:PWR_FLAG", "#FLG01", "PWR_FLAG", 30.48, 167.64, show_value=False)
    x, y, _dx, _dy = flag["1"]
    sh.wire(x, y, x, y + 2.54)
    sh.power("GND", x, y + 2.54, 0, 1)

    # Ports: zone 1 on the first row, zone 2 on the second, the panel last.
    origins = {"K1": (121.92, 12.7), "V1": (266.7, 12.7),
               "K2": (121.92, 83.82), "V2": (266.7, 83.82),
               "P": (121.92, 154.94)}
    refs = {"U": 1, "C": 1, "R": 1, "TP": 1}

    def nxt(kind):
        n = refs[kind]
        refs[kind] += 1
        return f"{kind}{n}"

    term_refs, de_refs, bias_refs = {}, {}, {}
    for spec in PORTS:
        port, kind = spec["port"], spec["kind"]
        bx, by = origins[port]
        width = 139.7 if kind != "P" else 284.48
        sh.box(bx, by, bx + width, by + 66.04, spec["title"])
        ux, uy = bx + 35.56, by + 30.48
        u = nxt("U")
        sh.place("carrier:THVD1410", u, "THVD1410", ux, uy,
                 ref_at=(ux - 6.35, uy - 13.97), value_at=(ux + 1.27, uy - 13.97),
                 fields={"Footprint": FP["U"], "MPN": "THVD1410DGK",
                         "Manufacturer": "Texas Instruments", "Supplier": "Mouser 595-THVD1410DGK"})
        sh.connect(u, 1, f"{port}_RX")
        sh.connect(u, 2, "GND")
        sh.connect(u, 3, f"{port}_DE")
        sh.connect(u, 4, f"{port}_TX")
        sh.connect(u, 5, "GND")
        sh.connect(u, 6, f"{port}_A")
        sh.connect(u, 7, f"{port}_B")
        sh.connect(u, 8, "+3V3")

        c = nxt("C")
        sh.place("Device:C", c, "100n", ux + 22.86, uy - 10.16,
                 fields={"Footprint": FP["C"], **PARTS["100n"]})
        sh.connect(c, 1, "+3V3")
        sh.connect(c, 2, "GND")

        rde = nxt("R")
        de_refs[port] = rde
        sh.place("Device:R", rde, "4k7", bx + 10.16, by + 55.88,
                 fields={"Footprint": FP["R"], **PARTS["4k7"]})
        sh.connect(rde, 1, f"{port}_DE")
        sh.connect(rde, 2, "GND")

        rt = nxt("R")
        term_refs[port] = rt
        sh.place("Device:R", rt, "120", bx + 68.58, uy,
                 fields={"Footprint": FP["R"], **PARTS["120"]})
        sh.connect(rt, 1, f"{port}_A")
        sh.connect(rt, 2, f"{port}_B")

        if kind in ("V", "P"):
            up, dn = nxt("R"), nxt("R")
            bias_refs[port] = (up, dn)
            sh.place("Device:R", up, "TBD", bx + 83.82, uy - 15.24, dnp=True,
                     fields={"Footprint": FP["R"]})
            sh.connect(up, 1, "+3V3")
            sh.connect(up, 2, f"{port}_A")
            sh.place("Device:R", dn, "TBD", bx + 83.82, uy + 15.24, dnp=True,
                     fields={"Footprint": FP["R"]})
            sh.connect(dn, 1, f"{port}_B")
            sh.connect(dn, 2, "GND")

        if kind in ("K", "V"):
            zone = spec["zone"]
            # Test points on pins 2, 3, 4 of the segment (HARDWARE.md §10).
            for i, (pin_no, net) in enumerate([(2, "GND"), (3, f"{port}_B"), (4, f"{port}_A")]):
                tp = nxt("TP")
                tx = bx + 99.06 + 7.62 * i
                sh.place("Connector:TestPoint", tp, f"{port}-{pin_no}", tx, by + 53.34,
                         ref_at=(tx + 1.27, by + 45.72), value_at=(tx + 1.27, by + 48.26),
                         fields={"Footprint": FP["TP"], "MPN": "1x1 male header, 2.54 mm",
                                 "Supplier": "Already owned"})
                sh.connect(tp, 1, net)
            j = {"K1": "J1", "V1": "J2", "K2": "J3", "V2": "J4"}[port]
            sh.place("Connector:6P6C", j, "6P6C", bx + 124.46, uy, mirror="y",
                     ref_at=(bx + 116.84, uy - 12.7), value_at=(bx + 116.84, uy + 10.16),
                     fields={"Footprint": FP["RJ12"], "MPN": "54601-906LF",
                             "Manufacturer": "Amphenol ICC",
                             "Supplier": "DigiKey, product 7589414"})
            for pin_no, net in valve_jack_pins(port, zone).items():
                sh.connect(j, pin_no, net)
        else:
            sh.place("Connector:8P8C", "J5", "8P8C",
                     bx + 124.46, uy, mirror="y",
                     ref_at=(bx + 116.84, uy - 15.24), value_at=(bx + 116.84, uy + 12.7),
                     fields={"Footprint": FP["J5"], "MPN": "54602-908LF",
                             "Manufacturer": "Amphenol ICC", "Supplier": "DigiKey 609-1046-ND"})
            for pin_no, net in PANEL_JACK_PINS.items():
                sh.connect("J5", pin_no, net)
            # Panel DC: its own jack, wired only to the panel jack's DC pins.
            jx, jy = bx + 175.26, uy
            sh.place("Connector:Barrel_Jack_Switch", "J6", "Panel DC in, 2.1 mm",
                     jx, jy, ref_at=(jx - 5.08, jy - 6.35), value_at=(jx - 7.62, jy + 7.62),
                     fields={"Footprint": FP["J6"], "MPN": "721A", "Manufacturer": "Switchcraft",
                             "Supplier": "DigiKey SC3508-ND",
                             "Mounting": "Box wall; two wires to the board's J6 pads"})
            sh.connect("J6", 1, "PANEL_DC+")
            sh.connect("J6", 2, "PANEL_DC-")
            sh.connect("J6", 3, None)
            sh.place("Device:NetTie_2", "NT1", "DC- to GND, here only", jx + 33.02, jy,
                     ref_at=(jx + 30.48, jy - 3.81), value_at=(jx + 25.4, jy + 5.08),
                     fields={"Footprint": FP["NT1"]})
            sh.connect("NT1", 1, "PANEL_DC-")
            sh.connect("NT1", 2, "GND")
            notes = [
                "J6 is on the box wall, wired to two pads on the board.",
                "It connects only to J5's DC pins: 4, 5 (+) and 7, 8 (-).",
                "DC- joins board ground at NT1 and nowhere else;",
                "DC+ touches no board net (HARDWARE.md §16, bench check 4).",
                "J6 pin 1 is taken as the centre contact, centre positive [I];",
                "the panel's adapter sets the voltage and polarity.",
            ]
            for i, line in enumerate(notes):
                sh.text(line, jx - 10.16, by + 45.72 + 2.54 * i)

    # Notes.
    notes = [
        "Notes",
        "1. Pins 2 and 5 pass straight through each zone: K1-V1 and K2-V2 share",
        "   GND and VALVEn_12V7. Pin 5 touches no other board net (§5, §9).",
        "2. One board ground joins both zones' pin 2 and the panel's DC-. If the",
        "   K-99695 doesn't join its jacks' pin 2 (§15 item 8), one zone's two",
        "   ports take an isolated transceiver instead.",
        "3. Every DE has a 4.7k pull-down: no port drives while the RP2350 is in",
        "   reset or booting (erratum E9, §5, bench check 1).",
        "4. RE is tied low, so each port hears its own echo (§5).",
        "5. Bias resistors (TBD) are not fitted until their values are set:",
        "   valve segments in Phase 3, the panel with the panel (§5, §16).",
        "6. 120R terminates every port at the controller end (§5).",
        "7. GPIO map is HARDWARE.md §8, proposed [I].",
        "8. THVD1410 pin numbers: TI's datasheet, Pin Functions, DGK [V].",
        "9. Connector pin 1 and 6 of each valve jack are not wired (§11).",
    ]
    for i, line in enumerate(notes):
        sh.text(line, 15.24, 186.69 + 3.0 * i, size=1.5 if i else 2.0, bold=(i == 0))

    lib_symbols = [Sym("lib_symbols"), *sh.lib.values()]
    sch = [
        Sym("kicad_sch"),
        [Sym("version"), VERSION],
        [Sym("generator"), Sym("eeschema")],
        [Sym("uuid"), ROOT_UUID],
        [Sym("paper"), "A3"],
        [
            Sym("title_block"),
            [Sym("title"), "Replacement controller carrier"],
            [Sym("date"), "2026-10-26"],
            [Sym("rev"), "B"],
            [Sym("company"), "kohler-dtv-plus"],
            [Sym("comment"), 1, "Pico 2 + WIZ850io + 5 x THVD1410, carrier PCB (carrier.kicad_pcb)"],
            [Sym("comment"), 2, "Generated by controller/hardware/tools/carrier_schematic.py"],
            [Sym("comment"), 3, "Specification: controller/docs/HARDWARE.md"],
        ],
        lib_symbols,
        *sh.items,
        [Sym("sheet_instances"), [Sym("path"), "/", [Sym("page"), "1"]]],
    ]
    library = [
        Sym("kicad_symbol_lib"),
        [Sym("version"), 20220914],
        [Sym("generator"), Sym("kicad_symbol_editor")],
    ]
    for s in custom:
        s = list(s)
        s[1] = s[1].split(":", 1)[1]
        library.append(s)
    return sch, library


# What `render.sh` checks the netlist against: every named net and the pins on
# it. Built from the tables above, not from the layout.
def expected() -> dict[str, set[str]]:
    nets: dict[str, set[str]] = {}

    def add(net, ref, num):
        if net:
            nets.setdefault(net, set()).add(f"{ref}.{num}")

    for n, (_name, net) in PICO_PINS.items():
        add(net, "A1", n)
    for i, (_n, net) in enumerate(WIZ_J1, start=1):
        add(net, "J10", i)
    for i, (_n, net) in enumerate(WIZ_J2, start=1):
        add(net, "J11", i)
    r = 1
    for k, spec in enumerate(PORTS, start=1):
        port = spec["port"]
        u = f"U{k}"
        for num, net in {1: f"{port}_RX", 2: "GND", 3: f"{port}_DE", 4: f"{port}_TX",
                         5: "GND", 6: f"{port}_A", 7: f"{port}_B", 8: "+3V3"}.items():
            add(net, u, num)
        add("+3V3", f"C{k}", 1)
        add("GND", f"C{k}", 2)
        add(f"{port}_DE", f"R{r}", 1)
        add("GND", f"R{r}", 2)
        add(f"{port}_A", f"R{r + 1}", 1)
        add(f"{port}_B", f"R{r + 1}", 2)
        r += 2
        if spec["kind"] in ("V", "P"):
            add("+3V3", f"R{r}", 1)
            add(f"{port}_A", f"R{r}", 2)
            add(f"{port}_B", f"R{r + 1}", 1)
            add("GND", f"R{r + 1}", 2)
            r += 2
    tp = 1
    for spec in PORTS:
        if spec["kind"] == "P":
            continue
        port = spec["port"]
        for net in ("GND", f"{port}_B", f"{port}_A"):
            add(net, f"TP{tp}", 1)
            tp += 1
        j = {"K1": "J1", "V1": "J2", "K2": "J3", "V2": "J4"}[port]
        for num, net in valve_jack_pins(port, spec["zone"]).items():
            add(net, j, num)
    for num, net in PANEL_JACK_PINS.items():
        add(net, "J5", num)
    add("PANEL_DC+", "J6", 1)
    add("PANEL_DC-", "J6", 2)
    add("PANEL_DC-", "NT1", 1)
    add("GND", "NT1", 2)
    return nets


def main() -> None:
    sch, library = build()
    (HERE / f"{PROJECT}.kicad_sch").write_text(dump(sch) + "\n")
    (HERE / f"{PROJECT}.kicad_sym").write_text(dump(library) + "\n")
    if len(sys.argv) > 1 and sys.argv[1] == "--expected":
        import json

        print(json.dumps({k: sorted(v) for k, v in sorted(expected().items())}, indent=1))


if __name__ == "__main__":
    main()
