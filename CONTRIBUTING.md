# Contributing

Contributions are welcome. Two things make this repository unusual, and both
matter more than the usual style guide.

## 1. This controls real water

The code here drives a shower's valve over an undocumented, unauthenticated API.
Water above **43 °C / 109 °F scalds**, and some controller endpoints can brick
the unit permanently.

**Read [DISCLAIMER.md](DISCLAIMER.md) first.** Then, without exception:

- **Never raise the CGI safety ceiling above 2/5.** The gate in
  [app/server/cgi-safety.mjs](app/server/cgi-safety.mjs) enforces it. Widening
  the exposed surface needs a recorded reason in the pull request.
- **Nothing in a test may open a valve.** `npm test` runs without hardware;
  `npm run selftest` is strictly read-only against a live controller.
- **Do not raise the polling rate.** 15 s idle / 5 s active. Faster has locked
  controllers up for hours — see [research/FIELD-NOTES.md](research/FIELD-NOTES.md).

## 2. Claims have to be grounded

This project's value is that its findings are checked against real hardware or a
cited source. Mark inference as inference, say when something is unverified, and
report failures plainly — including your own. A wrong turn that got corrected is
more useful here than a tidy narrative.

Evidence belongs in `research/`, with raw captures under `research/diagnostics/`
dated in the filename. Third-party material is vendored with provenance, never
silently copied — see [research/xagon0/PROVENANCE.md](research/xagon0/PROVENANCE.md).

[AGENT.md](AGENT.md) is the full contract for working in this repository. It
applies to humans as well as agents.

## How to contribute

1. Open an issue first for anything non-trivial — especially protocol changes,
   new dependencies, or anything touching the safety gate.
2. Branch, and keep commits grouped by theme. Explain *why*, and record what was
   wrong before when fixing something.
3. Run `cd app && npm run check` — typecheck, unit tests, and build.
4. Open the pull request. Say what you verified against real hardware and what
   you did not.
5. Accept the [CLA](CLA.md). Pull requests cannot be merged without it.

Documentation, field reports, and diagnostic captures from other DTV+ owners are
as valuable as code — arguably more so, given the open
[shutoff investigation](INVESTIGATIONS.md#i1--the-shower-stops-mid-use).

## Conduct

Be kind, assume good intent, and argue with the problem rather than the person.
