#!/usr/bin/env python3
"""Cut a byte-exact window out of a dho.py capture: slice_capture.py SRC.metadata.json DEST.metadata.json T0 T1

Writes DEST's channel .raw files holding samples [i0, i1) of SRC's, unchanged, and a metadata
file that records the window, the slice's own hash, and the full record's size and hash.
"""

import hashlib
import json
import sys
from pathlib import Path


def main() -> None:
    src, dest, t0, t1 = Path(sys.argv[1]), Path(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
    meta = json.loads(src.read_text())
    if any("slice_first_index" in i for i in meta["channels"].values()):
        raise SystemExit(f"{src} is already a slice")
    dest.parent.mkdir(parents=True, exist_ok=True)
    name = dest.name.replace(".metadata.json", "")
    meta["capture"] = name
    for ch, info in meta["channels"].items():
        raw = (src.parent / info["file"]).read_bytes()
        _, _, _, _, xinc, xorig, xref, *_ = map(float, info["preamble"].split(","))
        i0 = max(0, int(round((t0 - xorig) / xinc + xref)))
        i1 = min(len(raw), int(round((t1 - xorig) / xinc + xref)))
        part = raw[i0:i1]
        out = dest.parent / f"{name}.{ch.lower()}.raw"
        out.write_bytes(part)
        info.update({
            "file": out.name,
            "received_byte_count": len(raw),
            "received_sha256": hashlib.sha256(raw).hexdigest(),
            "slice_first_index": i0,
            "byte_count": len(part),
            "sha256": hashlib.sha256(part).hexdigest(),
        })
        info.pop("sha256_full", None)
        info.pop("raw_in_repo", None)
    meta["slice"] = (f"samples [slice_first_index, slice_first_index + byte_count) of the received record, "
                     f"unchanged; window {t0:+.3f} s to {t1:+.3f} s")
    meta["conversion"] = meta["conversion"].replace(
        "seconds = (index - xreference)", "seconds = (slice_first_index + index - xreference)")
    dest.write_text(json.dumps(meta, indent=2) + "\n")
    print(f"{dest.name}: {i1 - i0} points per channel ({t1 - t0:.2f} s)")


if __name__ == "__main__":
    main()
