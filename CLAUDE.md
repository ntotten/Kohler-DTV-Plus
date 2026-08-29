# CLAUDE.md

@AGENT.md

## Quick orientation

This repository drives a **Kohler DTV+ shower** whose K-99693 wall interface is
disconnected. It controls real water at real temperatures.

Before anything else, read [DISCLAIMER.md](DISCLAIMER.md). The short version:

- **Never issue a CGI endpoint rated above 2/5.** The gate in
  `app/server/cgi-safety.mjs` enforces it — do not weaken it.
- **Never open a valve without asking first**, every time. Read-only work is
  free; moving water is not.
- **Keep polling at 15 s idle / 5 s active.** Faster locks the controller up for
  hours. See `research/FIELD-NOTES.md` §1.
- Water above **43 °C / 109 °F scalds**. The controller's configured maximum
  (113 °F here) is above that, so it is not a safe bound.

Append significant events to [STORY-LOG.md](STORY-LOG.md) — this work is being
documented for YouTube ([@azab2c](https://www.youtube.com/@azab2c)) and possibly
for Kohler technical support. See the Story log section of
[AGENT.md](AGENT.md).

## Commands

```bash
npm install && npm run format                     # repo root: format everything
npm run format:check                              # ...or just check

cd app
npm run dev                                       # http://localhost:5180
npm run check                                     # typecheck + unit tests + build
npm run selftest -- --api http://127.0.0.1:5180   # live, strictly read-only
```
