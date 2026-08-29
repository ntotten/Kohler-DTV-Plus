# Field notes: what breaks when you automate a DTV+

Failure reports from people who have actually driven a Kohler DTV+ over the
network, with the source for each and what this project does about it.

Every entry is traceable to a public report, a commit, or a measurement against
our own controller (K-99695, firmware `0.0.3.89`). Where we could not verify a
claim, it says so.

Compiled 2026-07-26.

---

## 1. The controller locks up under polling — and only the network dies

**The single most important thing to know before automating this system.**

> "Since 0.4.x came out when i start my shower the controller becomes
> unavailable and generally doesn't come back online without a power cycle to
> the Kohler controller. It is not pingable, reachable via http, and the Kohler
> Konnect app is not working."
> — 38Cherry, [niemyjski/homeassistant-kohler#35](https://github.com/niemyjski/homeassistant-kohler/issues/35)

Reproduced deliberately by the same reporter:

> "I turned off 3 using the switch in HA. It turned off fine. Waited about 10
> seconds then turned off valve 2 and that immediately caused the controller to
> go unavailable."

Confirmed independently by the library author:

> "If you poll the controller too frequently, it can sometimes lock up. Probably
> a cruddy HTTP server it's running. I believe the previous integration used the
> default polling interval of 30s, this now has the update_interval set to 20s.
> Maybe that's too fast?"
> — dcmeglio, same thread

And by the integration maintainer:

> "I had it happen to me when I manually turned on valves in rapid succession
> via HA."
> — niemyjski, same thread

### Two details that change how you should think about it

**It recovers on its own, eventually.** "It went unavailable at 950am ish and
just leaving it alone it came back online at 1236pm" — roughly three hours. A
power cycle is faster but not required.

**The touchscreen keeps working.** "Only the web interface/api. The touch pad
still works." The lockup is confined to the network stack. For a normal user
that makes it a minor annoyance; for us, with no working touchscreen, it is a
total loss of control until it recovers.

### What the community settled on

After hitting this repeatedly, `homeassistant-kohler` converged on:

| Setting                    | Value  | Source                                                                                                                       |
| -------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------- |
| Idle poll interval         | 15 s   | [`coordinator.py:83`](https://github.com/niemyjski/homeassistant-kohler/blob/master/custom_components/kohler/coordinator.py) |
| Active poll interval       | 5 s    | `coordinator.py:120-126`                                                                                                     |
| Fast-poll tail after stop  | 120 s  | `coordinator.py:122-124`                                                                                                     |
| Command debounce           | 0.35 s | `QUICK_SHOWER_DEBOUNCE_SECONDS`                                                                                              |
| Post-command refresh delay | 1.0 s  | `POST_COMMAND_REFRESH_DELAY_SECONDS`                                                                                         |
| Request timeout            | 10 s   | `coordinator.py:105`                                                                                                         |

Kohler's own limit, per [xagon0's notes](xagon0/docs/troubleshooting/known-issues.md):
**two concurrent HTTP sessions**, recovering after ~20 s. Browser tabs, polling
and scripts all count against it.

### What we do

**Our first build polled every 2.5 s — roughly six times faster than the
interval already suspected of causing lockups, and it fetched two endpoints per
cycle.** That was wrong, and it was found by reading these reports rather than by
bricking the controller.

Now:

- 15 s idle / 5 s active with a 120 s tail, matching the converged answer
  ([`useShower.ts`](../app/src/state/useShower.ts)).
- `values.cgi` served from a 30 s server-side cache, so a normal poll costs
  **one** request rather than two ([`middleware.mjs`](../app/server/middleware.mjs)).
- Every request serialised through one queue with a 120 ms floor — we never open
  a second connection, so the two-session limit cannot be hit by us alone
  ([`kohler-client.mjs`](../app/server/kohler-client.mjs)).
- Temperature changes debounced 450 ms.

Sustained load is now about **0.07 req/s idle**, against roughly 0.8 req/s
before.

> ⚠️ Two copies of this app, or this app plus the controller's own web page open
> in a tab, can still exceed two sessions. Close the controller's web UI before
> relying on this one.

---

## 2. Outlet numbering is two different index spaces

**Symptom, reported by a user:**

> "when i turn on switch for outlet 2, immediately the switch for outlet 6 turns
> on, the main showerhead turns on, and the switch for outlet 2 turns off after a
> second. So, essentially, to turn on my main showerhead, i must turn on the
> switch for outlet 2, but to turn it off, i must turn off the switch for outlet 6."
> — [niemyjski/homeassistant-kohler#39](https://github.com/niemyjski/homeassistant-kohler/issues/39)

**Cause.** The controller uses two numbering schemes and they are not always the
same:

| Space                 | Where it appears                                                                                                | Meaning                               |
| --------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| **Slot** `N`          | `one_type`..`six_type`, `valveN_outletM_func` key names, and the digits in `quick_shower.cgi`'s `valve1_outlet` | Configuration position                |
| **Status index** `id` | `system_info.cgi`'s `valveNoutletM` booleans                                                                    | Where that outlet's state is reported |

The bridge is `valveN_outletM_func.id`. Both mature implementations do this, in
mirror-image form:

- [`hubitat-kohlerdtv`](https://github.com/dcmeglio/hubitat-kohlerdtv/blob/master/drivers/Kohler_DTV%2B_Shower.groovy)
  builds `portAssignments[id-1] = N` and emits `N` into the quick_shower string.
- `homeassistant-kohler` builds `mappings[N-1] = id` and reads
  `valve{v}outlet{id}`.

**On our controller the mapping is the identity** — slots 1-4 have ids 1-4 — so
we would never have found this by testing here:

```
slot 1: id=1 func=2   (showerhead)
slot 2: id=2 func=23  (Real Rain)
slot 3: id=3 func=8   (handshower)
slot 4: id=4 func=12  (bodyspray)
```

We implement the mapping anyway, with a regression test that remaps a slot so
the identity case cannot hide a bug ([`model.ts`](../app/src/api/model.ts)).

### Related trap: port count is not configured count

Our valve reports `valve1PortsAvailable = 6` but only **four** `_func` keys
exist — slots 5 and 6 have none. The Hubitat driver loops
`if (portsAvailable >= 6) ... valve1_outlet6_func.id`, which dereferences a null
on a system like ours. We default a missing `func` to the slot number and cover
it with a test.

---

## 3. Auto-purge means the shower is running before it says so

[niemyjski/homeassistant-kohler#45](https://github.com/niemyjski/homeassistant-kohler/issues/45)
added `PurgeActive` as a valve-on state:

> "This state occurs if the shower is configured to have a startup purge to
> remove cold water."

`valveN_Currentstatus` takes `On`, `PurgeActive`, or `Off`; the integration
treats the first two as running (`coordinator.py:443-445`).

**This applies to us:** `auto_purge = 1` and `auto_purge_enable = 1` on our
controller, so pressing start will run a purge cycle first. Our original code
watched only `shower_on` / `ui_shower_on` and would have offered "start" while
water was already flowing.

One wrinkle: **our controller reports `valve1_Currentstatus` as an empty string
when idle, not `"Off"`.** Anything comparing against `"Off"` needs to tolerate
that. The HA integration happens to, via a default.

Fixed, with the purge surfaced in the UI as "warming up — purging cold water".

---

## 4. Experimenting with commands can leave the system stuck

From the issue tracker of the repository this project descends from:

> "for a long time, I was using the genuis suggestions here to run http GET
> command to trigger shower. I was using
> `http://192.168.1.xxx/start_user.cgi?user=1`. That worked great. Recently I was
> trying to trigger a different user, and starting playing around with different
> commands -- and now the whole thing is shot. I cannot get this command to work
> -- and even using the web interface, I cannot get it to start any user from the
> web interface at all."
> — cyrulnik, [timelery/Kohler-DTV-Plus#2](https://github.com/timelery/Kohler-DTV-Plus/issues/2)

The thread was never resolved. We cannot tell from it which command caused the
damage, so treat it as an unexplained persistent failure following unstructured
CGI experimentation — which is precisely the risk the 0-5 rating system exists to
manage. The original README warns in the same spirit:

> "some of these work and others do not. When some are executed the controller
> web page appears to freeze and a reboot is required."

**What we do:** never issue an endpoint rated above 2/5, require explicit
exposure on top of a safe rating, and refuse unknown endpoint names outright.
See [DISCLAIMER.md](../DISCLAIMER.md) and
[`cgi-safety.mjs`](../app/server/cgi-safety.mjs).

---

## 5. Two different Kohler systems, two different APIs

Searches conflate these constantly. They share almost nothing:

| System                        | Control path                           | Auth               | Projects                                                                                                                                                                                                             |
| ----------------------------- | -------------------------------------- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **DTV+** (K-99695 controller) | **Local** CGI over HTTP/0.9 on port 80 | **None**           | [homeassistant-kohler](https://github.com/niemyjski/homeassistant-kohler), [hubitat-kohlerdtv](https://github.com/dcmeglio/hubitat-kohlerdtv), [kohler-python](https://github.com/dcmeglio/kohler-python), this repo |
| **Anthem / Konnect**          | **Cloud** REST via Kohler's backend    | Azure AD B2C OAuth | [ha-kohler-anthem](https://github.com/yon/ha-kohler-anthem), [kohler-konnect-ha](https://github.com/kenyonj/kohler-konnect-ha)                                                                                       |

The Anthem projects' problems are entirely different in kind — and instructive
about what local control buys you:

- **Kohler changed backend RBAC in May 2026** and writes started returning 403
  while reads kept working on cached tokens
  ([ha-kohler-anthem#5](https://github.com/yon/ha-kohler-anthem/issues/5)).
- **Play Integrity blocks credential capture** on modern Android, stalling setup
  ([ha-kohler-anthem#3](https://github.com/yon/ha-kohler-anthem/issues/3)).
- **A preset command sent the `STOP` valve mode** and the cloud returned HTTP 201
  while the device did nothing
  ([kohler-konnect-ha#20](https://github.com/kenyonj/kohler-konnect-ha/issues/20)).
- **The cloud accepts commands the device silently ignores** when a feature is
  disabled on the fixture — HTTP 200, no error, nothing happens
  ([kohler-konnect-ha#18](https://github.com/kenyonj/kohler-konnect-ha/issues/18)).

Nothing here can break our setup, because nothing here is in our path. A DTV+ on
the local CGI API keeps working when Kohler changes their backend, and keeps
working with no internet at all. The trade is that a fragile embedded web server
becomes the single point of failure — which is section 1.

The K-97999 Konnect module adds cloud/voice control to a DTV+; it does not
replace the local CGI API.

---

## 6. `values.cgi` intermittently reports a healthy valve as absent

**Our own observation, 2026-07-26.** Not seen in any community report we found.

A routine read returned:

```
valve_1_con_string     = 'dis'          (normally 'conn')
valve1_installed       = False          (normally True)
valve_2_con_string     = 'not_seen'
controller_con_string  = 'conn'         (unaffected)
amp_con_string         = 'conn'         (unaffected)
valve_1_version_string = '0.12'         (still present)
```

Four reads over the following minute all came back `conn` / `installed: true`,
with no command sent in between and nothing else touching the controller.
Frequency is roughly **one bad read in 30-50**, though that is a rough figure
from ordinary use rather than a controlled measurement.

Note that the valve's _firmware version_ was still reported in the bad payload,
so this looks like a partially-populated response rather than a genuine RS-485
dropout.

**Why it matters more than it looks.** A naive client shows a momentary glitch.
A client that caches `values.cgi` — which you want to do, to keep the request
rate down — caches the _bad_ payload and then insists the shower has no
configured outlets for the whole TTL. That is a disabled start button for 30
seconds, potentially with someone already standing in the shower.

**What we do.** A payload that loses a previously-installed valve must say so
twice before we accept it, and a suspect payload is never cached, so the next
poll re-reads immediately instead of waiting out the TTL. A genuine
disconnection still surfaces, one refresh later. The self-test re-reads once
before reporting a valve as disconnected, and says when it did.

If you maintain a DTV+ integration: this is worth guarding against regardless of
whether you cache, because any "is the system configured?" check built on a
single sample will occasionally be wrong.

---

## 7. Smaller gotchas

**Confirm the integration is current before debugging.** A "stopped working after
a year" report turned out to be an un-upgraded install with unpinned
dependencies ([#44](https://github.com/niemyjski/homeassistant-kohler/issues/44)).

**Kohler MAC prefixes are discoverable.** `homeassistant-kohler` added DHCP
autodiscovery by known Kohler OUI
([#43](https://github.com/niemyjski/homeassistant-kohler/issues/43)). Ours is
`00:14:6F:0E:53:E1`.

**Some clients mishandle these responses.** xagon0 notes Postman and PowerShell
`Invoke-WebRequest` often fail where Chrome and curl work — consistent with the
HTTP/0.9 replies we measured, since a client that insists on a status line has
nothing to parse.

**The K-99693 interface itself has a reliability record.** Owner reviews
describe crashes that "take 30 seconds to 1 minute to reboot, during which water
may or may not shut off randomly." We could not retrieve the review pages
directly (Home Depot returns 403 to automated fetches), so this is
**second-hand via search summary and should be treated as weak evidence** — but
it is consistent with this project's premise, a dead K-99693 in front of a
perfectly healthy controller.

---

## Unverified and open

Honest gaps, so nobody mistakes them for settled:

- **We have not reproduced the lockup**, deliberately. Everything in section 1 is
  other people's evidence plus our own conservative response to it.
- **What actually broke cyrulnik's system** in section 4 is unknown.
- **Whether massage `1` is single or wave** — the controller's own `control.html`
  says single, xagon0's docs say wave. We follow the controller. Untested here.
- **Massage cycling speed** — the interface exposes it; we have not located the
  parameter that carries it.
- **Steam, lighting and rain panel** — coded from others' references, untestable
  on this system, none installed.
- **Preset saving** — the `save_variable.cgi` sequence is not worked out.

---

## Sources

| Source                                                                                                                                     | What it gave us                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| [niemyjski/homeassistant-kohler](https://github.com/niemyjski/homeassistant-kohler)                                                        | Lockup reports, converged polling values, outlet mapping, PurgeActive |
| [dcmeglio/hubitat-kohlerdtv](https://github.com/dcmeglio/hubitat-kohlerdtv)                                                                | Independent confirmation of the outlet mapping direction              |
| [dcmeglio/kohler-python](https://github.com/dcmeglio/kohler-python)                                                                        | Endpoint and parameter reference                                      |
| [xagon0/Kohler-DTV-Plus](https://github.com/xagon0/Kohler-DTV-Plus)                                                                        | CGI risk ratings, session limit, protocol internals                   |
| [timelery/Kohler-DTV-Plus](https://github.com/timelery/Kohler-DTV-Plus)                                                                    | Original CGI enumeration; the persistent-failure report               |
| [yon/ha-kohler-anthem](https://github.com/yon/ha-kohler-anthem), [kenyonj/kohler-konnect-ha](https://github.com/kenyonj/kohler-konnect-ha) | Cloud-API contrast                                                    |

Nothing here is affiliated with or endorsed by Kohler Co.
