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
 */

export const MAX_RISK = 2;

/** @type {Record<string, {risk: number, expose: 'read'|'command'|false, note: string}>} */
export const CGI = {
  // ---------------------------------------------------------------- 0: safe
  'values.cgi': { risk: 0, expose: 'read', note: 'Full configuration + coarse state. Read-only.' },
  'system_info.cgi': { risk: 0, expose: 'read', note: 'Live status. Read-only.' },
  'languages.cgi': { risk: 0, expose: 'read', note: 'Installed language packs.' },
  'sim_dev_values.cgi': { risk: 0, expose: false, note: 'Simulated-device status. Unused.' },
  'landing_url.cgi': { risk: 0, expose: false, note: 'Default landing page. Unused.' },
  'files.cgi': { risk: 0, expose: false, note: 'File listing. Unused.' },
  'files_available.cgi': { risk: 0, expose: false, note: 'File listing. Unused.' },
  'ftp_status.cgi': { risk: 0, expose: false, note: 'FTP transfer status. Unused.' },
  'cerror_logs.cgi': {
    risk: 0,
    expose: 'read',
    note: 'Controller error log — 99-entry circular buffer, persists across power cycles. Read-only, and the primary evidence for diagnosing faults. Home Assistant ships it in its diagnostics download.',
  },
  'kerror_logs.cgi': { risk: 0, expose: 'read', note: 'Konnect error log. Read-only.' },

  // ------------------------------------------------------------ 1: low risk
  'stop_shower.cgi': { risk: 1, expose: 'command', note: 'Stops water. Failure-safe direction.' },
  'stop_user.cgi': { risk: 1, expose: 'command', note: 'Stops a running preset.' },
  'steam_off.cgi': { risk: 1, expose: 'command', note: 'Stops steam.' },
  'music_off.cgi': { risk: 1, expose: 'command', note: 'Stops audio.' },
  'light_off.cgi': { risk: 1, expose: 'command', note: 'Turns a light module off.' },
  'light_on.cgi': { risk: 1, expose: 'command', note: 'Turns a light module on.' },
  'rain_off.cgi': { risk: 1, expose: 'command', note: 'Turns the rain panel off.' },
  'rain_on.cgi': { risk: 1, expose: 'command', note: 'Rain panel colour or effect.' },
  'music_on.cgi': { risk: 1, expose: 'command', note: 'Starts audio at a volume.' },
  'bt_disconnect.cgi': { risk: 1, expose: false, note: 'Drops the Bluetooth device. Unused.' },
  'id_interface.cgi': { risk: 1, expose: false, note: 'Flashes an interface LED. Unused.' },
  'datatable.cgi': { risk: 1, expose: false, note: 'Datatable debug view. Unused.' },

  // ------------------------------------------------------------ 2: moderate
  'quick_shower.cgi': {
    risk: 2,
    expose: 'command',
    note: 'Opens valves at a temperature. The core command; see the scald note in DISCLAIMER.md.',
  },
  'start_user.cgi': { risk: 2, expose: 'command', note: 'Runs a stored preset.' },
  'steam_on.cgi': { risk: 2, expose: 'command', note: 'Starts steam at temp/time.' },
  'save_variable.cgi': {
    risk: 2,
    expose: 'command',
    note: 'Persistent config write, indices 1-105. Only index 43 (volume) is used by this app.',
  },
  'massage_toggle.cgi': {
    risk: 2,
    expose: false,
    note: 'Toggles massage. quick_shower is used instead.',
  },
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
  'fileupload.cgi': {
    risk: 5,
    expose: false,
    note: 'Firmware upload. A bad image bricks the unit.',
  },
  'unpack_bin.cgi': { risk: 5, expose: false, note: 'Unpacks an uploaded firmware image.' },
};

// Fail loudly at startup rather than shipping a table that contradicts itself.
for (const [name, e] of Object.entries(CGI)) {
  if (e.expose && e.risk > MAX_RISK) {
    throw new Error(
      `cgi-safety: ${name} is exposed as "${e.expose}" but rated ${e.risk}/5, above MAX_RISK ${MAX_RISK}`,
    );
  }
}

/**
 * Decide whether a request may proceed.
 * @param {string} name  e.g. "quick_shower.cgi"
 * @param {'read'|'command'} kind
 */
export function checkAccess(name, kind) {
  const entry = CGI[name];
  if (!entry) {
    return { allowed: false, status: 403, reason: `unknown endpoint: ${name}`, risk: null };
  }
  if (entry.risk > MAX_RISK) {
    return {
      allowed: false,
      status: 403,
      reason: `${name} is rated ${entry.risk}/5 (max allowed ${MAX_RISK}/5): ${entry.note}`,
      risk: entry.risk,
    };
  }
  if (entry.expose !== kind) {
    return {
      allowed: false,
      status: 403,
      reason: entry.expose
        ? `${name} is exposed for "${entry.expose}", not "${kind}"`
        : `${name} is not exposed by this app: ${entry.note}`,
      risk: entry.risk,
    };
  }
  return { allowed: true, status: 200, reason: 'ok', risk: entry.risk };
}

/** Everything currently reachable, for docs and the self-test. */
export function exposedEndpoints() {
  return Object.entries(CGI)
    .filter(([, e]) => e.expose)
    .map(([name, e]) => ({ name, ...e }));
}
