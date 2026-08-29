/**
 * Egress trace: a record of every request this app makes to the controller, and
 * every answer it gets back.
 *
 * Why this exists
 * ---------------
 * On 2026-08-04 every outlet tap sent `quick_shower.cgi` twice, ~120 ms apart.
 * Both commands set identical state, so the duplicate was invisible in
 * `values.cgi` and `system_info.cgi` — polling the controller harder would never
 * have found it. It existed only on the wire, outbound, and this app kept no
 * record of what it had told the shower to do.
 *
 * That is a confounder for the shutoff investigation, not just an app bug. When
 * a trace catches a shower stopping on its own, the first thing that has to be
 * ruled out is "our app asked for it". Without an egress log, a spontaneous stop
 * and an app-issued stop look identical from controller state alone.
 *
 * What it CANNOT see
 * ------------------
 * Only our own client. The K-99693 wall interface talks to the controller over
 * RS-485 (DTV+ discovery, then Amulet CRC) and issues commands as INVOKE_RPC
 * frames on that bus — it never touches HTTP and never passes through this
 * proxy. Anyone on the LAN can also reach the controller directly; it has no
 * authentication.
 *
 *   *** Absence of a REQ line means "we did not send it", NEVER "nobody sent
 *       it". That distinction is load-bearing the first time a trace shows an
 *       uncommanded stop. ***
 *
 * Rules this file obeys
 * ---------------------
 * 1. It sits on the path every command takes, so it must never fail a request.
 *    Every write is wrapped; a broken log disables itself and the shower keeps
 *    working.
 * 2. It never blocks. Writes are fire-and-forget onto a stream.
 * 3. It never fills the disk. The file rotates at MAX_BYTES and one previous
 *    generation is kept.
 *
 * Format
 * ------
 * One event per line, fixed leading columns, so `grep quick_shower`,
 * `grep ' ERR '` and `grep ' DIFF '` all work with no tooling:
 *
 *   2026-08-05T05:53:31.266Z  REQ   003f  quick_shower.cgi   valve1_outlet=13 ...
 *   2026-08-05T05:53:31.462Z  RES   003f  quick_shower.cgi   ok 196ms 21B body=":)"
 *   2026-08-05T05:53:41.010Z  RES   0040  values.cgi         ok 175ms keys=304
 *   2026-08-05T05:53:51.010Z  RES   0041  values.cgi         ok 26989ms keys=299 SHORT(-5)
 *   2026-08-05T05:56:41.882Z  ERR   0042  system_info.cgi    timeout after 8000ms attempt=1/3
 *
 * This text file is the single source of truth. There is no parallel JSONL: a
 * second artifact would mean a second write on the command path for no gain,
 * and the columns here are fixed enough to parse with a split.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** Rolling logs live outside the repo tree's committed evidence. Curated
 *  captures get downloaded and saved into research/diagnostics/ by hand, with a
 *  date in the filename, per AGENT.md. */
export const TRACE_DIR = process.env.DTV_TRACE_DIR || path.resolve(__dirname, '..', '.trace');
export const TRACE_FILE = path.join(TRACE_DIR, 'egress.log');

/** ~8 MB is several weeks of idle polling and still opens in an editor. */
const MAX_BYTES = Number(process.env.DTV_TRACE_MAX_BYTES || 8 * 1024 * 1024);
/** Keep the trailing end of a long line rather than the whole payload. */
const MAX_TAIL = 400;

const ENABLED = process.env.DTV_TRACE !== '0';

let stream = null;
let written = 0;
let broken = false;

/**
 * Highest key count seen per endpoint this session, so a short payload is
 * flagged relative to what this controller actually returns rather than against
 * a hardcoded number.
 *
 * This matters more than it looks. `values.cgi` intermittently answers with a
 * short payload in which a healthy, connected valve reads `installed: false` /
 * `con_string: "dis"` — the exact signature the shutoff investigation is
 * hunting. Healthy is 304 keys on this controller today, but it was 303 before
 * the wall interface was reconnected, and short reads have been seen at 300 and
 * at 299. A fixed threshold would be wrong within a week; the observed maximum
 * never is.
 */
const seenMaxKeys = new Map();

function ensureStream() {
  if (!ENABLED || broken) return null;
  if (stream) return stream;
  try {
    fs.mkdirSync(TRACE_DIR, { recursive: true });
    written = fs.existsSync(TRACE_FILE) ? fs.statSync(TRACE_FILE).size : 0;
    stream = fs.createWriteStream(TRACE_FILE, { flags: 'a' });
    // A disk that fills or a directory that vanishes must not take the shower
    // down with it.
    stream.on('error', () => {
      broken = true;
      stream = null;
    });
    return stream;
  } catch {
    broken = true;
    return null;
  }
}

