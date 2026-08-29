import { OUTLET_TYPES, parseOutletType } from './outlets';
import type { KohlerSystemInfo, KohlerValues, StatusResponse } from './types';

export interface Outlet {
  /**
   * Configuration slot, 1-6. This is the digit quick_shower.cgi expects in
   * valveN_outlet, and the index behind one_type..six_type.
   */
  position: number;
  /**
   * The index system_info.cgi reports this outlet's state under, i.e.
   * `valveNoutlet<statusIndex>`. Taken from valveN_outletM_func.id, which is a
   * *different index space* from `position` — they coincide on many systems but
   * not all. Conflating them is what made Home Assistant's integration turn on
   * outlet 6 when the user asked for outlet 2 (niemyjski#39).
   */
  statusIndex: number;
  typeId: number;
  label: string;
  massageCapable: boolean;
  /** Configured on this valve (type is not "outlet_0"). */
  configured: boolean;
  /**
   * The controller's own armed selection for this outlet. Careful: this is
   * *selection*, not flow — it stays true for the default outlet while the
   * shower is off. Use `isFlowing()` for "water is coming out of this".
   */
  selected: boolean;
  isDefault: boolean;
}

export interface Valve {
  num: 1 | 2;
  installed: boolean;
  connected: boolean;
  outlets: Outlet[];
  targetTemp: number;
  minTemp: number;
  maxTemp: number;
  massage: number;
  /** Raw valveN_Currentstatus: "", "Off", "On" or "PurgeActive". */
  statusText: string;
  /** Water is moving — including during an auto-purge warm-up. */
  running: boolean;
  /** Running the cold-water purge before the shower comes up to temperature. */
  purging: boolean;
}

export interface Preset {
  id: number;
  name: string;
  enabled: boolean;
}

export interface ShowerModel {
  online: boolean;
  /**
   * At least one reply has come back from the proxy. `online` on its own cannot
   * tell "first poll still in flight" from "controller unreachable", and the
   * difference matters: an unloaded model has six outlet slots of type 0, which
   * is indistinguishable from a valve with nothing configured. Without this the
   * first paint tells the user their shower has no outlets.
   */
  loaded: boolean;
  error?: string;
  ts: number;
  showerOn: boolean;
  /** Auto-purge warm-up in progress — water is flowing but still cold. */
  purging: boolean;
  steamRunning: boolean;
  /** True when *any* outlet is flowing or a preset is running. */
  running: boolean;
  massageEnabled: boolean;
  valves: Valve[];
  presets: Preset[];
  currentUser: number;
  audio: { installed: boolean; playing: boolean; volume: number; muted: boolean };
  steam: { installed: boolean; running: boolean; temp: number; minutesRemaining: number };
  units: 'F' | 'C';
  degree: string;
  controllerVersion: string;
  controllerIp: string;
  /** The K-99693 wall interface. false here is exactly the fault we route around. */
  interfacePresent: boolean;
  clock: string;
}

const POS_KEYS = ['one', 'two', 'three', 'four', 'five', 'six'] as const;

function num(v: unknown, fallback = 0): number {
  const n = typeof v === 'number' ? v : parseFloat(String(v ?? ''));
  return Number.isFinite(n) ? n : fallback;
}

function bool(v: unknown): boolean {
  if (typeof v === 'boolean') return v;
  const s = String(v ?? '').toLowerCase();
  return s === 'true' || s === '1' || s === 'on' || s === 'yes';
}

function buildValve(n: 1 | 2, values: KohlerValues | null, system: KohlerSystemInfo | null): Valve {
  const p = n === 2 ? 'v2_' : '';
  const installed = bool(values?.[n === 1 ? 'valve1_installed' : 'valve2_installed']);
  const defaultOutlet = num(values?.[n === 1 ? 'def_outlet' : 'v2_def_outlet']);

  const outlets: Outlet[] = POS_KEYS.map((key, i) => {
    const position = i + 1;
    const typeId = parseOutletType(values?.[`${p}${key}_type`]);
    const meta = OUTLET_TYPES[typeId] ?? OUTLET_TYPES[0];
    // valveN_outletM_func = { func: <fitting type>, id: <system_info index> }.
    // Absent for unconfigured slots, in which case the slot number is the only
    // thing left to fall back on.
    const func = values?.[`valve${n}_outlet${position}_func`] as
      | { func?: number; id?: number }
      | undefined;
    const statusIndex = num(func?.id, position);

    return {
      position,
      statusIndex,
      typeId,
      label: meta.label,
      // The controller stores a per-outlet massage flag, but Real Rain and the
      // bath spouts can never take part regardless of how it is configured.
      massageCapable: meta.massageCapable && bool(values?.[`${p}${key}_massage`]),
      configured: typeId !== 0,
      selected: bool(system?.[`valve${n}outlet${statusIndex}`]),
      isDefault: position === defaultOutlet,
    };
  });

  const isF = num(values?.units) === 0;
  const setpoint = num(system?.[`valve${n}Setpoint`], NaN);
  const fallbackTemp = num(values?.[n === 1 ? 'valve1_temp_string' : 'valve2_temp_string']);

  // "PurgeActive" is the auto-purge warm-up: water IS flowing, but the coarse
  // shower_on flag may not have caught up. Treating it as off makes the UI
  // offer "start" while the shower is already running (niemyjski#45).
  const statusText = String(system?.[`valve${n}_Currentstatus`] ?? '').trim();
  const purging = statusText === 'PurgeActive';

  return {
    num: n,
    installed,
    connected: String(values?.[n === 1 ? 'valve_1_con_string' : 'valve_2_con_string']) === 'conn',
    outlets,
    targetTemp: Number.isFinite(setpoint) ? setpoint : fallbackTemp,
    minTemp: isF ? 86 : 26,
    maxTemp: num(values?.[n === 1 ? 'max_temp' : 'v2_max_temp'], isF ? 113 : 45),
    massage: num(system?.[`valve${n}_massage`]),
    statusText,
    running: statusText === 'On' || purging,
    purging,
  };
}

