# Kohler DTV+ recovery reference

This directory contains a pre-investigation snapshot of the Kohler DTV+
controller at `192.168.4.80`. It was captured on 2026-08-22 before any valve
actuation or configuration change.

The primary recovery artifact is
[`2026-08-22-idle-baseline/controller-config.json`](../../research/diagnostics/2026-08-22-idle-baseline/controller-config.json).
It contains the complete 307-key `/values.cgi` response, `/system_info.cgi`,
the real-versus-simulated attachment state, a parsed full datatable snapshot,
capture times, and hashes of the raw source files. The unmodified endpoint
responses are alongside it. `manifest.json` records the request policy and
source metadata.

For what the hardware _is_ — processor, buses, protocols, error model, CGI
transport quirks and our own configuration decoded — see
[system-specification.md](../system-specification.md). This file is the restore procedure; that one is the
reference.

This material contains private household data, including user names, network
details, and controller pairing credentials. Keep the repository private and
do not publish or paste the raw files into public issues.

## Important recovery boundary

Do not replay `datatable.json` with `edit_dt.cgi`. The datatable mixes stored
configuration with live state, error flags, and ghost variables. The existing
safety classification rates direct datatable writes 4/5 and the factory-reset
endpoint 5/5. A blind replay could overwrite runtime state or destabilize the
controller.

If a reset is required, initiate it locally through the Kohler installer or
service workflow with an operator present. Do not call `reset_factory.cgi`
remotely. Restore through the supported installer UI, one category at a time,
and verify each category with one slow, connection-closing read. Stop if the
controller becomes slow or unreachable.

## Known-good topology

- One wall interface (`ui1`), named `un-named UI`.
- Zone 1: six-port valve, five outlets configured. Valve firmware `0.12`.
- Zone 2: three-port Prompt valve, all three outlets configured. Valve
  firmware `0.14`.
- Controller firmware `0.0.3.89`.
- Wall-interface firmware: Amulet `0.0.7.44`, coprocessor `0.0.1.8`, language
  `0.1.1.0`, touch `0.0.0.2`.
- No steam, music, amplifier, lighting, rainpanel, watertile, or Konnect module
  is installed.

Firmware versions are identification evidence, not values to write. Do not
downgrade a replacement component merely to match this snapshot.

## Restore procedure after a factory reset

1. Disconnect any diagnostic client and make sure no browser or automation is
   polling the controller.
2. Re-establish the network locally. The captured controller MAC is
   `00:14:6F:0F:37:82`; its address was `192.168.4.80`, gateway
   `192.168.4.1`, and Wi-Fi was off. Prefer restoring the address with the
   router's DHCP reservation instead of forcing an unverified controller-side
   static setting.
3. In installer setup, detect and assign the single wall interface, the Zone 1
   six-port valve, and the Zone 2 three-port Prompt valve. Confirm all three
   show connected before continuing.
4. Restore Fahrenheit units, `mm/dd/yy` date format, 12-hour time, and daylight
   saving enabled. Correct the controller's timezone if possible: the captured
   wall clock matched local time, but its rendered offset was `-0500` while the
   workstation was on EDT (`-0400`).
5. Restore Zone 1:
   - Five used outlets on ports 1-5; port 6 unused.
   - Outlet function records, in port order: `{func:17,id:3}`,
     `{func:17,id:5}`, `{func:6,id:1}`, `{func:17,id:2}`,
     `{func:17,id:4}`.
   - Default temperature `106°F`; maximum temperature `113°F`.
   - Default outlet raw value `1`; calibration code `173`.
   - Cold-water shutoff disabled (raw value `4`).
   - Maximum runtime disabled (`enable=0`, selection `0`).
   - Automatic purge disabled. Ports 1-5 are purge-eligible in the snapshot,
     but that eligibility does not enable automatic purge.
6. Restore Zone 2:
   - Three used outlets on ports 1-3.
   - Outlet function records, in port order: `{func:2,id:1}`,
     `{func:17,id:2}`, `{func:17,id:3}`.
   - Default temperature `101°F`; maximum temperature `113°F`.
   - Default outlet raw value `0`; calibration code `160`.
   - Cold-water shutoff disabled (raw value `4`).
   - Maximum runtime disabled (`enable=0`, selection `0`).
   - Automatic purge disabled. Ports 1-3 are purge-eligible in the snapshot,
     but that eligibility does not enable automatic purge.
7. Restore the two active user presets (`Nate` and `Amy`). Presets 3-5 were
   disabled; preset 6 was enabled. Use `controller-config.json` for all preset
   temperatures, outlet selections, names, and any interface-specific display
   details rather than guessing from the raw outlet function numbers.
8. Restore general settings: saved defaults enabled, massage feature enabled
   but no individual outlet marked as massage-capable, and settings/user/web
   locks all off. Preserve `ShowerConfiguration=127` unless the supported
   installer workflow derives it automatically from the detected hardware.
9. Leave all uninstalled accessory families disabled. Do not copy transient
   runtime fields such as `shower_on`, current status, error flags, live
   setpoints, connection strings, device-running state, or controller time.
10. After each category, make one sequential `/values.cgi` read with
    `Connection: close`; compare it to `controller-config.json`, then wait
    before the next read. A shortened payload is not proof that a valve is
    disconnected. Compare response size and key count and corroborate with
    attachment fields and the controller error log.
11. With an operator present, perform one manual, low-temperature water test per
    zone. Verify outlet mapping and delivered temperature with an independent
    thermometer before restoring normal setpoints. Stop water locally if the
    controller becomes unreachable or the delivered temperature is unexpected.

## Integrity check

From the repository root:

```sh
jq empty research/diagnostics/2026-08-22-idle-baseline/*.json
shasum -a 256 research/diagnostics/2026-08-22-idle-baseline/*.raw
```

Compare the results with `source_integrity` in `controller-config.json` and the
individual `*.metadata.json` files. The one-time raw datatable capture should
hash to:

```text
ea92096de1ed653234cdd6d08a055b69d96c698c0496e2e2158057370deca39c  datatable.raw
```

The datatable capture is a last-resort reference for a qualified technician,
not a restore script.
