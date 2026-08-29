import type { ShowerModel } from '../api/model';
import { HomeIcon } from './Icons';

interface Props {
  model: ShowerModel;
  busy: boolean;
  onStart: (id: number) => void;
  onStop: () => void;
  onHome: () => void;
}

export function UsersScreen({ model, busy, onStart, onStop, onHome }: Props) {
  const anyEnabled = model.presets.some((p) => p.enabled);
  const running = model.currentUser > 0;

  return (
    <div className="screen-body">
      <div className="preset-list">
        {model.presets.map((p) => (
          <button
            key={p.id}
            className={`preset${model.currentUser === p.id ? ' active' : ''}`}
            disabled={!p.enabled || busy}
            onClick={() => onStart(p.id)}
          >
            {p.name}
          </button>
        ))}

        {!anyEnabled && (
          <p className="sys-note">
            No presets are saved yet. Presets are stored on the controller and are normally created
            from the wall interface — with that unit offline, start the shower from the shower
            screen instead.
          </p>
        )}
      </div>

      <div className="action-bar">
        <button className="action bare" onClick={onHome}>
          <span className="glyph">
            <HomeIcon />
          </span>
          home
        </button>
        <button
          className={`action danger${busy ? ' busy' : ''}`}
          onClick={onStop}
          disabled={busy || !running}
        >
          <span className="glyph">stop</span>
        </button>
        <span />
      </div>
    </div>
  );
}
