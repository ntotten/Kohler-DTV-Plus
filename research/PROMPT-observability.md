# Priming prompt — observability / telemetry session

Copy everything below the line into a fresh chat in `e:\git\Kohler-DTV-Plus`.

**Revision 2026-08-04.** Revision 1 (2026-07-26) was written while the K-99693
interface was physically disconnected and before the app had ever opened a
valve. Both are now false. What changed, and why each change matters to this
session, is in "What changed since revision 1" below — read it, because three of
the changes alter the constraints rather than just the context.

**Amended 2026-08-04, 22:30.** Added the egress request/response tracing
requirement and the browser log viewer — change 5 below, and items 5-7 of "What
I want built". Short version: a trace of controller *state* cannot see what this
app told the shower to do, and we found out the hard way that it needed to.

---

I need to build passive observability for a Kohler DTV+ shower system so we can
root-cause a "shower randomly stops mid-use" fault. Please start by reading
`CLAUDE.md`, `AGENT.md`, `INVESTIGATIONS.md` (investigation I1),
`research/FIELD-NOTES.md` and `DESIGN.md` — they carry the context and the hard
constraints. Don't re-derive what's already in them.

Note that several of those documents are **stale in specific ways listed below**.
Where a document and this prompt disagree, this prompt is newer; where this
prompt and the live hardware disagree, the hardware wins.

## The problem

For ~2 months the shower stopped on its own 2-4 minutes into a shower. No
pattern found yet.

The decisive evidence is a video I recorded on 2026-07-14 (transcript at
`E:\proj-med\build-661-diag-kohler-shower\2026-07-14-DTV-shower-unexpectedly-stops.txt`,
technical content already extracted into `INVESTIGATIONS.md` I1):
**after the water stops, the controller still reports the shower as running** and
keeps doing so for about a minute before timing out to the clock screen. So
nothing commanded it off — the valve stops and the controller finds out late.
The setpoint also reverted 97 → 96 °F on its own during that session, and 96 is
`def_temp`.

**The most important fact, and it constrains everything:** I cleared the
controller's error log *before* filming that session, reproduced the shutoff on
camera, and captured the log *after*. It said `No errors are logged from
Controller`. Cleared before, reproduced during, empty after — a controlled
negative. Captures are in `research/diagnostics/2026-07-14-*.log`.

So the shutoff writes **nothing**: no detach, no valve fault, no task exception,
no link drop. And we know the mechanism works, because pulling the interface
connector on 07-25 logged code 100 within seconds.

That demoted what had been the leading theory. If the valve lost power or fell
off the RS-485 bus, code 100 is exactly what should have appeared.

**Important caveat on that negative:** it rules out things that write to the
controller's on-board log (codes 100-204). It does **not** rule out *valve*
errors, which travel the Saturn serial protocol and surface only as the transient
flags `valve1_ErrorFatal` / `valve1_ErrorResettable`. Those are current state, not
history — read them the next day and you learn nothing. Sampling them *during* a
shutoff is the single highest-value thing this work can do.

**Current leading hypothesis — tankless heater minimum-flow cutout.** My hot
water is **tankless**. Tankless units have a minimum activation flow (~0.5-0.75
GPM) and no reservoir, so if flow drops below it the burner stops and hot water
goes cold within seconds. The valve then can't reach setpoint and shuts off
rather than deliver cold water — which is behaviour I expect from it. Nothing
reaches the controller log because valve errors don't go there.

This fits the detail that previously looked contradictory: in the 07-14 repro I'd
**turned off the overhead and left only the handshower running** to save water,
about 3.5 minutes before it failed. Low flow is the trigger under this
hypothesis, not a counter-example.

**So be sceptical about what controller polling can achieve.** The chain above is
mostly invisible to it. Please tell me what would actually discriminate the
hypotheses — including instrumenting things that aren't the DTV+ at all (outlet
temperature, flow rate, the tankless unit's own fault log). I'd rather hear
"polling the controller won't answer this, here's what would" than get a
beautifully engineered logger that cannot see the fault.

---

## What changed since revision 1 — read this before planning

### 1. The K-99693 interface is reconnected and healthy — this is now a variable

Revision 1 said the interface was physically disconnected and that reconnecting
it was "a separate track from this session." It is no longer separate.

Verified against the live controller on 2026-08-04:

```
num_interface        = 1          (was 0)
ui1_con_string       = conn       (was not_seen)
valve_1_con_string   = conn       fw 0.12
controller fw        = 0.0.3.89   MAC 00:14:6F:0E:53:E1
```

