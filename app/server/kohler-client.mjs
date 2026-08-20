import net from 'node:net';
import { traceId, traceRequest, traceResponse, traceError } from './trace.mjs';

export const DEFAULT_HOST = process.env.KOHLER_HOST || '192.168.0.115';
const PORT = Number(process.env.KOHLER_PORT || 80);

/**
 * The controller runs "MQX HTTP - Freescale Embedded Web Server". Its .cgi
 * handlers reply with a bare JSON body and no status line at all — i.e. HTTP/0.9.
 * Node's http client rejects that outright ("Received HTTP/0.9 when not allowed"),
 * and so does undici/fetch, so we speak HTTP by hand over a raw TCP socket and
 * accept either shape.
 *
 * The server is also genuinely fragile: it has a small socket pool, occasionally
 * drops a connection mid-reply, and can take seconds to answer. Everything below
 * is built around that — one request at a time, spaced out, with retries.
 */
function rawRequest(path, { host, timeout }) {
  return new Promise((resolve, reject) => {
    const socket = net.connect({ host, port: PORT });
    const chunks = [];
    let settled = false;

    const finish = (err, value) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      err ? reject(err) : resolve(value);
    };

    socket.setTimeout(timeout);
    socket.on('connect', () => {
      // HTTP/1.0 + Connection: close makes the server end the stream itself,
      // which is our signal that the body is complete (there is no
      // Content-Length on the .cgi replies).
      socket.write(
        `GET ${path} HTTP/1.0\r\n` +
          `Host: ${host}\r\n` +
          `User-Agent: dtv-plus-web\r\n` +
          `Accept: */*\r\n` +
          `Connection: close\r\n\r\n`,
      );
    });
    socket.on('data', (c) => chunks.push(c));
    socket.on('end', () => finish(null, parseReply(Buffer.concat(chunks))));
    socket.on('timeout', () => finish(new Error(`timeout after ${timeout}ms`)));
    socket.on('error', (err) => {
      // A reset *after* the body has arrived is routine for this server —
      // treat whatever we already read as the response.
      if (chunks.length) finish(null, parseReply(Buffer.concat(chunks)));
      else finish(err);
    });
  });
}

function parseReply(buf) {
  if (buf.subarray(0, 5).toString('latin1') !== 'HTTP/') {
    // HTTP/0.9 — the whole payload is the body.
    return { status: 200, body: buf.toString('utf8') };
  }
  const crlf = buf.indexOf('\r\n\r\n');
  const lf = buf.indexOf('\n\n');
  const [end, gap] = crlf !== -1 ? [crlf, 4] : [lf, 2];
  const head = buf.subarray(0, end === -1 ? buf.length : end).toString('latin1');
  const status = Number(head.split(/\r?\n/, 1)[0].split(' ')[1]) || 200;
  return { status, body: end === -1 ? '' : buf.subarray(end + gap).toString('utf8') };
}

// --- Serialisation -------------------------------------------------------
// Overlapping requests are what actually wedges this controller, so every call
// goes through one queue with a floor on the gap between them.
const MIN_GAP_MS = 120;
let chain = Promise.resolve();
let lastAt = 0;

function enqueue(task) {
  const queuedAt = Date.now();
  const run = chain.then(async () => {
    const wait = MIN_GAP_MS - (Date.now() - lastAt);
    if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    // How long this call sat behind others. A rising queue wait is the first
    // visible sign that the controller is slowing down, well before anything
    // times out — so it is worth handing back to the caller for the trace.
    const startedAt = Date.now();
    try {
      const value = await task();
      return { value, queuedMs: startedAt - queuedAt };
    } finally {
      lastAt = Date.now();
    }
  });
  // Keep the chain alive even when a call rejects.
  chain = run.then(
    () => {},
    () => {},
  );
  return run;
}

/**
 * GET a .cgi endpoint. Returns { status, body, json } where `json` is the
 * parsed body when it parses, otherwise null.
 */
export async function kohlerGet(endpoint, params = {}, opts = {}) {
  const { host = DEFAULT_HOST, timeout = 8000, retries = 2 } = opts;
  const qs = new URLSearchParams(
    Object.entries(params).filter(([, v]) => v !== undefined && v !== null),
  ).toString();
  const path = `/${endpoint.replace(/^\//, '')}${qs ? `?${qs}` : ''}`;

  // One id spans every attempt for this logical call, so a retried request reads
  // as one story in the trace rather than three unrelated ones.
  const id = traceId();
  const attempts = retries + 1;

  let lastErr;
  for (let attempt = 0; attempt <= retries; attempt++) {
    const sentAt = Date.now();
    traceRequest(id, endpoint, params, attempt, attempts);
    try {
      const { value: res, queuedMs } = await enqueue(() => rawRequest(path, { host, timeout }));
      let json = null;
      const trimmed = res.body.trim();
      if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
        // Some replies use Python's repr — True/False/None and single quotes.
        try {
          json = JSON.parse(trimmed);
        } catch {
          try {
            json = JSON.parse(
              trimmed
                .replace(/'/g, '"')
                .replace(/\bTrue\b/g, 'true')
                .replace(/\bFalse\b/g, 'false')
                .replace(/\bNone\b/g, 'null'),
            );
          } catch {
            json = null;
          }
        }
      }
      traceResponse(id, endpoint, {
        ms: Date.now() - sentAt,
        queuedMs,
        status: res.status,
        body: res.body,
        json,
      });
      return { ...res, json, path, traceId: id };
    } catch (err) {
      lastErr = err;
      traceError(id, endpoint, err, attempt, attempts);
      if (attempt < retries) await new Promise((r) => setTimeout(r, 250 * (attempt + 1)));
    }
  }
  throw lastErr;
}
