/**
 * The egress trace sits on the path every valve command takes. These tests care
 * about two things above all: that it records enough to tell a duplicate
 * command from a deliberate one, and that it can never fail the request that
 * produced it.
 */
import { describe, it, expect, beforeEach, afterAll } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'dtv-trace-'));
process.env.DTV_TRACE_DIR = DIR;

const {
  traceLine,
  traceId,
  traceRequest,
  traceResponse,
  traceError,
  traceNote,
  TRACE_FILE,
  _resetTraceState,
} = await import('../trace.mjs');

async function read(): Promise<string[]> {
  // Writes are fire-and-forget on the request path, so flush before reading.
  await _resetTraceState();
  if (!fs.existsSync(TRACE_FILE)) return [];
  return fs.readFileSync(TRACE_FILE, 'utf8').split('\n').filter(Boolean);
}

beforeEach(async () => {
  await _resetTraceState();
  if (fs.existsSync(TRACE_FILE)) fs.rmSync(TRACE_FILE);
});

afterAll(() => fs.rmSync(DIR, { recursive: true, force: true }));

describe('line format', () => {
  it('puts timestamp, kind, id and endpoint in fixed leading columns', async () => {
    traceLine('RES', 'ab12', 'values.cgi', 'ok 175ms keys=304');
    const [line] = await read();
    expect(line).toMatch(
      /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z {2}RES {4}ab12 {2}values\.cgi {8}ok 175ms keys=304$/,
    );
  });

  it('keeps the columns aligned for the five-character kinds', () => {
    // CACHE and GUARD are longer than REQ/RES/ERR. If the kind column shifts by
    // a character, reading the file top to bottom stops working — which is the
    // whole point of this format.
    traceLine('RES', 'ab12', 'values.cgi', 'ok 175ms keys=304');
    traceLine('CACHE', '----', 'values.cgi', 'hit age=15s keys=304');
    traceLine('GUARD', 'ab13', 'values.cgi', 'suspect payload loses a valve');
    return read().then((lines) => {
      const idColumn = lines.map((l) => l.indexOf('values.cgi'));
      expect(new Set(idColumn).size).toBe(1);
    });
  });

  it('keeps the kind greppable with surrounding spaces', async () => {
    traceLine('ERR', 'ab12', 'system_info.cgi', 'timeout after 8000ms');
    traceLine('DIFF', 'ab13', 'system_info.cgi', 'ui_shower_on=false->true');
    const lines = await read();
    expect(lines.filter((l) => l.includes(' ERR '))).toHaveLength(1);
    expect(lines.filter((l) => l.includes(' DIFF '))).toHaveLength(1);
  });

  it('truncates a runaway payload rather than writing it whole', async () => {
    traceLine('RES', 'ab12', 'values.cgi', 'x'.repeat(5000));
    const [line] = await read();
    expect(line.length).toBeLessThan(500);
    expect(line.endsWith('…')).toBe(true);
  });
});

describe('correlation ids', () => {
  it('issues distinct four-character ids', () => {
    const ids = new Set(Array.from({ length: 200 }, () => traceId()));
    expect(ids.size).toBe(200);
    for (const id of ids) expect(id).toHaveLength(4);
  });
});

