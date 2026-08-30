# Kohler DTV+ controller protocol

Reverse-engineered from a live **K-99695** system controller (firmware
`0.0.3.89`) at `192.168.0.115`, by mirroring the controller's own web UI and
reading its JavaScript. Cross-checked against
[dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python) and
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) (vendored at
[research/xagon0/](research/xagon0/)).

Nothing here is documented or supported by Kohler.

> ⚠️ **[DISCLAIMER.md](DISCLAIMER.md) applies to everything on this page.**
> Several of these endpoints can wedge or brick the controller, and this system
> controls water hot enough to scald.

## Transport — read this first

The controller runs **MQX HTTP – Freescale Embedded Web Server**. Two quirks
will break a normal HTTP client:

1. **`.cgi` endpoints reply in HTTP/0.9** — a bare body with *no status line and
   no headers*. Node's `http`/`fetch` and Python's `requests` both reject this
   (`Received HTTP/0.9 when not allowed`). Static files (`.html`, `.js`, `.png`)
   *do* get a normal `HTTP/1.0 200 OK`.

   `curl` needs `--http0.9`. In this repo the fix is a raw TCP client:
   [app/server/kohler-client.mjs](app/server/kohler-client.mjs).

2. **Only two concurrent HTTP sessions are supported.** Exceeding it hangs the
   web server for roughly 20 seconds. Browser tabs, polling and scripts all
   count. Serialise every call and leave a gap between them (~120 ms works).

There is **no authentication** of any kind. Anything on the LAN can drive the
shower.

Some replies are Python `repr` rather than strict JSON — `True`/`False`/`None`
and single quotes. Parse defensively.

## Endpoints

Ratings use the 0-5 scale in [DISCLAIMER.md](DISCLAIMER.md). This repo enforces
a ceiling of **2/5** in [app/server/cgi-safety.mjs](app/server/cgi-safety.mjs);
that file is the authoritative table and rates ~50 endpoints.

A rating describes an endpoint, not its arguments, so the same table also
declares the **parameters** each exposed endpoint accepts and the values each
may take. Anything not named is refused with the same `403`. The parameter
names and ranges below are the controller's own, read from its web UI
([research/controller-mirror/js/](research/controller-mirror/js/)).

### Read

| Endpoint | Risk | Returns |
| --- | --- | --- |
| `values.cgi` | 0/5 | ~303 keys: full configuration plus coarse state. |
| `system_info.cgi` | 0/5 | 39 keys: live status the wall interface polls. |
| `languages.cgi` | 0/5 | Installed language packs. |
| `powerclean_check.cgi` | 3/5 | **Blocked.** Documented as able to *trigger* the steam power-clean cycle, not merely report it. |
| `mac.cgi`, `serial.cgi` | 3/5 | **Blocked.** Documented as causing system lockups. Both returned empty on this unit; the MAC is in `values.cgi` anyway. |

### Commands

All are `GET` with query parameters. A successful command returns `:)`.

| Endpoint | Risk | Parameters |
| --- | --- | --- |
| `quick_shower.cgi` | 2/5 | `valve_num`, `valve1_outlet`, `valve1_massage`, `valve1_temp`, `valve2_outlet`, `valve2_massage`, `valve2_temp` |
| `stop_shower.cgi` | 1/5 | — |
| `start_user.cgi` | 2/5 | `user` (1-6) |
| `stop_user.cgi` | 1/5 | — |
| `steam_on.cgi` | 2/5 | `temp`, `time` |
| `steam_off.cgi` | 1/5 | — |
| `music_on.cgi` / `music_off.cgi` | 1/5 | `volume` (0-100) |
| `light_on.cgi` | 1/5 | `module`, `intensity` (0-100) |
| `light_off.cgi` | 1/5 | `module` |
| `rain_on.cgi` | 1/5 | `mode=1&color=…` (hue 0-360, `-1` white) or `mode=2&effect=…` (0-7) |
| `rain_off.cgi` | 1/5 | — |
| `save_variable.cgi` | 2/5 | `index` (1-105), `value` — persistent config write; see [index table](research/controller-mirror/js/values.js). **This proxy accepts index 43 only** — see below. |

