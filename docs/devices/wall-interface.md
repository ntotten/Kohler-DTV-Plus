# K-99693 / K-99693-P wall interface

The DTV+ "digital interface" is the portrait touchscreen on the shower wall.
It looks like a dumb terminal; it is actually a networked peer that can run
the whole shower by itself.

## Variants — which one is which

Kohler shipped two hardware generations under almost the same name, and the
version numbers are the only reliable way to tell them apart:

|                  | **V1 "Amulet"**                          | **V2 (current, K-99693-P)**                                            |
| ---------------- | ---------------------------------------- | ---------------------------------------------------------------------- |
| Processor        | ColdFire MCF52252 (Amulet GUI platform)  | Newer SoC platform (unidentified; upstream calls it "RFS-based")       |
| Software line    | `0.1.x` / 3.7x — file `ui_amulet_v*.S19` | `0.0.7.xx` — file `dtvplus2_uiapp_v*.pack.tar` (the **Linux UI pack**) |
| Graphics         | basic                                    | enhanced                                                               |
| Update transport | S-record push                            | **chunked file transfer with MD5 verification**                        |

**Determination made easy:** Kohler's Konnect-module install sheet (a public
FCC exhibit — see [../research/fcc-filings.md](../fcc-filings.md))
lists a version matrix requiring "99693-P-NA UI sw **7.44**". A K-99693-P
reporting `amulet_version_string = 0.0.7.44` matches the Linux pack
`dtvplus2_uiapp_v0.0.7.44.pack.tar` exactly — the `amulet_*` field name is a
legacy label that outlived the hardware change. If your panel reports a
`0.0.7.x` string, it is the V2/Linux variant.

The panel reports four component versions, implying at least four updatable
parts: UI app (`0.0.7.44`), coprocessor (`0.0.1.8`), language pack
(`0.1.1.0`), and touch controller (`0.0.0.2`).

## The link — not Ethernet, despite the cable

The interface connects to the controller with up to 25 ft of RJ45-terminated
cable (plus an in-line coupler). **Nothing about it is Ethernet.** It is a
UART at **115200 8N1**:

1. **Discovery** — the controller's finder task locates the panel on the
   DTV+ bus as device `0x30` (v1) / `0x31` (v2).
2. **Data exchange** — from then on, everything moves to the **Amulet CRC**
   protocol: the controller pushes datatable variables to the panel
   (`SET_BYTE_VAR` / `SET_WORD_VAR` / `SET_STRING_VAR`), and the panel sends
   user actions back as `INVOKE_RPC`.

Two consequences worth knowing:

- **The panel consumes none of the controller's two-session HTTP budget.**
  (A browser open on the controller's web page does; the wall panel never
  touches HTTP.)
- **The panel can command the shower with no web trace at all.** Its
  `INVOKE_RPC` frames start/stop the water directly on the bus. When
  debugging "who turned the shower on/off", an empty HTTP log means "no web
  client did it" — never "nobody did it".

## Firmware updates to the panel

The controller stages UI images in its `a:\images\` directory and pushes them
over the UI link:

1. `SET_FILE_TRANSFER (0x20)` — name + size
2. `WRITE_LARGE_DATA` — chunked data, each chunk ACKed
3. `FLUSH_MD5 (0x21)` — MD5 of the complete file
4. `FILE_COMPLETE (0x22)` — panel verifies the hash, then applies

A failed MD5 discards the file and the transfer restarts from the beginning.

## Behavior notes

- Auto-dim after ~60 s idle (configurable); any touch restores brightness.
- Start screen is configurable (home / user selection / quick start /
  settings); beep, interface name, and user lock are `save_variable.cgi`
  indices 28–31 and 97.
- Error modals (require acknowledgement): _Water Too Hot_ (valve outlet
  overtemperature), _Steam Error_, _Valve Offline_, _Device Not Found_.
- Up to **three** interfaces per system; all share the datatable.
- Rated to 140 °F / 60 °C ambient; never mount above a steamhead; ADA
  compliant when installed per spec; bracket is K-99694.

## Teardown caution (from a documented repair)

The factory installation silicone-seals the housing to the wall _including
the blue seal plug_. Pulling the panel off the wall can leave the plug
attached to the wall and **drag the internal wire-to-board connector out of
its socket** — presenting as a dead interface (`num_interface = 0`). The
controller logs it as a `DETACH_EVENT` (code 100) naming the UI device byte.
The fix is a reseat, not a replacement. See the upstream investigation log
(linked in [../reference/links.md](../../research/reference-links.md)).

## If yours dies

The controller keeps working with no panel attached — it just has nothing
local to command it. The web API covers full control, and a working
community replacement UI exists (see
[../reference/links.md](../../research/reference-links.md)). A dead panel is an
inconvenience, not a system failure.