describe('what it has to make visible', () => {
  it('shows a doubled command as two REQ lines with different ids', async () => {
    // The 2026-08-04 defect: one outlet tap, two identical quick_shower calls
    // ~120 ms apart. Identical state both times, so controller polling could
    // never see it. This is the whole reason the egress log exists.
    const params = { valve1_outlet: '13', valve1_temp: 96, valve1_massage: 0 };
    for (const id of [traceId(), traceId()]) traceRequest(id, 'quick_shower.cgi', params, 0, 2);

    const reqs = (await read()).filter(
      (l) => l.includes(' REQ ') && l.includes('quick_shower.cgi'),
    );
    expect(reqs).toHaveLength(2);
    expect(reqs[0]).toContain('valve1_outlet=13 valve1_temp=96 valve1_massage=0');
    // Different ids, so the pair reads as two commands rather than one retried.
    expect(reqs[0].split(/\s+/)[2]).not.toBe(reqs[1].split(/\s+/)[2]);
  });

  it('marks a short payload relative to the largest seen, not a fixed number', async () => {
    // Healthy was 303 keys before the wall interface came back and 304 after,
    // and short reads have been observed at 300 and 299. Anything hardcoded
    // would be wrong within a week.
    const big = Object.fromEntries(Array.from({ length: 304 }, (_, i) => [`k${i}`, i]));
    const short = Object.fromEntries(Array.from({ length: 299 }, (_, i) => [`k${i}`, i]));
    traceResponse('aa01', 'values.cgi', { ms: 175, queuedMs: 0, status: 200, json: big });
    traceResponse('aa02', 'values.cgi', { ms: 26989, queuedMs: 0, status: 200, json: short });

    const lines = await read();
    expect(lines[0]).toContain('keys=304');
    expect(lines[0]).not.toContain('SHORT');
    expect(lines[1]).toContain('keys=299');
    expect(lines[1]).toContain('SHORT(-5)');
  });

  it('records a raised baseline rather than silently re-flagging', async () => {
    const a = Object.fromEntries(Array.from({ length: 303 }, (_, i) => [`k${i}`, i]));
    const b = Object.fromEntries(Array.from({ length: 304 }, (_, i) => [`k${i}`, i]));
    traceResponse('aa01', 'values.cgi', { ms: 1, queuedMs: 0, status: 200, json: a });
    traceResponse('aa02', 'values.cgi', { ms: 1, queuedMs: 0, status: 200, json: b });
    const lines = await read();
    expect(lines.some((l) => l.includes(' NOTE ') && l.includes('303->304'))).toBe(true);
  });

  it('reports queue wait only when it is worth noticing', async () => {
    traceResponse('aa01', 'values.cgi', { ms: 175, queuedMs: 4, status: 200, json: {} });
    traceResponse('aa02', 'values.cgi', { ms: 175, queuedMs: 1400, status: 200, json: {} });
    const lines = await read();
    expect(lines[0]).not.toContain('q=');
    expect(lines[1]).toContain('q=1400ms');
  });

  it('keeps a short command reply verbatim', async () => {
    traceResponse('aa01', 'stop_shower.cgi', { ms: 90, queuedMs: 0, status: 200, body: ':)' });
    expect((await read())[0]).toContain('body=":)"');
  });

  it('numbers retry attempts under one id', async () => {
    traceRequest('aa01', 'system_info.cgi', {}, 0, 3);
    traceError('aa01', 'system_info.cgi', new Error('timeout after 8000ms'), 0, 3);
    traceRequest('aa01', 'system_info.cgi', {}, 1, 3);
    const lines = await read();
    expect(lines[1]).toContain('timeout after 8000ms attempt=1/3');
    expect(lines[2]).toContain('retry=2/3');
  });
});

describe('it cannot break the shower', () => {
  // Failure to write a log line must never fail the request that produced it.
  // This is the app the operator uses to run a real shower, so a logger that
  // can throw is a logger that can stop someone showering.
  it('swallows a value that explodes when stringified', () => {
    const hostile = {
      toString() {
        throw new Error('boom');
      },
    };
    expect(() =>
      traceRequest('aa01', 'quick_shower.cgi', { valve1_temp: hostile }, 0, 1),
    ).not.toThrow();
    expect(() => traceNote(hostile as unknown as string)).not.toThrow();
  });

  it('drops undefined and null params the way the wire does', async () => {
    traceRequest('aa01', 'quick_shower.cgi', { a: undefined, b: null, c: 0 }, 0, 1);
    expect((await read())[0]).toMatch(/c=0$/);
  });

  it('reopens and keeps writing after the file is removed underneath it', async () => {
    traceNote('before');
    await _resetTraceState();
    fs.rmSync(TRACE_FILE, { force: true });
    expect(() => traceNote('after')).not.toThrow();
    expect((await read()).some((l) => l.includes('after'))).toBe(true);
  });
});
