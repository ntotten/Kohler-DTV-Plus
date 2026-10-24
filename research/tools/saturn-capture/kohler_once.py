#!/usr/bin/env python3
"""One request to a K-99695 controller, made the way app/server/kohler-client.mjs makes it.

    kohler_once.py --host CONTROLLER /system_info.cgi

Four endpoints only, all within the 2/5 ceiling of app/server/cgi-safety.mjs:
values.cgi and system_info.cgi (0/5), stop_shower.cgi (1/5), quick_shower.cgi
(2/5). It waits until 5 s have passed since its own previous call, the
controller's active polling floor, so it cannot overlap or hammer the web server. Other
clients (a browser tab on the controller, the app's proxy) count against the
controller's two-session limit too; see app/CLAUDE.md.

Prints the reply body. The .cgi replies are HTTP/0.9, a bare body.
"""

import argparse
import os
import socket
import sys
import tempfile
import time
from pathlib import Path

ALLOW = {"/values.cgi", "/system_info.cgi", "/quick_shower.cgi", "/stop_shower.cgi"}
MIN_GAP_S = 5.0


def request(host: str, path: str) -> str:
    if path.split("?")[0] not in ALLOW:
        raise PermissionError(f"{path.split('?')[0]} is not one of {sorted(ALLOW)}")
    stamp = Path(tempfile.gettempdir()) / f"kohler_once.{host}.last"
    try:
        since = time.time() - float(stamp.read_text())
    except (OSError, ValueError):
        since = float("inf")
    if since < MIN_GAP_S:
        time.sleep(MIN_GAP_S - since)
    stamp.write_text(str(time.time()))
    s = socket.create_connection((host, 80), timeout=10)
    s.settimeout(10)
    s.sendall(
        f"GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: dtv-plus-web\r\n"
        f"Accept: */*\r\nConnection: close\r\n\r\n".encode()
    )
    buf = b""
    try:
        while chunk := s.recv(65536):
            buf += chunk
    except (ConnectionResetError, socket.timeout):
        if not buf:  # a reset after the body has arrived is routine for this server
            raise
    finally:
        s.close()
        stamp.write_text(str(time.time()))
    if buf.startswith(b"HTTP/"):
        buf = buf.split(b"\r\n\r\n", 1)[-1]
    return buf.decode("utf-8", "replace")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default=os.environ.get("KOHLER_HOST"), help="controller address (or set KOHLER_HOST)")
    ap.add_argument("path", help='e.g. "/system_info.cgi"')
    args = ap.parse_args()
    if not args.host:
        ap.error("--host or KOHLER_HOST is required")
    sys.stdout.write(request(args.host, args.path))
    return 0


if __name__ == "__main__":
    sys.exit(main())
