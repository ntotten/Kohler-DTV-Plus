#!/usr/bin/env python3
"""Write the from-to wire list, cabling-wires.csv, from cabling.wireviz.yml:
one row per conductor, naming the pin and signal at each end. The drawing
(cabling.svg) shows the same connections; this is the list to build and check
them by.

Given the schematic's netlist, it also checks the drawing against the board:
both ends of every conductor that is carrier copper must be on one net.

Usage: wire_list.py [netlist]  (needs PyYAML, which WireViz installs)
"""

from __future__ import annotations

import csv
import io
import sys
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).parent))
from sexpr import find, find1, parse  # noqa: E402

HERE = Path(__file__).resolve().parent.parent
SRC = HERE / "cabling.wireviz.yml"
OUT = HERE / "cabling-wires.csv"

# The drawing's connectors that are the carrier's own parts, by reference.
CARRIER = {"J1": "J1", "J2": "J2", "J3": "J3", "J4": "J4", "J5": "J5", "J6P": "J6"}


def expand(spec) -> list:
    """WireViz pin lists: a value, a list, or 'a-b' ranges inside a list."""
    items = spec if isinstance(spec, list) else [spec]
    out = []
    for item in items:
        if isinstance(item, str) and "-" in item:
            a, b = (int(v) for v in item.split("-"))
            out += list(range(a, b + 1, 1 if b >= a else -1))
        else:
            out.append(item)
    return out


def label(connector: dict, pin) -> str:
    pins, labels = connector.get("pins", []), connector.get("pinlabels", [])
    return str(labels[pins.index(pin)]) if pin in pins and pins.index(pin) < len(labels) else ""


def title(name: str, connector: dict) -> str:
    return f"{name} ({connector['type']})" if connector.get("type") else name


def pin_nets(netlist: Path) -> dict[tuple[str, str], str]:
    tree = parse(netlist.read_text())[0]
    return {(find1(node, "ref")[1], find1(node, "pin")[1]): find1(net, "name")[1]
            for net in find(find1(tree, "nets"), "net") for node in find(net, "node")}


def check(rows: list[dict], nets: dict[tuple[str, str], str]) -> None:
    bad = []
    for r in rows:
        a, b = r["From"].split(" ")[0], r["To"].split(" ")[0]
        if a in CARRIER and b in CARRIER:
            na = nets.get((CARRIER[a], str(r["From pin"])))
            nb = nets.get((CARRIER[b], str(r["To pin"])))
            if na is None or na != nb:
                bad.append(f"{r['Cable']} wire {r['Wire']}: {a}-{r['From pin']} is {na}, "
                           f"{b}-{r['To pin']} is {nb}")
    if bad:
        raise SystemExit("cabling.wireviz.yml disagrees with the board:\n  " + "\n  ".join(bad))


def main() -> None:
    doc = yaml.safe_load(SRC.read_text())
    connectors, cables = doc["connectors"], doc["cables"]
    rows = []
    for chain in doc["connections"]:
        steps = [next(iter(step.items())) for step in chain]
        for i in range(1, len(steps) - 1, 2):
            (a, a_pins), (w, w_wires), (b, b_pins) = steps[i - 1], steps[i], steps[i + 1]
            cable = cables[w]
            colours = cable.get("colors", [])
            for pa, wire, pb in zip(expand(a_pins), expand(w_wires), expand(b_pins), strict=True):
                rows.append({
                    "Cable": w,
                    "Cable type": cable.get("type", ""),
                    "Wire": wire,
                    "Colour": colours[wire - 1] if wire <= len(colours) else "",
                    "From": title(a, connectors[a]),
                    "From pin": pa,
                    "From signal": label(connectors[a], pa),
                    "To": title(b, connectors[b]),
                    "To pin": pb,
                    "To signal": label(connectors[b], pb),
                })
    rows.sort(key=lambda r: (int(r["Cable"][1:]), r["Wire"]))
    if len(sys.argv) > 1:
        check(rows, pin_nets(Path(sys.argv[1])))
    buf = io.StringIO()
    w = csv.DictWriter(buf, fieldnames=list(rows[0]), lineterminator="\n")
    w.writeheader()
    w.writerows(rows)
    OUT.write_text(buf.getvalue())
    print(f"wire list: {len(rows)} conductors in {len({r['Cable'] for r in rows})} cables")


if __name__ == "__main__":
    main()