Three consequences, and the second one is a constraint, not context:

- **The shutoffs predate the interface's removal by ~2 months, and the interface
  has now been absent and present across the fault's lifetime.** That is a free
  natural experiment. "Does the shutoff still happen with the interface
  reconnected?" is now answerable and is one of the highest-value open questions.
- **The interface is a second HTTP client.** The controller's own UI polls
  `system_info.cgi` every 5 s and `values.cgi` every 10 s
  (`research/controller-mirror/js/control.js`), and the controller allows **two
  concurrent HTTP sessions** total. Our app is one. The wall interface may be
  another. **The concurrency budget is tighter than it was when revision 1 was
  written, and this is the constraint most likely to bite this session.** Work
  out what the reconnected interface actually costs before adding anything.
- **The premise text in `README.md` and `DESIGN.md` is now wrong.** Both still
  open with `num_interface = 0` / `ui1_con_string = not_seen` as the reason the
  project exists. Don't correct them as part of this session — just don't trust
  them.

### 2. The app has driven a real shower, successfully

Revision 1 and `DESIGN.md`'s "Known gaps" both say the app is unverified against
running water. **That is stale.** The operator has run a full shower through the
browser app against the live valve, and it worked. The command path is proven end
to end, not just as far as `stop_shower.cgi`.

This does **not** relax the constraint below: *you* still never open a valve. It
does mean the transport, the gate, `quick_shower.cgi` and the optimistic-state
handling are load-bearing production code with a real user, so telemetry must not
destabilise them.

### 3. `values.cgi` can return the degraded payload TWICE IN A ROW

`research/FIELD-NOTES.md` §6 documents an intermittent read where a healthy valve
reports `valve1_installed: false` / `valve_1_con_string: 'dis'`, recovering on the
next read — roughly one in 30-50. The proxy's guard
(`app/server/middleware.mjs:57-86`) is built on the assumption that a bad payload
does not repeat: it requires the loss to be reported **twice** before believing it.

**On 2026-08-04 it repeated.** `npm run selftest` read `values.cgi`, saw the valve
absent, re-read to rule out the known flap, and **got the same answer again**.
Six deliberate reads immediately afterwards were all healthy:

```
selftest read 1   installed=false   300 keys
selftest read 2   installed=false   300 keys     <- the guard's assumption fails here
follow-up  1..6   installed=true    304 keys     conn, fw 0.12, over 18 s
```

Two things follow, and the second is the useful one:

- **The twice-guard is defeated by exactly this event.** Two consecutive suspect
  reads make the middleware accept and cache the bad payload — the "disabled start
  button for 30 seconds, possibly with someone standing in the shower" outcome
  that FIELD-NOTES §6 says the guard exists to prevent.
- **The degraded payload is SHORT: 300 keys versus 304.** That is a cheap,
  reliable discriminator the code does not currently use. A truncated response and
  a genuine valve disconnection are not the same event and do not look the same on
  the wire. **Any telemetry that logs `valve_1_con_string` without also logging the
  key count will manufacture false valve-dropout events — and a valve dropout is
  precisely the signature this whole investigation is hunting.** Getting this
  wrong would not merely add noise; it would fabricate the finding.

Treat "record payload completeness alongside payload content" as a requirement,
not a nicety.

### 4. The cheapest experiment still has not been run

Revision 1 asked whether the high-flow experiment should come first. It should,
and it still has not happened. See "Sequencing" below.

### 5. The app was sending doubled valve commands, and nothing could see it

On 2026-08-04 we found that every outlet tap while water was running sent
`quick_shower.cgi` **twice**, about 120 ms apart, in `npm run dev`. Fixed in
`fa66f82`; the story is in `STORY-LOG.md`. Two things about it bear directly on
what you build.

**A controller-state trace would not have caught it.** Both commands set exactly
the same state, so the second one is invisible in `values.cgi` and
`system_info.cgi`. Polling the controller harder would not have helped; the
duplicate existed only on the wire, outbound. Confirming the fix had to be done
by hand in browser DevTools, because `app/server/middleware.mjs` logs nothing at
all — the app currently keeps **no record of what it told the shower to do**.

**That is a confounder for this investigation, not just an app bug.** If a
shutoff lands inside a captured session, the first thing you must be able to
rule out is "our app commanded something." Right now you cannot: a spontaneous
stop and an app-issued stop are indistinguishable from controller state alone.
Egress tracing is therefore a prerequisite for the state trace being
interpretable, not a nice-to-have alongside it.

