import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import * as api from '../api/client';
import {
  buildModel,
  encodeOutlets,
  toggleOutletSelection,
  usableOutlets,
  type ShowerModel,
} from '../api/model';
import type { StatusResponse } from '../api/types';

/**
 * Polling cadence.
 *
 * This is deliberately slow, and the reason is other people's field reports
 * rather than taste. The controller's embedded web server locks up under
 * sustained polling — it stops answering HTTP *and* ping, for hours, and
 * sometimes needs a power cycle. See research/FIELD-NOTES.md; the Home
 * Assistant integration hit this repeatedly at a 20 s interval and settled on
 * 15 s idle / 5 s active, which is what we match.
 *
 * The controller's own touchscreen keeps working through such a lockup, so the
 * failure is confined to the network stack — but that is exactly the part this
 * app depends on.
 */
const POLL_IDLE_MS = 15000;
const POLL_ACTIVE_MS = 5000;
/** Stay on the fast cadence briefly after the shower stops, as HA does. */
const ACTIVE_TAIL_MS = 120000;

/**
 * After we send a command the controller takes a moment to reflect it, and a
 * poll landing inside that window would yank the UI back to the old state. So
 * for this long after any command we keep showing what the user asked for.
 */
const GRACE_MS = 5000;

export interface ShowerState {
  model: ShowerModel;
  /** Positions (1-6) the user has selected on valve 1. */
  selection: Set<number>;
  targetTemp: number;
  massage: number;
  busy: boolean;
  lastError: string | null;
}