### `save_variable.cgi` is a write-anything

The rating is the endpoint's, and the endpoint is generic: one call writes any
of 105 persistent configuration variables. Rated against the write this app
makes — the amplifier's stored volume, index 43 — it is a 2/5. Rated against
what it *can* write it is not, and the indices differ enormously in consequence:

| Index | Variable | Consequence |
| --- | --- | --- |
| 39 | `valve_max_temp` | Raises the maximum temperature the valve will deliver |
| 41 | `valve_auto_purge` | Turns the cold-water purge on or off |
| 61, 62 | `six_port_calibration_valve1/2` | Factory calibration; wrong values move real water temperature away from the setpoint |
| 86, 88 | `wifi_password`, `wifi_SSID` | Network credentials |
| 99 | `max_valve_runtime` | Maximum runtime before the valve shuts off |

So the proxy allowlists the parameter as well as the endpoint: `index` must be
`43` and must be present, `value` must be 0-100. The other 104 indices are
refused before a packet leaves the machine. This is the constraint that makes
DISCLAIMER.md's "clamps to that limit; it never raises it" true of the proxy and
not only of the UI.

The vendored docs disagree on the spelling — `index`/`value` in
[cgi-endpoints.md](research/xagon0/docs/web-interface/cgi-endpoints.md),
`idx`/`val` in
[temperature-system.md](research/xagon0/docs/control-logic/temperature-system.md).
The controller's own JavaScript uses `index`/`value`, and the gate refuses every
name it does not recognise, so the question does not have to be settled on live
hardware to be safe. **Unverified either way on this unit.**

### Do not call

Rated 4/5 or 5/5 — permanently unreachable through this repo's proxy:

`reset_factory.cgi`, `clear_dt.cgi`, `fileupload.cgi`, `unpack_bin.cgi` (5/5);
`edit_dt.cgi`, `rpc.cgi`, `set_device.cgi`, `swapvalves.cgi`,
`forget_devices.cgi`, `reset_default.cgi`, `reset_users.cgi`,
`save_default.cgi` (4/5).

Also blocked at 3/5: `saveDT.cgi`, `saveUI.cgi`, `check_updates.cgi`,
`hiding.cgi`, `remove_module.cgi`, the `reset_*fault` family, `reset_user.cgi`.

### Lock flags

Configuration writes may be silently ignored while water is running:

| Flag | Value | Meaning |
| --- | --- | --- |
| `CGI_SHOWER_START` | `0x01` | A shower start is in progress. |
| `CGI_SHOWER_LOCK` | `0x02` | The shower is actively running. |

## Starting the shower

`quick_shower.cgi` takes the **complete desired state on every call**, so it is
equally the "start", "change outlets" and "change temperature" command.

`valve1_outlet` is the selected outlet positions **concatenated into one
string** — outlets 1, 3 and 4 are sent as `134`. An empty string means none; the
controller's own UI calls `stop_shower.cgi` instead of sending an empty set.

```
GET /quick_shower.cgi?valve_num=1
      &valve1_outlet=134&valve1_massage=0&valve1_temp=101
      &valve2_outlet=&valve2_massage=0&valve2_temp=100
```

`valve_num` selects which valve the call is *for*; parameters for both valves are
sent regardless. For a two-valve system the controller's UI issues the call
twice, once per valve.

Massage modes: `0` off, `1` single, `2` wave, `3` custom 1, `4` custom 2.

> **Conflict.** xagon0 documents `1 = wave, 2 = single`. This controller's own
> `control.html` labels the options `1 = Single, 2 = Wave`. We follow the
> controller, since that is the code the hardware shipped with. Worth verifying
> on your own unit before relying on it.

Temperature is in the system's **configured unit** (`values.units`: `0` = °F,
`1` = °C) — the controller's own UI sets its input bounds to 86-`max_temp` when
the system is in °F and 26-`max_temp` when in °C, and passes the value straight
through. xagon0 documents this parameter as Celsius, which most likely reflects a
Celsius-configured unit rather than a protocol difference.