export function buildModel(res: StatusResponse | null): ShowerModel {
  const values = res?.values ?? null;
  const system = res?.system ?? null;
  const online = Boolean(res?.ok && (values || system));

  const valves = ([1, 2] as const).map((n) => buildValve(n, values, system));
  // The per-outlet flags are the armed selection and are true at rest for the
  // default outlet, so they can never be part of this. valveN_Currentstatus is
  // included because it reports PurgeActive during warm-up, which the coarse
  // shower_on flag can lag behind.
  //
  // system_info is preferred outright when present: the proxy serves values.cgi
  // from a short cache to halve the request rate, so values.shower_on may be
  // stale by up to that TTL, while system_info is always fetched live.
  const showerOn = system
    ? bool(system.ui_shower_on) || valves.some((v) => v.running)
    : bool(values?.shower_on);
  const purging = valves.some((v) => v.purging);

  const presets: Preset[] = [1, 2, 3, 4, 5, 6].map((id) => ({
    id,
    name: String(values?.[`user_${id}`] ?? `Preset ${id}`),
    enabled: bool(values?.[`user_${id}_enabled`]),
  }));

  const isF = num(values?.units) === 0;

  return {
    online,
    loaded: res !== null,
    error: res?.error,
    ts: res?.ts ?? 0,
    showerOn,
    purging,
    steamRunning: bool(values?.steam_running) || bool(system?.ui_steam_running),
    running: showerOn || bool(system?.devices_running),
    massageEnabled: bool(values?.massage_enabled) && bool(values?.massage),
    valves,
    presets,
    currentUser: num(values?.CurrentUser),
    audio: {
      installed: bool(values?.amp_installed),
      playing: String(system?.musicStatus ?? 'Off').toLowerCase() !== 'off',
      volume: num(system?.volStatus?.toString().replace('%', ''), num(values?.volume, 50)),
      muted: String(system?.muteStatus ?? 'Off').toLowerCase() !== 'off',
    },
    steam: {
      installed: bool(values?.steam_installed),
      running: bool(system?.ui_steam_running),
      temp: num(system?.steamTempStatus),
      minutesRemaining: num(system?.steamTimeMinutes),
    },
    units: isF ? 'F' : 'C',
    degree: isF ? '°' : '°',
    controllerVersion: String(values?.controller_version_string ?? ''),
    controllerIp: String(values?.IP ?? res?.host ?? ''),
    interfacePresent: num(values?.num_interface) > 0,
    clock: String(values?.time ?? ''),
  };
}

/** The outlets a UI should offer for a valve: configured ones only. */
export function usableOutlets(valve: Valve): Outlet[] {
  return valve.outlets.filter((o) => o.configured);
}

export type ConnectionState = 'connecting' | 'running' | 'idle' | 'unreachable';

/**
 * What the status strip should report. Split out of the component so the
 * three-way distinction is testable without a DOM.
 *
 * `loaded` alone is not enough to claim "connecting": when the *first* poll
 * fails there is no response to store, so the model stays unloaded and only the
 * error says anything went wrong.
 */
export function connectionState(model: ShowerModel, lastError: string | null): ConnectionState {
  if (model.online) return model.showerOn ? 'running' : 'idle';
  if (!model.loaded && !lastError) return 'connecting';
  return 'unreachable';
}

/** True when water is actually coming out of this fitting. */
export function isFlowing(model: ShowerModel, outlet: Outlet): boolean {
  return model.showerOn && outlet.selected;
}

/**
 * Scald threshold. Water above 43 °C / 109 °F can scald, and faster than most
 * people expect — see DISCLAIMER.md. The controller's own `max_temp` may sit
 * above this (113 °F on this system), so the limit alone is not a safe signal
 * and the UI marks anything past it.
 */
export const SCALD_F = 109;
export const SCALD_C = 43;

export function isScaldRange(temp: number, units: 'F' | 'C'): boolean {
  return temp > (units === 'F' ? SCALD_F : SCALD_C);
}

export interface OutletToggle {
  /** The selection after the tap. */
  selection: Set<number>;
  /** True when the change has to be pushed to the controller now. */
  command: boolean;
}

/**
 * What one outlet tap means: the next selection, and whether a command follows.
 *
 * This is split out of `useShower` and kept pure on purpose. It used to live
 * inside the function handed to `setSelection`, which also fired the command —
 * and React invokes state updaters twice under `<StrictMode>` precisely to
 * expose that kind of impurity, so every tap in `npm run dev` sent
 * quick_shower.cgi twice about 120 ms apart. Rapid successive valve commands
 * are the controller's documented route to going unreachable for hours
 * (research/FIELD-NOTES.md §1). Deciding here and dispatching at the call site
 * means the decision can be run any number of times and still move water once.
 */
export function toggleOutletSelection(
  selection: ReadonlySet<number>,
  position: number,
  showerOn: boolean,
): OutletToggle {
  const next = new Set(selection);
  if (next.has(position)) next.delete(position);
  else next.add(position);
  // While water is flowing, toggling takes effect immediately — this is how the
  // real interface behaves. Idle, the tap only arms the outlet.
  return { selection: next, command: showerOn };
}

/**
 * quick_shower.cgi wants the selected positions concatenated into one string —
 * outlets 1, 3 and 4 are sent as "134". Empty string means "none".
 */
export function encodeOutlets(positions: Iterable<number>): string {
  return [...positions].sort((a, b) => a - b).join('');
}
