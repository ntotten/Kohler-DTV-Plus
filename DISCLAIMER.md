# Disclaimer

**Read this before running anything in this repository.**

This project drives a Kohler DTV+ shower system over an **undocumented,
unauthenticated** control interface that Kohler does not support. It exists
because the K-99693 wall interface on this system failed while the rest of the
system stayed healthy.

## Safety warning

The DTV+ controls **water temperature and flow**. Getting it wrong can cause:

- **Scalding.** Water above **43 °C / 109 °F** can scald. Children, the elderly,
  and anyone who cannot move away quickly are at far greater risk.
- **Water damage** from a valve left open, or a valve that fails to close.
- **Electrical hazard** from mis-wired RS-485, steam, or lighting circuits.
- **Bricked hardware** from bad firmware uploads or the wrong CGI call.

Never rely on a reported temperature as proof of the real one. The number shown
is the valve's own thermistor reading, not an independent measurement — drift,
bad calibration, or plumbing problems can put actual delivered water well away
from the setpoint. **Verify with a real thermometer** after any change, and
before anyone stands under it.

## Responsibility

- This project is **not affiliated with, endorsed by, or supported by Kohler
  Co.** All information was obtained by reverse engineering hardware we own.
- Using it may **void your warranty**.
- You assume **all responsibility** for anything that happens as a result.
- It is offered for **repair, education, and home automation** — nothing else.

## CGI safety ratings

Endpoints are rated 0-5. The scale is from
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) (see
[research/xagon0/DISCLAIMER.md](research/xagon0/DISCLAIMER.md)):

| Rating | Meaning |
| --- | --- |
| 0/5 | Safe — read-only or no side effects |
| 1/5 | Low risk — minor settings changes |
| 2/5 | Moderate — changes device behaviour |
| 3/5 | Caution — may cause lockups, requires reboot |
| 4/5 | Dangerous — can cause persistent issues |
| 5/5 | Critical — can brick the controller |

### How this repository enforces it

The ratings are not advisory here. They are a table in
[app/server/cgi-safety.mjs](app/server/cgi-safety.mjs), and the proxy refuses
anything that fails it:

- **Nothing rated above 2/5 is reachable.** Requests are rejected with `403`
  before any packet reaches the controller.
- **An endpoint must also be explicitly exposed**, as `read` or as `command`.
  Being rated `0/5` is not sufficient. The reachable surface is intentionally
  smaller than the safe surface.
- **Every exposed endpoint declares the parameters it accepts**, and the values
  each may take. A rating describes an endpoint, not its arguments — and
  `save_variable.cgi` is one call that writes any of 105 persistent config
  variables, `valve_max_temp` among them. Only index 43, the amplifier's volume,
  is accepted. Anything else is refused with the same `403`. See
  [PROTOCOL.md](PROTOCOL.md#save_variablecgi-is-a-write-anything) for what the
  other indices do and [STORY-LOG.md](STORY-LOG.md) for how long they were
  reachable.
- **Commands are `POST` only**, so no link, prefetch, or address-bar mistake can
  fire one.
- **The table self-checks at startup.** If an entry is ever exposed with a rating
  above the ceiling, the server throws instead of starting.
- The live surface is visible at `GET /api/safety`, and
  `npm run selftest -- --api <url>` asserts that the blocks hold.

Endpoints such as `reset_factory.cgi` (5/5), `clear_dt.cgi` (5/5),
`fileupload.cgi` (5/5), `unpack_bin.cgi` (5/5), `edit_dt.cgi` (4/5),
`rpc.cgi` (4/5), and `set_device.cgi` (4/5) are permanently unreachable.

### Two endpoints worth calling out

`mac.cgi` and `serial.cgi` are rated **3/5 — documented as able to cause system
lockups**. They are blocked here.

**Disclosure:** both were called once during initial exploration of this system,
before that rating was known to us. They returned empty responses and the
controller has remained healthy through every check since. They will not be
called again. The MAC address is available from `values.cgi` (0/5) anyway, which
is where this app reads it.

## Operating limits

These are properties of the controller, not preferences:

1. **Two concurrent HTTP sessions, maximum.** Exceeding it hangs the web server
   for roughly 20 seconds. Browser tabs, polling, and scripts all count. This
   app serialises every request through one queue with a minimum gap — but two
   copies of it, or the app plus the controller's own web page, can still push
   past the limit.
2. **`.cgi` responses are HTTP/0.9** — no status line, no headers, often no
   `Content-Length`. Read until the socket closes.
3. **Do not upload partial or truncated firmware.** It will fail CRC and the unit
   will not boot.
4. **Record your controller's IP, MAC and firmware versions now**, while it is
   working. You will want them for recovery.
5. **Change one thing at a time**, so you know what caused a problem.

## Before you begin

- Confirm your maximum temperature limit on the controller's own settings pages
  and satisfy yourself it is safe for everyone who uses the shower. This app
  clamps to that limit; it never raises it. The write that would raise it —
  `save_variable.cgi` index 39 — is refused by the proxy, so this holds for
  anything that can reach the proxy, not only for the app's own UI. It was not
  true before 2026-08-30.
- Keep this on a trusted network. The controller has **no authentication** —
  anything that can reach it can run your shower.
- Do not leave the shower able to start unattended.

## Credits

Protocol and safety analysis draws on
[xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus) and
[dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python), plus direct
reverse engineering of this system. See
[research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md) for provenance and
licensing notes on the vendored material — **the upstream repository states no
license.**
