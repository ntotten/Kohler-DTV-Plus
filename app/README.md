# DTV+ Shower — web interface

A replacement for the K-99693 wall interface. Drives a Kohler DTV+ system
directly over the K-99695 controller's undocumented CGI API, and looks like the
original touchscreen.

Built because the wall unit stopped talking to the controller — the controller
itself reports `num_interface = 0` while the valve and amplifier are both
healthy, so everything except the touchscreen still works.

> ⚠️ **Read [../DISCLAIMER.md](../DISCLAIMER.md) first.** This drives water
> temperature and flow on unsupported, unauthenticated hardware. Water above
> **43 °C / 109 °F can scald**, and some controller endpoints can brick the unit.
> Not affiliated with Kohler.

## Run it

```bash
npm install
npm run dev            # http://localhost:5180, also on your LAN IP
```

The controller address defaults to `192.168.0.115`. To change it, either set
`KOHLER_HOST` in the environment or put it in `.env.local`:

```
KOHLER_HOST=192.168.0.115
```

To host it on the network instead of running the dev server:

```bash
npm run build
npm run serve          # http://0.0.0.0:8080
```

`npm run serve` is a plain Node server with no dependencies — it will run on
anything on your LAN, including a Raspberry Pi. Add it to your phone's home
screen and it behaves like an app.

## How it talks to the controller

The controller cannot be reached with an ordinary HTTP client: its `.cgi`
handlers reply in **HTTP/0.9** — a bare body with no status line — which Node
and undici both reject outright. So `/api` is served by our own middleware that
speaks HTTP by hand over a raw TCP socket.

- [server/kohler-client.mjs](server/kohler-client.mjs) — raw socket transport,
  request serialisation, retries
- [server/middleware.mjs](server/middleware.mjs) — `/api` surface and the
  command allowlist
- [server/standalone.mjs](server/standalone.mjs) — production server

### Safety gate

The controller has no authentication and exposes endpoints that can wipe its
configuration or brick it. [server/cgi-safety.mjs](server/cgi-safety.mjs) rates
every known endpoint 0-5 (scale from
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus)) and the
proxy enforces it:

- **Nothing above 2/5 is reachable** — rejected with `403` before a packet is
  sent.
- **An endpoint must also be explicitly exposed** as `read` or `command`; a safe
  rating alone is not enough. The reachable surface is a small subset of the ~50
  known endpoints — `GET /api/safety` reports exactly which, and
  `safety.test.ts` pins the set so widening it is a deliberate edit.
- **Commands are `POST` only.**
- **The table self-checks at import**, so an over-permissive entry throws at
  startup instead of shipping.

`mac.cgi` and `serial.cgi` are rated 3/5 upstream — _documented as causing
system lockups_ — and are blocked. The MAC comes from `values.cgi` instead.

Because the controller allows only **2 concurrent HTTP sessions**, every request
goes through a single queue with a minimum gap. Running two copies of this app,
or this app plus the controller's own web page, can still exceed that.

### API

| Route                     | Purpose                                                                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `GET /api/status`         | Combined `values.cgi` + `system_info.cgi`. `values` is served from a 30 s cache (`valuesCached` says which); `?fresh=1` bypasses it. |
| `GET /api/safety`         | The live safety policy — risk ceiling and exposed endpoints.                                                                         |
| `GET /api/read/:name`     | Raw read passthrough, gated.                                                                                                         |
| `POST /api/command/:name` | Fire a command, gated. JSON body = query params.                                                                                     |

### Tests

```bash
npm test                                      # unit tests, no hardware
npm run selftest                              # live, strictly read-only
npm run selftest -- --api http://127.0.0.1:5180   # also exercises the proxy
npm run check                                 # typecheck + unit tests + build
```

`selftest` never sends a command and never opens a valve — it is safe to run at
any time, including while someone is showering. It verifies the safety gate
offline first, then reads status and configuration from the controller.

## Layout

```
src/api/       transport, types, and the raw -> model derivation
src/state/     polling + optimistic command state
src/ui/        screens, styled after the K-99693
public/fittings/  fitting icons, rebuilt for a light screen
```

The fitting icons come from the controller itself, converted by
[research/tools/make-fittings.py](../research/tools/make-fittings.py). Rerun that
if you re-mirror the controller.

## Notes on behaviour

- `quick_shower.cgi` takes the whole desired state each time, so selecting an
  outlet, changing temperature and pressing start are all the same call.
- **Polling is 15 s idle / 5 s active, deliberately.** The controller's web
  server locks up under sustained polling — for hours, sometimes needing a power
  cycle — and other integrations hit this repeatedly before settling on these
  values. `values.cgi` is cached 30 s server-side so a normal poll costs one
  request, not two. See [../research/FIELD-NOTES.md](../research/FIELD-NOTES.md).
- Temperature changes are debounced ~450 ms — this controller does not enjoy a
  request per arrow tap.
- After any command the UI holds its own state for 5 s so a poll landing
  mid-flight doesn't snap the display back.
- `valve1outletN` from the controller is the _armed selection_, not water flow,
  and it is indexed by `valveN_outletM_func.id` rather than the slot number.
- `valveN_Currentstatus` of `PurgeActive` means water is already running — it is
  the auto-purge warm-up.

See [../PROTOCOL.md](../PROTOCOL.md) for the full protocol.
