import type { Theme } from '../state/useTheme';
import type { Screen } from './screens';
import { HomeIcon } from './Icons';

interface Props {
  theme: Theme;
  onTheme: (t: Theme) => void;
  onNavigate: (s: Screen) => void;
  onHome: () => void;
}

const THEMES: { value: Theme; label: string; hint: string }[] = [
  { value: 'dark', label: 'dark', hint: 'easier on the eyes in a dim bathroom' },
  { value: 'light', label: 'light', hint: 'matches the original K-99693 screen' },
];

export function SettingsScreen({ theme, onTheme, onNavigate, onHome }: Props) {
  return (
    <div className="screen-body">
      <div className="sys-list">
        <div className="settings-group">display</div>
        {THEMES.map((t) => (
          <button
            key={t.value}
            className={`radio-row${theme === t.value ? ' on' : ''}`}
            onClick={() => onTheme(t.value)}
          >
            <span className="radio-dot" />
            <span className="radio-body">
              {t.label}
              <span className="radio-hint">{t.hint}</span>
            </span>
          </button>
        ))}

        <div className="settings-group">system</div>
        <button className="settings-link" onClick={() => onNavigate('system')}>
          system information
          <span aria-hidden>›</span>
        </button>

        <p className="sys-note">
          Shower configuration — outlets, temperature limits, presets — lives on the controller and
          is edited from its own web pages at the controller&rsquo;s address. This app reads that
          configuration but does not change it.
        </p>
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
