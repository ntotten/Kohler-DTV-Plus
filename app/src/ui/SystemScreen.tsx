import type { ShowerModel } from '../api/model';
import { HomeIcon } from './Icons';

interface Props {
  model: ShowerModel;
  onHome: () => void;
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="sys-row">
      <span className="k">{k}</span>
      <span className="v">{v}</span>
    </div>
  );
}

export function SystemScreen({ model, onHome }: Props) {
  const valve = model.valves[0];

  return (
    <div className="screen-body">
      <div className="sys-list">
        <Row k="controller" v={model.controllerIp} />
        <Row k="firmware" v={model.controllerVersion || '—'} />
        <Row k="valve 1" v={valve.connected ? 'connected' : 'not seen'} />
        <Row k="amplifier" v={model.audio.installed ? 'connected' : 'not installed'} />
        <Row k="steam" v={model.steam.installed ? 'installed' : 'not installed'} />
        <Row k="wall interface" v={model.interfacePresent ? 'connected' : 'not seen'} />
        <Row k="units" v={`°${model.units}`} />
        <Row k="temp range" v={`${valve.minTemp}° – ${valve.maxTemp}°`} />
        <Row k="outlets configured" v={valve.outlets.filter((o) => o.configured).length} />
        <Row k="massage" v={model.massageEnabled ? 'enabled' : 'disabled'} />
        <Row k="controller clock" v={model.clock || '—'} />
        <Row k="last poll" v={model.ts ? new Date(model.ts).toLocaleTimeString() : '—'} />

        {!model.interfacePresent && (
          <p className="sys-note">
            The controller reports no wall interface attached (<code>num_interface = 0</code>). That
            is the K-99693 fault this app works around — the valve, amplifier and controller are all
            healthy and reachable, so everything here drives the shower directly over the
            controller&rsquo;s own CGI API.
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
        <span />
        <span />
      </div>
    </div>
  );
}
