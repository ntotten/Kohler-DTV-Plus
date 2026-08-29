---
title: "Kohler DTV+ — State of the Project"
filename: "2026-08-04-KOHLER-DTV-PLUS-STATE-OF-PROJECT.md"
status: "point-in-time-assessment"
date: "2026-08-04"
generated_by: "work-scout sweep"
audience: ["Product Planner Agents", "Operator", "Dev Agents"]
purpose: >
  A snapshot of what this repository actually contains versus what its documents
  claim, taken nine days into its substantive life. It exists to support one
  decision: where the next hour of attention goes — the open hardware
  investigation, the physical repair record, or the software that is already
  further along than either.
sources:
  - "README.md"
  - "AGENT.md"
  - "CLAUDE.md"
  - "DISCLAIMER.md"
  - "DESIGN.md"
  - "PROTOCOL.md"
  - "FLOW.md"
  - "LICENSE.md"
  - "STORY-LOG.md"
  - "Images/README.md"
  - "app/README.md"
  - "app/server/cgi-safety.mjs"
  - "app/server/middleware.mjs"
  - "app/server/kohler-client.mjs"
  - "app/src/main.tsx"
  - "app/src/api/model.ts"
  - "app/src/api/config.ts"
  - "app/src/state/useShower.ts"
  - "app/package.json"
  - "viewer/README.md"
  - "viewer/package.json"
  - "research/FIELD-NOTES.md"
  - "research/SHUTOFF-INVESTIGATION.md"
  - "research/SOURCES.md"
  - "research/PROMPT-observability.md"
  - "cnc/ (directory listing; no documentation present)"
repo_activity:
  last_commit: "2026-08-04"
  quiet_days: 0
  is_active: true
invalidated_by: >
  The high-flow experiment being run (it resolves or kills the leading
  hypothesis and re-ranks everything); any telemetry capture landing in the
  tree; the raw fabrication footage being processed and indexed; or the
  shutoff recurring — or failing to recur — now that the interface is back.
corrections: >
  Revised 2026-08-04 after operator correction and a live read-only check
  against the controller. Three claims in the first draft were wrong, and all
  three were wrong the same way: I inferred project state from the repository
  when the repository was behind reality. The valve HAS been opened by the app;
  the K-99693 interface IS reconnected and healthy; Kohler support WAS
  contacted. See "Corrections" below.
---

# Kohler DTV+ — State of the Project

## 0. Corrections to the first draft of this document

This document was drafted from the repository, then corrected by the operator and
by a live read-only check against the controller on 2026-08-04. **Three claims
were wrong, and they were wrong in the same direction: the repository is behind
the world, so reading it as ground truth understated the project.** They are
recorded here rather than silently fixed, because the pattern is itself the
finding.

| First draft said                                             | Actually                                                                                                                                 | How it was established                                                                                              |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| "No valve has ever been opened by the app"                   | **The operator has run a full shower through the browser app against the live valve. It worked.** The command path is proven end to end. | Operator, 2026-08-04. The repo says otherwise at `DESIGN.md:203-206`.                                               |
| "The K-99693 interface is disconnected; `num_interface = 0`" | **It is reconnected and healthy.** `num_interface = 1`, `ui1_con_string = conn`.                                                         | Measured — `npm run selftest` and six direct `values.cgi` reads, 2026-08-04.                                        |
| "No record that Kohler support was contacted"                | **They were, on 2026-07-27, and quoted ~$2013 for a replacement** — which is what justified cutting the housing.                         | Operator's recording: `raw/obs/2026-07-27-kohler,digial-interface,tech-support (@04_07_$2013_for replacement).mp4`. |

A fourth correction is a reframing rather than a factual error: the parts viewer
is not an unowned side quest, it is a **deliberate Maker Galaxy experiment** —
see §3.2.

**The lesson for any future reader, including the next scout:** this project's
evidence lives in three places, and only one of them is the git tree. The other
two are the operator's recorded footage (`E:\proj-med\build-661-diag-kohler-shower\raw`,
45 clips) and the hardware itself. A sweep that reads only the repo will
systematically report this project as less finished than it is.

## 1. TLDR

