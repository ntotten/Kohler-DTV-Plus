# app/ — the K-99695 HTTP client

## This app caused I1

The shower stopping mid-use — investigated for a month as a valve or plumbing
fault — was **this code polling the controller until it hung**. Resolved
2026-08-29; see [INVESTIGATIONS.md § I1](../INVESTIGATIONS.md#i1--the-shower-stops-mid-use).

The failure is quiet and it is not safe. The controller's valve handling wedges
while its UI and web server keep answering, so:

- water stops, but `values.cgi` and the touchscreen keep reporting "running"
  for roughly a minute until a timeout catches up;
- **nothing** is written to the on-board error log;
- during recovery `values.cgi` returns a short payload that reports a connected
  valve as `dis` — the cause of [I3](../INVESTIGATIONS.md#i3--valuescgi-intermittently-drops-a-healthy-valve).

A client that hits this controller too hard therefore turns off a running
shower with no error surfaced anywhere. Treat the limits below as a safety
property, not a performance tuning knob.

## Hard limits

| Limit                    | Value                      |
| ------------------------ | -------------------------- |
| Poll rate                | **15 s idle / 5 s active** |
| Concurrent HTTP sessions | **2, absolute**            |
| Requests                 | Serialised; one in flight  |

Sources: [research/FIELD-NOTES.md](../research/FIELD-NOTES.md) §1, and
[STORY-LOG.md](../STORY-LOG.md) 2026-08-04 23:05 and 2026-08-29 13:53.

**The two-session budget is easy to blow without noticing:**

- A browser tab open on the controller's own web page is a client. Its
  `control.js` polls `system_info.cgi` every 5 s and `values.cgi` every 10 s.
- **A second copy of this proxy is a client.** The serialisation queue is
  per-process, so a `npm run dev` file-watch restart can briefly leave two
  processes talking to a controller that tolerates two sessions. Both hangs
  observed on 2026-08-04 followed exactly that.

The K-99693 wall interface is **not** an HTTP client and consumes none of the
budget — it is an RS-485 bus peer. Do not count it.

## The CGI safety gate

`server/cgi-safety.mjs` blocks anything rated above 2/5. Do not weaken it. See
[DISCLAIMER.md](../DISCLAIMER.md) for the scale.

Two known defects in that gate, reported upstream at
[aaronse/Kohler-DTV-Plus#1](https://github.com/aaronse/Kohler-DTV-Plus/issues/1)
and **not** fixed here:

- `powerclean_check.cgi` is rated 3/5 as a trigger. It is a 1 Hz status read —
  `settings.js` `powerclean_check_load()` GETs it and re-schedules itself.
- The real power-clean trigger is `save_variable.cgi?index=60&value=1`, which is
  rated 2/5, exposed as a command, and unconstrained by index.

Inert on this system, where steam reports `not_seen`. Still wrong.

## Commands

```bash
npm run check                                     # typecheck + tests + build
npm run selftest -- --api http://127.0.0.1:5180   # live, strictly read-only
```

Neither may ever open a valve.