export function useShower() {
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [selection, setSelection] = useState<Set<number>>(new Set());
  const [targetTemp, setTargetTemp] = useState<number>(100);
  const [massage, setMassage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const graceUntil = useRef(0);
  const seeded = useRef(false);
  const tempSendTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const model = useMemo(() => buildModel(status), [status]);
  const valve1 = model.valves[0];

  // --- Polling ----------------------------------------------------------
  const lastActiveAt = useRef(0);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const controller = new AbortController();

    const tick = async () => {
      let running = false;
      try {
        const res = await api.getStatus(controller.signal);
        if (!cancelled) {
          setStatus(res);
          setLastError(null);
        }
        // Read the run state straight off this response rather than from React
        // state, so the next interval is chosen without waiting for a re-render.
        const sys = res.system;
        running =
          Boolean(res.values?.shower_on) ||
          Boolean(sys?.ui_shower_on) ||
          ['On', 'PurgeActive'].includes(String(sys?.valve1_Currentstatus ?? '').trim()) ||
          ['On', 'PurgeActive'].includes(String(sys?.valve2_Currentstatus ?? '').trim());
      } catch (err) {
        if (!cancelled && !controller.signal.aborted) {
          setLastError(err instanceof Error ? err.message : String(err));
          setStatus((prev) => (prev ? { ...prev, ok: false } : prev));
        }
      } finally {
        if (!cancelled) {
          if (running) lastActiveAt.current = Date.now();
          const recentlyActive = Date.now() - lastActiveAt.current < ACTIVE_TAIL_MS;
          timer = setTimeout(tick, running || recentlyActive ? POLL_ACTIVE_MS : POLL_IDLE_MS);
        }
      }
    };
    tick();

    return () => {
      cancelled = true;
      controller.abort();
      clearTimeout(timer);
    };
  }, []);

  // --- Reconcile local intent with the controller ------------------------
  useEffect(() => {
    if (!model.online) return;
    const inGrace = Date.now() < graceUntil.current;

    // First successful read: adopt the configured defaults.
    if (!seeded.current) {
      seeded.current = true;
      const armed = valve1.outlets.filter((o) => o.selected && o.configured).map((o) => o.position);
      const def = valve1.outlets.filter((o) => o.isDefault && o.configured).map((o) => o.position);
      setSelection(new Set(armed.length ? armed : def));
      setTargetTemp(valve1.targetTemp || 100);
      setMassage(valve1.massage);
      return;
    }

    if (inGrace) return;

    // While water is running the controller is the authority on what is open.
    if (model.showerOn) {
      const open = valve1.outlets.filter((o) => o.selected && o.configured).map((o) => o.position);
      if (open.length) setSelection(new Set(open));
      setMassage(valve1.massage);
    }
    if (valve1.targetTemp) setTargetTemp(valve1.targetTemp);
  }, [model, valve1]);

  const refreshSoon = useCallback(() => {
    graceUntil.current = Date.now() + GRACE_MS;
  }, []);

  const run = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true);
      refreshSoon();
      try {
        await fn();
        setLastError(null);
      } catch (err) {
        setLastError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [refreshSoon],
  );

  /** Push the complete desired state to the controller. */
  const send = useCallback(
    (positions: Set<number>, temp: number, massageMode: number) => {
      const v2 = model.valves[1];
      if (positions.size === 0) return run(() => api.stopShower());
      return run(() =>
        api.quickShower({
          valveNum: 1,
          valve1Outlets: encodeOutlets(positions),
          valve1Massage: massageMode,
          valve1Temp: temp,
          valve2Outlets: '',
          valve2Massage: 0,
          valve2Temp: v2?.targetTemp || temp,
        }),
      );
    },
    [model.valves, run],
  );

  // --- Actions ----------------------------------------------------------
  const toggleOutlet = useCallback(
    (position: number) => {
      // Decide first, dispatch second. Deciding inside the setSelection updater
      // meant StrictMode's double-invocation sent the command twice — see
      // toggleOutletSelection.
      const { selection: next, command } = toggleOutletSelection(
        selection,
        position,
        model.showerOn,
      );
      setSelection(next);
      if (command) void send(next, targetTemp, massage);
      else refreshSoon();
    },
    [selection, model.showerOn, send, targetTemp, massage, refreshSoon],
  );

  const start = useCallback(() => {
    const positions = selection.size
      ? selection
      : new Set(
          usableOutlets(valve1)
            .filter((o) => o.isDefault)
            .map((o) => o.position),
        );
    if (!positions.size) {
      setLastError('Select at least one outlet first.');
      return;
    }
    setSelection(positions);
    void send(positions, targetTemp, massage);
  }, [selection, valve1, send, targetTemp, massage]);

  const stop = useCallback(() => {
    void run(() => api.stopShower());
  }, [run]);

  const adjustTemp = useCallback(
    (next: number) => {
      const clamped = Math.min(valve1.maxTemp, Math.max(valve1.minTemp, next));
      setTargetTemp(clamped);
      refreshSoon();
      // Debounced: arrow taps come in bursts and this controller does not enjoy
      // a request per tap.
      if (tempSendTimer.current) clearTimeout(tempSendTimer.current);
      if (model.showerOn) {
        tempSendTimer.current = setTimeout(() => {
          void send(selection, clamped, massage);
        }, 450);
      }
    },
    [valve1.maxTemp, valve1.minTemp, refreshSoon, model.showerOn, send, selection, massage],
  );

  const changeMassage = useCallback(
    (mode: number) => {
      setMassage(mode);
      refreshSoon();
      if (model.showerOn) void send(selection, targetTemp, mode);
    },
    [model.showerOn, send, selection, targetTemp, refreshSoon],
  );

  const startPreset = useCallback(
    (id: number) => {
      void run(() => api.startPreset(id));
    },
    [run],
  );

  const stopPreset = useCallback(() => {
    void run(() => api.stopPreset());
  }, [run]);

  useEffect(
    () => () => {
      if (tempSendTimer.current) clearTimeout(tempSendTimer.current);
    },
    [],
  );

  return {
    model,
    selection,
    targetTemp,
    massage,
    busy,
    lastError,
    actions: {
      toggleOutlet,
      start,
      stop,
      adjustTemp,
      changeMassage,
      startPreset,
      stopPreset,
    },
  };
}