This repository replaces the failed K-99693 wall interface of a Kohler DTV+
shower with a browser app, and runs an open investigation into why the shower
stops of its own accord two to four minutes into a use. **The app works against
real hardware — it has driven a full shower — and the physical repair has
succeeded: the interface is reconnected and reporting `conn`.** The software
rests on a working HTTP/0.9 transport behind a safety gate that is code rather
than documentation (56 passing tests, 18 of ~56 known CGI endpoints reachable,
nothing above 2/5). A second app, the parts viewer (164 passing tests, five
commits in the last two days), is a deliberate Maker Galaxy experiment in
rendering a vendor product alongside the add-on that repairs it; its view gizmo
is already on a path into that platform. **What is unfinished is the original
question and the record of the work.** The investigation still sits at a gate its
own documents name four times — a **free experiment: run the shower at high flow
and see if it survives longer** — which has not been run; the telemetry capture
specified for it was never built; and a week of physical fabrication that cut a
$1500 part open is missing from the story log, though **the raw footage exists**
and is queued for processing through `inferiere`. Two live findings from
2026-08-04 sharpen this: the interface's return is an unexploited natural
experiment on the fault, and `values.cgi` was observed returning its known
degraded payload **twice consecutively** — defeating the proxy guard built on the
assumption that it never repeats, and producing exactly the false valve-dropout
signature the investigation is hunting. The repo is nine days old in substance.
Nothing here is stale; the gap is between a working system and the written
record of why it works.

## 2. What it is intended to do

In the project's own terms (README.md:3-15, AGENT.md:7-16):

- **Replace a dead interface.** The K-99693 wall unit reports `not_seen` while
  `valve_1`, `amp` and `controller` all report `conn`. The shower works; nothing
  can command it. The controller's undocumented CGI API becomes the input.
  **⚠️ This premise is now historical.** Measured 2026-08-04: `num_interface = 1`,
  `ui1_con_string = conn` — the interface is back. `README.md:18-23` and
  `DESIGN.md:11-14` both still open with the old reading as the project's
  justification. The app is no longer the _only_ way to run this shower, which
  changes what it is for (a better remote interface, and now a diagnostic
  instrument) without changing whether it is worth having.
- **Find out why the shower stops mid-use.** An open investigation, ~2 months of
  symptoms, currently on its third leading hypothesis.
- **Be documented publicly.** Heading for a YouTube video (@azab2c) and possibly
  for Kohler technical support. AGENT.md:14-16 instructs that everything written
  should assume Kohler engineers and a general audience will read it.

### Deliberate stances — do not violate these without a thesis change

| Stance                                                          | Where                                  | Why it is load-bearing                                                                           |
| --------------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------------------ |
| **Nothing above 2/5 is reachable, ever**                        | `app/server/cgi-safety.mjs:27,117-123` | The table self-throws at import. Widening it needs a recorded reason and a deliberate test edit. |
| **Never open a valve without in-the-moment consent**            | AGENT.md:31-34                         | Read-only work is free; moving water is not. Asked every time, not once.                         |
| **15 s idle / 5 s active polling is a floor, not a preference** | `research/FIELD-NOTES.md:14-96`        | Three separate people wedged this controller with faster polling, taking it out for hours.       |
| **No test may ever open a valve**                               | AGENT.md:79-81, DESIGN.md:163          | `npm run selftest` is safe to run while someone is showering.                                    |
| **The viewer has no hardware surface**                          | `viewer/README.md:7-10`                | Two apps, no shared code, no shared port. The fabrication tool cannot move water.                |
| **A guessed unit is worse than a refusal**                      | `viewer/README.md:38-41`               | The viewer refuses catalog entries that do not declare `sourceUnit` and `sourceUpAxis`.          |
| **Mark inference as inference**                                 | AGENT.md:41-44                         | The project's value is that its findings are grounded.                                           |
| **Report your own failures, especially then**                   | AGENT.md:45-47                         | A corrected wrong turn is more useful than a clean narrative.                                    |

## 3. What is actually built

### 3.1 `app/` — the hardware interface

