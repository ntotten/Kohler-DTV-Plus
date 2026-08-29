import type { ShowerModel } from '../api/model';
import type { Screen } from './screens';
import { DropIcon, MusicIcon, SettingsIcon, SpaIcon, StopSquareIcon, UserIcon } from './Icons';

interface Props {
  model: ShowerModel;
  onNavigate: (screen: Screen) => void;
  onStopAll: () => void;
  busy: boolean;
}

function Tile({
  label,
  icon,
  onClick,
  disabled,
  running,
}: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  running?: boolean;
}) {
  return (
    <button className={`tile${running ? ' running' : ''}`} onClick={onClick} disabled={disabled}>
      {icon}
      {label}
    </button>
  );
}

export function HomeScreen({ model, onNavigate, onStopAll, busy }: Props) {
  const now = new Date();
  const time = now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }).toLowerCase();
  const date = now.toLocaleDateString([], {
    month: '2-digit',
    day: '2-digit',
    year: 'numeric',
  });

  return (
    <div className="screen-body">
      <div className="home-clock">
        <div className="time">{time}</div>
        <div className="date">{date}</div>
      </div>

      <div className="tile-grid">
        <Tile
          label="shower"
          icon={<DropIcon size={26} />}
          running={model.showerOn}
          onClick={() => onNavigate('shower')}
        />
        <Tile label="users" icon={<UserIcon size={26} />} onClick={() => onNavigate('users')} />
        <Tile
          label="music"
          icon={<MusicIcon size={26} />}
          disabled={!model.audio.installed}
          running={model.audio.playing}
          onClick={() => onNavigate('music')}
        />
        <Tile
          label="spa"
          icon={<SpaIcon size={26} />}
          disabled
          onClick={() => onNavigate('home')}
        />
        <Tile
          label="stop"
          icon={<StopSquareIcon size={26} />}
          disabled={busy || !model.running}
          onClick={onStopAll}
        />
        <Tile
          label="settings"
          icon={<SettingsIcon size={26} />}
          onClick={() => onNavigate('settings')}
        />
      </div>
    </div>
  );
}
