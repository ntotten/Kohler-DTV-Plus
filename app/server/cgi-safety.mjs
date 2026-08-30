/**
 * Safety policy for the controller's CGI surface.
 *
 * The DTV+ controller has no authentication and exposes endpoints that can
 * rewrite its datatable, wipe its configuration, or brick it outright. This
 * table is the single place that decides what this app is allowed to touch.
 *
 * Risk ratings use the 0-5 scale from xagon0's DTV+ analysis
 * (research/xagon0/DISCLAIMER.md):
 *
 *   0/5  Safe          read-only or no side effects
 *   1/5  Low risk      minor settings changes
 *   2/5  Moderate      changes device behaviour
 *   3/5  Caution       may cause lockups, requires reboot
 *   4/5  Dangerous     can cause persistent issues
 *   5/5  Critical      can brick the controller
 *
 * Ratings marked "documented" are stated upstream. The rest are assigned here
 * from what the endpoint does, erring high — an endpoint that writes flash,
 * resets state, or touches firmware is never rated below 3.
 *
 * Policy: nothing above MAX_RISK is reachable, and beyond that an endpoint must
 * be explicitly exposed. Both conditions must hold, so adding a `risk: 0` entry
 * does not by itself open it up.
 *
 * ---------------------------------------------------------------------------
 * Parameters
 *
 * A risk rating describes an endpoint. It does not describe an endpoint's
 * *arguments*, and for one endpoint here that gap was the whole gate:
 * `save_variable.cgi` is a generic write to any of 105 persistent config
 * variables. Rating it 2/5 is right for the one write this app makes — the
 * amplifier's stored volume — and wrong for the 104 it does not, which include
 * `valve_max_temp` (39), `valve_auto_purge` (41 — the cold-water purge the UI
 * reads run state from, research/FIELD-NOTES.md §3), the six-port calibration
 * codes (61, 62), `max_valve_runtime` (99) and the Wi-Fi credentials (86, 88).
 * Until 2026-08-30 the proxy passed the request body to the controller verbatim,
 * so anything that could reach it could write any of them.
 *
 * So every exposed endpoint also declares `params`: the parameter names it
 * accepts and the values each may take. Anything not named is refused. That
 * matters beyond value-checking — the vendored docs disagree about whether
 * save_variable takes `index`/`value` or `idx`/`val`
 * (research/xagon0/docs/web-interface/cgi-endpoints.md vs
 * .../control-logic/temperature-system.md), and refusing unknown names closes
 * that question without needing to settle it on live hardware.
 *
 * Parameter names and ranges below come from the controller's own web UI,
 * mirrored at research/controller-mirror/js/, which is the code the hardware
 * shipped with. `params: {}` means "takes no parameters" and is a real policy,
 * not a placeholder.
 *
 * Constraint forms:
 *   { enum: [...] }        value must equal one of these, compared as strings
 *   { int: [min, max] }    whole number, inclusive
 *   { num: [min, max] }    number, inclusive — temperatures step by 0.5 in C
 *   { match: /re/ }        string pattern
 *   add `required: true`   the request is refused when the parameter is absent
 */

export const MAX_RISK = 2;

/**
 * Outlet positions concatenated into one string — outlets 1, 3 and 4 are sent
 * as "134". Empty means none; the controller's own UI calls stop_shower.cgi
 * rather than sending an empty set. See PROTOCOL.md § Starting the shower.
 */
const OUTLET_STRING = /^[1-6]{0,6}$/;

/**
 * Valve and steam setpoints. The controller's own UI bounds these at 86-max_temp
 * in Fahrenheit and 26-max_temp in Celsius (control.js:1629-1647), where
 * max_temp is whatever the installer configured — 113 F on this system.
 *
 * This gate cannot be the scald guard: it does not know the configured unit or
 * the configured maximum, both of which live in values.cgi. 26-130 is a sanity
 * bound that admits either unit and rejects garbage. The real clamp is
 * max_temp, applied in the UI against the live value, and the reason that clamp
 * now means something is that index 39 below is no longer writable through this
 * proxy.
 */
const TEMPERATURE = [26, 130];

/**
 * @typedef {{enum?: (string|number)[], int?: number[], num?: number[], match?: RegExp, required?: boolean}} ParamSpec
 * @type {Record<string, {risk: number, expose: 'read'|'command'|false, note: string, params?: Record<string, ParamSpec>}>}
 */