| Claim                                                            | Label               | Evidence                                                                                                                                                                |
| ---------------------------------------------------------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Raw-socket HTTP/0.9 transport, one global queue, 120 ms floor    | **real**            | `app/server/kohler-client.mjs:17-91` — `enqueue()` chains every call; `MIN_GAP_MS = 120`                                                                                |
| Safety gate refuses >2/5 before a packet is sent                 | **real**            | `cgi-safety.mjs:130-154`; self-check throws at import (`:117-123`)                                                                                                      |
| 18 endpoints reachable — 5 read, 13 command                      | **real** — measured | `node -e` against `exposedEndpoints()` returns `total 18 / read 5 / command 13` of 56 rated names                                                                       |
| 56 unit tests, no hardware                                       | **real** — measured | `npm test` → 2 files, 56 passed                                                                                                                                         |
| `values.cgi` 30 s cache; `system_info.cgi` always live           | **real**            | `middleware.mjs:39,70-86,98-118`                                                                                                                                        |
| Valve-dropout blip filter ("must say so twice")                  | **real**            | `middleware.mjs:57-86`; pinned to the 2026-07-26 observation in FIELD-NOTES §6                                                                                          |
| Polling 15 s / 5 s with a 120 s tail                             | **real**            | `useShower.ts:20-23,88-90`                                                                                                                                              |
| Capacitor seam exists                                            | **real**            | `app/src/api/config.ts` — `VITE_API_BASE`, `apiUrl()`                                                                                                                   |
| Command path confirmed end-to-end, **including opening a valve** | **real**            | The operator has run a full shower through the app against the live valve. `DESIGN.md:203-206` still claims "no valve has been opened by this app yet" and is **stale** |
| Steam / lighting / rain panel                                    | **claimed**         | Coded from xagon0 and the controller's own JS. None installed here; untestable (DESIGN.md:207-209)                                                                      |
| Second valve                                                     | **claimed**         | Modelled, one valve on this system (DESIGN.md:214)                                                                                                                      |
| Android/Capacitor port                                           | **aspirational**    | A recommendation and a seam. No `android-cap` directory exists in this repo                                                                                             |
| Preset saving, massage speed                                     | **aspirational**    | `save_variable.cgi` sequence not worked out; speed parameter not located                                                                                                |

### 3.2 `viewer/` — the parts viewer

**What it is for** (operator, 2026-08-04 — not stated in the repo): a deliberate
experiment in rendering a **real vendor product together with an accessory or
add-on made to repair or upgrade it**. Maker Galaxy expects a class of projects
shaped exactly like that — mods and add-ons attached to existing vendor
products — and this repo is the concrete instance used to learn what such a
viewer needs before any of it is committed to the platform. Some of it graduates
and some does not: **the view gizmo is already on a path into Maker Galaxy.**

That reframes the "no hardware surface" separation. It is not that the viewer is
a side quest with no stated goal — it is that its goal lives in a different
repository's roadmap, and this repo is the sandbox. Its high commit velocity is
a prototype iterating, not attention leaking away from the shower.

**This purpose is recorded nowhere in the tree.** `viewer/README.md:371-383` and
`:418-429` describe the _mechanical_ Maker Galaxy alignment (shared catalog
fields, the ported `cameraFit.ts`, the decal record shape) but never say the app
exists to prove a Maker Galaxy hypothesis. A reader concludes it is a tool that
happens to resemble another project's.

| Claim                                                              | Label               | Evidence                                                                                    |
| ------------------------------------------------------------------ | ------------------- | ------------------------------------------------------------------------------------------- |
| 164 unit tests across 10 files                                     | **real** — measured | `npm test` in `viewer/` → 164 passed                                                        |
| Export gate re-derives the bbox from exported bytes                | **real**            | `viewer/scripts/verify-exports.ts`; `npm run check` includes it                             |
| K-99693 CAD is inches, Z-up, open, hollow                          | **real** — measured | 4,544 tris, 224 unshared edges, 190.30 cm³ enclosed after repair (`viewer/README.md:71-88`) |
| Mesh repair does not move the envelope                             | **real**            | Gate fails on >0.0001 mm drift; measured 0.000000 mm                                        |
| View cube with 26 pick regions, drag-to-orbit, tweened transitions | **real**            | `viewGizmo.ts`, `gizmoDrag.ts`, `cameraTween.ts` + their tests                              |
| Decal layer cannot reach an exported STL                           | **real**            | Asserted by the verify gate, not trusted                                                    |

### 3.3 `research/` — the investigation

