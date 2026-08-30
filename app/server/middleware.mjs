import { kohlerGet, DEFAULT_HOST } from './kohler-client.mjs';
import { checkAccess, checkParams, exposedEndpoints, MAX_RISK } from './cgi-safety.mjs';
import { traceNote, traceLine } from './trace.mjs';

function send(res, status, payload) {
  const body = JSON.stringify(payload);
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.setHeader('Cache-Control', 'no-store');
  res.end(body);
}

function readBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf8').trim();
      if (!raw) return resolve({});
      try {
        resolve(JSON.parse(raw));
      } catch {
        resolve({});
      }
    });
  });
}

/**
 * values.cgi is ~300 keys of configuration — outlet layout, temperature limits,
 * preset names — which changes only when someone edits it on the controller's
 * own pages. system_info.cgi carries everything that moves.
 *
 * Serving values.cgi from a short cache halves the request rate against a web
 * server that is documented to lock up under sustained polling (see
 * research/FIELD-NOTES.md). Run state is read from system_info, which is always
 * fetched live, so the cache cannot make the UI claim water is running when it
 * is not.
 */
const VALUES_TTL_MS = 30000;

/**
 * values.cgi occasionally returns a degraded payload: a valve that is present
 * and connected comes back `valve1_installed: false` with
 * `valve_1_con_string: "dis"`, and the very next read is normal again. Observed
 * once in roughly 30-50 reads on our controller with nothing else going on.
 *
 * Left alone this is worse than a blip, because the bad payload lands in the
 * cache and the UI insists the shower has no outlets for the next 30 seconds —
 * potentially while someone is standing in it. So a payload that *loses* a
 * previously-installed valve has to say so twice before we believe it. A real
 * disconnection still surfaces, one refresh later.
 */
function installedValves(json) {
  return [Boolean(json?.valve1_installed), Boolean(json?.valve2_installed)];
}

export function losesAValve(next, prev) {
  if (!prev) return false;
  const [p1, p2] = installedValves(prev);
  const [n1, n2] = installedValves(next);
  return (p1 && !n1) || (p2 && !n2);
}