export const CGI = {
  // ---------------------------------------------------------------- 0: safe
  'values.cgi': { risk: 0, expose: 'read', params: {}, note: 'Full configuration + coarse state. Read-only.' },
  'system_info.cgi': { risk: 0, expose: 'read', params: {}, note: 'Live status. Read-only.' },
  'languages.cgi': { risk: 0, expose: 'read', params: {}, note: 'Installed language packs.' },
  'sim_dev_values.cgi': { risk: 0, expose: false, note: 'Simulated-device status. Unused.' },
  'landing_url.cgi': { risk: 0, expose: false, note: 'Default landing page. Unused.' },
  'files.cgi': { risk: 0, expose: false, note: 'File listing. Unused.' },
  'files_available.cgi': { risk: 0, expose: false, note: 'File listing. Unused.' },
  'ftp_status.cgi': { risk: 0, expose: false, note: 'FTP transfer status. Unused.' },
  'cerror_logs.cgi': {
    risk: 0,
    expose: 'read',
    params: {},
    note: 'Controller error log — 99-entry circular buffer, persists across power cycles. Read-only, and the primary evidence for diagnosing faults. Home Assistant ships it in its diagnostics download.',
  },
  'kerror_logs.cgi': { risk: 0, expose: 'read', params: {}, note: 'Konnect error log. Read-only.' },

  // ------------------------------------------------------------ 1: low risk
  'stop_shower.cgi': { risk: 1, expose: 'command', params: {}, note: 'Stops water. Failure-safe direction.' },
  'stop_user.cgi': { risk: 1, expose: 'command', params: {}, note: 'Stops a running preset.' },
  'steam_off.cgi': { risk: 1, expose: 'command', params: {}, note: 'Stops steam.' },
  'music_off.cgi': {
    risk: 1,
    expose: 'command',
    // The controller's own music_off() sends the volume slider's position too
    // (control.js:602-614), which is odd but is what the hardware expects.
    params: { volume: { int: [0, 100] } },
    note: 'Stops audio.',
  },
  'light_off.cgi': {
    risk: 1,
    expose: 'command',
    params: { module: { enum: [1, 2, 3] } },
    note: 'Turns a light module off.',
  },
  'light_on.cgi': {
    risk: 1,
    expose: 'command',
    params: { module: { enum: [1, 2, 3] }, intensity: { int: [0, 100] } },
    note: 'Turns a light module on.',
  },
  'rain_off.cgi': { risk: 1, expose: 'command', params: {}, note: 'Turns the rain panel off.' },
  'rain_on.cgi': {
    risk: 1,
    expose: 'command',
    // mode=1 carries a colour, mode=2 an effect — never both, so neither is
    // required. Ranges from PROTOCOL.md § Commands.
    params: {
      mode: { enum: [1, 2] },
      color: { int: [-1, 360] },
      effect: { int: [0, 7] },
    },
    note: 'Rain panel colour or effect.',
  },
  'music_on.cgi': {
    risk: 1,
    expose: 'command',
    params: { volume: { int: [0, 100] } },
    note: 'Starts audio at a volume.',
  },
  'bt_disconnect.cgi': { risk: 1, expose: false, note: 'Drops the Bluetooth device. Unused.' },
  'id_interface.cgi': { risk: 1, expose: false, note: 'Flashes an interface LED. Unused.' },
  'datatable.cgi': { risk: 1, expose: false, note: 'Datatable debug view. Unused.' },

  // ------------------------------------------------------------ 2: moderate
  'quick_shower.cgi': {
    risk: 2,
    expose: 'command',
    // The controller takes the complete desired state on every call, so all
    // seven travel together. Massage modes are 0 off, 1 single, 2 wave,
    // 3 custom 1, 4 custom 2 (PROTOCOL.md § Starting the shower).
    params: {
      valve_num: { enum: [1, 2], required: true },
      valve1_outlet: { match: OUTLET_STRING },
      valve1_massage: { int: [0, 4] },
      valve1_temp: { num: TEMPERATURE },
      valve2_outlet: { match: OUTLET_STRING },
      valve2_massage: { int: [0, 4] },
      valve2_temp: { num: TEMPERATURE },
    },
    note: 'Opens valves at a temperature. The core command; see the scald note in DISCLAIMER.md.',
  },
  'start_user.cgi': {
    risk: 2,
    expose: 'command',
    params: { user: { int: [1, 6], required: true } },
    note: 'Runs a stored preset.',
  },
  'steam_on.cgi': {
    risk: 2,
    expose: 'command',
    // Time is 1-20 minutes — the bounds on the controller's own input
    // (control.html:317).
    params: { temp: { num: TEMPERATURE }, time: { int: [1, 20] } },
    note: 'Starts steam at temp/time.',
  },
  'save_variable.cgi': {
    risk: 2,
    expose: 'command',
    // Index 43 is const_music_volume in the controller's own index table
    // (research/controller-mirror/js/values.js:43), and the only variable this
    // app writes. The other 104 are refused here rather than merely unused:
    // 39 is valve_max_temp, which would let the proxy raise the temperature
    // ceiling DISCLAIMER.md promises it never raises. 0-100 is the range of the
    // controller's own volume slider (settings.js:460).
    params: {
      index: { enum: [43], required: true },
      value: { int: [0, 100], required: true },
    },
    note: 'Persistent config write. Only index 43 (amplifier volume) is accepted; the other 104 indices are refused, including 39 valve_max_temp, 41 valve_auto_purge, 61/62 six-port calibration, 99 max_valve_runtime and 86/88 Wi-Fi credentials.',
  },
  'massage_toggle.cgi': { risk: 2, expose: false, note: 'Toggles massage. quick_shower is used instead.' },
  'light_module.cgi': { risk: 2, expose: false, note: 'Secondary light modules. Unused.' },
  'change_user.cgi': { risk: 2, expose: false, note: 'Switches active user context. Unused.' },
  'update_change.cgi': { risk: 2, expose: false, note: 'Signals a config update. Unused.' },
  'BTKey.cgi': { risk: 2, expose: false, note: 'Sets the Bluetooth pairing key. Unused.' },
  'BTPin.cgi': { risk: 2, expose: false, note: 'Sets the Bluetooth PIN. Unused.' },

  // -------------------------------------------------------------- 3: caution
  'mac.cgi': { risk: 3, expose: false, note: 'Documented upstream as causing system lockups.' },
  'serial.cgi': { risk: 3, expose: false, note: 'Documented upstream as causing system lockups.' },
  'powerclean_check.cgi': {
    risk: 3,
    expose: false,
    note: 'Documented as able to trigger the steam power-clean cycle, not merely report it.',
  },
  'check_updates.cgi': { risk: 3, expose: false, note: 'Arms a firmware update poll.' },
  'saveDT.cgi': { risk: 3, expose: false, note: 'Commits the datatable to flash.' },
  'saveUI.cgi': { risk: 3, expose: false, note: 'Commits UI settings to flash.' },
  'hiding.cgi': { risk: 3, expose: false, note: 'Toggles UI visibility and debug flags.' },
  'remove_module.cgi': { risk: 3, expose: false, note: 'Removes a light module from config.' },
  'reset_fault.cgi': { risk: 3, expose: false, note: 'Clears fault flags.' },
  'reset_cfault.cgi': { risk: 3, expose: false, note: 'Clears controller fault log.' },
  'reset_kfault.cgi': { risk: 3, expose: false, note: 'Clears Konnect fault log.' },
  'reset_user.cgi': { risk: 3, expose: false, note: 'Erases one stored preset.' },

  // ------------------------------------------------------------ 4: dangerous
  'rpc.cgi': { risk: 4, expose: false, note: 'Arbitrary internal RPC by index.' },
  'edit_dt.cgi': { risk: 4, expose: false, note: 'Raw datatable read/write.' },
  'set_device.cgi': { risk: 4, expose: false, note: 'Rewrites the simulated-device map.' },
  'swapvalves.cgi': { risk: 4, expose: false, note: 'Swaps valve 1 and 2 configuration.' },
  'reset_users.cgi': { risk: 4, expose: false, note: 'Erases all presets.' },
  'reset_default.cgi': { risk: 4, expose: false, note: 'Resets system settings.' },
  'save_default.cgi': { risk: 4, expose: false, note: 'Overwrites the stored defaults.' },
  'forget_devices.cgi': { risk: 4, expose: false, note: 'Drops all paired devices.' },

  // ------------------------------------------------------------- 5: critical
  'reset_factory.cgi': { risk: 5, expose: false, note: 'Full factory wipe.' },
  'clear_dt.cgi': { risk: 5, expose: false, note: 'Clears the entire datatable.' },
  'fileupload.cgi': { risk: 5, expose: false, note: 'Firmware upload. A bad image bricks the unit.' },
  'unpack_bin.cgi': { risk: 5, expose: false, note: 'Unpacks an uploaded firmware image.' },
};

