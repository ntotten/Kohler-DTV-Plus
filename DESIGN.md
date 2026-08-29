# Design

How this app is put together, why it is shaped this way, and what the Android
port will need.

Read [DISCLAIMER.md](DISCLAIMER.md) first. [PROTOCOL.md](PROTOCOL.md) covers the
wire format.

## The problem

The K-99693 wall interface stopped working. The controller reports
`num_interface = 0` and `ui1_con_string = not_seen`, while `valve_1_con_string`,
`amp_con_string` and `controller_con_string` all read `conn`. So the shower is
fully functional and simply has nothing to command it.

The K-99695 controller exposes an undocumented CGI API on port 80 that its own
web pages use. That API is the replacement input.

## Constraints that shaped everything

These are properties of the hardware, not choices:

| Constraint                                                   | Consequence                                                                                 |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `.cgi` replies are **HTTP/0.9** — no status line, no headers | Node `http`, `fetch`/undici and browser XHR all refuse them. A raw TCP client is mandatory. |
| **2 concurrent HTTP sessions**, then a ~20 s hang            | One global request queue with a minimum gap. Never parallel.                                |
| No `Content-Length` on CGI replies                           | Read until the socket closes; `Connection: close` makes the server do that.                 |
| **No authentication at all**                                 | Anything on the LAN can run the shower. Network position is the only boundary.              |
| Endpoints that **brick or wedge** the unit                   | A hard safety gate, not an honour system.                                                   |
| Reported temperature is a **thermistor reading**             | Never presented as ground truth; scald threshold marked in the UI.                          |

## Shape

```
browser / WebView          Node process                   controller
┌──────────────┐  fetch   ┌────────────────────┐  raw    ┌─────────────┐
│ React + Vite │ ───────► │ /api middleware    │ ──TCP──►│ K-99695     │
│  (static)    │  JSON    │  safety gate       │ HTTP/0.9│ MQX httpd   │
└──────────────┘          │  queue + retries   │         └─────────────┘
                          └────────────────────┘
```

The Node layer is not incidental — **it is what makes the controller speakable
at all.** No browser or WebView can talk HTTP/0.9. Every platform therefore keeps
a proxy in the picture; only its location changes.

### Files

```
app/server/kohler-client.mjs   raw socket transport, queue, retries, repr-JSON
app/server/cgi-safety.mjs      risk table + access policy  ← the safety-critical file
app/server/middleware.mjs      /api surface, gate enforcement
app/server/standalone.mjs      production server (dependency-free)
app/vite.config.ts             mounts the same middleware in dev

app/src/api/config.ts          API base URL (the Capacitor seam)
app/src/api/types.ts           controller payload shapes
app/src/api/outlets.ts         fitting type table + icon resolution
app/src/api/model.ts           raw payloads -> ShowerModel
app/src/api/client.ts          typed command wrappers
app/src/state/useShower.ts     polling, optimistic state, debounce
app/src/state/useTheme.ts      dark/light, persisted
app/src/ui/                    screens styled after the K-99693
```

## Key decisions

### The safety gate is code, not documentation

`cgi-safety.mjs` rates ~50 endpoints 0-5 and refuses anything above **2/5**
before a packet is sent. An endpoint must _also_ be explicitly exposed as `read`
or `command` — a safe rating alone does not open it. 18 endpoints are reachable:
5 reads and 13 commands.

The table self-checks at import: an entry exposed above the ceiling throws at
startup rather than shipping. `safety.test.ts` pins the exposed set exactly, so
widening it requires editing a test that says why.

Commands are `POST` only, so no link or prefetch can fire one.

### Derive a model; never let components read raw payloads

`values.cgi` returns ~300 loosely-typed keys and `system_info.cgi` another 39,
with overlapping and occasionally contradictory meanings. `model.ts` is the only
place that interprets them; the UI sees a `ShowerModel`.

This caught the bug that mattered most: **`valve1outletN` is the armed
selection, not water flow.** The default outlet reads `true` while the shower is
off, so the first build reported "water running" on an idle system.

Two more corrections came out of other people's field reports rather than our
own hardware, because this system does not exhibit either:

- **Outlet numbering is two index spaces.** Slot numbers go into
  `quick_shower.cgi`; `system_info.cgi` reports under `valveN_outletM_func.id`.
  They coincide here, which is why conflating them looked fine — and why someone
  else's outlet 2 turned on their outlet 6.
- **`PurgeActive` means running.** Auto-purge is enabled on this system, so the
  warm-up flows cold water before `shower_on` is set. Watching only that flag
  would have offered "start" mid-shower.

Both are pinned by regression tests that deliberately construct the non-identity
case. See [research/FIELD-NOTES.md](research/FIELD-NOTES.md) §2-3.

### One queue, and a deliberately slow poll

Every controller request funnels through a single promise chain with a 120 ms
floor between calls, because overlapping requests are what actually wedge this
server.

