# Work ledger — Kohler-DTV-Plus

**Generated** 2026-08-04 · **Revised** 2026-08-04 after operator correction + a live read-only hardware check
**Repo** `e:\git\Kohler-DTV-Plus` · **Last commit** 2026-08-04 (0 quiet days)
**Read first:** [2026-08-04 State of the Project](../../docs/state/2026-08-04-KOHLER-DTV-PLUS-STATE-OF-PROJECT.md)
**Machine-readable:** [`kohler-dtv-plus-work-scout.json`](kohler-dtv-plus-work-scout.json)
**Batch plan attached:** [`B01-strictmode-double-command.md`](B01-strictmode-double-command.md)

A replacement browser interface for a Kohler DTV+ shower, plus an open investigation into why
the shower stops mid-use — documented publicly for YouTube and for Kohler support.

> ### Revision note — three findings were wrong, all in the same direction
>
> The first draft read the repository as ground truth. The repository is **behind reality**, so
> it systematically understated the project. Corrected:
>
> | First draft                                          | Actually                                                                                         |
> | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
> | "No valve has ever been opened by the app"           | A full shower has been run through the app against the live valve. It worked.                    |
> | "The interface is disconnected, `num_interface = 0`" | **Measured 2026-08-04:** `num_interface = 1`, `ui1_con_string = conn`. Reconnected and healthy.  |
> | "No record Kohler was contacted"                     | Contacted **2026-07-27**; quoted **~$2013** for a replacement — which is what justified the cut. |
> | "The fabrication week's detail is decaying"          | **45 raw clips exist**, queued for processing through `e:\git\inferiere`. Nothing is being lost. |
>
> **The durable lesson:** this project's evidence lives in three places and only one is the git
> tree. The others are the operator's footage (`E:\proj-med\build-661-diag-kohler-shower\raw`)
> and the hardware. A repo-only sweep will always report this project as less finished than it is.