// Fail loudly at startup rather than shipping a table that contradicts itself.
for (const [name, e] of Object.entries(CGI)) {
  if (e.expose && e.risk > MAX_RISK) {
    throw new Error(
      `cgi-safety: ${name} is exposed as "${e.expose}" but rated ${e.risk}/5, above MAX_RISK ${MAX_RISK}`,
    );
  }
  // An exposed endpoint with no parameter policy is how save_variable.cgi came
  // to accept all 105 config indices while looking like a 2/5 volume control.
  // The next generic endpoint does not get to arrive the same way.
  if (e.expose && !e.params) {
    throw new Error(
      `cgi-safety: ${name} is exposed as "${e.expose}" with no parameter policy. ` +
        'Declare the parameters it accepts, or `params: {}` if it takes none.',
    );
  }
}

/** Own properties only — `CGI['constructor']` must not resolve to Object's. */
function own(obj, key) {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

function deny(reason, risk) {
  return { allowed: false, status: 403, reason, risk };
}

/**
 * Decide whether a request may proceed, on the endpoint alone.
 *
 * Callers that pass parameters through to the controller must also call
 * checkParams — the two answer different questions.
 *
 * @param {string} name  e.g. "quick_shower.cgi"
 * @param {'read'|'command'} kind
 */
export function checkAccess(name, kind) {
  if (!own(CGI, name)) {
    return deny(`unknown endpoint: ${name}`, null);
  }
  const entry = CGI[name];
  if (entry.risk > MAX_RISK) {
    return deny(
      `${name} is rated ${entry.risk}/5 (max allowed ${MAX_RISK}/5): ${entry.note}`,
      entry.risk,
    );
  }
  if (entry.expose !== kind) {
    return deny(
      entry.expose
        ? `${name} is exposed for "${entry.expose}", not "${kind}"`
        : `${name} is not exposed by this app: ${entry.note}`,
      entry.risk,
    );
  }
  return { allowed: true, status: 200, reason: 'ok', risk: entry.risk };
}

/**
 * Strict decimal. Number() would accept "0x10", "1e2" and " 43 ", each of which
 * reaches the controller as the *original string* — so the gate would be
 * validating a different value from the one that gets sent.
 */
const DECIMAL = /^-?\d+(\.\d+)?$/;

/** @returns {string|null} why the value is refused, or null if it is fine */
function refuseValue(spec, raw) {
  const s = String(raw);
  if (spec.enum) {
    return spec.enum.some((v) => String(v) === s)
      ? null
      : `must be one of ${spec.enum.join(', ')}`;
  }
  const range = spec.int ?? spec.num;
  if (range) {
    if (!DECIMAL.test(s)) return 'must be a plain decimal number';
    const n = Number(s);
    if (spec.int && !Number.isInteger(n)) return 'must be a whole number';
    if (n < range[0] || n > range[1]) return `must be between ${range[0]} and ${range[1]}`;
    return null;
  }
  if (spec.match) {
    return spec.match.test(s) ? null : `must match ${spec.match.source}`;
  }
  return null;
}

/**
 * Decide whether a request's *parameters* may proceed.
 *
 * Refuses any parameter the endpoint does not declare, any value outside the
 * declared constraint, and any declared-required parameter that is missing.
 *
 * @param {string} name
 * @param {Record<string, unknown>} params
 */
export function checkParams(name, params = {}) {
  if (!own(CGI, name)) {
    return deny(`unknown endpoint: ${name}`, null);
  }
  const entry = CGI[name];
  const policy = entry.params;
  if (!policy) {
    // Unreachable while the startup self-check holds, but deny rather than fall
    // open if it ever stops holding.
    return deny(`${name} has no parameter policy, so nothing is permitted`, entry.risk);
  }

  // kohlerGet drops null and undefined before building the query string, so
  // they are never sent and the gate must not police them.
  const given = Object.entries(params ?? {}).filter(([, v]) => v !== undefined && v !== null);
  const allowed = Object.keys(policy);

  for (const [key, raw] of given) {
    if (!own(policy, key)) {
      return deny(
        allowed.length
          ? `${name} does not accept parameter "${key}" (accepts: ${allowed.join(', ')})`
          : `${name} takes no parameters, got "${key}"`,
        entry.risk,
      );
    }
    const why = refuseValue(policy[key], raw);
    if (why) {
      return deny(`${name} parameter "${key}" ${why} — got ${JSON.stringify(raw)}`, entry.risk);
    }
  }

  const names = new Set(given.map(([k]) => k));
  for (const key of allowed) {
    if (policy[key].required && !names.has(key)) {
      return deny(`${name} requires parameter "${key}"`, entry.risk);
    }
  }

  return { allowed: true, status: 200, reason: 'ok', risk: entry.risk };
}

function describeSpec(spec) {
  const suffix = spec.required ? ' (required)' : '';
  if (spec.enum) return `one of ${spec.enum.join(', ')}${suffix}`;
  if (spec.int) return `whole number ${spec.int[0]}-${spec.int[1]}${suffix}`;
  if (spec.num) return `number ${spec.num[0]}-${spec.num[1]}${suffix}`;
  if (spec.match) return `matching ${spec.match.source}${suffix}`;
  return `any value${suffix}`;
}

/**
 * The parameter policy in a form that survives JSON.stringify — a RegExp
 * serialises to `{}`, which would make GET /api/safety quietly claim an
 * unconstrained parameter is unconstrained in a different way.
 */
export function describeParams(policy) {
  if (!policy) return null;
  return Object.fromEntries(Object.entries(policy).map(([k, s]) => [k, describeSpec(s)]));
}

/** Everything currently reachable, for docs and the self-test. */
export function exposedEndpoints() {
  return Object.entries(CGI)
    .filter(([, e]) => e.expose)
    .map(([name, e]) => ({ name, ...e, params: describeParams(e.params) }));
}
