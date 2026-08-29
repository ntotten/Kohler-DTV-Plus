import { describe, expect, it } from 'vitest';
// @ts-expect-error -- plain .mjs, shared with the server
import { CGI, MAX_RISK, checkAccess, exposedEndpoints } from '../../../server/cgi-safety.mjs';
// @ts-expect-error -- plain .mjs, shared with the server
import { losesAValve } from '../../../server/middleware.mjs';
import { isScaldRange, SCALD_C, SCALD_F } from '../model';

interface Entry {
  risk: number;
  expose: 'read' | 'command' | false;
  note: string;
}
const table = CGI as Record<string, Entry>;

/**
 * These guard the one thing in this project that can damage hardware. If the
 * policy is ever loosened by accident, this is what should fail.
 */
describe('CGI safety gate', () => {
  it('keeps the ceiling at 2/5', () => {
    expect(MAX_RISK).toBe(2);
  });

  it('exposes nothing above the ceiling', () => {
    for (const e of exposedEndpoints() as (Entry & { name: string })[]) {
      expect(e.risk, `${e.name} is exposed at ${e.risk}/5`).toBeLessThanOrEqual(MAX_RISK);
    }
  });

  it.each([
    ['reset_factory.cgi', 5],
    ['clear_dt.cgi', 5],
    ['fileupload.cgi', 5],
    ['unpack_bin.cgi', 5],
    ['edit_dt.cgi', 4],
    ['rpc.cgi', 4],
    ['set_device.cgi', 4],
    ['swapvalves.cgi', 4],
    ['forget_devices.cgi', 4],
    ['reset_default.cgi', 4],
  ])('blocks %s (rated %i/5) for both reads and commands', (name, risk) => {
    expect(table[name].risk).toBe(risk);
    expect(checkAccess(name, 'read').allowed).toBe(false);
    expect(checkAccess(name, 'command').allowed).toBe(false);
  });

  it.each(['mac.cgi', 'serial.cgi'])(
    '%s stays blocked — documented upstream as causing lockups',
    (name) => {
      expect(table[name].risk).toBeGreaterThanOrEqual(3);
      expect(checkAccess(name, 'read').allowed).toBe(false);
    },
  );

  it('blocks powerclean_check.cgi, which can trigger a clean cycle', () => {
    expect(table['powerclean_check.cgi'].risk).toBeGreaterThanOrEqual(3);
    expect(checkAccess('powerclean_check.cgi', 'read').allowed).toBe(false);
  });

  it('denies unknown endpoints by default', () => {
    expect(checkAccess('made_up.cgi', 'command').allowed).toBe(false);
    expect(checkAccess('', 'read').allowed).toBe(false);
  });

  it('will not let a read endpoint be driven as a command, or vice versa', () => {
    expect(checkAccess('values.cgi', 'read').allowed).toBe(true);
    expect(checkAccess('values.cgi', 'command').allowed).toBe(false);
    expect(checkAccess('quick_shower.cgi', 'command').allowed).toBe(true);
    expect(checkAccess('quick_shower.cgi', 'read').allowed).toBe(false);
  });

  it('allows exactly the endpoints the app needs, and no more', () => {
    const exposed = (exposedEndpoints() as { name: string }[]).map((e) => e.name).sort();
    expect(exposed).toEqual(
      [
        'values.cgi',
        'system_info.cgi',
        'languages.cgi',
        // Read-only fault history. Exposed deliberately: it is the primary
        // evidence for the mid-shower shutoffs under investigation, and it is
        // what identified the interface detach. See INVESTIGATIONS.md I1.
        'cerror_logs.cgi',
        'kerror_logs.cgi',
        'stop_shower.cgi',
        'stop_user.cgi',
        'steam_off.cgi',
        'music_off.cgi',
        'light_off.cgi',
        'light_on.cgi',
        'rain_off.cgi',
        'rain_on.cgi',
        'music_on.cgi',
        'quick_shower.cgi',
        'start_user.cgi',
        'steam_on.cgi',
        'save_variable.cgi',
      ].sort(),
    );
  });

  it('rates every endpoint it knows about on the 0-5 scale', () => {
    for (const [name, e] of Object.entries(table)) {
      expect(e.risk, name).toBeGreaterThanOrEqual(0);
      expect(e.risk, name).toBeLessThanOrEqual(5);
      expect(e.note, name).toBeTruthy();
    }
  });
});

describe('transient bad-read guard', () => {
  // values.cgi occasionally reports a healthy valve as absent for one read.
  // Caching that payload blanks the UI for the whole TTL — see FIELD-NOTES.md §6.
  const healthy = { valve1_installed: true, valve2_installed: false };
  const degraded = { valve1_installed: false, valve2_installed: false };

  it('flags a payload that drops a previously-installed valve', () => {
    expect(losesAValve(degraded, healthy)).toBe(true);
  });

  it('accepts a steady state, and a valve appearing', () => {
    expect(losesAValve(healthy, healthy)).toBe(false);
    expect(losesAValve(degraded, degraded)).toBe(false);
    expect(losesAValve(healthy, degraded)).toBe(false);
  });

  it('has nothing to compare against on the first read', () => {
    expect(losesAValve(degraded, null)).toBe(false);
  });

  it('notices a second valve dropping too', () => {
    expect(
      losesAValve(
        { valve1_installed: true, valve2_installed: false },
        {
          valve1_installed: true,
          valve2_installed: true,
        },
      ),
    ).toBe(true);
  });
});

describe('scald threshold', () => {
  it('marks anything above 109 F / 43 C', () => {
    expect(SCALD_F).toBe(109);
    expect(SCALD_C).toBe(43);
    expect(isScaldRange(110, 'F')).toBe(true);
    expect(isScaldRange(109, 'F')).toBe(false);
    expect(isScaldRange(44, 'C')).toBe(true);
    expect(isScaldRange(43, 'C')).toBe(false);
  });

  it('flags temperatures the controller would still permit', () => {
    // This system's configured max_temp is 113 F, above the scald threshold —
    // so the controller's own limit cannot be treated as a safety signal.
    expect(isScaldRange(113, 'F')).toBe(true);
  });
});