> **Path note, added after this sweep.** `research/SHUTOFF-INVESTIGATION.md` was folded into a
> new top-level [INVESTIGATIONS.md](../../INVESTIGATIONS.md) as investigation **I1**, and the old
> file deleted. `file:line` citations to it below were accurate when written; read them against
> I1. W1's experiments now live in I1's _Next to try_ queue as **E1** and **E2**, and W10's as
> **E8**/**E9** under **I3** — that queue, not this ledger, is where they are tracked from here.

## The ledger

| #          | Title                                                                     | Shape            | Age           | Route        | Cost of delay                                                                             |
| ---------- | ------------------------------------------------------------------------- | ---------------- | ------------- | ------------ | ----------------------------------------------------------------------------------------- |
| **W1**     | [Two discriminating showers nobody has run](#w1)                          | blocked-decision | 9 d           | `just-do-it` | Investigation parked; a free natural experiment decays as memory of "before" fades        |
| **W10**    | [The degraded `values.cgi` payload repeats, defeating the guard](#w10) 🔥 | silent-failure   | 0 d           | `batch`      | Every day is a chance to blank the UI mid-shower, or to fabricate a valve-dropout finding |
| ~~**W2**~~ | [StrictMode double-fires `quick_shower.cgi`](#w2) — **DONE `fa66f82`**    | silent-failure   | 9 d           | ~~`batch`~~  | — closed 2026-08-04                                                                       |
| **W8**     | [Four documents describe a world that has moved on](#w8)                  | doc-rot          | 9 d           | `just-do-it` | The front page states a premise the hardware contradicts                                  |
| **W6**     | [Kohler was contacted and quoted $2013; the repo doesn't know](#w6)       | doc-rot          | 8 d           | `just-do-it` | The number that justifies the whole repair survives only in a filename                    |
| **W4**     | [Telemetry specified, never built](#w4)                                   | dormant-decision | 0 d (revised) | `batch`      | Every shutoff meanwhile destroys transient evidence rather than deferring it              |
| **W3**     | [The fabrication record is blocked on another repo's pipeline](#w3)       | drift            | 7 d           | `study`      | Low — the source is safe; but the dependency is invisible to both repos                   |
| **W5**     | [Degraded `values.cgi` silently clamps to 113 °F](#w5)                    | silent-failure   | 1 d           | `batch`      | Nothing here; undercuts the published scald policy for anyone else                        |
| **W7**     | [`cnc/` — twelve undocumented files](#w7)                                 | orphan           | 3 d           | `just-do-it` | Cheap now; in six months nobody knows which cover is on the wall                          |
| **W9**     | [Unlicensed vendored material under a draft licence](#w9)                 | irreversible     | 9 d           | `just-do-it` | Compounds with reach, not time; forks aren't retractable                                  |

---

<a id="w1"></a>

## W1 · Two discriminating showers that nobody has run

**Shape** blocked-decision · **Last substantive touch** 2026-07-26 (9 days) · **Route** `just-do-it`

```text
Problem: The shower stopped itself mid-use for ~2 months, and the two cheapest experiments that
         would discriminate between the surviving hypotheses have never been run.
Cause:   Both are operator actions rather than code changes, so no development session picks
         them up and they fall through the gap between sessions.
Fix:     Run two showers — one at high flow (several outlets, well above any tankless minimum
         firing flow) against the known handshower-alone case; and one ordinary shower now that
         the interface is reconnected, to see whether the fault survived the repair.
Impact:  High flow stable + low flow failing ≈ confirms the tankless chain and points outside
         the DTV+. The interface run answers a question that was untestable for nine days.
Risk:    Moves real water at a configured max of 113 °F, above the 109 °F scald threshold —
         needs explicit in-the-moment operator consent, with the operator present.
Goal:    A measured high-flow duration recorded beside the low-flow one, and a recorded answer
         to "does it still happen with the interface back".
```

The high-flow experiment is named as the cheapest next step by four documents —
[SHUTOFF-INVESTIGATION.md:12-14](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use) in bold, again at
[:229-240](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use) and
[:386-389](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use), and once more in the observability prompt.
[STORY-LOG.md](../../STORY-LOG.md) records no such run.

**The second shower is newly possible and nobody has noticed.** The shutoffs predate the
interface's removal by ~2 months, and the interface has now been _absent and present_ across the
fault's lifetime — a free natural experiment created by the repair. No document asks the
question.

**What its age means:** nine days is young, but this is the only item four documents independently
name as the next step and that still has not happened — the signature of work with no owner.

**Steelman against:** the shutoff interval already scatters between "a couple of minutes" and ~4
minutes with no identified pattern, so a single high-flow run that survives 10 minutes proves
less than it feels like. The operator may reasonably prefer to spend that water on runs captured
by telemetry (W4) rather than observed by stopwatch — and if it needs three or four repetitions
per condition to mean anything, it stops being free.

---

<a id="w10"></a>

## W10 · 🔥 The degraded `values.cgi` payload can repeat, defeating the guard built to catch it

**Shape** silent-failure · **Found** 2026-08-04, this sweep · **Route** `batch` · **On fire**

```text
Problem: values.cgi returned its known degraded payload — healthy valve reported absent — on two
         consecutive reads, which is the one case the proxy's guard assumes cannot happen.
Cause:   middleware.mjs requires a valve loss to be reported twice before believing it, on the
         evidence that the blip never repeated; two consecutive suspects therefore cause the
         middleware to accept and cache the bad payload.
Fix:     Discriminate on payload completeness rather than repetition — the degraded response is
         short (300 keys against 304), so a truncated read is distinguishable from a real
         disconnection on the first sample.
Impact:  The UI stops being able to insist the shower has no configured outlets for a 30 s TTL,
         and any future telemetry stops being able to manufacture false valve-dropout events.
Risk:    A key-count threshold hard-codes an assumption about a payload that firmware could
         change; it needs to be derived or asserted rather than pasted as a magic number.
Goal:    Two consecutive degraded reads never reach the cache or the UI as a valve disconnection,
         pinned by a test that replays this exact 300-key payload twice.
```

Observed during this sweep. `npm run selftest` read `values.cgi`, saw the valve absent, re-read
to rule out the known flap per its own guard at
[selftest.mjs:138-153](../../app/scripts/selftest.mjs#L138-L153) — and got the same answer again.
Six deliberate reads immediately afterwards were all healthy:

```
selftest read 1   installed=false  con=dis    300 keys
selftest read 2   installed=false  con=dis    300 keys   <- the guard's premise fails here
follow-up  1..6   installed=true   con=conn   304 keys   fw 0.12, over 18 s
```

Against [middleware.mjs:41-51](../../app/server/middleware.mjs#L41-L51), whose comment states the
premise explicitly ("the very next read is normal again"), and the guard at
[:76-80](../../app/server/middleware.mjs#L76-L80). Documented as non-repeating in
[FIELD-NOTES.md §6](../FIELD-NOTES.md#L234-L272).

**Why this is on fire and not merely a bug.** It fails in two directions at once. Toward the
user, it produces the exact outcome FIELD-NOTES §6 says the guard exists to prevent — "a disabled
start button for 30 seconds, potentially with someone already standing in the shower." Toward the
investigation, it fabricates evidence: "controller has lost the valve" is _precisely_ the
signature being hunted, and [SHUTOFF-INVESTIGATION.md:164-175](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use)
already promoted one such sample to evidentiary status. Telemetry built on the current logic
would generate that finding on schedule.

**The fix is cheap because the discriminator is free.** 300 versus 304 keys. A truncated response
and a genuine disconnection do not look the same on the wire, and the code currently reads only
the content, never the completeness.

**What its age means:** zero days — but this is not a recency artifact. The underlying blip is
documented from 2026-07-26; what is new is a _counter-example to the assumption the mitigation
rests on_, which only appears if someone happens to read twice at the wrong moment.

**Steelman against:** n=1. Two consecutive degraded reads were observed once, and the sample
immediately afterwards was six-for-six healthy, so the base rate for repetition may be very low —
the existing guard may still be right almost always. The key-count discriminator is also inferred
from a single pair of observations (300 vs 304); it could be a coincidence of which keys the
truncation dropped rather than a reliable length signal, and hard-coding it risks trading a rare
false-negative for a systematic one. Measuring the distribution before changing the logic may be
the better first move — which is why this routes to `batch` and not `just-do-it`.

---

<a id="w2"></a>

## W2 · ✅ CLOSED — React StrictMode double-fired `quick_shower.cgi` on an outlet toggle

**Shape** silent-failure · **Closed** 2026-08-04 in `fa66f82` · **Was** `batch`, on fire

> **Resolved the same day it was found.** The decision moved into a pure
> `toggleOutletSelection()` in `app/src/api/model.ts` and the command now dispatches once from
> the caller; `<StrictMode>` stays on. Pinned two ways — `app/test/hookHarness.ts` reproduces the
> doubling so it was **measured, not inferred** (closing the caveat below and satisfying task
> B01.T02), and a second test fails if a command is ever written back inside a state updater.
> `npm run check` exits 0 — B01's hard exit criterion. Story-log entry: 2026-08-04 22:10.
>
> Kept in full below because the reasoning is the reusable part, and because the batch plan it
> produced is the worked example for the format.

**Reframed as a batch plan:** [`B01-strictmode-double-command.md`](B01-strictmode-double-command.md),
in the PCFIRG format defined by `E:\git\llm-fab\.fab\process\BATCH-DEV.md`. Header reproduced here:

```text
Problem: Tapping an outlet while the shower is running sends quick_shower.cgi twice in
         npm run dev, which is the documented way to run the app and the operator's only
         remote way to run the shower.
Cause:   toggleOutlet issues the command from inside the setSelection updater, and <StrictMode>
         double-invokes updater functions in development — an impure updater is exactly what
         that double-invocation exists to expose.
Fix:     Extract the toggle decision into a pure function, apply it outside the updater, and
         issue the command once from the caller.
Impact:  The controller receives one command per tap in every build, and the impurity that
         produced the duplicate becomes a test failure rather than an invisible extra request.
Risk:    Doing nothing leaves duplicate valve commands ~120 ms apart on a controller whose
         documented failure mode is rapid successive valve commands going unreachable for up to
         three hours; doing it touches the app's hottest interaction path, so the pure function
         must preserve today's behaviour exactly.
Goal:    A single outlet tap produces exactly one quick_shower.cgi request, pinned by a test
         that fails if a dispatch is ever moved back inside a state updater.
```

**Hard exit criterion:** `npm run check` exits 0 in `app/`.

[useShower.ts:173-186](../../app/src/state/useShower.ts#L173-L186) side-effects inside the
updater; [main.tsx:6-10](../../app/src/main.tsx#L6-L10) enables StrictMode. The client queue
([kohler-client.mjs:71-91](../../app/server/kohler-client.mjs#L71-L91)) serialises them 120 ms
apart, so this is a doubled command rather than a second HTTP session — but rapid successive
valve commands are what [FIELD-NOTES.md:26-29](../FIELD-NOTES.md#L26-L29) records as having taken
a controller offline.

**Route changed** from `just-do-it` to `batch`: the plan's first two tasks are to pin the defect
with a failing test and to _measure_ the doubling rather than infer it, which is more than a
no-ceremony fix carries. The batch explicitly forbids the tempting wrong fix — disabling
StrictMode, which hides the detector rather than the defect.

**Raised stakes since the first draft:** the app is no longer a prototype. It has driven a real
shower, and the operator uses it.

**Steelman against:** StrictMode double-invocation is development-only; `npm run serve` does not
do it. The queue serialises, so the two-session limit is never breached, and the community lockup
reports involve a browser tab plus an integration rather than two serialised requests. This may
be a doubled request the controller simply absorbs, and nobody has observed it misbehaving.

> ⚠️ **Verification caveat, carried into the plan as task B01.T02.** The mechanism is confirmed by
> reading both files plus React's documented StrictMode contract. I did **not** instrument the
> running app to observe two POSTs.

---

<a id="w8"></a>

## W8 · Four documents describe a world that has moved on

**Shape** doc-rot · **Last substantive touch** 2026-07-26 (9 days) · **Route** `just-do-it`

```text
Problem: The repository's front page and its architecture doc both open by stating a premise the
         hardware now contradicts, and its honest-limitations section understates what has been
         proven.
Cause:   The interface was reconnected and a real shower was run without any document being
         revisited; separately, PROTOCOL.md's endpoint table was never updated when the gate was
         widened for the investigation.
Fix:     Correct the four in one pass — the premise in README and DESIGN, the "no valve opened"
         limitation, PROTOCOL.md's Read table, and the two stale test counts.
Impact:  A reader — human, agent, or Kohler engineer — stops being told the interface is dead and
         the app unproven when neither is true.
Goal:    No document states a fact that npm test, the live controller, or the safety gate
         contradicts.
```

Five concrete corrections:

| Document                                                                                  | Says                                                                              | Actually                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md:13-23](../../README.md#L13-L23)                                                | `num_interface = 0`, `ui1_con_string = not_seen` as the reason the project exists | Measured 2026-08-04: `1` and `conn`                                                                                                                                                    |
| [DESIGN.md:11-14](../../DESIGN.md#L11-L14)                                                | Same premise                                                                      | Same                                                                                                                                                                                   |
| [DESIGN.md:203-206](../../DESIGN.md#L203-L206)                                            | "no valve has been opened by this app yet"                                        | A full shower has been run through it                                                                                                                                                  |
| [PROTOCOL.md:45-53](../../PROTOCOL.md#L45-L53)                                            | Three readable endpoints                                                          | Five — `cerror_logs.cgi` and `kerror_logs.cgi` are exposed at [cgi-safety.mjs:40-45](../../app/server/cgi-safety.mjs#L40-L45); [FLOW.md:161-163](../../FLOW.md#L161-L163) has it right |
| [DESIGN.md:166](../../DESIGN.md#L166) / [viewer/README.md:87](../../viewer/README.md#L87) | 49 / 88 unit tests                                                                | 56 / 164, measured                                                                                                                                                                     |

**What its age means:** every one of these is a document whose last touch predates the change it
should reflect. That is the age tell itself, not a coincidence — and the premise rot is nine days
old only because the whole repo is.

**Steelman against:** the premise text is _historical_ and arguably correct as history — the
project exists because the interface failed, and that remains true even though it has since been
repaired. Rewriting the front page to say "the interface works now" risks obscuring why any of
this was built, and the fix is a narrative decision (does the README tell the origin story or the
current state?) rather than a factual correction. Only the test counts and the endpoint table are
unambiguous errors.

---

<a id="w6"></a>

## W6 · Kohler was contacted and quoted $2013, and the repository does not know

**Shape** doc-rot · **Last substantive touch** 2026-07-27 (8 days) · **Route** `just-do-it`

```text
Problem: The plan of record still presents cutting the housing as conditional on Kohler having no
         supported route — but both steps were completed, and the outcome that justified the cut
         exists only as text in a video filename on another drive.
Cause:   The contact happened on camera rather than at the keyboard, and AGENT.md's "Kohler
         support is contacted, and what they said" trigger has never fired.
Fix:     Append the support contact and the ~$2013 replacement quote to STORY-LOG.md, and rewrite
         the plan of record in the past tense.
Impact:  The economic justification for cutting a $1500 part — the number that makes the repair
         rational rather than reckless — becomes part of the record instead of a filename.
Goal:    The repo states what was asked of Kohler on 2026-07-27 and what came back.
```

**Corrected from the first draft, which asked whether step 1 had been skipped.** It was not.
[SHUTOFF-INVESTIGATION.md:367-375](../../INVESTIGATIONS.md#i1--the-shower-stops-mid-use) required contacting
Kohler first; that happened 2026-07-27, the answer was a ~$2013 replacement and no supported
reconnection route, and the cut proceeded the next day. **The process was followed correctly and
not written down** — evidence:
`raw/obs/2026-07-27-kohler,digial-interface,tech-support (@04_07_$2013_for replacement).mp4`.

Separately, seven **For Kohler** entries across STORY-LOG remain uncollated
([AGENT.md:76-77](../../AGENT.md#L76-L77)) — contact has now happened once, on a different
subject, and the questions about CAD, logging and HTTP/0.9 are still undelivered.

**Steelman against:** the $2013 quote will surface anyway when the footage is processed (W3), so
writing it by hand now duplicates the pipeline's output and risks a transcription error on the
one number that matters. And the seven For Kohler entries are better delivered as one considered
package alongside a resolved investigation than appended to a support call about a part that has
already been modified.

---

<a id="w4"></a>

## W4 · The telemetry capture was specified and never built

**Shape** dormant-decision · **Prompt revised** 2026-08-04 by this sweep · **Route** `batch`

```text
Problem: No trace has ever been captured spanning a real shutoff, so the transient valve flags
         that would confirm or kill the leading hypothesis go unobserved every time it happens.
Cause:   A complete brief was written as a priming prompt for a session never run; nothing
         referenced it and no telemetry code exists in the tree.
Fix:     Run the revised brief after W1 reports — or retire it in favour of instrumenting outside
         the DTV+ — and record which and why.
Impact:  Either a JSONL trace sampling valve1_ErrorFatal/ErrorResettable through a shutoff, or a
         recorded decision that controller telemetry cannot see this fault.
Risk:    The brief warns against "a beautifully engineered logger that cannot see the fault";
         building it before W1 reports is the specific way to waste the effort.
Goal:    The observability question has an answer in the repo — built or declined, with reasons.
```

**[`research/PROMPT-observability.md`](../PROMPT-observability.md) has been reviewed and revised
to revision 2 as part of this sweep.** It was written on 2026-07-26 against two premises that are
now false — that the interface was disconnected, and that the app had never opened a valve — and
running it unchanged would have primed a session with stale constraints. What changed:

1. **The reconnected interface is now a first-class variable, and one change is a constraint, not
   context.** The interface is a second HTTP client polling `system_info.cgi` every 5 s against a
   two-session ceiling. The concurrency budget is tighter than when revision 1 was written, and
   the brief now says to establish what the interface costs _before_ adding anything.
2. **The natural experiment is called out** — the interface has been absent and present across
   the fault's lifetime, so "does the shutoff still happen now?" is newly answerable.
3. **W10 is folded in as a requirement**, not a note: record payload completeness alongside
   payload content, because telemetry that logs `con_string` without the key count will
   manufacture the exact valve-dropout finding the session is hunting. A new row was added to the
   signature table for "truncated read (NOT a fault)".
4. **The stale claims are flagged rather than silently fixed**, so the session knows which
   documents to distrust without being sent to correct them.
5. **Sequencing is now explicit and numbered**, with the high-flow experiment before the code and
   the operator gate before any water.

**Steelman against:** the brief's own argument remains the strongest one. The controlled negative
shows the shutoff writes nothing, and the mechanical/hydraulic hypothesis predicts controller
telemetry will record only "running, then timed out" — which is already known. A logger could
cost a week and confirm a negative. Instrumenting the tankless unit and outlet water temperature
is cheaper per bit of information and sits outside the fragile controller entirely.

---

<a id="w3"></a>

## W3 · The fabrication record is blocked on another repository's pipeline

**Shape** drift · **Last substantive touch** 2026-07-28 (7 days) · **Route** `study`

```text
Problem: A reader of this repo cannot tell that a $1500 part was machined open between 2026-07-27
         and 08-03, because the story log jumps straight over the fabrication week.
Cause:   The work is fully captured — 45 raw clips — but the footage has not been processed, and
         the repo's narrative is downstream of a pipeline in a different repository.
Fix:     Scope what processing these clips through `e:\git\inferiere` actually requires — rename,
         transcribe, index, summarise, package for ZoomTube — and what it emits that STORY-LOG.md
         can be derived from.
Impact:  The project's most visual sequence becomes tellable, and the pattern generalises to
         every future session that films rather than types.
Goal:    A known, costed path from 45 raw clips to dated story-log entries.
```

**Corrected from the first draft, which called this a decay clock.** It is not.
`E:\proj-med\build-661-diag-kohler-shower\raw` holds **45 clips** spanning 2026-06-25 to 07-29:

| Source   | Clips | Covers                                                                               |
| -------- | ----- | ------------------------------------------------------------------------------------ |
| `mob/`   | 19    | The original removal that pulled the connector; the whole 07-28 fabrication day      |
| `obs/`   | 18    | The Kohler support call (07-27); 07-28 bench work; rear access cover install (07-29) |
| `gopro/` | 4     | —                                                                                    |
| `img/`   | 4     | 2026-07-27 stills                                                                    |

Nothing is being lost, so the urgency drops sharply and the **shape changes**: this is not "write
the missing entries from memory", it is "run the pipeline, then derive the log from what it
produces". Hand-writing entries now would duplicate the pipeline's work and present recall as
observation — the failure mode [AGENT.md:41-44](../../AGENT.md#L41-L44) exists to prevent.

**Route changed** from `just-do-it` to `study`: the commissioning signal holds — the questions
cluster in one domain (what does `inferiere` need, what does it emit, what does ZoomTube
packaging imply for a repo's story log), the answer is genuinely unknown from here, and it is a
cross-repository dependency that **neither repository records**.

**Steelman against:** the technical outcome is already preserved — toolpaths, DXF, both STLs and
the photographs are committed, and the 2026-08-03 gasket entry captures the reasoning that
mattered. The footage will be processed eventually for the video regardless, and the story log
can be written then at no extra cost. Commissioning a study to plan a pipeline that is going to
run anyway is ceremony; the honest alternative is a one-line note in the story log saying "the
fabrication week is on video, unprocessed" and moving on.

---

<a id="w5"></a>

## W5 · A degraded `values.cgi` silently declares the system Fahrenheit and clamps the ceiling to 113

**Shape** silent-failure · **Last substantive touch** 2026-08-03 (1 day) · **Route** `batch`

```text
Problem: When values.cgi is unavailable but system_info.cgi is live — a state the proxy
         explicitly supports — the app treats the system as Fahrenheit and clamps the user's
         temperature ceiling to 113, regardless of the controller's real configuration.
Cause:   num(values?.units) returns fallback 0 for a missing payload, and 0 is the encoding for
         Fahrenheit — so "unknown" and "Fahrenheit" are the same value.
Fix:     Treat missing units as unknown rather than Fahrenheit: refuse adjustment, or hold the
         last known unit, instead of defaulting into a unit-specific ceiling.
Impact:  On a Celsius-configured DTV+, the UI's own ceiling becomes 113 °C — on the one path
         DISCLAIMER.md promises will never raise the configured limit.
Risk:    Refusing adjustment during a partial outage removes a control while someone may be in
         the shower; the failure-safe direction needs deciding rather than assuming.
Goal:    No unit-specific bound is ever derived from an absent payload, pinned by a test.
```

The chain: [model.ts:90-93](../../app/src/api/model.ts#L90-L93) (`num` fallback `0`) →
[:136](../../app/src/api/model.ts#L136) (`isF = units === 0`) →
[:152-153](../../app/src/api/model.ts#L152-L153) (`maxTemp` fallback 113) →
[useShower.ts:204-219](../../app/src/state/useShower.ts#L204-L219) (the clamp). Reachable because
[middleware.mjs:98-118](../../app/server/middleware.mjs#L98-L118) serves a status response with
`values: null` when only `system_info.cgi` answers. Against
[DISCLAIMER.md:101-103](../../DISCLAIMER.md#L101-L103).

**Now sharing a root cause with W10.** Both are the app mishandling a `values.cgi` that arrives
degraded rather than absent — and W10 proves that degraded arrivals are more common and more
persistent than assumed. Consider fixing them in one batch.

**Steelman against:** it cannot fire here — this system is Fahrenheit, so `units` is 0 either way
and the fallback is correct. It needs a Celsius system **and** a `values.cgi` outage **and**
`system_info.cgi` still answering **and** a user reaching for the temperature control. The
controller's own UI bounds Celsius input at 26–`max_temp` and would likely reject 113 anyway.

---

<a id="w7"></a>

## W7 · `cnc/` holds twelve undocumented files, including the toolpath that cut a $1500 part

**Shape** orphan · **Last substantive touch** 2026-08-01 (3 days) · **Route** `just-do-it`

```text
Problem: cnc/ has no README and no provenance — nothing records which machine, which cutter,
         which of the five toolpaths was run, or which of the two cover revisions is installed.
Cause:   Committed as build output from two fabrication sessions, in commits naming the file
         rather than the setup.
Fix:     Add cnc/README.md: per file, what it cuts, on what machine with what tooling, whether it
         was run, and what changed between rear-cover v1 and v2.
Risk:    Publishing a toolpath that cuts a live shower fixture invites someone to run it on
         different workholding; the README must carry that warning, not read as an invitation.
Goal:    Every file has one line saying what it is and whether it was run; the current cover
         revision is identifiable without opening a slicer.
```

[`new-rear-port-cover.stl`](../../cnc/new-rear-port-cover.stl) versus
[`new-rear-port-cover-v2.stl`](../../cnc/new-rear-port-cover-v2.stl) cannot be told apart from the
repo. [Images/README.md](../../Images/README.md) already establishes the pattern.

**Cheaper than it was:** `obs/2026-07-29 22-40-21_installing_rear_access_cover.mp4` shows which
cover actually went on, so W3's pipeline supplies most of the answer.

**Steelman against:** these are working files from a one-off repair on one unit with one machine
and jig, not a publishable process. A README risks implying reproducibility that does not exist.
Deleting the superseded v1 files might serve a reader better than documenting them.

---

<a id="w9"></a>

## W9 · A public repo carries three unlicensed third-party bodies under a licence labelled a draft

**Shape** irreversible · **Last substantive touch** 2026-07-26 (9 days) · **Route** `just-do-it`

```text
Problem: This repo vendors xagon0's analysis, Kohler's guide and controller UI mirror, and
         content inherited from timelery — none of which grants any licence — and applies to its
         own work a licence whose vendored text is headed "Status: Draft".
Cause:   The question was resolved honestly at the documentation level by writing a scope table,
         but nothing was ever asked of the three upstreams.
Fix:     Ask xagon0 for explicit permission to vendor and redistribute; decide whether a draft
         licence is the right thing to publish under before a video points an audience here.
Risk:    Asking may get a "no", forcing removal of research/xagon0/ — material the protocol and
         safety analysis leans on heavily.
Goal:    Every third-party body has either an explicit permission or a recorded decision to carry
         it reference-only with that risk accepted in writing.
```

[LICENSE.md:15-38](../../LICENSE.md#L15-L38) (the scope table),
[:44-49](../../LICENSE.md#L44-L49) (the draft header),
[Images/README.md:12-18](../../Images/README.md#L12-L18).

**What its age means:** age is irrelevant here, and that is the point — this is structural, not
decaying. Cheap now, while the audience is small; expensive after a video drives traffic.

**Steelman against:** LICENSE.md already handles this about as well as documentation can. Nothing
is being sold or relicensed, and the use is repair and research. Asking xagon0 risks a refusal
that removes material currently carried without objection.

---

## Considered and dropped

| Candidate                                                          | Why it didn't clear the bar                                                                                                                                                                                                                               |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DESIGN.md's Known gaps (presets, massage speed, second valve, PWA) | Spec-compliant deferrals declared plainly at [DESIGN.md:202-216](../../DESIGN.md#L202-L216). Deliberately deferred is not drift. The one item that _has_ graduated — "no valve opened" — moved into W8.                                                   |
| The Android / Capacitor port                                       | Aspirational by design, decision already made, seam verified in [config.ts](../../app/src/api/config.ts). Scope not yet started is not a finding.                                                                                                         |
| `SOURCES.md` swept exactly once                                    | Nine days is not stale for an index of slow-moving upstream repos. Flagging it would be promoting a weak item to fill an age quota.                                                                                                                       |
| The viewer has no stated goal in-repo                              | **Withdrawn on correction.** It is a deliberate Maker Galaxy experiment and the gizmo is already graduating. The residual — that neither repo records which experiments cross over — is noted in the state doc §7, and is too small to bill as work here. |
| The `values.cgi` blip filter's `suspectCount` logic                | **Partially withdrawn.** The state machine behaves as written; its _premise_ does not hold. Promoted to W10.                                                                                                                                              |
| The viewer's view-cube work (5 commits / 2 days)                   | In flight with an owner and momentum. WIP is not priority.                                                                                                                                                                                                |
| `viewer/README.md` has two items numbered `3`                      | A markdown slip with no consequence. Fold into any edit that touches the file.                                                                                                                                                                            |
| The 2017 fork-era content                                          | Genuinely dead and correctly so. Only live residue is licensing, captured in W9.                                                                                                                                                                          |

## Quota check — **not met, and it cannot be**

**0 of 10** candidates have a last-substantive-touch older than 90 days.

The repository's substantive history begins **2026-07-26 — nine days ago**. The only content
older than 90 days is the 2017 fork stub: eight commits, a README and a Jekyll config, the latter
deleted and the former rewritten (`git log --until=2018-01-01` confirms the tree held two files).
There is no old code, no old decision register, and no aging dependency here to find.

Applying the quota's intent rather than its letter: **W9** is the age-irrelevant structural item,
and **W6** is the one where age did real damage — an eight-day-old $2013 quote that justifies the
entire repair and survives only in a filename.

**The genuine anti-recency risk in this repo is not old-versus-new but repo-versus-world.** The
loudest workstream (the viewer, 5 commits in 2 days) produced **zero** candidates. The findings
came from the quiet investigation, from the operator's corrections, and from a single read-only
hardware check that contradicted three documents in one command.

## Caveats

**Verified by execution:** test counts (`npm test`, both apps); exposed-endpoint count (`node`
against `exposedEndpoints()`); the git age map for every non-binary tracked file; and — new in
this revision — a live read-only check of the controller (`npm run selftest` plus six direct
`values.cgi` reads), which established `num_interface = 1`, `ui1_con_string = conn`, valve `conn`
fw 0.12, and the two consecutive degraded payloads behind W10.

**Verified by reading:** every claim cited with a `file:line`.
**Verified by operator report:** the successful real shower, the Maker Galaxy purpose of the
viewer, and the `inferiere` processing plan.

**Not measured:** W2's doubled request is confirmed by code reading plus React's documented
StrictMode contract — the batch plan's task B01.T02 exists to measure it. W5 is confirmed by
tracing `num()`/`isF`/`maxTemp`; no Celsius controller exists to demonstrate it. W10's key-count
discriminator rests on **one** observed pair (300 vs 304) and needs a distribution before it is
trusted as a threshold.

**Not read:** `PROTOCOL.md` lines 149-212, the vendored xagon0 documentation, the
controller-mirror JavaScript, the 45 raw video clips (filenames only), and the binary CNC/CAD
assets beyond their listings.

**Cross-repo distortion to watch for:** this repo's documentation quality is unusually high, and
the first draft of this ledger was still wrong three times — always in the direction of
understating the project, because the repo lags the work. **When sweeping a repo whose operator
films the work, budget for a correction pass and ask before concluding something did not happen.**
