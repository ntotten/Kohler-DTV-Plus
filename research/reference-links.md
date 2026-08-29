# Reference links

Every external source this archive relies on, annotated. We deliberately
**link rather than copy**: upstream licenses differ, and links keep this
repo's licensing clean. If a link dies, check the Wayback Machine.

## Primary reverse-engineering projects

- **[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus)** —
  the deepest hardware and wire-protocol analysis: ColdFire/NAND/board
  detail, the Saturn / DTV+ / Amulet-CRC protocol docs, boot process, NAND
  flash recovery, the full CGI endpoint reference with safety ratings, error
  codes, timing constants, implementation quirks. Also hosts the controller
  **board photograph** (`Images/`) showing the debug footprints described in
  [../docs/controller-hardware.md](../docs/controller-hardware.md).
- **[aaronse/Kohler-DTV-Plus](https://github.com/aaronse/Kohler-DTV-Plus)** —
  a working replacement web UI (React + Vite) with a hard CGI safety gate
  (`app/server/cgi-safety.mjs` is the risk table this archive cites); a
  verbatim mirror of the controller's own web UI
  (`research/controller-mirror/`, Kohler's shipped HTML/JS); the
  [INVESTIGATIONS.md](https://github.com/aaronse/Kohler-DTV-Plus/blob/master/INVESTIGATIONS.md)
  log of the mid-shower shutoff investigation; and FIELD-NOTES on what breaks
  when you automate a DTV+. Original work there is under the Open Maker
  License.
- **[timelery/Kohler-DTV-Plus](https://github.com/timelery/Kohler-DTV-Plus)** —
  the original 2017 CGI enumeration from which both above descend.

## Community integrations

- [niemyjski/kohler-python](https://github.com/niemyjski/kohler-python) —
  Python API client.
- [niemyjski/homeassistant-kohler](https://github.com/niemyjski/homeassistant-kohler) —
  Home Assistant integration.
- [dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python) —
  endpoint and parameter reference.
- [dcmeglio/hubitat-kohlerdtv](https://github.com/dcmeglio/hubitat-kohlerdtv) —
  Hubitat integration.

## Kohler's own public documents

- K-99695 controller:
  [product page](https://www.kohler.com/en/products/showers/shop-shower-trims-valves/dtv-system-controller-module-99695) ·
  [spec sheet (PDF)](https://techcomm.kohler.com/techcomm/pdf/K-99695_spec_US-CA_Kohler_en.pdf)
- K-99693-P interface:
  [product page](https://www.kohler.com/en/products/showers/shop-shower-trims-valves/dtv-digital-interface-99693-p) ·
  [spec sheet (PDF)](https://techcomm.kohler.com/techcomm/pdf/K-99693-P_spec_US-CA_Kohler_en.pdf)
  (original K-99693:
  [spec sheet (PDF)](https://techcomm.kohler.com/techcomm/pdf/K-99693_spec_US-CA_Kohler_en.pdf))
- DTV+ User Guide 1241234-5 — "Digital Interface and System Controller for
  DTV+" (techcomm.kohler.com; also rendered in aaronse's
  `research/reference/`).
- K-97999 Konnect module
  [product page](https://www.kohler.com/en/products/showers/shop-shower-trims-valves/dtv-kohler-konnect-module-97999).

## FCC public exhibits

Full list and analysis in [../research/fcc-filings.md](../docs/fcc-filings.md).

- [N82-KOHLER010](https://fccid.io/N82-KOHLER010) — DTV+ Amplifier
  (K-99696), 2014.
- [N82-KOHLER021](https://fccid.io/N82-KOHLER021) /
  [N82-KOHLER022](https://fccid.io/N82-KOHLER022) — UART/RS485 Cloud Modules, 2017.
- [N82-KOHLER032](https://fccid.io/N82-KOHLER032) /
  [N82-KOHLER033](https://fccid.io/N82-KOHLER033) — UART/RS485 Cloud Modules, 2019.
- [N82-KOHLER029](https://fccid.io/N82-KOHLER029) — DTV Konnect Module
  (K-97999), 2019 — internal photos + the install sheet with the software
  version matrix.

## Patent literature

- [US 9,777,470 B2 — Shower control system with network
  features](https://patents.google.com/patent/US9777470B2/en) — the DTV+
  system patent. See [../research/patents.md](../docs/patents.md).
- [US 9,085,881 B2](https://patents.google.com/patent/US9085881B2/en) —
  priority parent.

## Platform sources

- **[wk2325272/MQX_3.8.1](https://github.com/wk2325272/MQX_3.8.1)** — a full
  mirror of Freescale's MQX 3.8.1 (BSD-licensed), the exact RTOS family this
  firmware runs. The web-server analysis in
  [../research/firmware-extraction.md](../docs/repair/firmware-extraction-notes.md)
  cites `rtcs/source/httpd/httpd.c` and `httpd_supp.c` here.
- [CISA ICSA-21-119-04](https://www.cisa.gov/uscert/ics/advisories/icsa-21-119-04) —
  the "BadAlloc" advisory covering
  [CVE-2021-22680](https://nvd.nist.gov/vuln/detail/CVE-2021-22680) (MQX
  allocator integer overflow).

## Hardware referenced

- P&E Micro USB Multilink / Cyclone — ColdFire BDM probes (for the J201
  footprint).
- TBLCF (Turbo BDM Light ColdFire) — open-hardware ColdFire BDM interface.
- T48 / TL866II+ — NAND programmers used by the upstream chip-off recovery
  guide.
- Any 3.3 V USB-UART cable — for the J904 console footprint.
