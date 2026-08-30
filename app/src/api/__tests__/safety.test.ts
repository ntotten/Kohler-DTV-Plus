import { describe, expect, it } from 'vitest';
// @ts-expect-error -- plain .mjs, shared with the server. Kept on one line: the
// directive has to sit directly above the module specifier.
import { CGI, MAX_RISK, checkAccess, checkParams, exposedEndpoints } from '../../../server/cgi-safety.mjs';
// @ts-expect-error -- plain .mjs, shared with the server
import { losesAValve } from '../../../server/middleware.mjs';
import { isScaldRange, SCALD_C, SCALD_F } from '../model';

interface Entry {
  risk: number;
  expose: 'read' | 'command' | false;
  note: string;
  params?: Record<string, unknown>;
}
const table = CGI as Record<string, Entry>;
const allows = (name: string, params: Record<string, unknown>) =>
  (checkParams(name, params) as { allowed: boolean }).allowed;

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

/**
 * A risk rating describes an endpoint, not its arguments. save_variable.cgi is
 * rated 2/5 for the one write this app makes — the amplifier's volume — but it
 * is a generic write to any of 105 persistent config variables, and until
 * 2026-08-30 the proxy handed the request body to the controller verbatim.
 * Index 39 is valve_max_temp: the ceiling DISCLAIMER.md promises this app
 * "clamps to ... never raises".
 */