**What this does *not* mean.** The shutoffs predate this app by ~2 months, and
predate the doubled command. It is not a suspect for the original fault. It is
noise that must be excluded from future traces.

**And a limitation to state plainly in what you build:** our egress log sees
only *our* client. The reconnected K-99693 wall interface is a second client
talking to the same controller, and its commands do not pass through our proxy.
"No REQ line in our log" means "we didn't send it", never "nobody sent it". Say
so in the log's own documentation, because that distinction will be load-bearing
the first time a trace shows an uncommanded stop.

---

## What I want built

A passive telemetry capture that runs on this dev box and gives us a trace
spanning a real shutoff. Two halves, and both are required: **what the
controller reported** (state) and **what our app asked it to do** (egress). A
trace with only the first half cannot rule our own client out as the cause, and
cannot see a duplicate command at all — see change 5.

Decisions already made:

1. **Runs on the dev machine for now.** If this proves worth it, we migrate to my
   home server. Don't build for the home server yet, but don't paint us into a
   corner either.
2. **No extra load on the controller.** Piggyback on the app's existing polling
   (15 s idle / 5 s active) rather than adding a second poller.
   `research/FIELD-NOTES.md` §1 explains why this is non-negotiable: three
   separate people have locked this controller up with polling, taking it out for
   hours. **With the wall interface reconnected this matters more than it did** —
   see change 1 above.
3. **Parseable, not enormous.** Capture what's useful for this class of fault.
   Rotate or cap it. Revision 1 said JSONL; item 6 now also demands something a
   human can grep and read in sequence. Resolve that deliberately rather than by
   accident — see "Log format" below.
4. **Capture completeness, not just content.** Every record carries the payload's
   key count and whether the proxy served it from cache, so a truncated read is
   distinguishable from a real state change after the fact. See change 3.
5. **Trace egress, not just state — every request AND its response.** Each call
   the proxy makes to the controller gets a line when it is sent and a line when
   it answers, correlated by a short id, with the outcome, the duration and the
   payload size. This covers reads and commands alike. Without it we cannot tell
   an app-issued stop from a spontaneous one, and we cannot see a duplicate
   command at all. See change 5.
6. **A format that is trivially greppable and readable in sequence.** One event
   per line, fixed leading columns, so `grep quick_shower`, `grep ' ERR '` and
   `grep ' DIFF '` all work with no tooling. Reading the file top to bottom
   should make the sequence of activity obvious, and **state changes should
   appear as their own lines rather than having to be diffed by eye across
   full payload dumps.** Emit a state line only when something actually changed;
   an unchanged poll is one short line, not a payload.
7. **The operator reads the log from a browser, including a phone.** I am not
   walking to the dev box or SSHing into a home server to find out what happened
   — often I will be standing in a bathroom, wet, having just watched the shower
   stop. The log has to be reachable from the same browser I use to run the
   shower, over the LAN, with the newest lines first or auto-scrolled to the end.

Please treat these as settled unless you find a concrete reason one is wrong, in
which case say so.

### Log format — a strawman to improve on, not a spec

Propose your own if you can do better, but it must keep the properties in items
5 and 6. This shape satisfies them:

```
2026-08-04T22:14:03.118Z  REQ   a3f1  quick_shower.cgi  valve1_outlet=13 valve1_temp=96 valve1_massage=0
2026-08-04T22:14:03.256Z  RES   a3f1  quick_shower.cgi  ok 138ms 21B
2026-08-04T22:14:08.004Z  REQ   a3f2  system_info.cgi
2026-08-04T22:14:08.100Z  RES   a3f2  system_info.cgi   ok 96ms keys=39
2026-08-04T22:14:08.101Z  DIFF  a3f2  valve1_Currentstatus ""->"On"  ui_shower_on false->true
2026-08-04T22:14:23.010Z  RES   a3f5  values.cgi        ok 210ms keys=300 cached=false SHORT
2026-08-04T22:16:41.882Z  ERR   a41c  system_info.cgi   timeout 5000ms
```

Why each part earns its place:

- **`REQ`/`RES` correlated by id** — this is the pair that makes duplicates
  visible. Two `REQ` lines for `quick_shower.cgi` 120 ms apart is the defect from
  change 5, unmissable on sight. It also gives you real latency, which is the
  early-warning sign of the lockup in `FIELD-NOTES.md` §1.
- **`DIFF` lines** — the answer to "what actually changed and when". They are what
  you will actually read when a shutoff trace comes in.
