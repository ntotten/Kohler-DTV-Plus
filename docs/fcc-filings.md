# FCC filings for the DTV+ family

The FCC's equipment-authorization database is a public record, and for this
product family it contains internal photographs, user documentation, and test
reports. Wired devices (the K-99695 controller, the K-99693 interface, the
valves) have no radio and therefore **no FCC filings of their own** — but the
wireless peripherals do, and they reveal the platform family.

Links below are to the filing index pages; exhibits (internal/external
photos, manuals) are linked from each page.

## The filings that matter

| FCC ID                                                                                   | Device                              | Filed | What it shows                                                                                                                                      |
| ---------------------------------------------------------------------------------------- | ----------------------------------- | ----- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| [N82-KOHLER010](https://fccid.io/N82-KOHLER010)                                          | **DTV+ Amplifier** (K-99696)        | 2014  | Bluetooth amp/power board. Same product generation as the controller; mostly analog/audio design.                                                  |
| [N82-KOHLER021](https://fccid.io/N82-KOHLER021) / [-022](https://fccid.io/N82-KOHLER022) | **UART / RS485 Cloud Module**       | 2017  | Kohler's first cloud bridge boards — Wi-Fi module + serial to the product.                                                                         |
| [N82-KOHLER032](https://fccid.io/N82-KOHLER032) / [-033](https://fccid.io/N82-KOHLER033) | **UART / RS485 Cloud Module** (rev) | 2019  | The 2019 revision: a Chinese Wi-Fi module (SRRC-ID'd, MXCHIP-class) + serial flash + RS-485 connector; a through-hole debug header row is visible. |
| [N82-KOHLER029](https://fccid.io/N82-KOHLER029)                                          | **DTV Konnect Module** (K-97999)    | 2019  | The DTV+ cloud bridge. See below.                                                                                                                  |

## The Konnect module (K-97999) is a Linux-class computer

From the public internal photographs of N82-KOHLER029:

- ARM-class BGA SoC + ISSI SDRAM + Kingston NAND flash
- A **microSD slot** — removable storage, which typically means the root
  filesystem lives on media you can read with a card reader
- Wi-Fi via a shielded module (tested by Laird Connectivity)
- PCB-1283079; rows of labelled test points

The install sheet in the same filing matters just as much: it gates the
Konnect module on a **minimum software matrix** —

| Hardware                  | Minimum software |
| ------------------------- | ---------------- |
| 99693-P-NA UI             | **7.44**         |
| 99693-P-NA Eco UI         | 8.11             |
| 99695-NA Controller       | 3.75             |
| 99695-E-NA Eco Controller | 4.14             |

— which is the document that lets us map DTV+ software lines to hardware
generations (see [../docs/wall-interface.md](devices/wall-interface.md)).

## Why this matters

- **Repair:** the peripherals' debug footprints and storage layouts are
  public-record knowledge, and the Konnect's microSD card is the easiest
  possible firmware source for that module.
- **License compliance:** both the Konnect module and the V2 wall interface
  are Linux-class systems. To our knowledge Kohler has never published the
  corresponding source or written offers. If you own these devices, you have
  standing to ask for it.
- **No shared secrets leaked:** we reviewed the public exhibits for shared
  credentials, update-server details, or debug pinouts applicable to the
  controller. None were published. The controller's FTP password exists only
  in its firmware.

## Also in the database

Kohler's broader connected-product family (intelligent toilets, H2Wise water
monitors, voice mirrors, DTV Mode, and the newer "Kohler Ventures" line) all
have filings under grantee codes N82 / 2APQB / 2AELD / 2AKPV / 2BMN9. They
postdate the DTV+ and use different platforms; they are catalogued for
completeness in the search results but not analyzed here.
