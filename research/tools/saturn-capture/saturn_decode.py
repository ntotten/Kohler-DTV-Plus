#!/usr/bin/env python3
"""Decode Saturn frames from a two-channel RS-485 capture.

    saturn_decode.py poll-1.metadata.json            frames, with checksum checks
    saturn_decode.py poll-1.metadata.json --bytes    every UART byte instead
    saturn_decode.py capture.csv                     a t_s,CH1,CH2 CSV export works too

Input is a <name>.metadata.json written by dho.py, with its .raw (or .raw.gz)
channel files beside it. The bus is the CH1 - CH2 difference; mark (idle,
logic 1) is CH1 above CH2. Put CH1 on the positive conductor or pass --invert.
UART is 9600 8N1, LSB first, sampled at mid-bit.

Each frame prints its time (seconds, 0 at the trigger), its bytes, and whether
the checksum matches with the sync bytes included ("incl") and/or excluded
("excl"). Saturn framing: AA 55 ADDRESS CONTROL LEN DATA[LEN] CHECKSUM.
Needs numpy.
"""

import argparse
import csv
import gzip
import json
import sys
from pathlib import Path

import numpy as np


def load(path: Path) -> tuple[np.ndarray, np.ndarray]:
    """Time (s) and differential volts, CH1 - CH2."""
    if path.suffix == ".csv":
        with path.open() as f:
            rows = list(csv.reader(f))[1:]
        a = np.array(rows, dtype=float)
        return a[:, 0], a[:, 1] - a[:, 2]
    meta = json.loads(path.read_text())
    volts, t = [], None
    for info in list(meta["channels"].values())[:2]:
        _, _, _, _, xinc, xorig, xref, yinc, yorig, yref = map(float, info["preamble"].split(","))
        f = path.parent / info["file"]
        if not f.exists() and (path.parent / (info["file"] + ".gz")).exists():
            f = path.parent / (info["file"] + ".gz")
        data = gzip.decompress(f.read_bytes()) if f.suffix == ".gz" else f.read_bytes()
        raw = np.frombuffer(data, dtype=np.uint8).astype(np.float32)
        volts.append((raw - yorig - yref) * yinc)
        if t is None:
            t = (np.arange(raw.size) - xref) * xinc + xorig
    n = min(v.size for v in volts)
    return t[:n], volts[0][:n] - volts[1][:n]


def uart(t: np.ndarray, v: np.ndarray, baud: float) -> list[tuple[float, int, bool]]:
    """(start time, byte, stop bit ok) for each byte. Threshold 0 V differential."""
    mark = v > 0
    dt = float(t[1] - t[0])
    per_bit = 1.0 / baud / dt
    starts = np.flatnonzero(mark[:-1] & ~mark[1:]) + 1  # mark -> space edges
    out, next_free = [], 0
    for s in starts:
        if s < next_free:
            continue
        idx = (s + per_bit * (np.arange(10) + 0.5)).astype(int)
        if idx[-1] >= mark.size:
            break
        bits = mark[idx]
        if bits[0]:  # start bit not held: a glitch
            continue
        value = int(sum(int(b) << k for k, b in enumerate(bits[1:9])))
        out.append((float(t[s]), value, bool(bits[9])))
        next_free = s + int(per_bit * 9.5)
    return out


def frames(stream: list[tuple[float, int, bool]]) -> list[tuple[float, list[int]]]:
    out, i = [], 0
    b = [x[1] for x in stream]
    while i + 5 < len(b):
        if b[i] == 0xAA and b[i + 1] == 0x55:
            end = i + 6 + b[i + 4]
            if end <= len(b):
                out.append((stream[i][0], b[i:end]))
                i = end
                continue
        i += 1
    return out


def checks(frame: list[int]) -> str:
    incl = (-sum(frame[:-1])) & 0xFF == frame[-1]
    excl = (-sum(frame[2:-1])) & 0xFF == frame[-1]
    return "incl" if incl else "excl" if excl else "BAD "


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("capture", type=Path)
    ap.add_argument("--baud", type=float, default=9600)
    ap.add_argument("--invert", action="store_true", help="CH2 is the positive conductor")
    ap.add_argument("--bytes", action="store_true", help="print every byte, not frames")
    args = ap.parse_args()
    t, v = load(args.capture)
    if args.invert:
        v = -v
    stream = uart(t, v, args.baud)
    if args.bytes:
        for ts, value, stop in stream:
            print(f"{ts:10.6f}  {value:02X}{'' if stop else '  framing error'}")
        return 0
    for ts, fr in frames(stream):
        print(f"{ts:10.6f}  {checks(fr)}  {' '.join(f'{x:02X}' for x in fr)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