describe('CGI parameter policy', () => {
  it('every exposed endpoint declares one', () => {
    for (const e of exposedEndpoints() as (Entry & { name: string })[]) {
      expect(e.params, `${e.name} is exposed with no parameter policy`).toBeTruthy();
    }
  });

  describe('save_variable.cgi', () => {
    it('accepts index 43, the amplifier volume the app actually writes', () => {
      expect(allows('save_variable.cgi', { index: 43, value: 60 })).toBe(true);
      // The body arrives as JSON, but a string index is the same write.
      expect(allows('save_variable.cgi', { index: '43', value: '60' })).toBe(true);
    });

    it('refuses index 39, valve_max_temp — the temperature ceiling', () => {
      const gate = checkParams('save_variable.cgi', { index: 39, value: 60 }) as {
        allowed: boolean;
        status: number;
        reason: string;
      };
      expect(gate.allowed).toBe(false);
      // The same 403 the risk ceiling uses, so the two refusals look alike.
      expect(gate.status).toBe(403);
      expect(gate.reason).toContain('index');
    });

    it.each([
      [39, 'valve_max_temp — raises the temperature ceiling'],
      [41, 'valve_auto_purge — the cold-water purge, FIELD-NOTES.md §3'],
      [61, 'six_port_calibration_valve1 — factory calibration'],
      [62, 'six_port_calibration_valve2 — factory calibration'],
      [86, 'wifi_password'],
      [88, 'wifi_SSID'],
      [99, 'max_valve_runtime'],
    ])('refuses index %i (%s)', (index) => {
      expect(allows('save_variable.cgi', { index, value: 1 })).toBe(false);
    });

    it('refuses every index except 43', () => {
      const accepted = [];
      for (let index = 1; index <= 105; index++) {
        if (allows('save_variable.cgi', { index, value: 50 })) accepted.push(index);
      }
      expect(accepted).toEqual([43]);
    });

    it('will not take the write without an index', () => {
      // If `index` could be omitted, whatever the controller defaults to would
      // be written and the allowlist would never be consulted.
      expect(allows('save_variable.cgi', { value: 60 })).toBe(false);
      expect(allows('save_variable.cgi', {})).toBe(false);
    });

    it('bounds the value to the range of the controller\'s own volume slider', () => {
      expect(allows('save_variable.cgi', { index: 43, value: 0 })).toBe(true);
      expect(allows('save_variable.cgi', { index: 43, value: 100 })).toBe(true);
      expect(allows('save_variable.cgi', { index: 43, value: 101 })).toBe(false);
      expect(allows('save_variable.cgi', { index: 43, value: -1 })).toBe(false);
    });

    it('refuses the idx/val spelling rather than deciding which docs are right', () => {
      // xagon0 documents both `index`/`value` and `idx`/`val`; the controller's
      // own JS uses `index`/`value`. Refusing unknown names closes the question
      // without needing live hardware to settle it.
      expect(allows('save_variable.cgi', { idx: 39, val: 120 })).toBe(false);
      expect(allows('save_variable.cgi', { index: 43, value: 60, idx: 39 })).toBe(false);
    });

    it('validates the value that will actually be sent, not a coerced one', () => {
      // Number("0x27") is 39, but the query string would carry "0x27".
      expect(allows('save_variable.cgi', { index: '0x2b', value: 60 })).toBe(false);
      expect(allows('save_variable.cgi', { index: ' 43 ', value: 60 })).toBe(false);
      expect(allows('save_variable.cgi', { index: 43, value: '1e2' })).toBe(false);
    });
  });

  describe('the other exposed commands', () => {
    it('accepts what the app sends', () => {
      expect(
        allows('quick_shower.cgi', {
          valve_num: 1,
          valve1_outlet: '134',
          valve1_massage: 0,
          valve1_temp: 101,
          valve2_outlet: '',
          valve2_massage: 0,
          valve2_temp: 100,
        }),
      ).toBe(true);
      expect(allows('start_user.cgi', { user: 3 })).toBe(true);
      expect(allows('steam_on.cgi', { temp: 110, time: 10 })).toBe(true);
      expect(allows('music_on.cgi', { volume: 50 })).toBe(true);
      expect(allows('music_off.cgi', { volume: 50 })).toBe(true);
      expect(allows('light_on.cgi', { module: 1, intensity: 100 })).toBe(true);
      expect(allows('light_off.cgi', { module: 1 })).toBe(true);
      expect(allows('stop_shower.cgi', {})).toBe(true);
      // Celsius setpoints step by 0.5 on the controller's own input.
      expect(allows('quick_shower.cgi', { valve_num: 1, valve1_temp: 40.5 })).toBe(true);
      // rain_on carries a colour or an effect, never both.
      expect(allows('rain_on.cgi', { mode: 1, color: 220 })).toBe(true);
      expect(allows('rain_on.cgi', { mode: 2, effect: 3 })).toBe(true);
    });

    it('refuses out-of-range arguments', () => {
      expect(allows('start_user.cgi', { user: 7 })).toBe(false);
      expect(allows('start_user.cgi', { user: 0 })).toBe(false);
      expect(allows('quick_shower.cgi', { valve_num: 1, valve1_outlet: '789' })).toBe(false);
      expect(allows('quick_shower.cgi', { valve_num: 3 })).toBe(false);
      expect(allows('quick_shower.cgi', { valve_num: 1, valve1_massage: 9 })).toBe(false);
      expect(allows('light_on.cgi', { module: 4, intensity: 100 })).toBe(false);
      expect(allows('music_on.cgi', { volume: 500 })).toBe(false);
      expect(allows('steam_on.cgi', { temp: 110, time: 240 })).toBe(false);
    });

    it('refuses parameters the endpoint does not declare', () => {
      expect(allows('stop_shower.cgi', { valve1_temp: 130 })).toBe(false);
      expect(allows('start_user.cgi', { user: 1, index: 39 })).toBe(false);
      expect(allows('quick_shower.cgi', { valve_num: 1, valve3_temp: 120 })).toBe(false);
    });

    it('ignores null and undefined, which never reach the controller', () => {
      // kohlerGet drops them before building the query string.
      expect(allows('light_on.cgi', { module: 1, intensity: null })).toBe(true);
      expect(allows('stop_shower.cgi', { volume: undefined })).toBe(true);
    });
  });

  describe('reads', () => {
    it('takes no parameters on any exposed read endpoint', () => {
      for (const name of ['values.cgi', 'system_info.cgi', 'languages.cgi', 'cerror_logs.cgi']) {
        expect(allows(name, {})).toBe(true);
        expect(allows(name, { index: 39 })).toBe(false);
      }
    });
  });

  it('denies parameters for an endpoint it does not know', () => {
    expect(allows('made_up.cgi', {})).toBe(false);
    // Inherited Object properties are not a policy.
    expect(allows('constructor', {})).toBe(false);
    // JSON.parse gives __proto__ as an *own* property (an object literal would
    // not), and `policy['__proto__']` would otherwise resolve to Object's.
    expect(allows('save_variable.cgi', JSON.parse('{"index":43,"value":60,"__proto__":1}'))).toBe(
      false,
    );
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
    expect(losesAValve({ valve1_installed: true, valve2_installed: false }, {
      valve1_installed: true,
      valve2_installed: true,
    })).toBe(true);
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
