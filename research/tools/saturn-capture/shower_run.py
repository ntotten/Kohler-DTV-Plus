#!/usr/bin/env python3
"""Record the valve bus while one outlet starts and stops. This runs water.

    shower_run.py --scope SCOPE --controller CONTROLLER --out DIR --valve 2 --outlet 1 --temp 96

Sequence, every controller call at least 5 s from the last:

1. Scope to roll mode (500 ms/div, 10 Mpt: about 6.5 s of record at 1.25 MSa/s) and run.
2. quick_shower.cgi for the chosen valve and outlet position; the other valve gets no outlet.
3. 4 s later, stop the scope and save the record (shower-start).
4. system_info.cgi, to record the running state.
5. Run the scope, stop_shower.cgi, 4 s later stop and save (shower-stop).
6. system_info.cgi again.

stop_shower.cgi is sent whatever happens after step 1, including a failed or
timed-out start. --temp is in the controller's configured unit and is refused
above 100 (°F); water above 109 °F / 43 °C scalds, and the controller's own
maximum is not a safe bound (DISCLAIMER.md).

Channel setup is left as it is: CH1 on the positive conductor, CH2 on the
negative, probe ratios matching the probes. Every controller reply is
appended to DIR/controller-calls.log.
"""

import argparse
import time
from pathlib import Path

from dho import Scope, grab, png
from kohler_once import request


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scope", required=True)
    ap.add_argument("--controller", required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--valve", type=int, choices=[1, 2], required=True)
    ap.add_argument("--outlet", required=True, help="outlet position(s), concatenated: 1, or 13 for 1 and 3")
    ap.add_argument("--temp", type=int, default=96)
    args = ap.parse_args()
    if not 86 <= args.temp <= 100:
        ap.error("--temp must be 86-100")
    args.out.mkdir(parents=True, exist_ok=True)
    log = (args.out / "controller-calls.log").open("a")

    def call(path: str) -> str:
        body = request(args.controller, path)
        log.write(f"{time.strftime('%Y-%m-%dT%H:%M:%S%z')} GET {path}\n{body}\n\n")
        log.flush()
        print(f"{time.strftime('%H:%M:%S')} {path.split('?')[0]} -> {body[:120]!r}")
        return body

    v1 = args.outlet if args.valve == 1 else ""
    v2 = args.outlet if args.valve == 2 else ""
    start = (
        f"/quick_shower.cgi?valve_num={args.valve}"
        f"&valve1_outlet={v1}&valve1_massage=0&valve1_temp={args.temp}"
        f"&valve2_outlet={v2}&valve2_massage=0&valve2_temp={args.temp}"
    )

    scope = Scope(args.scope)
    scope.send(":RUN")
    scope.send(":TIMebase:MAIN:SCALe 0.5")
    scope.send(":TIMebase:MAIN:OFFSet 0")
    scope.send(":ACQuire:MDEPth 10M")
    time.sleep(2.5)
    try:
        call(start)
        time.sleep(4)
        scope.send(":STOP")
        grab(scope, args.out, "shower-start", ["CHANnel1", "CHANnel2"])
        png(scope, args.out, "shower-start")
        call("/system_info.cgi")
        scope.send(":RUN")
        time.sleep(2.5)
    finally:
        call("/stop_shower.cgi")
    time.sleep(4)
    scope.send(":STOP")
    grab(scope, args.out, "shower-stop", ["CHANnel1", "CHANnel2"])
    png(scope, args.out, "shower-stop")
    call("/system_info.cgi")


if __name__ == "__main__":
    main()
