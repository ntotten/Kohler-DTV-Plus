#!/usr/bin/env python3
"""Write the carrier's own footprints, carrier.pretty, from the module makers'
drawings. Every other footprint on the board is KiCad's own library.

  Pico2_Socketed  Raspberry Pi Pico 2 on two 1x20 sockets: 21 x 51 mm, rows
                  17.78 mm apart, pin 1 1.61 mm in and 1.37 mm down from the
                  USB corner, micro-USB overhanging the top edge by 1.3 mm
                  (Pico 2 datasheet, Figure 3) [V].
  WIZ850io_J1     The WIZ850io's J1 row on a 1x6 socket, carrying the
                  module's outline: 23 x 25 mm, rows 20.32 mm apart, the top
                  pin 6.4 mm below the top edge, the RJ45 16.1 mm wide and
                  overhanging the top edge by 2.7 mm (WIZnet dimension
                  drawing) [V].
  WIZ850io_J2     The J2 row, 20.32 mm to the right of J1. It has no
                  courtyard of its own: it sits under the module, inside J1's.

Pin 1 of each is its origin. Run by render.sh; nobody edits carrier.pretty by
hand.
"""

from __future__ import annotations

import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from sexpr import Sym, dump  # noqa: E402

HERE = Path(__file__).resolve().parent.parent
LIB = HERE / "carrier.pretty"
VERSION = 20221018  # KiCad 7 footprint format
NS = uuid.UUID("0e5b7a52-41c9-4b8e-8f3a-2c6d1b7e9a10")

PITCH = 2.54


def uid(*parts: object) -> str:
    return str(uuid.uuid5(NS, "/".join(str(p) for p in parts)))


class Fp:
    def __init__(self, name: str, descr: str, tags: str) -> None:
        self.name = name
        self.items: list = [
            Sym("footprint"),
            name,
            [Sym("version"), VERSION],
            [Sym("generator"), Sym("pcbnew")],
            [Sym("layer"), "F.Cu"],
            [Sym("descr"), descr],
            [Sym("tags"), tags],
            [Sym("attr"), Sym("through_hole")],
        ]

    def text(self, kind, value, x, y, layer, size=1.0, angle=0, hide=False):
        effects = [Sym("effects"), [Sym("font"), [Sym("size"), size, size],
                                    [Sym("thickness"), round(size * 0.15, 3)]]]
        if hide:
            effects.append(Sym("hide"))
        at = [Sym("at"), x, y] + ([angle] if angle else [])
        self.items.append([Sym("fp_text"), Sym(kind), value, at, [Sym("layer"), layer],
                           effects, [Sym("tstamp"), uid(self.name, kind, value, x, y)]])

    def line(self, x1, y1, x2, y2, layer, width):
        self.items.append([
            Sym("fp_line"), [Sym("start"), x1, y1], [Sym("end"), x2, y2],
            [Sym("stroke"), [Sym("width"), width], [Sym("type"), Sym("solid")]],
            [Sym("layer"), layer], [Sym("tstamp"), uid(self.name, "line", layer, x1, y1, x2, y2)],
        ])

    def rect(self, x1, y1, x2, y2, layer, width):
        for a, b, c, d in ((x1, y1, x2, y1), (x2, y1, x2, y2), (x2, y2, x1, y2), (x1, y2, x1, y1)):
            self.line(a, b, c, d, layer, width)

    def pad(self, number, x, y, square=False, size=1.7, drill=1.0):
        self.items.append([
            Sym("pad"), str(number), Sym("thru_hole"), Sym("rect" if square else "circle"),
            [Sym("at"), round(x, 3), round(y, 3)], [Sym("size"), size, size], [Sym("drill"), drill],
            [Sym("layers"), "*.Cu", "*.Mask"], [Sym("tstamp"), uid(self.name, "pad", number)],
        ])

    def write(self) -> None:
        LIB.mkdir(exist_ok=True)
        (LIB / f"{self.name}.kicad_mod").write_text(dump(self.items) + "\n")


def socket_silk(fp: Fp, x: float, y0: float, n: int) -> None:
    """Outline a 1xn socket strip running down from (x, y0), pin 1 marked."""
    h = PITCH / 2 + 0.06
    fp.rect(round(x - h, 2), round(y0 - h, 2), round(x + h, 2), round(y0 + PITCH * (n - 1) + h, 2),
            "F.SilkS", 0.12)
    fp.line(round(x - h - 0.4, 2), round(y0 - h - 0.4, 2), round(x - h - 0.4, 2), round(y0, 2),
            "F.SilkS", 0.12)