| Claim                                                     | Label                                     | Evidence                                                                                                                                                                             |
| --------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The shutoff was not commanded                             | **real**                                  | 2026-07-14 recording: controller reports running ~1 min after water stops (SHUTOFF-INVESTIGATION.md:44-56)                                                                           |
| The shutoff writes nothing to the controller log          | **real** — controlled negative            | Log cleared before, repro filmed, empty after (`:69-104`)                                                                                                                            |
| Valve errors are **not** excluded by that negative        | **real** — and a recorded self-correction | Valve codes are transient Saturn flags, not log entries (`:105-127`)                                                                                                                 |
| Tankless minimum-flow cutout is the leading hypothesis    | **claimed** — untested                    | A mechanism that fits every observation. No experiment has been run (`:190-250`)                                                                                                     |
| Community failure reports drove the polling design        | **real**                                  | FIELD-NOTES §1, with sources and line-numbered upstream references                                                                                                                   |
| A trace spanning a real shutoff                           | **does not exist**                        | No telemetry code anywhere in the tree; `research/diagnostics/` holds three files, all from 2026-07-26 or earlier                                                                    |
| Kohler support contacted; ~$2013 quoted for a replacement | **real** — and recorded outside the repo  | `raw/obs/2026-07-27-kohler,digial-interface,tech-support (@04_07_$2013_for replacement).mp4`. This is what discharged step 1 of the plan of record and justified cutting the housing |
| The interface is reconnected and healthy                  | **real** — measured 2026-08-04            | `num_interface = 1`, `ui1_con_string = conn`, valve `conn` fw 0.12, controller fw `0.0.3.89`, 304 keys                                                                               |

**New finding, 2026-08-04 — the degraded `values.cgi` payload can repeat.**
FIELD-NOTES §6 records an intermittent read (~1 in 30-50) where a healthy valve
comes back `valve1_installed: false` / `con_string: 'dis'`, recovering on the next
read. The proxy's guard (`middleware.mjs:57-86`) is built on that non-repetition:
a loss must be reported **twice** before it is believed. During this sweep,
`npm run selftest` hit it, re-read to rule out the flap, and **got the same
answer again** — then six deliberate reads over 18 s were all healthy:

```
selftest read 1   installed=false   300 keys
selftest read 2   installed=false   300 keys    <- the guard's premise fails here
follow-up  1..6   installed=true    304 keys    conn, fw 0.12
```

Two consequences. The guard is **defeated by exactly the event it was written
for** — two consecutive suspects cause the middleware to accept and cache the bad
payload, which is the "no configured outlets for 30 s, possibly with someone
standing in the shower" outcome FIELD-NOTES §6 exists to prevent. And the
degraded payload is **short — 300 keys against 304** — a cheap discriminator the
code does not use. Any telemetry logging `con_string` without payload
completeness will manufacture false valve-dropout events, which is precisely the
signature the investigation is hunting.

### 3.4 `cnc/` — the physical repair

**real, and undocumented.** Twelve files, 2.9 MB: a jig DXF, four `.E12`
projects, five `.gcode` toolpaths, two rear-cover STL revisions. Disk timestamps
run 2026-07-28 → 08-01. There is **no README, no provenance, and no record of
which machine, which cutter, which file was actually run, or which cover
revision is installed.** This is the only part of the tree that has physically
altered a $1500 part.

## 4. Ambition vs. implemented reality — the gaps

Ordered by how much they matter, not by size.

### 4.1 The investigation's own cheapest next step has not been taken — **drift**

`SHUTOFF-INVESTIGATION.md:12-14` states it in bold: _"The cheapest next step is
an experiment, not code:_ run the shower with several outlets open, well above
any minimum firing flow, and see whether it survives materially longer." It is
repeated at `:229-240` and again at `:386-389`. `PROMPT-observability.md:142-147`
asks the same question a third time.

Nine days on, `STORY-LOG.md` records no such run. The experiment is free, needs
no code, and is close to conclusive in either direction. Everything downstream —
whether to instrument the controller at all, whether attention moves to the
tankless unit or to the valve hardware — is waiting on it.

### 4.2 The telemetry capture was specified and never built — **dormant decision**

`research/PROMPT-observability.md` is a complete, decision-laden brief written
2026-07-26: JSONL, piggyback on existing polling, no extra controller load, a
five-step working agreement with an explicit operator gate before any water
runs. It is 187 lines of settled design.