export function createKohlerMiddleware({ host = DEFAULT_HOST } = {}) {
  let valuesCache = null;
  let lastGood = null;
  let suspectCount = 0;

  // Stamp the start of every process, so a trace is self-describing about which
  // controller it came from and what the cache was doing to the reads in it.
  traceNote(`proxy start host=${host} valuesTtl=${VALUES_TTL_MS}ms pid=${process.pid}`);

  async function readValues() {
    if (valuesCache && Date.now() - valuesCache.at < VALUES_TTL_MS) {
      // A cache hit sends nothing, so it produces no REQ/RES pair. Say so
      // explicitly: a reader must never mistake thirty silent seconds for
      // thirty seconds of confirmation that the valve was still there.
      traceLine(
        'CACHE',
        '----',
        'values.cgi',
        `hit age=${Math.round((Date.now() - valuesCache.at) / 1000)}s keys=${
          Object.keys(valuesCache.json || {}).length
        }`,
      );
      return { json: valuesCache.json, cached: true };
    }
    const r = await kohlerGet('values.cgi', {}, { host, timeout: 8000 });
    if (!r.json) return { json: valuesCache?.json ?? lastGood, cached: Boolean(lastGood) };

    if (losesAValve(r.json, lastGood) && suspectCount < 1) {
      suspectCount++;
      // Do not cache it, so the next poll re-reads rather than waiting out the TTL.
      traceLine(
        'GUARD',
        r.traceId ?? '----',
        'values.cgi',
        `suspect payload loses a valve (keys=${Object.keys(r.json).length}) — serving last good, not cached`,
      );
      return { json: lastGood, cached: true, suspect: true };
    }
    if (losesAValve(r.json, lastGood)) {
      // The guard has been defeated: two suspect payloads in a row. Recorded
      // loudly because the accepted payload now says a healthy valve is absent,
      // which is exactly the signature the shutoff investigation is hunting.
      traceLine(
        'GUARD',
        r.traceId ?? '----',
        'values.cgi',
        `ACCEPTED valve-loss after ${suspectCount + 1} consecutive suspect reads (keys=${
          Object.keys(r.json).length
        })`,
      );
    }

    suspectCount = 0;
    lastGood = r.json;
    valuesCache = { at: Date.now(), json: r.json };
    return { json: r.json, cached: false };
  }

  return async function kohlerMiddleware(req, res, next) {
    const url = new URL(req.url, 'http://localhost');
    if (!url.pathname.startsWith('/api/')) return next();

    try {
      // --- Combined status: the one call the UI polls on a timer. ----------
      if (url.pathname === '/api/status') {
        const fresh = url.searchParams.get('fresh') === '1';
        if (fresh) valuesCache = null;

        const [values, system] = await Promise.allSettled([
          readValues(),
          kohlerGet('system_info.cgi', {}, { host, timeout: 8000 }),
        ]);
        const v = values.status === 'fulfilled' ? values.value.json : null;
        const s = system.status === 'fulfilled' ? system.value.json : null;
        if (!v && !s) {
          return send(res, 502, {
            ok: false,
            error: values.reason?.message || system.reason?.message || 'controller unreachable',
            host,
          });
        }
        return send(res, 200, {
          ok: true,
          ts: Date.now(),
          host,
          values: v,
          system: s,
          valuesCached: values.status === 'fulfilled' ? values.value.cached : false,
        });
      }

      // --- The safety policy itself, so the UI and tests can show it. ------
      // Ratings and the per-endpoint parameter policy both, since a rating on
      // its own says nothing about what arguments an endpoint will take.
      if (url.pathname === '/api/safety') {
        return send(res, 200, { ok: true, maxRisk: MAX_RISK, exposed: exposedEndpoints() });
      }

      // --- Raw read passthrough (diagnostics). ----------------------------
      if (url.pathname.startsWith('/api/read/')) {
        const name = url.pathname.slice('/api/read/'.length);
        const query = Object.fromEntries(url.searchParams);
        // Two questions, two answers, same 403: may this endpoint be read at
        // all, and may it be read with these arguments. Every read endpoint
        // exposed here takes no parameters, so the second refuses all of them.
        for (const gate of [checkAccess(name, 'read'), checkParams(name, query)]) {
          if (!gate.allowed) {
            return send(res, gate.status, { ok: false, error: gate.reason, risk: gate.risk });
          }
        }
        const r = await kohlerGet(name, query, { host });
        return send(res, 200, { ok: true, name, json: r.json, body: r.json ? undefined : r.body });
      }

      // --- Commands. POST only, so nothing can fire one by navigation. -----
      if (url.pathname.startsWith('/api/command/')) {
        const name = url.pathname.slice('/api/command/'.length);
        if (req.method !== 'POST') return send(res, 405, { ok: false, error: 'POST required' });
        const gate = checkAccess(name, 'command');
        if (!gate.allowed) {
          return send(res, gate.status, { ok: false, error: gate.reason, risk: gate.risk });
        }
        const params = await readBody(req);
        // The risk rating covers the endpoint, not its arguments. Without this
        // second gate save_variable.cgi is a write to any of 105 persistent
        // config variables, valve_max_temp included, wearing a 2/5 rating
        // earned by the one write this app actually makes.
        const args = checkParams(name, params);
        if (!args.allowed) {
          return send(res, args.status, { ok: false, error: args.reason, risk: args.risk });
        }
        const r = await kohlerGet(name, params, { host, timeout: 12000, retries: 1 });
        // Any command may have moved something values.cgi reports (save_variable
        // certainly does), so drop the cache rather than serve a stale view.
        valuesCache = null;
        return send(res, 200, { ok: true, name, params, status: r.status, body: r.body?.slice(0, 500) });
      }

      return send(res, 404, { ok: false, error: 'unknown endpoint' });
    } catch (err) {
      return send(res, 502, { ok: false, error: String(err?.message || err), host });
    }
  };
}