def pico2() -> Fp:
    fp = Fp(
        "Pico2_Socketed",
        "Raspberry Pi Pico 2 on two 1x20 2.54 mm sockets, USB at the top edge. "
        "Pico 2 datasheet, Figure 3: https://datasheets.raspberrypi.com/pico/pico-2-datasheet.pdf",
        "Raspberry Pi Pico 2 RP2350 socket",
    )
    # Board edges relative to pin 1.
    left, top = -1.61, -1.37
    right, bottom = left + 21.0, top + 51.0
    usb_w, usb_over = 8.0, 1.3
    cx = left + 10.5
    fp.text("reference", "REF**", cx, 6.0, "F.SilkS")
    fp.text("value", "Raspberry Pi Pico 2", cx, bottom - 3.0, "F.Fab")
    fp.text("user", "${REFERENCE}", cx, 24.13, "F.Fab")
    for n in range(1, 41):
        if n <= 20:
            x, y = 0.0, PITCH * (n - 1)
        else:
            x, y = 17.78, PITCH * (40 - n)
        fp.pad(n, x, y, square=(n == 1))
    fp.rect(left, top, right, bottom, "F.Fab", 0.1)
    fp.rect(round(cx - usb_w / 2, 2), round(top - usb_over, 2), round(cx + usb_w / 2, 2),
            round(top + 4.0, 2), "F.Fab", 0.1)
    socket_silk(fp, 0.0, 0.0, 20)
    socket_silk(fp, 17.78, 0.0, 20)
    fp.text("user", "USB", cx, 1.6, "F.SilkS", size=0.8)
    pad = 0.25
    fp.rect(round(left - pad, 2), round(top - usb_over - pad, 2), round(right + pad, 2),
            round(bottom + pad, 2), "F.CrtYd", 0.05)
    return fp


# WIZ850io geometry relative to J1 pin 1.
WIZ_LEFT, WIZ_TOP = -1.34, -6.4
WIZ_W, WIZ_H = 23.0, 25.0
WIZ_RJ_W, WIZ_RJ_OVER = 16.1, 2.7
WIZ_ROW_GAP = 20.32


def wiz_j1() -> Fp:
    fp = Fp(
        "WIZ850io_J1",
        "WIZnet WIZ850io, row J1 on a 1x6 2.54 mm socket, with the module outline; "
        "J2 is footprint WIZ850io_J2, 20.32 mm to the right. "
        "https://docs.wiznet.io/Product/ioModule/wiz850io",
        "WIZnet WIZ850io W5500 Ethernet module socket",
    )
    cx = WIZ_LEFT + WIZ_W / 2
    fp.text("reference", "REF**", cx, 17.0, "F.SilkS")
    fp.text("value", "WIZ850io", cx, 19.0, "F.Fab")
    fp.text("user", "${REFERENCE}", cx, 8.0, "F.Fab")
    for n in range(1, 7):
        fp.pad(n, 0.0, PITCH * (n - 1), square=(n == 1))
    fp.rect(WIZ_LEFT, WIZ_TOP, WIZ_LEFT + WIZ_W, WIZ_TOP + WIZ_H, "F.Fab", 0.1)
    fp.rect(round(cx - WIZ_RJ_W / 2, 2), round(WIZ_TOP - WIZ_RJ_OVER, 2),
            round(cx + WIZ_RJ_W / 2, 2), round(WIZ_TOP + 4.0, 2), "F.Fab", 0.1)
    socket_silk(fp, 0.0, 0.0, 6)
    fp.text("user", "RJ45", cx, WIZ_TOP + 2.0, "F.SilkS", size=0.8)
    pad = 0.25
    fp.rect(round(WIZ_LEFT - pad, 2), round(WIZ_TOP - WIZ_RJ_OVER - pad, 2),
            round(WIZ_LEFT + WIZ_W + pad, 2), round(WIZ_TOP + WIZ_H + pad, 2), "F.CrtYd", 0.05)
    return fp


def wiz_j2() -> Fp:
    fp = Fp(
        "WIZ850io_J2",
        "WIZnet WIZ850io, row J2 on a 1x6 2.54 mm socket. Its courtyard is "
        "WIZ850io_J1's, which covers the whole module.",
        "WIZnet WIZ850io W5500 Ethernet module socket",
    )
    fp.text("reference", "REF**", 0.0, 15.5, "F.SilkS")
    fp.text("value", "WIZ850io J2", 0.0, 17.5, "F.Fab")
    for n in range(1, 7):
        fp.pad(n, 0.0, PITCH * (n - 1), square=(n == 1))
    socket_silk(fp, 0.0, 0.0, 6)
    return fp


def main() -> None:
    for fp in (pico2(), wiz_j1(), wiz_j2()):
        fp.write()


if __name__ == "__main__":
    main()