- **`keys=` on every read** — change 3's requirement. Flag the short payload
  inline (`SHORT` above, or whatever you prefer) so a truncated read cannot be
  mistaken for a valve dropout while skimming.
- **`cached=`** — the proxy serves `values.cgi` from a short cache; a reader must
  never mistake a cache hit for a fresh confirmation.

Keep the JSONL from item 3 as the canonical machine-readable record if you want
both — but if you produce two artifacts, they must come from one source of
truth, and tell me which one is authoritative.

### Reading it from the browser

Item 7 is a requirement, not a stretch goal. The realistic scenario is: the
shower has just stopped, I am in the bathroom with a phone, and I want to know
what the last thirty seconds looked like before the detail is gone.

What it needs:

- **Served by the proxy we already run.** The dev server binds to all interfaces
  (`host: true` in `app/vite.config.ts`), so the phone can already reach it —
  the same must work under `npm run serve` (`app/server/standalone.mjs`), not
  only under `npm run dev`.
- **Reading the log costs the controller nothing.** It is a local file. Serving
  it must not touch the controller, must not open an HTTP session to it, and the
  page must not become a second poller — the two-session budget in change 1 is
  the constraint most likely to bite this session, and a log viewer would be an
  absurd way to spend it. If the page refreshes itself, it refreshes from the
  local file only.
- **Tail by default, filterable.** Last N lines, newest visible without
  scrolling, plus a way to narrow to a substring so `quick_shower` or `DIFF` is
  one tap. Raw text is fine and probably better than a UI — I want to be able to
  select and paste it into a chat or a support ticket.
- **Downloadable whole**, so a full trace can go into `research/diagnostics/`
  with a date in the filename, per `AGENT.md`.
- **Not routed through the CGI safety gate.** `app/server/cgi-safety.mjs` guards
  controller endpoints; a log route is local file serving and has nothing to do
  with it. Do not widen the gate to accommodate this, and do not let the log
  route become a way to reach the controller.
- Note in your handoff that this exposes the log to anything on the LAN. It holds
  no credentials, but it does hold the controller's IP and MAC and a record of
  when the shower ran. Tell me if you think that needs more than a note.

## The question I most want you to investigate first

**Is there a better transport than polling?** Before writing a poller, look for
anything push-shaped on this hardware:

- **Partially answered already — start from this.** The controller's own web UI
  does nothing push-shaped. `research/controller-mirror/js/control.js` contains
  exactly two timers and no `EventSource`, `WebSocket`, long-poll or chunked
  stream anywhere in the mirrored JS:

  ```js
  setInterval(function () { loadXMLDoc();   }, 5000);   // system_info.cgi
  setInterval(function () { load_status();  }, 10000);  // values.cgi
  ```

  Two things follow. First, Kohler's own UI polls `system_info.cgi` at **5 s**,
  which is exactly our active rate — so we are within what the hardware was
  designed to serve, and the lockups others hit were probably about
  *concurrency* rather than interval alone. Second, if a push transport exists
  it is undocumented and unused by the vendor's own client, so treat finding one
  as unlikely. Confirm by checking whether any endpoint holds the socket open
  rather than closing it, but timebox it.
- **New, and now the more interesting version of this question:** the wall
  interface is back on the bus and is itself a client. Does its presence show up
  anywhere readable — a session count, a last-seen timestamp, a changing field
  that only moves when it polls? If the controller exposes anything about *who
  else is talking to it*, that is both a confounder we must record and possibly a
  cheaper signal than anything we would build.
- Is there anything on the RS-485 side we could observe passively? The valve and
  controller talk Saturn protocol; `research/xagon0/docs/protocols/` documents
  it. A passive bus tap would see the failure directly instead of inferring it
  from the controller's late timeout — that's a hardware question, but tell me if
  it's the right answer.
- Does the Konnect module (`konnect_installed = true` on this system) expose
  anything locally that's more event-driven?

If the answer is "no, polling is all there is", say so plainly and move on — but
I'd like that established rather than assumed.

## What the trace needs to distinguish

`INVESTIGATIONS.md` I1 has the signature table. In short:

