# Public-Record Sources

Documentation about this system that exists in the public record — patents and FCC equipment-authorization exhibits. Both are citable, stable, and (unlike vendor pages) do not disappear when a product is discontinued.

---

## FCC filings

Wired devices (the K-99695 controller, the K-99693 interface, the valves) have no radio and **no FCC filings of their own**. The wireless peripherals do, and they reveal the platform family. Exhibits (internal/external photos, manuals, test reports) are linked from each filing's index page.

| FCC ID | Device | Filed | What it shows |
|---|---|---|---|
| [N82-KOHLER010](https://fccid.io/N82-KOHLER010) | DTV+ Amplifier (K-99696) | 2014 | Bluetooth amp/power board; same product generation as the controller |
| [N82-KOHLER021](https://fccid.io/N82-KOHLER021) / [N82-KOHLER022](https://fccid.io/N82-KOHLER022) | UART / RS485 Cloud Module | 2017 | Kohler's first cloud bridge boards — Wi-Fi module + serial to the product |
| [N82-KOHLER032](https://fccid.io/N82-KOHLER032) / [N82-KOHLER033](https://fccid.io/N82-KOHLER033) | UART / RS485 Cloud Module (rev) | 2019 | Chinese Wi-Fi module (SRRC-ID'd) + serial flash + RS-485 connector; through-hole debug header row visible |
| [N82-KOHLER029](https://fccid.io/N82-KOHLER029) | DTV Konnect Module (K-97999) | 2019 | See below |

### The Konnect module (K-97999) is a Linux-class computer

From the public internal photographs of N82-KOHLER029:

| Component | Observation |
|---|---|
| SoC | ARM-class BGA processor |
| Memory | ISSI SDRAM + Kingston NAND flash |
| Storage | **microSD slot** — removable storage; typically means the root filesystem can be read with a card reader |
| Wi-Fi | Shielded module (tested by Laird Connectivity) |
| Board | PCB-1283079, rows of labelled test points |

The install sheet in the same filing gates the Konnect module on a minimum software matrix — the document that lets owners map DTV+ software lines to hardware generations:

| Hardware | Minimum software |
|---|---|
| 99693-P-NA UI | 7.44 |
| 99693-P-NA Eco UI | 8.11 |
| 99695-NA Controller | 3.75 |
| 99695-E-NA Eco Controller | 4.14 |

Used in [devices/touchscreen-ui.md](devices/touchscreen-ui.md#identifying-the-hardware-version) to identify the V2 (Linux) interface.

### What the filings do NOT contain

No shared credentials, update-server details, or controller debug pinouts were published in any exhibit reviewed. The controller's FTP password exists only in its firmware.

---

## The DTV+ patent

**US 9,777,470 B2 — "Shower control system with network features"** (Kohler Co., filed 2015, priority chain to 2010, granted 2017).

Full text: <https://patents.google.com/patent/US9777470B2/en>

This is the only first-party architecture documentation that exists. It describes, in the builder's own words:

| Patent content | Observed system |
|---|---|
| Central controller + one or more control panels + water/steam/audio/lighting/aromatherapy subsystems | K-99695 + K-99693(-P) + valves + peripherals |
| Mixing valves with multiple outlet ports and independently controlled temperature zones | DTV 6-port / Prompt 2-port / Prompt 3-port on the two valve buses |
| Control panel with electronic display + capacitive touch in a waterproof housing | K-99693 wall interface (FIG. 3-4 cross-section) |
| Update data from "an Internet file server" — firmware, control parameters, configuration, user interfaces, spa experiences | The FTP update pipeline ([firmware-update.md](firmware-update.md)) |
| Usage information collected by the controller and reported to the remote system | Explains the upload-status fields in `ftp_status.cgi` |
| Multi-stage spa experiences as stored temperature/flow profiles | The spa/therapy programs in the UI |

The figures double as a UI reference: outlet-icon control screens (FIG. 7-12), warm-up/purge (FIG. 13), steam (FIG. 14-15), lighting (FIG. 16-18), audio (FIG. 19-20), spa (FIG. 21-22), user profiles (FIG. 23), shower-layout programming (FIG. 25-28), temperature zones and therapy (FIG. 29-33).

Priority parent: [US 9,085,881 B2](https://patents.google.com/patent/US9085881B2/en).

> **Note:** read the claims before assuming a feature is free to reimplement in a commercial product. (Informational, not legal advice.)
