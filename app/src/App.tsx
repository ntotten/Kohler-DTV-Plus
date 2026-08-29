import { useEffect, useState } from 'react';
import { useShower } from './state/useShower';
import { useTheme } from './state/useTheme';
import { HomeScreen } from './ui/HomeScreen';
import { ShowerScreen } from './ui/ShowerScreen';
import { UsersScreen } from './ui/UsersScreen';
import { MusicScreen } from './ui/MusicScreen';
import { SettingsScreen } from './ui/SettingsScreen';
import { SystemScreen } from './ui/SystemScreen';
import { ChevronDownIcon, ChevronUpIcon, PowerIcon } from './ui/Icons';
import { SCREEN_TITLE, type Screen } from './ui/screens';
import { connectionState, type ConnectionState } from './api/model';

const CONN_LABEL: Record<ConnectionState, string> = {
  connecting: 'connecting…',
  running: 'water running',
  idle: 'connected · idle',
  unreachable: 'controller unreachable',
};

/** Neutral dot while connecting: nothing is wrong yet. */
const CONN_DOT: Record<ConnectionState, string> = {
  connecting: '',
  running: 'ok',
  idle: 'warn',
  unreachable: 'bad',
};

export default function App() {
  const { model, selection, targetTemp, massage, busy, lastError, actions } = useShower();
  const { theme, setTheme } = useTheme();
  const [screen, setScreen] = useState<Screen>('home');
  const [clock, setClock] = useState(() => new Date());

  useEffect(() => {
    const t = setInterval(() => setClock(new Date()), 15_000);
    return () => clearInterval(t);
  }, []);

  // The hardware up/down keys adjust temperature from any screen whenever the
  // shower can act on it — same as the real interface.
  const tempKeysLive = model.online && model.valves[0].installed;
  const conn = connectionState(model, lastError);

  const stopEverything = () => {
    if (model.currentUser > 0) actions.stopPreset();
    else actions.stop();
  };

  const timeLabel = clock
    .toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
    .replace(' ', '')
    .toLowerCase();

  return (
    <div className="device">
      <div className="bezel">
        <div className="screen">
          <div className="screen-head">
            <span className="title">{SCREEN_TITLE[screen]}</span>
            <span className="clock">{timeLabel}</span>
          </div>

          {screen === 'home' && (
            <HomeScreen
              model={model}
              busy={busy}
              onNavigate={setScreen}
              onStopAll={stopEverything}
            />
          )}

          {screen === 'shower' && (
            <ShowerScreen
              model={model}
              selection={selection}
              targetTemp={targetTemp}
              massage={massage}
              busy={busy}
              theme={theme}
              onToggleOutlet={actions.toggleOutlet}
              onStart={actions.start}
              onStop={actions.stop}
              onMassage={actions.changeMassage}
              onHome={() => setScreen('home')}
            />
          )}

          {screen === 'users' && (
            <UsersScreen
              model={model}
              busy={busy}
              onStart={actions.startPreset}
              onStop={actions.stopPreset}
              onHome={() => setScreen('home')}
            />
          )}

          {screen === 'music' && <MusicScreen model={model} onHome={() => setScreen('home')} />}

          {screen === 'settings' && (
            <SettingsScreen
              theme={theme}
              onTheme={setTheme}
              onNavigate={setScreen}
              onHome={() => setScreen('home')}
            />
          )}

          {screen === 'system' && (
            <SystemScreen model={model} onHome={() => setScreen('settings')} />
          )}

          {lastError && <div className="toast">{lastError}</div>}

          <div className="status-strip">
            <span className={`dot ${CONN_DOT[conn]}`} />
            {CONN_LABEL[conn]}
            <span className="spacer" />
            {model.controllerIp}
          </div>
        </div>

        <div className="hardkeys">
          <button className="hardkey" disabled aria-label="power (hardware only)">
            <PowerIcon />
          </button>
          <button
            className={`hardkey${tempKeysLive ? ' enabled' : ''}`}
            disabled={!tempKeysLive}
            onClick={() => actions.adjustTemp(targetTemp - 1)}
            aria-label="decrease temperature"
          >
            <ChevronDownIcon />
          </button>
          <button
            className={`hardkey${tempKeysLive ? ' enabled' : ''}`}
            disabled={!tempKeysLive}
            onClick={() => actions.adjustTemp(targetTemp + 1)}
            aria-label="increase temperature"
          >
            <ChevronUpIcon />
          </button>
        </div>
      </div>
    </div>
  );
}