`grep` for `jsonl`, `telemetry` or `trace` across `app/` and `viewer/` returns
nothing outside `node_modules`. The session it primes never happened. The
document's own most valuable line is `:43-45`: sampling
`valve1_ErrorFatal` / `valve1_ErrorResettable` _during_ a shutoff is "the single
highest-value thing this work can do," and those flags are invisible after the
fact.

This gap is honest about itself — the same document argues controller polling may
not be able to see the fault at all — which is exactly why it needs a decision
rather than quiet dormancy.

### 4.3 The physical fabrication week is captured on video and absent from the repo — **drift, but not decay**

`AGENT.md:53-63` lists the append triggers. Two of them fired repeatedly between
2026-07-28 and 2026-08-01:

> - Hardware behaves unexpectedly, or the controller does something new
> - **A physical action is taken on the hardware (cables, housings, breakers)**

`STORY-LOG.md` jumps from `## 2026-07-27` (line 119) straight to `## 2026-08-03`
(line 13). In between: a jig was cut, a $1500 interface housing was machined
open, and two revisions of a replacement rear cover were designed and printed.
The evidence exists — `cnc/` (12 files) and `Images/2026-07-28-*.png` (4
photographs) — but the narrative does not. The cut is mentioned only in passing,
inside the first clause of an entry about sealant (`STORY-LOG.md:45`).

AGENT.md:65-67 states the reason this matters: _"write it as it happens, because
the detail is gone by the next session."_

**Correction to the first draft: the detail is not gone.** The operator has
**45 raw clips** at `E:\proj-med\build-661-diag-kohler-shower\raw`, spanning
2026-06-25 to 07-29 — the original removal that pulled the connector
(`mob/2026-06-25-...[!fail-disconnected-cable].MP4`), the Kohler support call, the
whole 07-28 fabrication day (17 `obs` + 15 `mob` clips), and the rear access
cover being installed (`obs/2026-07-29 22-40-21_installing_rear_access_cover.mp4`).
The plan of record is to process them through `e:\git\inferiere` — rename with
meaningful titles, transcribe, index, summarise, and package for ZoomTube
multi-dimensional playback.

So this is **not a race against fading recall**, and the first draft was wrong to
frame it that way. It is a pipeline that has not been run, sitting between a
complete primary source and an incomplete repo. What that changes:

- The urgency drops sharply. Nothing is being lost.
- The **shape** changes: this is not "write the missing entries from memory", it
  is "run the processing pipeline, then let the story log be derived from what it
  produces." Hand-writing entries now would duplicate work the pipeline does
  better and would present recall as observation.
- The **dependency becomes visible**: the repo's narrative gap is blocked on
  another repository's tooling being pointed at this footage. That is a real
  cross-project dependency and it is recorded nowhere.

What remains true regardless: the story log currently jumps 07-27 → 08-03, and a
reader of this repo alone cannot tell that a $1500 part was machined open in
between.

### 4.4 The plan of record was discharged, and the repo does not know it — **doc rot, not a dormant decision**

`SHUTOFF-INVESTIGATION.md:367-373`:

> 1. Contact Kohler technical support for a recommended reconnection method.
> 2. **Only if there is no supported route:** 3D scan the interface, generate a
>    CNC toolpath, and cut a surgical access opening over the connector.

**Both steps happened, in order.** Kohler technical support was contacted on
2026-07-27 and quoted **~$2013 for a replacement** — recorded in the filename of
`raw/obs/2026-07-27-kohler,digial-interface,tech-support (@04_07_$2013_for
replacement).mp4`. That answer is precisely what discharged the condition: there
was no supported reconnection route, only a $2013 part, so the cut proceeded the
next day. The process was followed correctly.

**The repository records none of it.** `AGENT.md:57` names "Kohler support is
contacted, and what they said" as a story-log trigger; it has never fired, so the
document still presents the cut as conditional on a step whose outcome exists
only in a video filename on another drive.

This is the strongest single example of the pattern in §0: the project did the
careful thing and did not write it down. The $2013 quote is also the entire
economic justification for the repair — it is the number that makes cutting a
$1500 part rational rather than reckless, and it is the number a viewer will most
want. Losing it to an unprocessed filename is the expensive part.

