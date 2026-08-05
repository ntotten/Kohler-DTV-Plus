# AGENT.md — Kohler DTV+ project contract

Guidance for AI and human agents working in this repository.

## What this project is

A replacement interface for a Kohler DTV+ shower whose K-99693 wall unit is
disconnected, and an open investigation into why the shower stops mid-use.

It is also being documented publicly: the work is heading for a YouTube video on
[@azab2c](https://www.youtube.com/@azab2c), and findings may be shared with
Kohler technical support and their product team. **Assume anything you write here
may be read by Kohler engineers and by a general audience.**

## Read before working

| Document | When |
| --- | --- |
| [DISCLAIMER.md](DISCLAIMER.md) | **Always, first.** Safety policy and the CGI risk scale. |
| [PROTOCOL.md](PROTOCOL.md) | Anything touching the controller API. |
| [DESIGN.md](DESIGN.md) | Anything touching the app's architecture. |
| [research/FIELD-NOTES.md](research/FIELD-NOTES.md) | Before changing polling, command, or state-handling behaviour. Other people's failures are recorded there for a reason. |
| [INVESTIGATIONS.md](INVESTIGATIONS.md) | Any diagnostic or observability work. Open questions and the experiments queued to answer them. **Add findings here; add what happened to the story log.** |
| [research/SOURCES.md](research/SOURCES.md) | Before searching the web from scratch. |

## Hard rules

1. **Never issue a CGI endpoint rated above 2/5.** The gate in
   [app/server/cgi-safety.mjs](app/server/cgi-safety.mjs) enforces this; do not
   weaken it. Widening the exposed surface must come with a recorded reason.
2. **Never open a valve without explicit, in-the-moment operator consent.** This
   controls real water in a real bathroom. Read-only work needs no permission;
   anything that moves water needs asking, every time.
3. **Respect the controller's fragility.** Two concurrent HTTP sessions maximum,
   serialised requests, 15 s idle / 5 s active polling. Raising the rate risks a
   lockup that takes the system out for hours.
4. **Verify claims against the hardware or a source.** This project's value is
   that its findings are grounded. Mark inference as inference and say when
   something is unverified.
5. **Report failures plainly**, including your own. A wrong turn that got
   corrected is more useful to the video and to Kohler than a clean-looking
   narrative.

## Story log

Append to [STORY-LOG.md](STORY-LOG.md) whenever something happens that a viewer,
a Kohler engineer, or future-you would want to know. This is the raw material for
the video and for any support conversation — write it as it happens, because the
detail is gone by the next session.

**Append when:**

- A diagnostic finding lands, especially one that changes a hypothesis
- A hypothesis is killed, and by what evidence
- Hardware behaves unexpectedly, or the controller does something new
- A physical action is taken on the hardware (cables, housings, breakers)
- Kohler support is contacted, and what they said
- A milestone ships, or something is proven to work against real hardware
- We get something wrong and correct it — **especially then**

**Do not append** routine refactors, passing tests, or tidy-ups.

**Format** — newest entries at the top of the log body, under a date heading:

```markdown
## 2026-07-26

### 12:42 — Video review kills the "phantom touch" theory
The 2026-07-14 recording shows the controller still reporting "running" for
about a minute after the water stops. A commanded stop sets state immediately,
so nothing commanded this. The valve stops and the controller finds out later.

**Why it matters:** moves the investigation from the interface to the valve.
**For Kohler:** the controller has no idea the valve has gone until it times out.
```

Timestamps are local time. Include a **Why it matters** line. Add a **For
Kohler** line when the entry is something their support or product team should
see — those get collated when we contact them.

## Conventions

- **Commits:** grouped by theme, never one huge commit. Explain *why*, and record
  what was wrong before when fixing something.
- **Evidence lives in `research/`.** Raw captures under `research/diagnostics/`
  with the date in the filename.
- **Third-party material is vendored with provenance**, never silently copied —
  see [research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md).
- **Personal media** (video, transcripts, photos) stays out of the repo unless
  the operator asks. Reference the path and extract the technical content.
- **Tests:** `npm test` for anything without hardware, `npm run selftest` for
  read-only live checks. Neither may ever open a valve.

## Verification

```bash
cd app
npm run check                                     # typecheck + unit tests + build
npm run selftest -- --api http://127.0.0.1:5180   # live, strictly read-only
```
