#!/usr/bin/env python3
"""Water-on tests that record a valve bus around each controller call. These run water.

    bus_tests.py --scope S --controller C --out DIR long-run [--minutes 22]
    bus_tests.py --scope S --controller C --out DIR steps temps|outlets|pause|other-valve
    bus_tests.py --scope S --controller C --out DIR fail-off

Common options: --valve 2 --outlet 1 --temp 96 (°F, refused outside 86-100).

long-run    Starts the outlet, records the bus in back-to-back roll chunks (5 s/div,
            10 Mpt, 125 kSa/s) for --minutes, stops, records two more chunks. No
            status polling during the run. Shows the runtime-timer refresh at 900 s.
steps       One roll record (500 ms/div, 1.25 MSa/s) around each call:
              temps        start at 90 °F, then 95, 99 and 100
              outlets      start on --outlet, add position 2, remove it
              pause        start, stop_shower (pause), start again (resume)
              other-valve  start the other valve only, recording this bus
fail-off    Starts the outlet, prints PULL NOW 20 s later for the operator to cut the
            controller's power, records through the outage and the restart, and
            sends stop_shower as soon as the controller answers again.

Every mode sends stop_shower.cgi at the end whatever happens. Each chunk or step
is saved with dho.grab (full record, as received) and decoded with saturn_decode.py;
the log is DIR/run.log and every controller reply DIR/controller-calls.log.
A quick_shower.cgi call with an empty outlet string for the other valve starts that
valve too, on whatever outlets it has armed (see the 2026-10-24 dataset).
"""

import argparse
import json
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

from dho import Scope, grab, png
from kohler_once import request

HERE = Path(__file__).resolve().parent


class Run:
    def __init__(self, args: argparse.Namespace):
        self.a = args
        self.out: Path = args.out
        self.out.mkdir(parents=True, exist_ok=True)

    def log(self, msg: str) -> None:
        line = f"{datetime.now(timezone.utc).strftime('%H:%M:%S')} {msg}"
        print(line, flush=True)
        with (self.out / "run.log").open("a") as f:
            f.write(line + "\n")

    def call(self, path: str) -> str:
        body = request(self.a.controller, path)
        with (self.out / "controller-calls.log").open("a") as f:
            f.write(f"{time.strftime('%Y-%m-%dT%H:%M:%S%z')} GET {path}\n{body}\n\n")
        self.log(f"{path.split('?')[0]} -> {body[:100]!r}")
        return body

    def quick_shower(self, temp: int, outlets: str | None = None, valve: int | None = None) -> str:
        valve = valve or self.a.valve
        outlets = self.a.outlet if outlets is None else outlets
        v1, v2 = (outlets, "") if valve == 1 else ("", outlets)
        return (f"/quick_shower.cgi?valve_num={valve}&valve1_outlet={v1}&valve1_massage=0&valve1_temp={temp}"
                f"&valve2_outlet={v2}&valve2_massage=0&valve2_temp={temp}")

    def scope(self, *cmds: str) -> None:
        s = Scope(self.a.scope)
        for c in cmds:
            s.send(c)
        s.sock.close()

    def save(self, name: str, screenshot: bool = False) -> None:
        s = Scope(self.a.scope)
        s.send(":STOP")
        meta = grab(s, self.out, name, ["CHANnel1", "CHANnel2"])
        if screenshot:
            png(s, self.out, name)
        s.send(":RUN")
        s.sock.close()
        frames = subprocess.run([sys.executable, str(HERE / "saturn_decode.py"), str(meta)],
                                capture_output=True, text=True).stdout
        (self.out / f"{name}.frames.txt").write_text(frames)
        lines = frames.splitlines()
        commands = [l for l in lines if " 07 00 " not in l[:40] and " 01 01 0B " not in l]
        self.log(f"{name}: {len(lines)} frames, {frames.count('BAD')} bad, {len(commands)} other than polls")
        for l in commands[:8]:
            self.log(f"   {l.strip()}")

    def http_up(self) -> bool:
        try:
            socket.create_connection((self.a.controller, 80), timeout=2).close()
            return True
        except OSError:
            return False


