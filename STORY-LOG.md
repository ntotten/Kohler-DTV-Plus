# Story log

Significant events, findings and reversals, newest first. Raw material for the
YouTube video ([@azab2c](https://www.youtube.com/@azab2c)) and for conversations
with Kohler technical support.

Entries marked **For Kohler** are collated when contacting support.

See the Story log section of [AGENT.md](AGENT.md) for what to append and how.

---

## 2026-08-04

### 23:10 — The egress log's first capture caught a controller hang, and explained the `values.cgi` blip

The egress trace went in tonight — a line for every request this app sends to
the controller and every answer it gets back, correlated by a short id. Within
twenty minutes of running it, it caught the thing the investigation has been
guessing about since 07-26.

```
06:05:18.112Z  REQ   8fn5  values.cgi
06:05:26.117Z  ERR   8fn5  values.cgi        timeout after 8000ms attempt=1/3
06:05:58.636Z  ERR   8fn5  values.cgi        timeout after 8000ms attempt=2/3
06:06:10.072Z  RES   8fn7  values.cgi        ok 27434ms q=24249ms keys=299
06:06:10.231Z  RES   8fn8  system_info.cgi   ok 19464ms q=19426ms keys=36
06:06:25.307Z  CACHE ----  values.cgi        hit age=15s keys=299
```

(Timestamps are UTC in the log itself; this is 23:05-23:06 local.)

**The degraded `values.cgi` payload is not a random partial response. It is what
the controller's web server returns while recovering from a hang.** The short
payload — 299 keys against a healthy 304, with a connected valve reported
`installed: false` / `con_string: "dis"` — arrived only after the server had
stopped answering for roughly twenty seconds and two 8 s timeouts had already
elapsed. `system_info.cgi` came back short in the same window too, 36 keys
against 39, which nobody had noticed before because nothing was counting.

The last line is the failure [FIELD-NOTES.md](research/FIELD-NOTES.md) §6 warns
about, recorded happening: the 299-key payload went into the proxy's 30 s cache
and was served from it. The guard that exists to prevent exactly this did not
fire, because the middleware process had just restarted and had no `lastGood` to
compare against — **the guard is disarmed on the first read after every restart.**

**Why it matters:** the 2026-07-26 "transient valve dropout, caught once" was
promoted to evidence on the strength of looking like the shutoff signature. It
almost certainly was not a valve dropout at all. More importantly for what comes
next: any telemetry that logs `con_string` without logging latency and key count
will manufacture valve-dropout findings out of web-server hiccups.

**For Kohler:** when the DTV+ controller's embedded HTTP server recovers from a
session-limit hang, it serves a structurally valid but incomplete JSON body in
which connected devices are reported as absent. There is no error status — the
reply parses cleanly and looks authoritative. A client cannot distinguish it from
a genuine device dropout except by counting keys or noticing that the response
took twenty seconds.

### 23:05 — We caused both hangs, and it is worth saying how

Neither hang tonight was spontaneous. The first followed a deliberate burst of
stacked reads while probing whether the short payload correlated with request
spacing; the second followed editing `server/*.mjs` while `npm run dev` was
running, which makes Vite restart the middleware — and a restarting middleware
does not inherit the old process's in-flight requests or its serialisation queue.
Six different pids appear in the trace over seven minutes.

So the app's careful one-request-at-a-time queue is **per process**, and during a
dev-server reload there are briefly two processes talking to a controller that
tolerates two sessions total.

The controller recovered on its own both times, in under thirty seconds, and six
spaced reads afterwards were clean at ~175 ms.

**Why it matters:** it is a live hazard while developing against this hardware,
and it means the trace's own timestamps around a code change are suspect. It also
supports the reading that the community's lockups are about **concurrency**
rather than interval — Kohler's own web page polls `system_info.cgi` at 5 s, the
same rate as our active cadence, without trouble.

### 22:55 — There is no measured water temperature anywhere in the CGI API

Checked because hypothesis 0 (tankless minimum-flow cutout) predicts outlet
temperature falling before the shutoff, and it would be a great deal cheaper to
read that from the controller than to instrument the plumbing.

It is not there. `valve1_temp_string` and `valve1Setpoint` both read 96 with a
setpoint of 96; nothing in `values.cgi`'s 304 keys or `system_info.cgi`'s 39
carries an actual thermistor reading. The controller does have the number — it
reads it from the valve over Saturn and pushes `DT_W_Temperature` to the
touchscreen over Amulet — but no CGI endpoint surfaces it.

xagon0's [values-cgi-guide](research/xagon0/docs/web-interface/values-cgi-guide.md)
documents a `values.cgi?type=word` form returning a raw datatable array, with
"word index 0 = valve 1 current temp". **That does not exist on firmware
0.0.3.89.** Measured: `?type=byte`, `?type=word`, `?type=string` and
`?page=control` all return the identical 304-key object as the bare call. The
parameters are ignored. Kohler's own `control.js` and `settings.js` never pass
them either.

**Why it matters:** the leading hypothesis's primary signature is invisible to
controller telemetry. Confirming it needs a temperature sensor on the outlet, not
a better poller.

### 22:45 — The wall interface is not an HTTP client, so it costs nothing from the two-session budget

The observability brief assumed the reconnected K-99693 was a second HTTP client
and that the concurrency budget had tightened. It is not, and it has not.

The K-99693 is a device on the RS-485 bus: discovered over the DTV+ protocol as
device ID `0x30`, then synchronised over Amulet CRC at 115200 baud on a 50 ms
tick. User input comes back as `INVOKE_RPC` frames on that bus. It never opens a
TCP connection. The confusion is understandable — the thing polling
`system_info.cgi` every 5 s and `values.cgi` every 10 s is the controller's *web
page*, `control.js`, which is a different client entirely.

Confirmed against the live unit: `num_interface = 1`, `ui1_con_string = conn`,
`valve_1_con_string = conn` fw 0.12, controller fw 0.0.3.89.

**Why it matters:** the telemetry build has the same two-session budget it always
had. It does **not** mean the interface is invisible — it can command the shower
over the bus, and those commands never pass through our proxy and never appear in
the egress log. "No REQ line" still means "we didn't send it", never "nobody did".

### 22:10 — Our own app was sending every outlet tap twice

Measured, not inferred: in `npm run dev` — the documented way to run this app,
and the operator's only remote way to run the shower — one tap on an outlet
while water was flowing sent `quick_shower.cgi` **twice**, milliseconds apart.

The cause is ours, not the controller's. `toggleOutlet` computed the new
selection inside the function it handed to React's `setSelection`, and fired the
valve command from in there. React's `<StrictMode>` deliberately invokes those
functions twice in a development build to expose exactly that kind of impurity —
a function that does more than compute a value. It found ours. Nothing on screen
ever showed it; the second command was invisible unless you were watching the
proxy.

Pinning it needed the hook to actually run, and adding a React renderer to the
test setup was more dependency than this deserved, so `app/test/hookHarness.ts`
is a ~120-line stand-in that answers the five hooks `useShower` uses and
reproduces the one behaviour under test. Before the fix:

```
toggleOutlet(1)  ->  quick_shower.cgi  valve1_outlet=13   insideUpdater=true
                     quick_shower.cgi  valve1_outlet=13   insideUpdater=true
```

With StrictMode's double-invocation switched off in the same harness, one call.
That is the whole defect.

The fix moves the decision into a pure `toggleOutletSelection()` in
`app/src/api/model.ts` and dispatches once from the caller. A second test reads
`useShower.ts` and fails if a command is ever written back inside a state
updater, in any code path — verified by temporarily reverting the fix and
watching it name the offending updater. 71 tests pass; `npm run selftest` still
passes read-only against the live unit.

**Why it matters:** rapid successive valve commands are the exact input
[FIELD-NOTES.md](research/FIELD-NOTES.md) §1 blames for the controller going
unreachable for hours. We spent this project being careful about polling
cadence while quietly doubling the commands that matter most.

**For Kohler:** if any of your own tooling drives `quick_shower.cgi` from a
React state updater under StrictMode, it has this bug too. More usefully: the
controller gives a client no way to notice it — there is no request id, no
idempotency key, and no rejection of a duplicate command that arrives
milliseconds after an identical one. A client cannot tell a doubled command from
a deliberate one.

### 13:40 — The `values.cgi` blip repeats, and the guard against it assumes it can't

A read-only sweep of the controller caught the known degraded `values.cgi`
payload **twice in a row** — which is the one case the proxy's mitigation is
built to assume cannot happen.

`npm run selftest` read `values.cgi`, saw `valve1_installed: false` /
`valve_1_con_string: 'dis'` on a valve that was fine, re-read to rule out the
flap documented in [FIELD-NOTES.md](research/FIELD-NOTES.md) §6 — and got the
same answer again. Six deliberate reads immediately afterwards were all healthy:

```
selftest read 1   installed=false  con=dis    300 keys
selftest read 2   installed=false  con=dis    300 keys
follow-up  1..6   installed=true   con=conn   304 keys   fw 0.12, over 18 s
```

FIELD-NOTES §6 records this as a one-in-30-to-50 read that "recovers on the next
read", and `app/server/middleware.mjs` encodes exactly that: a payload losing a
previously-installed valve must say so **twice** before it is believed. Two
consecutive suspects therefore defeat the guard — the bad payload is accepted and
cached, which is the "no configured outlets for 30 seconds, possibly with someone
standing in the shower" outcome the guard exists to prevent.

The useful half of the finding: **the degraded payload is short — 300 keys
against 304.** A truncated response and a genuine valve disconnection do not look
the same on the wire, and the code currently reads only the content, never the
completeness. That is a cheaper and more direct discriminator than repetition.

**Why it matters:** it fails in two directions. Toward the user, it can blank the
UI mid-shower. Toward the investigation, it *fabricates* the exact signature we
are hunting — "the controller has lost the valve" is the state the 2026-07-14
video shows during a shutoff, and one such sample has already been promoted to
evidence. Any telemetry that logs `con_string` without logging payload
completeness will generate that finding on schedule. The observability brief has
been updated to require it.

Honest bound on this: n=1 for the repetition, and the 300-versus-304 signal rests
on a single observed pair. Neither is a measured distribution yet.

**For Kohler:** `values.cgi` intermittently returns a short, partially-populated
response — 300 keys instead of 304 — in which a healthy, connected valve is
reported as `installed: false` / `dis`, while the valve's firmware version is
still present in the same payload. It can occur on consecutive requests. Any
client that treats a single such read as a state change will report a valve
dropout that did not happen.

---

## 2026-08-03

### 19:05 — Corrected: the interface is portrait, and the CAD is lying on its side

The decal drawn an hour ago was landscape. It was wrong, and the correction is more interesting than the mistake.

The reasoning that produced it went: the mesh's flat face measures 131.07 × 81.47 mm, and the K-99693 spec sheet says 5-1/4 in wide by 3-5/16 in high, so 131 is the width and 81 the height. Landscape. That reading is wrong, and Kohler's own product photography shows it immediately — the unit is clearly taller than it is wide.

The **K-99694 interface mounting bracket** drawing, already sitting in `research/`, settles it: 3-5/16" (84 mm) wide by 5-5/8" (143 mm) tall, portrait, with the wiring boss at the bottom and the mounting tabs at the top. So the interface is portrait, and the CAD is authored **on its side**: the product's vertical runs along the model's X axis, and product-down is +X — the end carrying the raised connector block on the wall side.

The confirmation came from an unexpected direction. Our own app's UI is 1120 × 1800, aspect 0.6222. The faceplate, read portrait, is 81.4677 / 131.0691 = 0.6216. **0.11% apart.** The UI had been laid out to the real device's proportions months ago, so the app itself was carrying the answer.

**Why it matters:** the same off-by-90° would rotate any part someone cut from this file. It also means none of the viewer's standard views shows the interface upright — they are CAD views, and this CAD is sideways. The fix is a decal-driven "look at it, upright", since the decal anchor's own up-vector is the only place the product's orientation is recorded.

**For Kohler:** the K-99693 visualisation CAD is oriented with the product's height along X and the faceplate on +Y. Read with the usual CAD convention — X wide, Z high, front at −Y — the part comes out landscape and back-to-front. The spec sheet's "5-1/4 in wide" compounds it.

### 18:20 — Kohler's K-99693 CAD has no screen, and its front is on the back

Trying to put a picture of our replacement interface onto the model's display, we went looking for the display. There isn't one.

The published K-99693 CAD models the face as a **single flat two-triangle rectangle**, 131.0691 × 81.4677 mm, with no display window, no bezel step and no button outlines. Everything the user actually touches is absent. The model is a marketing visualisation: Kohler's own renders presumably composite a screen image onto that blank plate, exactly as we now do.

The second surprise is orientation. In the CAD's own frame the blank faceplate is on **+Y**, and the side with a large circular boss, two mounting tabs and a connector block is on −Y. Standard CAD view conventions put "front" at −Y, so the viewer's Front button shows the **wall side** of the part. The product's front is the CAD's back. Nothing is wrong with the file; it is just not oriented the way its view names imply, and anyone taking a "front elevation" screenshot from it will publish the back of the unit.

Both facts were established by measurement, not by looking: a coplanar-face pass over the mesh found one perfectly flat quad and no other candidate, and an orthographic depth map of each side confirmed which was which.

**Why it matters:** the model is authority for the outside envelope and for nothing about the interface itself. Any dimension taken off it for screen size, button placement or bezel thickness would be invented. It also means our decal anchor is a measured face carrying artwork that is ours — that separation is now recorded in the file itself rather than left for a viewer to infer.

**For Kohler:** the published K-99693 visualisation CAD contains no display aperture or control features, and its faceplate sits on the CAD's rear axis. Both are easy to miss, and both mislead anyone using the file for fit work.

### 16:18 — The rear cover becomes a gasket problem, not a glue problem

The access opening is cut, the connector is reachable again, and the replacement rear cover is now printed in TPU.

The cover is not just a plug. It has a broad external flange that overlaps the original ABS housing, plus an internal 5 × 5 mm stand-off positioned over an empty area of the PCB. The stand-off normally floats about **1 mm above the board**; it exists to stop someone pressing the soft TPU cover inward during installation and collapsing it onto the electronics.

That geometry changed the sealant question.

The first instinct was to reach for something labelled waterproof: bathroom silicone, E6800, conformal coating, perhaps even dielectric grease rubbed around the joint. But this is not really a question of which chemical sounds most waterproof. It is a question of what the joint is mechanically asking the material to do.

The TPU flange gives us a broad surface and a long leak path. What it wants is a **flexible, formed-in-place gasket** between TPU and ABS, with enough body to fill print texture, machining irregularities and small dimensional errors.

#### The sealant tournament

|     Rank | Candidate                                        | Rating for this joint | What looked attractive                                                                                                          | What killed it—or nearly did                                                                                                                                             | Verdict                                                |
| -------: | ------------------------------------------------ | :-------------------: | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
| 🥇 **1** | **Permatex “The Right Stuff” 90 Minute**         |         ⭐⭐⭐⭐⭐         | Thick, flexible gasket maker; fills irregularities; works naturally with the wide flange; can form a continuous exterior fillet | TPU adhesion still depends on scuffing, cleaning and leaving enough sealant thickness                                                                                    | **Winner: primary TPU-to-ABS seal**                    |
| 🥈 **2** | **E6800**                                        |         ⭐⭐⭐⭐☆         | Flexible waterproof adhesive with more structural holding power than ordinary silicone                                          | Longer cure, messier removal and less gasket-like; potentially overkill if the flange is already well located                                                            | **Best backup if retention becomes a problem**         |
| 🥉 **3** | **Clear bathroom-grade silicone**                |         ⭐⭐⭐☆☆         | Designed for wet environments; familiar; easy to tool into a fillet                                                             | Product chemistry varies, and adhesion to printed TPU is uncertain                                                                                                       | **Plausible, but less confidence than Right Stuff**    |
|    **4** | **E6000**                                        |         ⭐⭐⭐☆☆         | Flexible adhesive/sealant already on hand                                                                                       | E6800 is the more appropriate member of the family for environmental exposure                                                                                            | **Usable fallback, no compelling reason to choose it** |
|    **5** | **DAP Dynaflex 230**                             |         ⭐⭐☆☆☆         | Flexible, paintable and easy to apply                                                                                           | Fundamentally a building/trim caulk, not the material I want protecting electronics inside a shower wall                                                                 | **Wrong tier of product for the consequences**         |
|    **6** | **Dielectric grease / Super Lube**               |         ⭐⭐☆☆☆         | Compatible with TPU and ABS; fills microscopic gaps; moisture resistant                                                         | Only works as an aid to a **mechanically compressed removable gasket**. Here it would contaminate the surfaces and sabotage the adhesive bond holding the cover in place | **Clever idea, wrong joint**                           |
|    **7** | **MG Chemicals 422C silicone conformal coating** |         ⭐☆☆☆☆         | Already used to protect PCBs from condensation and moisture                                                                     | It is a thin coating, not a gap-filling housing seal. It cannot bridge the TPU/ABS joint or replace a gasket                                                             | **Useful on the PCB, useless as the perimeter seal**   |
|    **8** | **Elmer’s Glue-All**                             |         ☆☆☆☆☆         | It exists                                                                                                                       | Water, heat, humidity, ABS, TPU and expensive shower electronics all vote no                                                                                             | **Absolutely not**                                     |

The dielectric-grease idea was the most tempting wrong turn.

A thin grease film can improve a rubber O-ring or a gasket that is trapped and compressed by screws. This cover is not screwed down. The sealant also has to help retain the TPU flange against the ABS shell. Putting grease between them would make the surfaces easier to separate and much harder to bond later.

The conformal coating was the opposite problem. **MG Chemicals 422C belongs one layer deeper.** It can protect exposed PCB work as a secondary defence against condensation, but calling it the enclosure seal would confuse moisture resistance with actual gap sealing.

#### Final stack

```text
           SHOWER / HUMID AIR
                    ↓

      ┌──────────────────────────┐
      │       TPU COVER          │
      │                          │
      └─────────┬──────┬─────────┘
                │      │
         broad outer flange
        ╔══════════════════╗
        ║  RIGHT STUFF RTV ║  ← continuous gasket layer
════════╩══════════════════╩════════  ABS rear housing
                  │
                  │  ~1 mm normal clearance
                  ▼
             TPU stand-off
                  │
                  ·
              PCB surface

Optional secondary defence:
MG 422C on appropriate exposed PCB areas—not in the flange joint.
```

Installation plan:

1. Lightly scuff the underside of the TPU flange and the mating ABS.
2. Clean both surfaces and let them dry completely.
3. Apply a continuous bead of **Permatex Right Stuff** beneath the flange.
4. Seat the cover without crushing all of the sealant out of the joint.
5. Tool the squeeze-out into a small continuous exterior fillet.
6. Hold the cover in position while it cures.
7. Keep the internal TPU stand-off clear of the PCB under normal conditions; it is a last-resort deflection stop, not a permanent support post.

**Why it matters:** the best material did not win because it had the strongest waterproof claim on the tube. It won because it matched the geometry.

The wide TPU flange turns the repair into a gasket joint, and gasket maker is the most natural answer. E6800 remains available if testing shows that the cover needs more adhesive retention. The conformal coating remains useful as secondary PCB protection. The grease stays off every surface we expect to bond.

This should make the repair highly resistant to splashes, humid air and condensation. It does **not** recreate a factory-tested hermetic enclosure, and an FDM-printed TPU part should not be casually declared vapor-proof. But it is a much more defensible repair than painting the seam with conformal coating or trusting grease to stay where it was rubbed.

---

## 2026-07-27

### 12:50 — A parts viewer ships, and Kohler's own CAD turns out not to be printable

The K-99693 wall interface is disconnected and will need physical modification,
so the CAD Kohler publishes for it now has a home: a **separate** web app at
`viewer/`, sharing no code, no build and no port with the app that drives the
valve. That separation is deliberate — the hardware app moves real water and
stays lean; this one has no hardware surface at all and cannot open anything.

It loads OBJ, STL, 3MF, glTF/GLB, 3DS and PLY, measures the part, and exports a
binary STL for printing or CAM.

Two findings came out of building it.

**The published CAD is in inches, and nothing in the file says so.** No mesh
format — OBJ, STL, 3MF, PLY — records units or which way is up. Read the K-99693
OBJ as millimetres and you get a faceplate 5 mm wide that looks perfectly
normal on screen. The units were settled by measurement, not assumption: the
mesh bounding box is 5.259 × 1.214 × 3.310 along X/Y/Z, and the spec sheet says
5¼ × 1³⁄₁₆ × 3⁵⁄₁₆ inches. Width maps to X, height to Z. So: inches, Z-up, Y is
depth. The app now refuses to load any catalogued file that doesn't declare
both, because a guessed unit is worse than a refusal — it looks like an answer.

Measured through the export path, the part comes out **133.59 × 30.84 ×
84.07 mm** against a published 133.35 × 30.16 × 84.14. Width and height agree to
a quarter of a millimetre. Depth is 0.68 mm over, plausibly a trim-ring detail
in the CAD that the published figure excludes — not yet checked against the
physical part.

**The mesh is not watertight.** 224 unshared edges: it is an open surface, not a
solid. It displays and measures fine, but a slicer will need to repair it before
it prints, and CAM packages that want a closed solid will refuse it or produce a
bad toolpath. The app withholds a volume figure for this part rather than
reporting the meaningless number a signed-volume sum gives on an open mesh.

**Why it matters:** the modification work now has a measurable, exportable model
instead of a caliper and a guess — but the file needs repairing first, and
anyone who assumed Kohler's CAD was print-ready would have found that out on the
printer.

### 16:10 — The CAD is repairable, and it is emptier than it looks

Chased the watertightness problem down properly. The 224 bad edges are 222
boundary edges across 11 open loops, plus 2 non-manifold T-junction seams where
four triangles share a 0.03 mm edge.

**Welding is not the fix, and it was worth proving rather than assuming.** A
tolerance sweep from 0.0001 mm to 0.5 mm leaves the boundary count flat at 222
until the tolerance gets large enough to start fusing genuinely separate
surfaces — at 0.1 mm it removes 6 boundary edges and creates 155 new
non-manifold ones. These are real holes, not duplicate-vertex cracks.

All 11 loops turned out to be closed, and 8 of them exactly planar, so they cap
cleanly. The viewer now welds, caps and collapses the seams: 4,544 → 4,736
triangles, watertight and manifold, **and the outer envelope does not move by
0.0001 mm on any axis** — enforced by the verify gate, because a repair that
shifts the surface a toolpath is cut against would be worse than no repair.

One bug worth recording because it would have shipped silently: the first
implementation derived cap orientation from the best-fit plane normal, but the
triangulator normalizes its winding against a basis built from that same normal,
so the two cancelled and every cap kept the boundary's winding — inverted. It
still reported watertight. Only the volume was wrong, and only the cylinder test
caught it. Orientation is now decided from the boundary half-edge direction,
which is topology and cannot cancel.

**The part is hollow.** Enclosed volume after capping is 190.30 cm³ against a
346 cm³ bounding box, and there is no geometry between the front bezel and the
rear plate beyond the side walls. No PCB, no wire-to-board connector, no ribs or
bosses.

**Why it matters:** the immediate job is CNC machining a rear access opening to
reconnect a power+signal wire that came off the board. This model is a reliable
guide to the outside of the part and no guide at all to what sits behind any
given point on the rear face. Clearances have to come from the physical part.

**For Kohler:** the published K-99693 CAD is a visualization shell — open,
hollow, no internal structure. That is a reasonable thing to publish, but it is
worth stating on the download page, because it is dimensionally trustworthy
enough on the outside to be mistaken for an engineering model.

**For Kohler:** the CAD published for K-99693 is an open mesh with 224 unshared
edges and cannot be printed or machined without repair. It also carries no unit
declaration, which is unavoidable in OBJ but does mean the file is only safe
alongside the spec sheet. Dimensionally it checks out on width and height to
better than 0.25 mm; the published depth is 0.68 mm under what the model
contains.

---

## 2026-07-26

### 13:37 — The repo stops being a fork, and gets a licence

This project had been a fork of
[timelery/Kohler-DTV-Plus](https://github.com/timelery/Kohler-DTV-Plus) since
2022 — a 2017 repo of CGI notes, 8 commits, untouched since. What's here now
shares almost nothing with it but the starting point, and GitHub was still
presenting the work as a contribution to someone else's project.

Rather than ask GitHub Support to detach it, the history was pushed to a fresh
standalone repo and the original name reclaimed. All 38 commits survive with
authorship intact, including Tim Elery's original 8 — the fork relationship is
gone, the provenance is not.

Three things landed with it:

1. **Attribution that was actually missing.** The README credited xagon0,
   dcmeglio and Kohler's user guide, but never named the repo this started
   from. Now it does.
2. **A licence.** The
   [Open Maker License](https://github.com/aaronse/OpenMakerLicense) — AGPL-3.0
   with a maker addendum — covering work original to this repo, with an
   explicit scope table for what it *cannot* cover.
3. **`_config.yml` deleted.** A one-line Jekyll theme setting from 2017, for a
   GitHub Pages site that was never enabled.

**Why it matters:** the licence question was live regardless of the fork. Three
bodies of third-party material sit in this tree — xagon0's analysis, Kohler's
guide and controller mirror, and Tim's original notes — and **none of the three
states a licence**. That's now written down where someone can find it before
they copy something, rather than being implied by a `PROVENANCE.md` two
directories deep. With a video pointing people here, the difference matters.

One caveat recorded honestly: the 38 commits were public in a fork network for
years, so they stay fetchable by SHA through the parent repo forever. Detaching
doesn't retract that, and nothing here pretends it does.

### 13:30 — Tankless heater reframes everything, and I had over-claimed

The hot water source is a **tankless** heater. Two consequences.

First, a correction to my own analysis. I had listed valve faults among the
things the empty controller log ruled out. **That was wrong.** Valve error codes
travel the Saturn serial protocol and surface as `valve1_ErrorFatal` /
`valve1_ErrorResettable` — *current-state flags, not history*. The on-board log
holds controller codes 100-204 only. A valve error that trips and clears leaves
no trace, so reading those flags the next day proves nothing.

Second, tankless supplies the mechanism that was missing:

1. Tankless units have a minimum activation flow (~0.5-0.75 GPM) and no
   reservoir — below it the burner stops and hot water goes cold in seconds.
2. The valve can't reach its 96 °F setpoint.
3. The valve shuts off rather than deliver cold water — exactly what the operator
   expected on camera: *"it's supposed to cut off if they can't achieve the
   desired temperature."*
4. Nothing appears in the controller's log, because valve errors don't go there.

**And it explains the detail that didn't fit.** In the 07-14 repro the operator
turned off the overhead and left *only the handshower* running, to save water,
~3.5 minutes before it failed. Under every other theory that's neutral. Under
this one it's the trigger — a single handshower is exactly where a tankless unit
can fall below minimum firing flow.

**Why it matters:** there is now a test that costs nothing and beats any amount
of code — run the shower with several outlets open, well above minimum flow, and
see if it survives longer. If high flow is stable and low flow fails, that's
close to conclusive.
**For Kohler:** when the valve cannot achieve setpoint and shuts off, is that
surfaced anywhere persistent? Right now it appears to leave no record once the
transient flag clears, which makes the failure very hard for an owner to
diagnose.

### 13:05 — The shutoff logs absolutely nothing, and that is now proven

The operator cleared the controller's error log **before** filming the
2026-07-14 repro, and captured it again **after**. The result:

```
No errors are logged from Controller
```

Cleared before, shutoff reproduced during, empty after. This turns yesterday's
ambiguous "the log might have been cleared" into a controlled negative result.

It rules out everything that writes to the log: device detach, device
unresponsive, valve faults, task exceptions and aborts, link drops, config
errors. And we know the mechanism works, because the UI disconnection on 07-25
was logged as code 100 within seconds.

That undercuts the hypothesis this session had been building toward. If the
valve had lost power or dropped off the RS-485 bus, code 100 is exactly what
should have appeared. It didn't.

**New leading theory:** something mechanical or hydraulic stops the water, and
the electronics never find out — a thermal or anti-scald cutoff, hot supply
exhaustion, or supply pressure loss. That explains every observation at once,
including the silence.

**Why it matters:** it changes what to instrument. If the cause is mechanical,
controller telemetry will never show it — a trace would only ever record
"running, then timed out", which we already know. The informative signal is
probably outside the controller: outlet temperature, flow, supply behaviour.
**For Kohler:** a reproducible condition in which the valve stops delivering
water, the controller continues reporting a running shower for ~1 minute, and
nothing is written to the error log. Is there any mechanical or thermal cutoff in
the valve or its install that closes flow without signalling the controller?

Bonus: the captured log header preserves the interface's own firmware versions
(UI OS v0.0.7.44, Touch Panel v0.0.0.2), which we can no longer read now that it
reports `not_seen`.

### 12:45 — Video review kills the leading hypothesis

Reviewed the operator's 2026-07-14 recording of a live shutoff. The decisive
moment is at 06:46, immediately after the water stops:

> "if I go over to the shower, it says that it's still running. It thinks that
> it's still pushing water out, or at least the controller does, but obviously
> it's not."

The controller keeps displaying a running shower for **about a minute**, then
times out and reverts to the clock screen.

A commanded stop — from the interface, this app, or anything else — sets the
controller's state to off immediately. That is not what happens. **The water
stops first and the controller finds out later.**

This kills the theory that a failing K-99693 was sending spurious stop commands,
which the operator was independently sceptical of. The investigation moves from
the interface to the valve.

Two supporting details from the same recording: the setpoint reverted from 97 to
96 °F on its own (96 is `def_temp`, so something reloaded defaults — the
signature of a reset), and there were no error messages or status LEDs anywhere.

**Why it matters:** we were about to instrument the wrong end of the system.
**For Kohler:** the controller has no awareness that the valve has stopped
delivering water until an internal timeout fires roughly a minute later. During
that window it reports a running shower to the user and to the API.

### 12:10 — Error log read for the first time

`cerror_logs.cgi` holds exactly one entry in a 99-slot circular buffer that
survives power cycles:

```
[10:32.42 p.m. 07/25/2026] 100:  UI Error
```

Code 100 is `DETACH_EVENT`, device byte `0x30` = primary UI — the interface
connector coming out the previous evening. Nothing else. Two months of shutoffs
produced no valve detach, no valve fault, no task exception, no link drop.
`kerror_logs.cgi` reports nothing from Konnect.

**Why it matters:** either these shutoffs genuinely aren't logged, or the log was
cleared at some point. We can't tell which, and it's now the biggest gap in the
evidence.
**For Kohler:** is there any condition under which the controller stops
delivering water without writing to the error log? And can the log be cleared by
a normal operation such as a firmware update or factory reset?

### 11:43 — Caught a valve dropout while idle

A routine `values.cgi` read returned `valve_1_con_string: 'dis'` and
`valve1_installed: false` on a valve that was healthy immediately before and
after. Four reads over the following minute were all normal, with no command
sent. Roughly one bad read in 30-50.

Initially written off as a partially-populated HTTP response. After the video
review it looks more interesting — "controller has lost the valve" is exactly
the state a shutoff produces.

**Why it matters:** it may be a quiet-moment glimpse of the actual failure mode.
Also forced a real fix: the proxy caches `values.cgi`, so one bad read would
have blanked the UI for 30 seconds — possibly with someone standing in the
shower.

### 10:30 — Community research changed the app's design

Surveyed everyone who has driven a DTV+ over the network. Three separate people
had locked this controller up with polling — no HTTP, no ping, for hours,
sometimes needing a power cycle, while the touchscreen kept working.

**Our app was polling every 2.5 seconds, roughly six times faster than the
interval already known to be unsafe.** Backed off to 15 s idle / 5 s active and
cached the configuration endpoint. Idle load went from ~0.8 to ~0.07 req/s.

Also found and fixed two bugs this hardware cannot demonstrate: outlet numbering
uses two different index spaces (they happen to coincide here), and `PurgeActive`
means water is already running during the auto-purge warm-up.

**Why it matters:** the fix arrived from reading other people's failures rather
than from wedging our own controller.

### 09:00 — First working control path

Reverse-engineered the controller's API by mirroring its own web UI. The blocker
nobody's writeup leads with: the `.cgi` endpoints answer in **HTTP/0.9** — a bare
body with no status line — which Node, `fetch` and every browser reject outright.
Solved with a raw TCP client.

Built a React app styled after the K-99693, driving the live controller.
`stop_shower.cgi` returned the controller's `:)` — command path confirmed
end-to-end without opening a valve.

**For Kohler:** the local CGI API answering in HTTP/0.9 makes the controller
unreachable from any standard HTTP client library. A status line would make this
integrable with no other change.

## 2026-07-25

### 22:32 — Interface connector pulled out

The original installation silicone-sealed the interface housing to the wall,
including the blue seal plug. Removing the interface left the plug attached to
the wall, which pulled the internal wire-to-board connector out of its socket.

Logged by the controller as `DETACH_EVENT` at 22:32.

The connector is not reachable without opening the housing. Contacts were
inspected while accessible: clean copper and gold, no corrosion — the housing was
vapor-tight and recessed well away from direct water.

The original intent was only to inspect for corrosion behind the panel.

**Why it matters:** this is why the project exists. It is *not* the cause of the
shutoffs, which predate it by ~2 months.
**For Kohler:** sealing the housing and the blue seal plug together at
installation makes the interface effectively non-removable without pulling the
internal connector. Is there a supported method for reconnecting it without
cutting the housing open? The unit is ~$1500.