Polling is **15 s idle, 5 s active**, with a 120 s tail after the shower stops.
That is not a guess — the controller's web server locks up under sustained
polling and other integrations converged on exactly these numbers after hitting
it repeatedly (see [research/FIELD-NOTES.md](research/FIELD-NOTES.md) §1). Our
first build polled at 2.5 s, about six times faster than the interval already
suspected of causing lockups; reading the field reports caught it before the
hardware did.

`values.cgi` is configuration and changes only when someone edits it on the
controller, so the proxy serves it from a 30 s cache and a normal poll costs one
request rather than two. Run state is therefore read from `system_info.cgi`,
which is always fetched live — a stale cached `shower_on` must never keep the UI
claiming water is running.

Commands are optimistic. For 5 s after any command the UI keeps showing what the
user asked for, because the controller takes a moment to reflect a change and a
poll landing inside that window would visibly snap the display backwards.
Temperature is debounced 450 ms — arrow taps arrive in bursts and this controller
does not enjoy a request each.

### `quick_shower.cgi` takes whole state

It carries outlets, massage and temperature for both valves on every call, so
"start", "change outlets" and "change temperature" are one function. Outlet
positions concatenate into a string: outlets 1, 3, 4 become `"134"`. An empty
selection routes to `stop_shower.cgi`, as the controller's own UI does.

### Assets are the controller's own

The fitting icons are pulled from the controller and rebuilt by
`research/tools/make-fittings.py`: alpha from source luminance, ink flat per
theme. Two wrinkles needed handling — a soft glow in the source art that read as
a smudge once inverted, and brightness encoding selection inconsistently across
types, which left Real Rain looking permanently selected. Alpha is normalised per
image, then scaled by state, so every fitting agrees: dim when unselected, solid
when selected.

The distinctive Kohler bracket lozenge is baked into that art, so the fitting
buttons get it for free; elsewhere it is two facing pseudo-element half-frames.

### Dark by default

Requested, and right: this gets used in a dim bathroom. Light reproduces the
K-99693 as shipped and lives in settings. Theme is an explicit choice persisted
to `localStorage`, not a mirror of the OS setting — someone picking "light"
wants the authentic look regardless of what their phone is doing.

Themes are CSS custom properties on `:root[data-theme]`, with a matching icon set
per theme.

## Testing

Nothing in the test suite can open a valve.

- **`npm test`** — 49 unit tests, no hardware. Fixtures are verbatim captures
  from the live controller. Covers model derivation, the armed-vs-flowing
  distinction, degradation when the controller is unreachable, and the safety
  gate.
- **`npm run selftest`** — live and strictly read-only. Verifies the gate
  offline first, then reads status and configuration and asserts consistency
  between the two endpoints. Safe to run while someone is showering.
- **`npm run check`** — typecheck + unit tests + build.

The live flow test — actually opening a valve — is deliberately manual and
operator-initiated.

## Android / Capacitor port

Modelled on `e:\git\mg-controller\apps\android-cap`, which uses
`webDir: '../web/dist'` and an env-driven `server.url`.

**The one thing that must be understood:** a Capacitor build is a static bundle
with no Node process, and `CapacitorHttp` will not rescue it — the blocker is
HTTP/0.9 at the protocol level, which OkHttp rejects just as Node does. So:

| Option                                           | Verdict                                                                                                                                     |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Point the app at a LAN-hosted `npm run serve`    | **Recommended.** Zero new code; needs an always-on box.                                                                                     |
| Native raw-socket Capacitor plugin               | Removes the server dependency, but reimplements the transport, queue and safety gate in Java/Kotlin — including duplicating the risk table. |
| Talk to the controller directly from the WebView | Not possible. HTTP/0.9.                                                                                                                     |

The seam is already in place: `src/api/config.ts` reads `VITE_API_BASE`, and all
requests route through `apiUrl()`. Building the bundle for a phone is:

```bash
VITE_API_BASE=http://192.168.0.20:8080 npm run build
```

Android also needs `allowMixedContent` / cleartext for plain HTTP on the LAN, as
the mg-controller config already does.

## Known gaps

- **Unverified against running water.** Every read path and the command
  transport are confirmed against live hardware (`stop_shower.cgi` returned
  `:)`), but no valve has been opened by this app yet.
- **Steam, lighting and rain panel are coded but untestable here** — this system
  has none installed. Those paths follow the controller's own JS and xagon0's
  reference; treat them as unproven.
- **Presets are read-only.** Saving one is a `save_variable.cgi` sequence that
  has not been worked out; all six read as unsaved on this system.
- **Massage speed is UI-only.** The control exists in the sheet but the
  controller's speed parameter has not been located; only mode is sent.
- **Second valve is modelled but unexercised** — this system has one.
- **No PWA manifest or service worker yet** — deferred pending the hosting
  decision.