def long_run(r: Run) -> None:
    r.scope(":RUN", ":TRIGger:SWEep AUTO", ":TIMebase:MAIN:SCALe 5", ":TIMebase:MAIN:OFFSet 0", ":ACQuire:MDEPth 10M")
    time.sleep(3)
    n = 0
    try:
        r.call(r.quick_shower(r.a.temp))
        t0 = time.time()
        while time.time() - t0 < r.a.minutes * 60:
            time.sleep(max(0.0, min(50.0, r.a.minutes * 60 - (time.time() - t0))))
            n += 1
            r.save(f"chunk-{n:02d}")
    finally:
        r.call("/stop_shower.cgi")
    for _ in range(2):
        time.sleep(50)
        n += 1
        r.save(f"chunk-{n:02d}")
    r.call("/system_info.cgi")


def steps(r: Run) -> None:
    t = r.a.temp
    other = 1 if r.a.valve == 2 else 2
    plan = {
        "temps": [("start-90", r.quick_shower(90)), ("to-95", r.quick_shower(95)),
                  ("to-99", r.quick_shower(99)), ("to-100", r.quick_shower(100))],
        "outlets": [("start", r.quick_shower(t)), ("add-2", r.quick_shower(t, r.a.outlet + "2")),
                    ("remove-2", r.quick_shower(t))],
        "pause": [("start", r.quick_shower(t)), ("pause", "/stop_shower.cgi"), ("resume", r.quick_shower(t))],
        "other-valve": [("start", r.quick_shower(t, "1", other))],
    }[r.a.which]

    def step(label: str, path: str) -> None:
        r.scope(":RUN")
        time.sleep(2.5)
        r.call(path)
        time.sleep(4)
        r.save(label, screenshot=True)

    r.scope(":RUN", ":TRIGger:SWEep AUTO", ":TIMebase:MAIN:SCALe 0.5", ":TIMebase:MAIN:OFFSet 0", ":ACQuire:MDEPth 10M")
    try:
        for label, path in plan:
            step(label, path)
    finally:
        try:
            step("stop", "/stop_shower.cgi")
        except Exception as e:  # the water must stop even if the scope fails
            r.log(f"capture failed during stop ({e}); stopping without it")
            r.call("/stop_shower.cgi")
    r.call("/system_info.cgi")


def fail_off(r: Run) -> None:
    r.scope(":RUN", ":TRIGger:SWEep AUTO", ":TIMebase:MAIN:SCALe 5", ":TIMebase:MAIN:OFFSet 0", ":ACQuire:MDEPth 10M")
    time.sleep(3)
    n, stopped, seen_down = 0, False, False
    try:
        r.call(r.quick_shower(r.a.temp))
        t_on = time.time()
        told = False
        while time.time() - t_on < 15 * 60:
            time.sleep(25)
            if not told and time.time() - t_on > 20:
                r.log("PULL NOW: cut the controller's power")
                told = True
            n += 1
            r.save(f"chunk-{n:02d}")
            up = r.http_up()
            if not up and not seen_down:
                seen_down = True
                r.log("controller unreachable")
            if seen_down and up:
                r.log("controller reachable again")
                time.sleep(5)
                r.call("/stop_shower.cgi")
                stopped = True
                break
            if not seen_down and time.time() - t_on > 4 * 60:
                r.log("no power cut after 4 minutes")
                break
    finally:
        if not stopped and r.http_up():
            r.call("/stop_shower.cgi")
    for _ in range(2):
        time.sleep(25)
        n += 1
        r.save(f"chunk-{n:02d}")
    if r.http_up():
        r.call("/system_info.cgi")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scope", required=True)
    ap.add_argument("--controller", required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--valve", type=int, choices=[1, 2], default=2)
    ap.add_argument("--outlet", default="1", help="outlet position(s), concatenated")
    ap.add_argument("--temp", type=int, default=96)
    sub = ap.add_subparsers(dest="mode", required=True)
    lr = sub.add_parser("long-run")
    lr.add_argument("--minutes", type=float, default=22)
    st = sub.add_parser("steps")
    st.add_argument("which", choices=["temps", "outlets", "pause", "other-valve"])
    sub.add_parser("fail-off")
    a = ap.parse_args()
    if not 86 <= a.temp <= 100:
        ap.error("--temp must be 86-100")
    run = Run(a)
    run.log(f"{a.mode} {json.dumps({k: str(v) for k, v in vars(a).items()})}")
    {"long-run": long_run, "steps": steps, "fail-off": fail_off}[a.mode](run)


if __name__ == "__main__":
    main()
