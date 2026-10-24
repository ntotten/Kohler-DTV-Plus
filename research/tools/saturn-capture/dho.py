#!/usr/bin/env python3
"""Rigol DHO800/DHO900 over SCPI on raw TCP: commands, full-memory waveforms, screenshots.

    dho.py --host SCOPE query "*IDN?" ":CHANnel1:SCALe?"     print each reply
    dho.py --host SCOPE send ":SINGle"                        commands without replies
    dho.py --host SCOPE grab --out DIR --name poll-1          CH1 + CH2, full memory, as received
    dho.py --host SCOPE png --out DIR --name poll-1           screenshot

`grab` needs the scope stopped (after :SINGle triggers, or :STOP). It writes
<name>.channel1.raw and <name>.channel2.raw, the BYTE samples exactly as
:WAVeform:DATA? returned them, and <name>.metadata.json with the preamble,
the scope settings, timestamps and SHA-256. saturn_decode.py reads that set.

Scope behaviour this relies on, found on firmware 00.01.05:
- :WAVeform:MODE RAW returns nothing; MAXimum returns the full memory once stopped.
- The preamble's point count follows :WAVeform:STARt/STOP, and a window left
  past the end of a shorter record blocks both, so each channel passes
  through NORMal mode (which resets them) before the full-depth read.
- Timebase changes are refused while stopped (the record is fixed); send :RUN first.
- 50 ms/div and slower is roll mode, which never triggers but can be stopped and read.
- Changing a channel's probe ratio rescales the trigger level with it.
"""

import argparse
import hashlib
import json
import os
import socket
import sys
from datetime import datetime, timezone
from pathlib import Path

PORT = 5555
CHUNK = 250_000
SETTINGS = [
    "*IDN?", ":ACQuire:MDEPth?", ":ACQuire:SRATe?", ":ACQuire:TYPE?",
    ":TIMebase:MAIN:SCALe?", ":TIMebase:MAIN:OFFSet?",
    ":TRIGger:MODE?", ":TRIGger:SWEep?", ":TRIGger:EDGE:SOURce?", ":TRIGger:EDGE:SLOPe?", ":TRIGger:EDGE:LEVel?",
]
CHAN_SETTINGS = ["SCALe", "OFFSet", "PROBe", "COUPling", "BWLimit"]


class Scope:
    def __init__(self, host: str, port: int = PORT, timeout: float = 10.0):
        self.timeout = timeout
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)

    def send(self, cmd: str) -> None:
        self.sock.sendall(cmd.encode() + b"\n")

    def _exact(self, n: int) -> bytes:
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(min(65536, n - len(buf)))
            if not chunk:
                raise ConnectionError("scope closed the connection")
            buf += chunk
        return buf

    def query(self, cmd: str) -> str:
        self.send(cmd)
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = self.sock.recv(65536)
            if not chunk:
                break
            buf += chunk
        return buf.decode(errors="replace").strip()

    def query_block(self, cmd: str) -> bytes:
        """An IEEE 488.2 definite-length block, #<n><length><data>, without its header."""
        self.send(cmd)
        head = self._exact(2)
        if head[:1] != b"#":
            raise ValueError(f"expected a block, got {head!r}")
        length = int(self._exact(int(head[1:2])))
        data = self._exact(length)
        self.sock.settimeout(0.5)
        try:
            self.sock.recv(16)  # trailing newline
        except socket.timeout:
            pass
        finally:
            self.sock.settimeout(self.timeout)
        return data


def stamp() -> tuple[str, str]:
    t = datetime.now(timezone.utc)
    return t.isoformat(timespec="seconds").replace("+00:00", "Z"), t.astimezone().isoformat(timespec="seconds")


def grab(scope: Scope, out: Path, name: str, chans: list[str]) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    meta: dict = {"capture": name, "transport": f"SCPI over raw TCP, port {PORT}", "channels": {}}
    meta["started_utc"], meta["started_local"] = stamp()
    meta["scope_settings"] = {q: scope.query(q) for q in SETTINGS}
    for ch in chans:
        for k in CHAN_SETTINGS:
            meta["scope_settings"][f":{ch}:{k}?"] = scope.query(f":{ch}:{k}?")
    for ch in chans:
        scope.send(f":WAVeform:SOURce {ch}")
        # A read window left over from a deeper record (STOP beyond the current
        # depth) makes every STARt/STOP change fail. Passing through NORMal
        # resets it, and MAXimum then opens it to the full record.
        scope.send(":WAVeform:MODE NORMal")
        scope.send(":WAVeform:MODE MAXimum")
        scope.send(":WAVeform:FORMat BYTE")
        depth = int(float(scope.query(":ACQuire:MDEPth?")))
        scope.send(":WAVeform:STARt 1")
        scope.send(f":WAVeform:STOP {depth}")
        preamble = scope.query(":WAVeform:PREamble?")
        points = int(float(preamble.split(",")[2]))
        raw = b""
        for start in range(1, points + 1, CHUNK):
            scope.send(f":WAVeform:STARt {start}")
            scope.send(f":WAVeform:STOP {min(start + CHUNK - 1, points)}")
            raw += scope.query_block(":WAVeform:DATA?")
        if not raw:
            raise RuntimeError(f"{ch}: no data; is the scope stopped with a capture?")
        path = out / f"{name}.{ch.lower()}.raw"
        path.write_bytes(raw)
        meta["channels"][ch] = {
            "file": path.name,
            "waveform_mode": "MAXimum",
            "waveform_format": "BYTE",
            "read": f":WAVeform:DATA? in chunks of {CHUNK} points",
            "preamble": preamble,
            "byte_count": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest(),
        }
        print(f"{ch}: {len(raw)} points -> {path}")
    meta["completed_utc"], meta["completed_local"] = stamp()
    meta["conversion"] = (
        "preamble = format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference; "
        "volts = (byte - yorigin - yreference) * yincrement; "
        "seconds = (index - xreference) * xincrement + xorigin, 0 s at the trigger"
    )
    meta_path = out / f"{name}.metadata.json"
    meta_path.write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {meta_path}")
    return meta_path


def png(scope: Scope, out: Path, name: str) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    data = scope.query_block(":DISPlay:DATA? PNG")
    path = out / f"{name}.png"
    path.write_bytes(data)
    print(f"wrote {path}")
    return path


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default=os.environ.get("DHO_HOST"), help="scope address (or set DHO_HOST)")
    ap.add_argument("--port", type=int, default=PORT)
    sub = ap.add_subparsers(dest="cmd", required=True)
    q = sub.add_parser("query")
    q.add_argument("commands", nargs="+")
    s = sub.add_parser("send")
    s.add_argument("commands", nargs="+")
    g = sub.add_parser("grab")
    g.add_argument("--out", type=Path, required=True)
    g.add_argument("--name", required=True)
    g.add_argument("--chans", nargs="+", default=["CHANnel1", "CHANnel2"])
    p = sub.add_parser("png")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--name", required=True)
    args = ap.parse_args()
    if not args.host:
        ap.error("--host or DHO_HOST is required")
    scope = Scope(args.host, args.port)
    if args.cmd == "query":
        for c in args.commands:
            print(f"{c} -> {scope.query(c)}")
    elif args.cmd == "send":
        for c in args.commands:
            scope.send(c)
        print(f"errors: {scope.query(':SYSTem:ERRor?')}")
    elif args.cmd == "grab":
        grab(scope, args.out, args.name, args.chans)
    else:
        png(scope, args.out, args.name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