Separately, seven entries across STORY-LOG carry a **For Kohler** line, which
`AGENT.md:76-77` says "get collated when we contact them." Contact has now
happened once, on a different subject, and no collation exists — so those
questions about CAD, logging and HTTP/0.9 are still undelivered.

### 4.5 Two safety-adjacent code paths are weaker than the docs imply — **drift**

Both are described in §5 of the work ledger with evidence; stated here because
they are gaps between claim and code, not merely bugs:

- **`DISCLAIMER.md:101-103` claims "This app clamps to that limit."** It does —
  to `max_temp` when `values.cgi` is readable. When it is not,
  `model.ts:136,152-153` silently falls back to **Fahrenheit and 113**, because
  `num(undefined)` is `0` and `units === 0` means °F. On a Celsius-configured
  DTV+ with a degraded `values.cgi`, the UI's own ceiling becomes 113 °C. This
  system is °F, so it cannot demonstrate the bug — the same shape as the two
  bugs FIELD-NOTES §2–3 is proud of having caught for other people.
- **`useShower.ts:175-185` sends a valve command from inside a `setState`
  updater**, and `main.tsx:7` mounts the app in `<StrictMode>`. React's
  documented contract double-invokes updater functions in development, so
  toggling an outlet _while water is running_ issues `quick_shower.cgi` twice in
  the documented run mode (`npm run dev`). Rapid successive valve commands are
  precisely what FIELD-NOTES §1 records as having taken a controller offline.

### 4.6 Spec-compliant deferrals — the gaps that are _not_ drift

DESIGN.md:202-216 lists these itself, plainly, which is the correct handling:
steam/lighting/rain unproven, presets read-only, massage speed UI-only, second
valve unexercised, no PWA manifest. **None of these is a finding.** A planner
should read them as scope the project has consciously declined, not as rot.

**One item on that list has graduated and not been struck:** "Unverified against
running water. …no valve has been opened by this app yet" (DESIGN.md:203-206) is
no longer true. A full shower has been run through the app. That line is the
project's most conservative self-assessment and it now understates what has been
proven — worth correcting precisely _because_ the honest-limitations section is
what makes the rest of the document trustworthy.

### 4.7 Counted-artifact rot — **doc rot, trivial**

`DESIGN.md:166` says 49 unit tests; there are 56. `viewer/README.md:87` says 88;
there are 164. `PROTOCOL.md:45-53`'s Read table omits `cerror_logs.cgi` and
`kerror_logs.cgi`, which are exposed reads (`cgi-safety.mjs:40-45`) and the
investigation's primary evidence — while `FLOW.md:161-163` records them
correctly. PROTOCOL.md is named in AGENT.md's required-reading table for anything
touching the controller API.

## 5. Age and activity

The repo has **two histories**. Eight commits from 2017-12-04 (Tim Elery, a
README and a Jekyll config), then a 8½-year silence, then 54 commits in nine
days beginning 2026-07-26. The 2017 layer is inert: `_config.yml` was deleted on
2026-07-26 and nothing else survives from it but README lineage. **There is no
old code in this repository.**

| Period                  | What moved                                                                                         |
| ----------------------- | -------------------------------------------------------------------------------------------------- |
| 2026-07-26 (34 commits) | Everything: app, safety gate, protocol docs, the whole investigation, the licence, the fork detach |
| 2026-07-27 (3)          | Parts viewer ships                                                                                 |
| 2026-07-28 → 08-01      | **Physical fabrication. Two commits, both binary-only (`cnc/`, `Images/`), no narrative**          |
| 2026-08-03 (10)         | Mesh repair, decals, view gizmo, cold-start UI fix, gasket write-up                                |
| 2026-08-04 (5)          | View-cube drag tuning — the only genuinely in-flight workstream                                    |

**In flight:** the viewer's view cube (5 commits in 2 days, `git status` clean) —
a Maker Galaxy prototype, with the gizmo already heading for that platform.
**Paused:** the shutoff investigation — last substantive touch 2026-07-26, and
its own next step is an operator action, not code.
**Never started:** telemetry (`PROMPT-observability.md`, revised 2026-08-04 by
this sweep to reflect the reconnected interface and the repeating degraded
payload; still unbuilt).
**Complete but unrecorded:** the physical repair. The housing was cut, the TPU
cover printed and installed, and the interface reconnected — and the repo's
narrative stops before all of it. Forty-five clips are waiting on `inferiere`.