function rotateIfNeeded() {
  if (written < MAX_BYTES) return;
  try {
    stream?.end();
    stream = null;
    fs.renameSync(TRACE_FILE, `${TRACE_FILE}.1`);
    written = 0;
  } catch {
    /* keep appending to the current file rather than losing the trace */
  }
}

/** Emit one line. Never throws, never blocks, never rejects. */
export function traceLine(kind, id, endpoint, tail = '') {
  if (!ENABLED || broken) return;
  try {
    const s = ensureStream();
    if (!s) return;
    const clipped = tail.length > MAX_TAIL ? `${tail.slice(0, MAX_TAIL)}…` : tail;
    // 5-wide kind column: REQ/RES/ERR/DIFF/NOTE are shorter, CACHE/GUARD are
    // not, and a column that shifts by a character defeats reading the file by
    // eye — which is the whole point of the format.
    const line = `${new Date().toISOString()}  ${String(kind).padEnd(5)}  ${id}  ${String(
      endpoint,
    ).padEnd(17)} ${clipped}\n`;
    written += Buffer.byteLength(line);
    s.write(line);
    rotateIfNeeded();
  } catch {
    broken = true;
  }
}

// --- Correlation ids -------------------------------------------------------
// Four base-36 characters. Short enough to scan a column by eye, long enough
// that a REQ and its RES are unambiguous within any window a human reads.
let counter = Math.floor(Math.random() * 36 ** 4);
export function traceId() {
  counter = (counter + 1) % 36 ** 4;
  return counter.toString(36).padStart(4, '0');
}

/**
 * Wrap an emitter so nothing it does can escape into the caller.
 *
 * `traceLine` guards its own write, but the emitters below build their tail
 * string first — and formatting a caller's value can itself throw. A unit test
 * caught exactly that, with a param whose `toString` throws reaching
 * `quick_shower.cgi`'s trace line and taking the valve command down with it.
 */
function safe(fn) {
  return (...args) => {
    try {
      fn(...args);
    } catch {
      /* a lost log line is always better than a lost command */
    }
  };
}

/** Render params the way they go on the wire, so a REQ line is copy-pasteable. */
function renderParams(params) {
  const entries = Object.entries(params || {}).filter(([, v]) => v !== undefined && v !== null);
  if (!entries.length) return '';
  return entries.map(([k, v]) => `${k}=${String(v)}`).join(' ');
}

export const traceRequest = safe(function traceRequest(id, endpoint, params, attempt, attempts) {
  const retry = attempt > 0 ? ` retry=${attempt + 1}/${attempts}` : '';
  traceLine('REQ', id, endpoint, `${renderParams(params)}${retry}`.trim());
});

/**
 * @param {object} r  { ms, queuedMs, status, body, json }
 */
export const traceResponse = safe(function traceResponse(id, endpoint, r) {
  const parts = ['ok', `${r.ms}ms`];
  // Queue wait is the early-warning sign that the controller is bogging down:
  // requests pile up behind a slow one long before anything times out.
  if (r.queuedMs > 50) parts.push(`q=${r.queuedMs}ms`);
  if (r.status && r.status !== 200) parts.push(`http=${r.status}`);

  if (r.json && typeof r.json === 'object' && !Array.isArray(r.json)) {
    const keys = Object.keys(r.json).length;
    parts.push(`keys=${keys}`);
    const max = seenMaxKeys.get(endpoint) ?? 0;
    if (keys > max) {
      seenMaxKeys.set(endpoint, keys);
      if (max > 0) traceLine('NOTE', id, endpoint, `key baseline raised ${max}->${keys}`);
    } else if (keys < max) {
      // A short payload and a genuine device dropout are different events and
      // must not be confused while skimming — see the comment on seenMaxKeys.
      parts.push(`SHORT(-${max - keys})`);
    }
  } else {
    const body = String(r.body ?? '');
    parts.push(`${Buffer.byteLength(body)}B`);
    if (body.length && body.length <= 60) parts.push(`body=${JSON.stringify(body.trim())}`);
  }
  traceLine('RES', id, endpoint, parts.join(' '));
});

export const traceError = safe(function traceError(id, endpoint, err, attempt, attempts) {
  traceLine(
    'ERR',
    id,
    endpoint,
    `${String(err?.message || err)} attempt=${attempt + 1}/${attempts}`,
  );
});

/** Free-text marker: process start, config, operator notes. */
export const traceNote = safe(function traceNote(text) {
  traceLine('NOTE', '----', '-', String(text));
});

/**
 * Wait for buffered lines to reach the file. Writes are deliberately
 * fire-and-forget on the request path, so anything that needs to *read* the log
 * back — the viewer, the tests — flushes first.
 */
export function flushTrace() {
  return new Promise((resolve) => {
    if (!stream) return resolve();
    const s = stream;
    stream = null;
    s.end(resolve);
  });
}

/** Reset in-memory state. Tests only. */
export async function _resetTraceState() {
  seenMaxKeys.clear();
  await flushTrace();
  written = 0;
  broken = false;
}