> ⚠️ **Scald risk.** `max_temp` is whatever the installer configured — 113 °F on
> this system, which is *above* the 109 °F / 43 °C scald threshold. The
> controller's limit is therefore not a safety guarantee. Clamp to `max_temp`,
> never raise it, and verify real water temperature with a thermometer. Note also
> that the reported temperature is the valve's own thermistor reading, not an
> independent measurement.

## Reading state

### What "on" means

`system_info.cgi`'s `valve1outlet1..6` booleans are the **armed selection, not
water flow** — the default outlet reads `true` while the shower is idle. Use
`values.shower_on` or `system_info.ui_shower_on` to decide whether water is
actually moving.

### Useful fields

`system_info.cgi`:

| Field | Meaning |
| --- | --- |
| `ui_shower_on`, `ui_steam_running` | Water / steam actually running. |
| `valve1Setpoint` | Target temperature. |
| `valve1outlet1..6` | Armed outlet selection (see above). |
| `valve1_massage` | Current massage mode. |
| `valve1_Currentstatus` | Free text, e.g. `Off`. |
| `musicStatus`, `volStatus`, `muteStatus` | Amplifier. |
| `steamTimeStatus`, `steamTimeMinutes` | Steam remaining. |
| `devices_running` | Anything at all running. |

`values.cgi`:

| Field | Meaning |
| --- | --- |
| `one_type`..`six_type` | Valve 1 fitting per position, `outlet_0`..`outlet_23`. |
| `one_massage`..`six_massage` | Whether that position participates in massage. |
| `v2_*` | Same for valve 2. |
| `def_temp`, `max_temp` | Default and maximum temperature. |
| `def_outlet` | Default outlet position. |
| `user_1..6`, `user_N_enabled` | Presets. |
| `*_con_string` | Component health: `conn`, `not_seen`, `dis`. |
| `num_interface` | Number of wall interfaces the controller can see. |

### Outlet types

`outlet_N` maps to a fitting icon, not a text label — Kohler ships no names.
Derived from the controller's own art (`/images/outlets/N_on.png`):

| N | Fitting | N | Fitting |
| --- | --- | --- | --- |
| 0 | not assigned | 12 | bodyspray, round |
| 1-2 | showerhead | 13-14 | bodysprays (2) |
| 3-4 | two showerheads | 15-16 | bodysprays (3) |
| 5-6 | rainhead | 17-18 | foot spray |
| 7-8 | handshower | 19-20 | foot sprays (2) |
| 9-10 | bath spout | 21-22 | foot sprays (3) |
| 11 | bodyspray, square | 23 | Real Rain |

Types 9, 10 and 23 cannot take part in massage — the controller's own UI
excludes them regardless of the per-outlet massage flag.

## This system

Captured 2026-07-26 from `192.168.0.115`:

- Valve 1, six-port, four outlets configured: showerhead (`outlet_2`), Real Rain
  (`outlet_23`), handshower (`outlet_8`), bodyspray (`outlet_12`). Default is
  position 3, handshower.
- Massage enabled on positions 1, 3, 4 — Real Rain is excluded by design.
- 96 °F default, 113 °F maximum.
- K-99696 amplifier connected. No steam, lighting or rain panel.
- `num_interface = 0` and `ui1_con_string = not_seen` — **the K-99693 wall
  interface is not seen by the controller**, which is the fault this repo works
  around. Valve, amplifier and controller all report `conn`.

Raw captures: [values.cgi](research/controller-mirror/values-live.json),
[system_info.cgi](research/controller-mirror/si.json).

## Further reading

The vendored [research/xagon0/](research/xagon0/) tree goes well beyond the web
interface — RS-485 wire protocols (Saturn for valves, Amulet for the
touchscreen), the datatable layout, error codes, boot process and NAND recovery.
Start at [research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md) for
licensing and the points where it disagrees with this system.