**The oldest load-bearing thing nobody has revisited** is
`research/FIELD-NOTES.md` — single commit, 2026-07-26, 331 lines. It is the file
that dictates the polling constants, the outlet index mapping, the `PurgeActive`
handling and the blip filter; AGENT.md names it as required reading before
changing polling, command, or state-handling behaviour. Nine days is not old, but
it is the file with the widest blast radius and the least attention since
creation, and its §1 conclusions rest entirely on other people's reports.

**Assumptions that could expire, none yet:** controller firmware `0.0.3.89` and
whether Kohler has published an update (SOURCES.md:74-76); the upstream
repositories tracked in SOURCES.md, swept exactly once, on 2026-07-26; three
bodies of vendored third-party material that **state no licence at all**
(LICENSE.md:26-33) in a repo about to be pointed at by a video; and the licence
itself, which is applied to this public repo carrying the header **"Status:
Draft — Community Feedback Welcome"** (LICENSE.md:46).

## 6. What is next — the project's own answer, tested

The documents give three answers, and they do not agree on order.

1. **`SHUTOFF-INVESTIGATION.md:12-14` — run the high-flow experiment.** Free,
   needs no code, close to conclusive. **Still credible; it is the correct next
   step, and it is operator work.**
2. **`PROMPT-observability.md` — build the JSONL telemetry capture.** Credible
   _as a second step_, and the document itself says so (`:142-147`). Building it
   before the experiment risks "a beautifully engineered logger that cannot see
   the fault" — the brief's own phrase.
3. **`SHUTOFF-INVESTIGATION.md:367-373` — contact Kohler, then cut if
   necessary.** **Done, both steps, and the document does not know it.** Rewrite
   it in the past tense with the $2013 quote as the recorded reason.

A fourth step exists that no document names, and it may be the best one:

4. **Exploit the natural experiment the repair just created.** The shutoffs
   predate the interface's removal by ~2 months; the interface has now been
   absent and present across the fault's lifetime. _Does the shutoff still
   happen now?_ Nobody has asked. It costs a shower, it needs no code, and it
   discriminates a whole class of hypotheses that the disconnection made
   untestable.

The viewer's next step is not stated in this repo, but it is not undirected — it
is a Maker Galaxy prototype and the gizmo is already graduating (§3.2). What a
planner should know is that its roadmap lives in another repository, so reading
this one gives no visibility into when it is done.

**So: the next step is operator work, not dev work** — one high-flow shower, and
one shower run to see whether the fault survived the repair, ideally the same
session. The highest-value thing an agent can do here is not add code; it is run
the footage pipeline and rewrite the four documents that are behind reality.

## 7. Cross-project relationships

This repo is unusually explicit about its siblings, and all three references
resolve:

| Sibling                                  | Relationship                                                                                                                                                                                                                                                                                                                                                                                                                               | Divergence a planner must not paper over                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`e:\git\mg-controller`**               | DESIGN.md:178-200 models the Android/Capacitor port on `apps/android-cap` (**verified to exist**)                                                                                                                                                                                                                                                                                                                                          | The DTV+ port cannot follow mg-controller's pattern directly: HTTP/0.9 means a Node proxy stays in the picture on every platform. `CapacitorHttp` does not rescue it. Copying the config without that constraint produces an app that silently cannot talk to the hardware.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| **Maker Galaxy** (`mg-web`, `mg-api`, …) | **`viewer/` is a Maker Galaxy experiment, not merely an aligned one.** It exists to learn what a viewer needs when a vendor product and a repair add-on are rendered together — a project shape Maker Galaxy expects — and the view gizmo is already on a path into that platform. Mechanically: the catalog's `files[]` fields and `cameraFit.ts` are a deliberate port of `viewerHelpers.js`; decal records follow Studio's markup model | **`sourceUnit` and `sourceUpAxis` are additions Maker Galaxy does not have.** Its viewer assumes STL/3MF are already in millimetres — true for maker-authored models, false for manufacturer CAD. A part moving _into_ that catalog loses the declaration this viewer refuses to load without: the same class of error the viewer exists to prevent, running in the opposite direction. **This is now a graduation risk, not a hypothetical** — whatever crosses over must carry the declaration with it or the guarantee is lost at the border. Second divergence: **this repo has no visibility into which experiments graduate.** The gizmo's path into Maker Galaxy is recorded in neither repo. |
| **`e:\git\inferiere`**                   | The 45 raw clips at `E:\proj-med\build-661-diag-kohler-shower\raw` are queued for processing there — rename, transcribe, index, summarise, package for ZoomTube playback                                                                                                                                                                                                                                                                   | **This repo's narrative gap (§4.3) is blocked on another repo's tooling.** Nothing in either repository records the dependency, so a planner looking at Kohler-DTV-Plus sees an unwritten story log and no reason it is unwritten, while a planner looking at `inferiere` sees no consumer waiting.                                                                                                                                                                                                                                                                                                                                                                                                  |
| **`e:\git\OpenMakerLicense`**            | LICENSE.md is vendored verbatim from commit `543803f`, with a re-sync instruction rather than a local edit                                                                                                                                                                                                                                                                                                                                 | The vendored text is a **draft**. If the canonical licence changes, this repo's terms silently diverge from the canonical ones until someone re-syncs. Nothing checks this.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |

Beyond the portfolio: this project deliberately positions itself against the
**upstream DTV+ community** (`SOURCES.md` Tier 1) rather than duplicating it —
FIELD-NOTES exists to import other people's failures. Two bugs were fixed here
from reports of hardware this system does not have. That relationship is an
asset and has been swept exactly once.

## 8. Reading list + authority chains

> **Path note, added after this snapshot was taken.** `research/SHUTOFF-INVESTIGATION.md`
> was folded into a new top-level [INVESTIGATIONS.md](../../INVESTIGATIONS.md) as
> investigation **I1**, and the old file was deleted. Citations to it below are
> left as they were — this is a dated snapshot and its references were accurate
> on 2026-08-04 — but read them against I1.

For a fresh agent, in order:

1. `DISCLAIMER.md` — always, first. Safety policy and the risk scale.
2. `AGENT.md` — the working contract and the story-log convention.
3. `research/SHUTOFF-INVESTIGATION.md` — why the project exists beyond the app.
4. `research/FIELD-NOTES.md` — before changing polling, commands, or state.
5. `DESIGN.md` then `FLOW.md` — architecture, then the request path end to end.
6. `PROTOCOL.md` — the wire format.
7. `STORY-LOG.md` — narrative and reversals.
8. `viewer/README.md` — only if touching the viewer; it shares nothing with `app/`.

### Who wins when two disagree

| Area                                    | Authority                                                                                | Beaten by                                                                                                                                                                                       |
| --------------------------------------- | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **What is reachable on the controller** | `app/server/cgi-safety.mjs` — it is the running table and it self-throws                 | `GET /api/safety` beats even the file when the two could differ; PROTOCOL.md, DISCLAIMER.md and both READMEs are downstream prose and **are currently behind it** (§4.7)                        |
| **Wire format and parameters**          | `PROTOCOL.md`, then `research/controller-mirror/js/` — the controller's own shipped code | Where xagon0's docs and the controller's own JS conflict (massage `1`), **the controller wins**; PROTOCOL.md:116-119 records why                                                                |
| **Timings and polling**                 | `useShower.ts` / `middleware.mjs` constants                                              | `FIELD-NOTES.md` §1 is the _justification_ and must be updated with any change; `FLOW.md`'s timings table is a mirror and loses                                                                 |
| **What the controller does**            | Measurement against the hardware, then a cited community report                          | Any uncited claim. AGENT.md:41-44 requires inference be marked as inference                                                                                                                     |
| **Investigation state**                 | `research/SHUTOFF-INVESTIGATION.md` for the current ranking                              | `STORY-LOG.md` for _when and why_ a hypothesis died. Where they conflict, the investigation doc is the summary and the log is the evidence — **but the log is currently missing a week** (§4.3) |
| **Units, orientation, geometry**        | `viewer/`'s catalog declarations + `npm run verify` against the real CAD                 | The K-99694 bracket drawing beats the K-99693 spec sheet on orientation (STORY-LOG.md:19-27); a measurement beats a published figure                                                            |
| **Licensing**                           | `LICENSE.md`'s scope table for what is and is not covered                                | The canonical OpenMakerLicense repo beats the vendored copy; nothing beats "the upstream states no licence"                                                                                     |