| Hypothesis | Signature |
| --- | --- |
| **Tankless min-flow → valve cutout** | `valve1_ErrorResettable` sets transiently (look for `ALG_COLD_TIMEOUT` 38 / `ALG_HOT_TIMEOUT` 39); outlet temperature falls before the stop |
| Valve power loss / reset | `valve_1_con_string` → `dis`, setpoint reverts to `def_temp`, controller still reports running for ~1 min |
| RS-485 comms loss | `conn` → `dis`, no setpoint reversion |
| Controller reboot | Unreachable 30-60 s |
| Purely mechanical/hydraulic | Nothing anywhere — controller simply times out |
| **Truncated read (NOT a fault)** | `con_string` → `dis` **with a short payload** — 300 keys rather than 304. Must be excluded before any of the above is claimed. |
| **Commanded by our app (NOT a fault)** | A `REQ` line for `stop_shower.cgi` or `quick_shower.cgi` in the egress log, just before the state change. Must be excluded first — see change 5. |

Note the last three rows. If the trace shows *nothing*, that is itself a result,
and it points outside the controller. If it shows a valve dropout, the first
question is whether the payload was complete — see change 3. And before any stop
is called spontaneous, the egress log must show we did not ask for it — bearing
in mind our log cannot see what the wall interface sent.

## Sequencing — do these in order

1. **Answer the transport question**, and more importantly tell me **whether
   controller telemetry can discriminate the hypotheses at all**, given the
   controlled negative. If it can't, say so and tell me what would.
2. **Tell me whether the high-flow experiment should come before the code.** Run
   the shower with several outlets open, well above any minimum firing flow, and
   see whether it survives materially longer than the handshower-alone case. If
   high flow is stable and low flow fails, that's close to conclusive with no
   instrumentation at all. My instinct is that it should come first and that the
   logger should be running while I do it, so the run is captured either way —
   tell me if you disagree.
3. **Propose what to capture and the record shape**, before writing code.
4. **Build the egress request/response log first.** It is the smallest piece, it
   is entirely client-side, it costs the controller nothing, and everything
   downstream depends on being able to say what our app did and did not send.
   Do not fold it into a larger telemetry build — land it on its own so it is
   working before anything else is added to the proxy.
5. **Then build the state trace and the browser viewer**, and get both capturing
   on the dev box.
6. **Stop there and show me real captured traces.** I want to look at actual
   content — a few minutes of idle capture, read in the browser on my phone the
   way I would actually read it — and understand what's in each field before I
   run any water.
7. **Only once I've reviewed that and confirmed will I start a shower.** Treat
   "the operator has reviewed the trace and said go" as a hard gate. Do not ask me
   to start the shower before step 6 is done.

Also worth capturing: wall-clock duration from shower start to shutoff, across
many events, so we can see whether it clusters (timer) or scatters (fault).

`cerror_logs.cgi` and `kerror_logs.cgi` are already exposed as reads (0/5) — poll
them for *changes* rather than continuously.

## Open questions this session may be able to close

Beyond the trace itself, these are now answerable and were not before:

- **Does the shutoff still happen with the interface reconnected?** It has been
  absent and present across the fault's lifetime, which is a free natural
  experiment. Even a "yes, unchanged" is worth recording.
- **How much of the two-session budget does the reconnected interface consume?**
  This decides whether telemetry can add anything at all.
- **How often does the degraded `values.cgi` payload actually occur, and does it
  cluster?** We now have a discriminator (key count) and no measurement. If it
  clusters around anything — time, load, the interface polling — that is itself a
  finding, and possibly a related one.

## Constraints you must not break

- **Nothing above 2/5** on the CGI risk scale. The gate in
  `app/server/cgi-safety.mjs` enforces it; don't weaken it. Widening the exposed
  surface needs a recorded reason and the pinned test updated deliberately.
- **Never open a valve.** All of this is read-only. I'll run showers manually.
  (The app has now driven a real shower successfully — that does not change this
  rule for you.)
- **Two concurrent HTTP sessions maximum**, serialised, with a gap — and budget
  for the wall interface being one of them now.
- **The app must stay usable.** It is my only remote way to run the shower, and it
  is now genuinely in use rather than a prototype. Tracing sits on the path every
  command takes, so a logger that can throw, block or fill a disk is a logger
  that can stop me showering. Failure to write a log line must never fail the
  request that produced it.
- **The log viewer adds zero controller load.** Local file only, no session, no
  poller. See "Reading it from the browser".

## Also

Append anything significant to `STORY-LOG.md` per the convention in `AGENT.md`.
This is being documented for YouTube (@azab2c) and may go to Kohler support, so
findings, reversals and mistakes are all worth capturing as they happen.

Sample output I can eyeball matters more than completeness — if a field is
cryptic, tell me what it means and why it's worth logging.
