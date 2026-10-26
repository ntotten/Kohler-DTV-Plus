#!/usr/bin/env python3
"""Check the netlist KiCad reads from carrier.kicad_sch against the tables in
carrier_schematic.py: every named net holds exactly the pins it should, and
no pin is on a net it shouldn't be. Usage: check_netlist.py <netlist file>."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from carrier_schematic import expected  # noqa: E402
from sexpr import find, find1, parse  # noqa: E402


def read(path: str) -> dict[str, set[str]]:
    tree = parse(Path(path).read_text())[0]
    nets: dict[str, set[str]] = {}
    for net in find(find1(tree, "nets"), "net"):
        name = find1(net, "name")[1].lstrip("/")
        nodes = {f"{find1(n, 'ref')[1]}.{find1(n, 'pin')[1]}" for n in find(net, "node")}
        nodes = {n for n in nodes if not n.startswith("#")}
        if name.startswith("unconnected-") or name.startswith("Net-("):
            for n in nodes:
                nets.setdefault(f"?{name}", set()).add(n)
            continue
        nets[name] = nodes
    return nets


def main() -> int:
    got = read(sys.argv[1])
    want = expected()
    problems = []
    for name, pins in sorted(want.items()):
        have = got.get(name, set())
        if have != pins:
            missing = sorted(pins - have)
            extra = sorted(have - pins)
            problems.append(f"{name}: missing {missing} extra {extra}")
    for name, pins in sorted(got.items()):
        if name not in want and not name.startswith("?"):
            problems.append(f"{name}: not expected, has {sorted(pins)}")
        if name.startswith("?") and any(not p.startswith("A1.") for p in pins) and len(pins) > 1:
            problems.append(f"{name}: unnamed net joins {sorted(pins)}")
    named = set().union(*want.values())
    stray = sorted(p for n, ps in got.items() if n.startswith("?") for p in ps if p in named)
    if stray:
        problems.append(f"pins left off their net: {stray}")
    if problems:
        print("netlist check FAILED")
        for p in problems:
            print("  " + p)
        return 1
    print(f"netlist check passed: {len(want)} nets, {len(named)} pins")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
