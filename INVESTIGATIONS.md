# Investigations

Open questions this project is actively trying to answer, and the experiments
queued to answer them. One entry per investigation, newest first.

This is the **live state** of each investigation — what we currently believe and
what to try next. It is not a history: how a hypothesis died goes in
[STORY-LOG.md](STORY-LOG.md), and raw captures go in
[research/diagnostics/](research/diagnostics/).

---

## The pattern

*This section is the transferable part. A project that starts because something
broke and would not stop breaking needs exactly four documents, and this is the
one that is usually missing.*

### Three homes, and only three

| Home | Holds | Tense |
| --- | --- | --- |
| **INVESTIGATIONS.md** (this file) | What we believe now, and what to try next | present / future |
| [STORY-LOG.md](STORY-LOG.md) | What happened, what it changed, what we got wrong | past |
| `research/diagnostics/` | Raw captures, dated in the filename | evidence |

Everything else is a spec, and specs multiply. Resist a new document per
investigation: an investigation that outgrows a section here is usually two
investigations, or is finished and belongs in the story log.

### The shape of an entry

Each investigation carries these, in this order. **The order is the point.**

1. **Status line** — open / blocked / resolved / abandoned, and the leading
   hypothesis in one clause.
2. **Symptom** — what is actually observed, in the operator's words where
   possible. Not a theory.
3. **Next to try** — the queue. *This comes before the analysis on purpose.*
4. **What we know** — evidence, each item traceable to a capture, a measurement,
   or a source.
5. **Ruled out** — and by what. A hypothesis killed is progress; record the
   killer, not just the corpse.
6. **Hypotheses, ranked** — with what would confirm or kill each.
7. **Open questions** — the unknowns that are not yet experiments.

**Why "Next to try" sits third.** Every session that opens this repo can write
code; almost none can run a shower. Actions needing hardware, water, or a human
are the ones that get skipped, and burying them under a hypothesis ranking is how
they stayed skipped for nine days while four documents named one of them as the
cheapest next step. Put the runnable things where a tired person sees them first.

### Writing an experiment

```
- [ ] **E<n> · <Short name>.**
  - **Discriminates:** which hypotheses this separates.
  - **Method:** what to actually do.
  - **A positive result means / a negative result means:** both, before running it.
  - **⚠️ Consent:** present only if it moves water, mains, or the hardware.
```

Predicting both outcomes before running is not ceremony. An experiment whose
negative result you cannot describe is not an experiment — it is a hope.

### Rules

- **Anything that moves water, touches mains, or alters hardware needs explicit
  in-the-moment operator consent, every time.** Read-only work is free. See
  [DISCLAIMER.md](DISCLAIMER.md).
- **Strike a finished experiment, don't delete it** — a struck line with its
  result is how the next reader knows it was tried.
- **Result to the story log, verdict to here.** The narrative of what happened
  belongs in STORY-LOG.md; this file records only what it changed.
- **Mark inference as inference.** Per [AGENT.md](AGENT.md), the value of this
  work is that it is grounded. Say when something is unverified.

---

## Index

| # | Investigation | Status | Leading hypothesis | Next action |
| --- | --- | --- | --- | --- |
| [I1](#i1--the-shower-stops-mid-use) | The shower stops mid-use | **open** | Tankless minimum-flow cutout → valve cannot reach setpoint → valve shuts off | E1 · high-flow shower |
| [I2](#i2--the-k-99693-interface-was-disconnected) | The K-99693 interface was disconnected | **resolved** 2026-07-29 | Sealed housing pulled the internal connector out | — |
| [I3](#i3--valuescgi-intermittently-drops-a-healthy-valve) | `values.cgi` intermittently drops a healthy valve | **open** | Truncated response, not a real dropout | E7 · measure the rate |
| [I4](#i4--is-automatic-purge-on) | Is automatic purge on? | **open** | Two of our own documents disagree; the live controller settles it | E12 · re-read the setting |
| [I5](#i5--the-saturn-register-map-is-contradictory) | The Saturn register map is contradictory | **open** | The vendored sources describe different register numbering | E13 · resolve from capture |

---

## I1 — The shower stops mid-use

**Status: open.** Leading hypothesis: **tankless heater minimum-flow cutout →
valve cannot reach setpoint → valve shuts off.** The hypothesis has changed twice,
both on 2026-07-26.

| Revision | Leading hypothesis | What changed it |
| --- | --- | --- |
| Initial | Failing K-99693 sending spurious stop commands | — |
| 2nd | Valve power loss or RS-485 comms loss | Video: the controller still reports "running" for ~1 min after the water stops, so nothing commanded it |
| **Current** | **Tankless minimum-flow cutout** | Hot source is tankless; and valve errors never reach the controller's on-board log, so the empty log does not exclude them |

### Symptom

For roughly two months the shower stopped of its own accord partway through use.
Observed durations before shutoff: "a couple of minutes", "about 4 minutes", and
~3.5 minutes in the recorded session. No pattern identified.

**Not yet re-tested since the interface was reconnected on 2026-07-29.** Every
observation below predates the repair.

### Next to try

- [ ] **E1 · High-flow shower.** *The cheapest next step, and it needs no code.*
  - **Discriminates:** H0 (tankless minimum flow) from everything else.
  - **Method:** run with several outlets open — overhead plus body sprays — well
    above any plausible minimum firing flow. Time it. Compare against the
    handshower-alone case, which failed at ~3.5 minutes on 2026-07-14.
  - **Survives materially longer:** H0 is close to confirmed, and the answer is
    probably outside the DTV+ entirely — go to the tankless unit next.
  - **Fails just as readily:** H0 is dead. Attention moves to the valve hardware
    itself (H4) and to the heater's own fault history.
  - **⚠️ Consent:** moves water at up to 113 °F. Operator present.

- [ ] **E2 · Ordinary shower, now the interface is back.**
  - **Discriminates:** anything involving the interface from everything else —
    and it is free, because the fault has now existed both with the interface
    absent and with it present.
  - **Method:** one normal shower, handshower-only, matching the 2026-07-14
    conditions as closely as possible. Time to shutoff, or note that it did not.
  - **Still stops:** the interface is conclusively not involved — which is what
    we already believe, but it would be believed on evidence rather than on the
    absence of a mechanism.
  - **Does not stop, repeatedly:** unexpected and important. Something about the
    reconnection changed the system, and the whole ranking needs revisiting.
  - **⚠️ Consent:** moves water. Operator present.
  - *Note: run E1 and E2 in the same session if convenient, but not the same
    shower — they need different outlet configurations.*

- [ ] **E3 · Identify the tankless unit.**
  - **Discriminates:** nothing directly — it supplies the number H0 rests on.
  - **Method:** make, model, and rated minimum activation flow from the unit's
    own plate or manual. Then whether it keeps a fault log, and if so read it.
  - **Why it may beat everything above:** if the unit logs flame-loss or
    minimum-flow faults with timestamps, and one lines up with a shutoff, that is
    the whole answer without touching the DTV+.
  - Read-only. No consent needed beyond access to the unit.

- [ ] **E4 · Duration census.**
  - **Discriminates:** timer-like causes from fault-like ones.
  - **Method:** record wall-clock start → shutoff for every event from now on.
    The operator's instinct on 2026-07-14 was right: "I'll figure out exactly the
    exact time."
  - **Clusters tightly:** points at a timer or a watchdog.
  - **Scatters:** points at a fault or a threshold being crossed.
  - Costs nothing beyond noting two times per shower.

- [ ] **E5 · Outlet water temperature over time.**
  - **Discriminates:** H0 and H1 from H2/H3 — a temperature drop *preceding* the
    stop is H0's signature and is invisible to the controller.
  - **Method:** any logging thermometer at an outlet. This is deliberately
    outside the DTV+, because the chain H0 describes is mostly invisible to
    controller polling.
  - **Confirmed necessary, 2026-08-04.** There is **no measured water temperature
    anywhere in the CGI API** — checked, not assumed. `valve1_temp_string` and
    `valve1Setpoint` both carry the setpoint; none of `values.cgi`'s 304 keys or
    `system_info.cgi`'s 39 carries a thermistor reading. The controller *has* the
    number — it reads it from the valve over Saturn and pushes
    `DT_W_Temperature` to the touchscreen — but no endpoint surfaces it. xagon0's
    documented `values.cgi?type=word` datatable form, which would have given
    "word index 0 = valve 1 current temp", **does not exist on firmware
    0.0.3.89**: `?type=byte`, `?type=word`, `?type=string` and `?page=control`
    all return the identical 304-key object as the bare call, and Kohler's own
    `control.js` never passes them either. So this experiment cannot be replaced
    by a better poller.
  - *Open: whether `valve1_temp_string` starts tracking actual temperature while
    water is running. Only ever sampled idle, where setpoint and actual coincide.
    Worth one glance during the first captured shower before buying a sensor.*
  - **⚠️ Consent:** requires a shower to be running.

- [ ] **E10 · Measure the handshower's flow rate with a bucket.**
  - **Discriminates:** H0, before any shutoff has to happen — and it is cheaper
    than E1, which is why it goes above it in practice.
  - **Method:** run the handshower alone into a bucket of known volume, time it,
    convert to GPM. Compare against the tankless unit's rated minimum activation
    flow from E3.
  - **Handshower flow is comfortably above the minimum:** H0's premise fails and
    the hypothesis is dead without ever reproducing the fault.
  - **It is at or below the minimum:** H0 goes from plausible to near-confirmed,
    and E1 becomes a formality.
  - **⚠️ Consent:** moves water, briefly, at whatever temperature is set.

- [ ] **E6 · Telemetry capture spanning a shutoff.**
  - **Discriminates:** H2 and H3 cleanly; H0 and H4 only if
    `valve1_ErrorResettable` / `ErrorFatal` are sampled *during* the event.
  - **Method:** the brief in
    [research/PROMPT-observability.md](research/PROMPT-observability.md), already
    revised for the reconnected interface.
  - **Sequence after E1.** The brief itself warns against building "a beautifully
    engineered logger that cannot see the fault", and which hypothesis survives
    E1 decides whether controller telemetry can see anything at all.
  - **Shows nothing at all:** that is a real result. It points outside the
    controller and promotes H1.

- [ ] **E7 · Recirculation pump correlation.**
  - **Discriminates:** a supply-side pressure cause (part of H1).
  - **Method:** establish whether there is one, and whether its cycling
    correlates with shutoffs.

### What we know

**The shower was not commanded off.** From the operator's 2026-07-14 recording
(transcript at `E:\proj-med\build-661-diag-kohler-shower\2026-07-14-DTV-shower-unexpectedly-stops.txt`,
not copied into this repo):

| Time | Observation |
| --- | --- |
| 00:57 | "Shower disappeared. No, it says, thinks it's running. Sort of problem with the valve." |
| 03:10 | Overhead turned off, **handshower alone left running** — deliberately reducing flow |
| 04:58-05:23 | Target raised to 97 °F — then "**Target's gone down to 96**", reverting on its own |
| ~06:40 | "**it just kind of seized up and stopped**" — water stops, nothing was touched |
| 06:46 | "**if I go over to the shower, it says that it's still running.**" |
| 07:11 | "…at some point it figures out, oh, either the poor valve has lost power or shut down" |
| 08:21 | Controller returns to the clock screen, now correctly showing not-running |

A stop command — from the interface, the app, or anything else — sets the
controller's state to off *immediately*. That is not what happens. **The water
stops first and the controller finds out about a minute later.**

Two further clues from the same session: the setpoint reverted 97 → 96 on its
own, and 96 is `def_temp` — something reloaded defaults, which is a reset
signature. And the ~1 minute detection delay is a timeout, which bounds the
detection path we can instrument.

**The shutoff writes nothing to the controller's log — a controlled negative.**
The operator cleared the error log **before** filming, reproduced the shutoff on
camera, and captured the log **after**:

```
Controller (S): v0.0.3.89
...
No errors are logged from Controller
```

Captures: [2026-07-14-controller_Error-after-repro.log](research/diagnostics/2026-07-14-controller_Error-after-repro.log),
[2026-07-14-konnect_Error-after-repro.log](research/diagnostics/2026-07-14-konnect_Error-after-repro.log).

Cleared before, reproduced during, still empty after. **And the mechanism
demonstrably works** — the interface disconnection on 07-25 was logged as code
100 within seconds. So the controller is perfectly capable of noticing a device
dropping off the bus. It did not do so here.

**⚠️ What that negative does NOT rule out — and an earlier revision of this
document got this wrong.** Valve faults (`OVERTEMP_*`, `ALG_*`, `RELAY_FAULT`,
codes 2-39) were listed as excluded. That was incorrect, and the distinction
matters:

- **Valve error codes** are *"reported by the mixing valve hardware over the
  Saturn serial protocol"* — they surface as `valveN_ErrorFatal` /
  `valveN_ErrorResettable`, which are **current-state flags, not history**.
- **The on-board log** holds *"Codes 100-204… logged by the DTV+ controller
  itself"*.

Both from [xagon0 error-codes.md](research/xagon0/docs/troubleshooting/error-codes.md).
A transient valve error — trips, then recovers — leaves **no trace at all** once
the flag clears. Reading `ErrorResettable` the next day, as we did, tells us
nothing about what happened during the shutoff. **Sampling those flags during a
shutoff is the single highest-value thing telemetry can do.**

**Interface firmware, preserved by the captured log header** — versions we could
not read while it reported `not_seen`:

```
User Interface1 (S):  OS v0.0.7.44 · Graphics v0.0.1.7
                      Language v0.1.1.0 · Touch Panel v0.0.0.2
Kohler Konnect:       OS v0.0.1.77 · Graphics v0.0.1.9
Valve 1:              FW v0.12
Controller (S):       v0.0.3.89
```

**The log today** holds one entry — `cerror_logs.cgi`, read 2026-07-26 (raw:
[error-log-2026-07-26.txt](research/diagnostics/error-log-2026-07-26.txt)):

```
[10:32.42 p.m. 07/25/2026] 100:  UI Error
```

Code **100 = `DETACH_EVENT`**, source UI — the interface disconnection. This is a
99-entry circular buffer in flash that survives power cycles. Consistent with the
controlled capture: the only thing ever logged is the UI detach.
`valve1_ErrorFatal` and `valve1_ErrorResettable` are both `0`.

### Ruled out

| Cause | Why not |
| --- | --- |
| Configurable runtime limit | `max_valve1_runtime_enable = 0`, `max_valve1_runtime = 0` |
| Prompt 3 valve watchdog | 1800 s, not 2-4 min — and this is a six-port valve, not a Prompt 3 |
| Commanded stop, any source | Controller still reports running after water stops |
| Failing interface sending spurious stops | Same evidence. Struck 2026-07-26 |
| Interface corrosion | Contacts inspected: clean copper/gold, vapor-tight, recessed away from water |
| Hot supply *exhaustion* | Source is tankless — no reservoir to run down |

### Hypotheses, ranked

> Re-ranked 2026-07-26 after the controlled log capture. Anything that would
> write to the error log is heavily penalised: a reproduced shutoff wrote
> nothing, while a real detach wrote immediately.

**H0 · Tankless minimum-flow cutout → valve cannot reach setpoint → valve shuts
off — leading.**

The hot water source is tankless, which supplies the mechanism that was missing:

1. Tankless heaters have a **minimum activation flow**, typically 0.5-0.75 GPM.
   Below it the burner will not fire or drops out, and with no reservoir the hot
   supply goes cold within seconds.
2. Hot supply becomes unavailable to the mixing valve.
3. **The valve cannot reach its 96 °F setpoint.** All proportional mixing logic
   runs inside the valve's own firmware
   ([temperature-system.md](research/xagon0/docs/control-logic/temperature-system.md));
   the controller only sends a setpoint and reads back actual temperature.
4. **The valve shuts off rather than deliver water at the wrong temperature** —
   the operator's own expectation, stated on camera at 04:14: *"I know it's
   supposed to cut off if they can't achieve the desired temperature."* Candidate
   codes: `ALG_COLD_TIMEOUT` (38) — *"hot supply may be unavailable"* — or
   `ALG_HOT_TIMEOUT` (39).
5. **Nothing reaches the controller's log,** because valve errors travel Saturn as
   transient flags rather than log codes 100-204.

**It explains the detail that previously did not fit.** At 03:10 the operator
turned off the overhead and left **only the handshower** running, to save water.
The shutoff followed ~3.5 minutes later. Under every other hypothesis that is
neutral or protective; under this one it is **the trigger**. The 97 → 96
reversion fits too — the target was raised while the system was already
struggling, and fell back to `def_temp`.

**Confirm or kill:** E1. **Instrument:** `valve1_ErrorResettable` /
`ErrorFatal` at 5 s through a shutoff; outlet temperature falling before the stop.

**H1 · Something mechanical or hydraulic; the electronics never know.**

The combination to explain is: water stops instantly · nothing commanded · the
controller believes it is still running · **nothing logged** · no fault flags ·
no status LEDs. An electrical or bus failure struggles with the last three. A
mechanical or hydraulic cutoff explains all of them trivially, because nothing in
that path reports to firmware:

- Thermal / anti-scald mechanical cutoff — many thermostatic mixing valves close
  flow mechanically if the cold supply fails or outlet temperature exceeds a
  limit. Purely hydraulic, invisible to the controller.
- Supply pressure loss — a pump cycling, a pressure-balancing element, another
  fixture drawing.
- A valve mechanism closing without reporting.

**Why this matters for instrumentation:** if the cause is mechanical, **the
controller's telemetry will never show it.** A trace would only ever record
"running, then timed out" — which we already know. The informative signal is
almost certainly outside the controller.

**Confirm or kill:** E5, E7. Retained as the fallback if E1 kills H0.

**H2 · Valve loses power or resets — demoted.**

Previously leading. Fits the observable behaviour, including the `def_temp`
reversion as a reset signature. **What demotes it:** a valve losing power should
drop off the RS-485 bus, and the controller demonstrably logs that as code 100
within seconds. The controlled capture shows no such entry. Not impossible — a
reset fast enough to recover before the detach timeout might slip through — but
it now has to explain the silence rather than being supported by it.

**Confirm or kill:** `valve_1_con_string` and `valve1_installed` sampled through
a shower. `conn` → `dis` at the moment water stops puts this straight back on
top. **⚠️ See I3 first** — a short payload can fake exactly this.

**H3 · RS-485 comms loss — close second to H2.**

Mechanically similar from the controller's side and hard to distinguish without
valve-side visibility. xagon0 documents the failure modes: runs over 50 ft,
missing 120 Ω termination, cabling near AC lines
([known-issues.md](research/xagon0/docs/troubleshooting/known-issues.md)).

**Note:** if the valve merely lost comms but kept power it would normally *keep
running* until its own safety timeout — yet the water stops immediately. That
argues for H2 over H3, or for a valve that fails closed on comms loss.

**Confirm or kill:** `conn` → `dis` with **no** setpoint reversion. Probably needs
physical inspection of the valve's wiring, or a scope on the bus.

**H4 · Valve-side fault with a cause other than H0.**

An `OVERTEMP_*`, `ALG_*_TIMEOUT` or `RELAY_FAULT` from a failing thermistor, a
sticking mixing motor, or a relay that intermittently fails to hold. Not a
separate story from H0 so much as the same mechanism with a different upstream
cause. **If E1 shows the shutoff happens regardless of flow, this becomes
leading** and points at the valve hardware itself.

**H5 · Controller crash-and-reboot — effectively excluded.**

Would leave a task exception (130-146) and make the controller unreachable for
30-60 s. The video shows it responsive and displaying throughout.

### What would settle it

A trace spanning a real shutoff, sampled at the app's existing 5 s active rate —
no added controller load:

| Hypothesis | Signature |
| --- | --- |
| H0 · tankless min-flow | `valve1_ErrorResettable` sets transiently (38/39); outlet temperature falls before the stop |
| H1 · mechanical | Nothing anywhere — controller simply times out |
| H2 · valve power loss | `conn` → `dis`, setpoint reverts to `def_temp`, controller still reports running ~1 min |
| H3 · RS-485 | `conn` → `dis` with no setpoint reversion |
| H4 · valve fault | New `cerror_logs.cgi` entry and/or `ErrorFatal`/`ErrorResettable` set |
| H5 · controller reboot | Controller unreachable 30-60 s |
| **Not a fault** | `con_string` → `dis` **with a short payload** — see [I3](#i3--valuescgi-intermittently-drops-a-healthy-valve). Must be excluded before claiming any of the above. |

The ~1 minute detection timeout means 5 s sampling gives roughly 12 samples
across the transition — ample resolution.

### Open questions

- **Does the shutoff still happen at high flow?** → E1
- **Does it still happen now the interface is reconnected?** → E2
- **What is the tankless unit's make, model and minimum activation flow, and does
  it keep a fault log?** → E3
- Does the valve or the install include a **mechanical** anti-scald or
  cold-supply-failure cutoff that closes flow without telling the electronics?
- Is there a recirculation pump, and does its cycling correlate? → E7
- Does the valve have its own power feed that could be independently monitored?
- Does shutoff timing cluster or scatter? → E4
- ~~Was the error log ever cleared?~~ **Answered.** Cleared deliberately before
  the 2026-07-14 repro; still empty afterwards. The negative is real.
- ~~What is the hot water source?~~ **Answered: tankless.**
- ~~Does the shutoff still happen with the interface disconnected?~~ **Moot** —
  the interface has since been reconnected. Superseded by E2.

### Confounders to record

During the 2026-07-14 recording the operator had **a web browser open on the
controller's own web page**. The controller supports only two concurrent HTTP
sessions ([FIELD-NOTES.md](research/FIELD-NOTES.md) §1). Unlikely to explain
shutoffs that predate the browser being connected — the operator notes "web
browser is not normally connected" — but any future trace should record whether
other clients were active.

~~**New since 2026-07-29:** the reconnected wall interface is itself an HTTP
client, polling `system_info.cgi` every 5 s and `values.cgi` every 10 s. It now
consumes part of the two-session budget.~~ **Wrong — corrected 2026-08-04.**

The K-99693 is **not an HTTP client and consumes none of the two-session budget.**
It is a device on the RS-485 bus: discovered over the DTV+ protocol as device ID
`0x30`, then synchronised over Amulet CRC at 115200 baud on a 50 ms tick
([touchscreen-ui.md](research/xagon0/docs/devices/touchscreen-ui.md),
[system-overview.md](research/xagon0/docs/system-overview.md)). Its detach on
07-25 was logged as code 100 with the UI device byte, alongside the valve and
amplifier — bus peers, not web clients. The thing that polls `system_info.cgi`
every 5 s and `values.cgi` every 10 s is the controller's own **web page**,
`control.js`, which is a different client entirely.

**What still needs recording, and matters more:** the interface *can command the
shower* over the bus, as `INVOKE_RPC` frames. Those commands never pass through
our proxy and never appear in the egress log. So an absent `REQ` line means "we
did not send it" — never "nobody sent it".

**Two clients that do count:** any browser tab open on the controller's own web
page, and — found the hard way on 2026-08-04 — **a second copy of our own proxy**.
The serialisation queue is per process, so while `npm run dev` restarts the
middleware after a file change there are briefly two processes talking to a
controller that tolerates two sessions. Both hangs observed that night followed
exactly that, or a deliberate burst.

---

## I2 — The K-99693 interface was disconnected

**Status: resolved 2026-07-29.** Cause understood, repair completed, interface
reporting `conn`.

**Not implicated in [I1](#i1--the-shower-stops-mid-use)** — the shutoffs predate
this by ~2 months — but it is why this project exists.

### Symptom

`num_interface = 0`, `ui1_con_string = not_seen`, while `valve_1`, `amp` and
`controller` all reported `conn`. The shower was fully functional with nothing to
command it.

### Cause

The original installation silicone-sealed the interface housing to the wall, and
sealed the blue seal plug along with it. Removing the interface left the plug
attached to the wall, which pulled the internal wire-to-board connector out of
its socket. Logged by the controller at 22:32 on 2026-07-25 as `DETACH_EVENT`.

The connector was not reachable without opening the housing. Contacts were
inspected while accessible: clean copper and gold, no corrosion — the housing was
vapor-tight and recessed away from direct water. The original intent had only
been to inspect for corrosion.

### Resolution

The plan of record required contacting Kohler first, and cutting only if there
was no supported route. Both steps happened, in order:

1. **Kohler technical support contacted 2026-07-27.** No supported reconnection
   method; a replacement was quoted at **~$2013**. That is what made cutting a
   ~$1500 part the rational option rather than a reckless one.
2. **Access opening machined 2026-07-28**, using a jig cut for the purpose
   ([cnc/](cnc/)). Connector reached and reconnected.
3. **TPU rear cover printed and installed 2026-07-29**, sealed with Permatex
   "The Right Stuff" as a formed-in-place gasket — the reasoning is in
   [STORY-LOG.md](STORY-LOG.md), 2026-08-03 16:18.

Verified 2026-08-04: `num_interface = 1`, `ui1_con_string = conn`.

> **Recorded honestly:** most of this week is captured on video and not yet
> written up. 45 raw clips are queued for processing. The support call's outcome
> currently survives as text in a filename, which is why it is stated here.

### Open questions

- Does the repair hold? The TPU/ABS gasket joint is splash- and
  condensation-resistant by design, but an FDM-printed part in a shower wall is
  not a factory-tested hermetic enclosure. Worth re-checking after some months.
- **For Kohler:** sealing the housing and the blue seal plug together at
  installation makes the interface effectively non-removable without pulling the
  internal connector. Is there a supported method that does not require cutting?

---

## I3 — `values.cgi` intermittently drops a healthy valve

**Status: mechanism identified 2026-08-04, rate not yet measured.** The degraded
payload is **what the controller's web server returns while recovering from a
hang** — short, slow, and preceded by timeouts. It is an HTTP-layer artefact and
says nothing about the valve.

### Symptom

A routine `values.cgi` read returns a connected, healthy valve as absent:

```
valve_1_con_string     = 'dis'          (normally 'conn')
valve1_installed       = False          (normally True)
controller_con_string  = 'conn'         (unaffected)
valve_1_version_string = '0.12'         (still present — the valve's firmware
                                         version survives in the bad payload)
```

Roughly one read in 30-50, from ordinary use rather than controlled measurement.
First seen 2026-07-26. See [FIELD-NOTES.md](research/FIELD-NOTES.md) §6.

### Next to try

- [x] **E8 · Measure the rate and the length.** — *first pass run 2026-08-04,
  read-only. Answered the mechanism; did not yet measure the rate.*
  - **Result: every degraded read observed with timing attached was both short
    and slow.** Three degraded reads across two events, each arriving only after
    the server had stopped answering — two 8 s timeouts elapsed first, and the
    read that finally returned took 27 s. Raw capture:
    [2026-08-04-egress-trace-first-capture.log](research/diagnostics/2026-08-04-egress-trace-first-capture.log)
    lines 62-85.
  - **20 spaced reads (10 s apart, 3.5 min) were all clean** — 304 keys, ~175 ms,
    no variation. The blip does not occur under an unloaded read pattern.
  - **`system_info.cgi` degrades the same way**, 36 keys against 39, in the same
    window. Nobody had noticed because nothing had been counting it.
  - **The key count is not a fixed number.** Short reads have now been seen at
    299 and 300; healthy was 303 before the interface was reconnected and is 304
    after. Any threshold hard-coded from one observation would be wrong within a
    week — compare against the largest payload seen in the session instead.
  - **Still open:** the natural rate. Everything degraded tonight followed load
    we generated, so this is a mechanism finding, not a frequency one. E11.

- [ ] **E9 · Fix the guard.** — *unblocked by E8, deliberately not done in the
  same change as the egress log.*
  - The current guard requires a loss to be reported **twice** before believing
    it, and on 2026-08-04 the degraded payload arrived twice consecutively. It is
    also **disarmed on the first read after every process restart**, because
    `lastGood` starts null and a payload cannot "lose" a valve it never had —
    which is how a 299-key payload reached the 30 s cache and was served from it
    that night, recorded as a `CACHE ... keys=299` line in the capture.
  - **Better rule, now that the mechanism is known:** distrust a valve-loss
    payload when it is *short relative to the session maximum* **or** *slow
    relative to the endpoint's normal ~175 ms*, rather than counting repetitions.
    Both are properties of the single sample, so it works on the first read and
    after a restart.

- [ ] **E11 · Measure the natural rate.**
  - **Discriminates:** whether the degraded payload happens on its own at all, or
    only ever after something has stressed the web server.
  - **Method:** now that every read is logged with key count and duration, just
    let it run. `grep SHORT` over a week of ordinary use.
  - **Never short without a preceding slow read or timeout:** the "one bad read
    in 30-50" figure in [FIELD-NOTES.md](research/FIELD-NOTES.md) §6 needs
    rewriting, and so does its explanation.
  - **Short reads with normal latency appear:** the hang mechanism is only part
    of the story and truncation is real and separate.
  - Read-only, zero added load — it is a side effect of the trace already running.

### What we know

**2026-07-26** — one bad read, four normal reads over the following minute with
no command sent in between. Written off at the time as a partially-populated HTTP
response, since the valve's firmware version survived in the same payload.

**2026-08-04** — **it repeated on consecutive reads**, which the mitigation
assumes cannot happen. `npm run selftest` read `values.cgi`, saw the valve
absent, re-read to rule out the flap, and got the same answer again. Six
deliberate reads immediately afterwards were all healthy:

```
selftest read 1   installed=false  con=dis    300 keys
selftest read 2   installed=false  con=dis    300 keys
follow-up  1..6   installed=true   con=conn   304 keys   fw 0.12, over 18 s
```

**The degraded payload is short — 300 keys against 304.** A truncated response
and a genuine disconnection do not look the same on the wire, and the proxy
currently reads only the content, never the completeness.

**2026-08-04, later that night** — the egress trace caught it twice with timing
attached, and the timing is the whole story:

```
06:05:18.112Z  REQ   8fn5  values.cgi
06:05:26.117Z  ERR   8fn5  values.cgi        timeout after 8000ms attempt=1/3
06:05:58.636Z  ERR   8fn5  values.cgi        timeout after 8000ms attempt=2/3
06:06:10.072Z  RES   8fn7  values.cgi        ok 27434ms q=24249ms keys=299
06:06:10.231Z  RES   8fn8  system_info.cgi   ok 19464ms q=19426ms keys=36
06:06:25.307Z  CACHE ----  values.cgi        hit age=15s keys=299
```

The short payload never arrives on a healthy connection. It arrives when the web
server has stopped answering and is coming back — **it is a recovery signature,
not a truncation in transit and not a dropout.** The last line is this
investigation's stated user-facing failure, recorded happening: the 299-key
payload entered the 30 s cache and was served from it.

**What caused the hang was us**, both times: a deliberate burst of stacked reads,
and editing `server/*.mjs` while `npm run dev` was running, which restarts the
middleware without inheriting the old process's in-flight requests. That does not
weaken the mechanism finding — it strengthens it, because the trigger was
concurrency and the response was reproducible.

**Honest bound:** the mechanism rests on three degraded reads across two events,
both induced. Whether it ever happens unprovoked is E11, and the "one bad read in
30-50" figure has never been measured under controlled conditions.

### Why it matters more than it looks

It fails in two directions:

- **Toward the user.** A client that caches `values.cgi` — which you want to do,
  to keep the request rate down — caches the *bad* payload and then insists the
  shower has no configured outlets for the whole TTL. That is a disabled start
  button for 30 seconds, potentially with someone already standing in the shower.
- **Toward [I1](#i1--the-shower-stops-mid-use).** "The controller has lost the
  valve" is *precisely* the signature that investigation is hunting, and one such
  sample has already been promoted to evidentiary status. Telemetry built on the
  current logic would generate that finding on schedule.

### Open questions

- ~~Does it cluster around load?~~ **Answered: yes, and load appears to be the
  whole mechanism.** Every instance seen with timing attached followed a stall.
  Whether it *ever* happens without one is E11.
- ~~Does it cluster around the wall interface's own polling?~~ **Moot** — the
  interface is not an HTTP client at all. See I1's confounders.
- ~~Is 304 the invariant full length?~~ **No.** 303 before the interface was
  reconnected, 304 after; short reads at 299 and 300. Compare against the
  session maximum, never a constant.
- Does the controller log anything when its web server hangs? **Observed: no.**
  `cerror_logs.cgi` gained no entry across either hang — the log still holds only
  the two UI detach events. This widens what "the error log is empty" is
  compatible with, which matters for [I1](#i1--the-shower-stops-mid-use).
- **For Kohler:** `values.cgi` intermittently returns a short, partially-populated
  response in which a healthy connected valve reads `installed: false` / `dis`
  while its firmware version is still present. It can occur on consecutive
  requests. Any client treating a single such read as a state change will report
  a dropout that did not happen. **Added 2026-08-04:** it is specifically what the
  server returns while recovering from a session-limit hang — the reply parses
  cleanly, carries no error status, and is indistinguishable from a real dropout
  except by counting keys or noticing the response took twenty seconds.
  `system_info.cgi` does the same thing, 36 keys against 39. Nothing is written to
  the controller's error log when this happens.


---

## I4 — Is automatic purge on?

**Status: open.** Two documents in this repository state opposite values;
neither was re-read against the live controller when the other was written.

### Symptom

| Source | States |
| --- | --- |
| [research/FIELD-NOTES.md](research/FIELD-NOTES.md) §3 | `auto_purge = 1`, `auto_purge_enable = 1`; start runs a purge cycle first |
| [docs/system-specification.md](docs/system-specification.md) §2 | "automatic purge disabled" |

### Next to try

- [ ] **E12 · Re-read the setting.**
  - **Discriminates:** the two documents.
  - **Method:** one idle `values.cgi` read. Compare `auto_purge`,
    `auto_purge_enable`, and the per-valve purge fields. Record which document
    was written from a live read. Read-only.
  - **Purge is on:** the replacement controller needs a purge state,
    "confirmed off" means flow stopped rather than valve acknowledged off, and
    Phase 3/4 stop-latency figures must be measured against flow.
  - **Purge is off:** FIELD-NOTES §3 takes a dated correction.

- [ ] **E12b · Observe both edges.** Conditional on E12 returning enabled.
  - **Method:** during capture scenario 3, record whether water moves before
    the valve reports on and whether it continues after stop is acknowledged.
  - **⚠️ Consent:** moves water. Operator present.

### What we know

- `values.cgi` exposes `auto_purge` and `auto_purge_enable`.
- FIELD-NOTES §3 records `valveN_Currentstatus` taking `PurgeActive` as a
  distinct value from `On`, and that this controller reports an empty string
  rather than `Off` when idle.

### Open questions

- Which reading is current: system-specification.md is dated 2026-08-22,
  FIELD-NOTES §3 was compiled 2026-07-26. Provenance is not recorded for either
  value.
- Does purge run at the commanded setpoint or at a purge-specific temperature?

---

## I5 — The Saturn register map is contradictory

**Status: open.** The vendored sources describe different register numbering
for the same protocol. Blocking for the replacement controller's encoder.

### Symptom

| Item | [valve-control.md](docs/devices/valve-control.md) | [saturn-protocol.md](research/xagon0/docs/protocols/saturn-protocol.md) |
| --- | --- | --- |
| Register `0x15` | Firmware info, and calibration read | Read configuration parameters |
| Register `0x10` | Not stated | Read calibration data |
| Prompt 3 master address | `0x10` always | `0x00` with DTV+ hardware |
| Response timing | 320 ms communication timeout | 400 ms response timeout, 320 ms message timeout |

### Next to try

- [ ] **E13 · Resolve from passive capture.**
  - **Discriminates:** the two sources, against this hardware.
  - **Method:** Phase 1 of
    [CONTROLLER-DESIGN.md](docs/replacement-controller/CONTROLLER-DESIGN.md).
    Decode K-99695 boot and idle traffic; record control bytes sent, responses,
    and cadence. Receive-only front end; no transmission.
  - **Matches one source:** that source becomes the encoder reference; the other
    is annotated as describing a different revision.
  - **Matches neither:** the capture is authoritative for firmware `0.12` and
    `0.14`.

### What we know

- Both sources are tier **[C]** — third-party firmware analysis, unverified
  against this unit. See [docs/system-specification.md](docs/system-specification.md).
- `saturn-protocol.md` contains a worked example in which a `0x1E` Prompt 3-Port
  responds to master address `0x00`. **Inference**, not confirmation: one
  capture, hardware unknown.
- The vendored material carries no license;
  see [research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md).

### Open questions

- Does our six-port valve at firmware `0.12` use the same map as the Prompt 3 at
  `0.14`, or do the two firmwares differ?
