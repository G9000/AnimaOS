import {
  APPEARANCE_ACCENTS,
  APPEARANCE_DENSITIES,
  APPEARANCE_TEXT_SIZES,
  APPEARANCE_THEMES,
  useAppearancePreferences,
  type AppearanceAccent,
  type AppearanceDensity,
  type AppearanceTextSize,
  type AppearanceTheme,
} from '../lib/appearance-preferences';

const THEMES: Record<
  AppearanceTheme,
  { name: string; summary: string }
> = {
  dark: { name: 'Dark', summary: 'Easy on the eyes at night' },
  light: { name: 'Light', summary: 'Warm paper, high contrast' },
  system: { name: 'Match system', summary: 'Follows your computer theme' },
};

const ACCENTS: Record<AppearanceAccent, string> = {
  pink: 'Anima pink',
  lavender: 'Lavender',
  amber: 'Amber',
  sage: 'Sage',
  coral: 'Coral',
  paper: 'Paper',
};

const TEXT_SIZES: Record<AppearanceTextSize, string> = {
  small: 'Small',
  default: 'Default',
  large: 'Large',
};

const DENSITIES: Record<AppearanceDensity, string> = {
  comfortable: 'Comfortable',
  compact: 'Compact',
};

/** How anima looks on this computer. These choices do not change agent data. */
export function AppearancePage() {
  const [preferences, setPreferences] = useAppearancePreferences();
  const customAccent = preferences.accent === 'custom';
  return (
    <div className="settings-form appearance-settings">
      <section className="settings-group appearance-theme-section">
        <h2 className="settings-group-title">Theme</h2>
        <fieldset className="settings-fieldset">
          <legend className="sr-only">Theme</legend>
          <div className="appearance-theme-grid">
            {APPEARANCE_THEMES.map((theme) => {
              const item = THEMES[theme];
              return (
                <label
                  className={`appearance-theme-card${preferences.theme === theme ? ' is-selected' : ''}`}
                  key={theme}
                >
                  <input
                    type="radio"
                    name="appearance-theme"
                    value={theme}
                    checked={preferences.theme === theme}
                    onChange={() => setPreferences({ theme })}
                  />
                  <span className="appearance-theme-preview" data-theme={theme}>
                    <span className="appearance-theme-preview-sidebar" />
                    <span className="appearance-theme-preview-content">
                      <i />
                      <i />
                      <i />
                      <b />
                    </span>
                  </span>
                  <span className="appearance-choice-copy">
                    <strong>{item.name}</strong>
                    <small>
                      {theme === 'system' && preferences.theme === 'system'
                        ? `${item.summary} · now dark`
                        : item.summary}
                    </small>
                  </span>
                  {preferences.theme === theme && (
                    <span className="appearance-current">CURRENT</span>
                  )}
                </label>
              );
            })}
          </div>
        </fieldset>
      </section>

      <section className="settings-group appearance-accent-section">
        <h2 className="settings-group-title">Accent</h2>
        <p className="settings-group-help">
          Used for highlights, the orb, and anything that needs you.
        </p>
        <fieldset className="settings-fieldset">
          <legend className="sr-only">Accent</legend>
          <div className="appearance-accent-options">
            {APPEARANCE_ACCENTS.map((accent) => (
              <label key={accent} className="appearance-accent-choice">
                <input
                  type="radio"
                  name="appearance-accent"
                  value={accent}
                  aria-label={ACCENTS[accent]}
                  checked={preferences.accent === accent}
                  onChange={() => setPreferences({ accent })}
                />
                <span data-accent={accent} aria-hidden />
              </label>
            ))}
            {/* Any colour: choosing it brings back the owner's own, and the
             *  picker changes it. */}
            <label
              className="appearance-accent-choice"
              data-selected={customAccent || undefined}
              title="Pick your own colour"
            >
              <input
                type="color"
                aria-label="Custom accent"
                value={preferences.customAccent}
                onClick={() => setPreferences({ accent: 'custom' })}
                onChange={(event) =>
                  setPreferences({
                    accent: 'custom',
                    customAccent: event.target.value.toLowerCase(),
                  })
                }
              />
              <span
                data-accent="custom"
                style={
                  customAccent
                    ? { background: preferences.customAccent }
                    : undefined
                }
                aria-hidden
              />
            </label>
            <span className="appearance-accent-name">
              {customAccent
                ? `Custom · ${preferences.customAccent}`
                : ACCENTS[preferences.accent as AppearanceAccent]}
            </span>
          </div>
        </fieldset>
      </section>

      <section className="settings-group appearance-display-section">
        <h2 className="settings-group-title">Display</h2>
        <fieldset className="settings-fieldset appearance-display-group">
          <legend className="sr-only">Text size</legend>
          <span className="appearance-display-text" aria-hidden="true">
            <strong>Text size</strong>
            <small>Chat and settings text</small>
          </span>
          <div className="appearance-segmented-options">
            {APPEARANCE_TEXT_SIZES.map((size) => (
              <label key={size}>
                <input
                  type="radio"
                  name="appearance-text-size"
                  value={size}
                  checked={preferences.textSize === size}
                  onChange={() => setPreferences({ textSize: size })}
                />
                <span>{TEXT_SIZES[size]}</span>
              </label>
            ))}
          </div>
        </fieldset>
        <fieldset className="settings-fieldset appearance-display-group">
          <legend className="sr-only">Density</legend>
          <span className="appearance-display-text" aria-hidden="true">
            <strong>Density</strong>
            <small>Space between messages and rows</small>
          </span>
          <div className="appearance-segmented-options">
            {APPEARANCE_DENSITIES.map((density) => (
              <label key={density}>
                <input
                  type="radio"
                  name="appearance-density"
                  value={density}
                  checked={preferences.density === density}
                  onChange={() => setPreferences({ density })}
                />
                <span>{DENSITIES[density]}</span>
              </label>
            ))}
          </div>
        </fieldset>
        <label className="appearance-motion-row">
          <span className="appearance-choice-copy">
            <strong>Reduce motion</strong>
            <small>Calmer orb and fewer animations</small>
          </span>
          <input
            type="checkbox"
            checked={preferences.reduceMotion}
            onChange={(event) =>
              setPreferences({ reduceMotion: event.target.checked })
            }
          />
        </label>
        <label className="appearance-motion-row">
          <span className="appearance-choice-copy">
            <strong>Advanced input</strong>
            <small>Do/Plan and the model picker beside attachments</small>
          </span>
          <input
            type="checkbox"
            checked={preferences.advancedInput}
            onChange={(event) =>
              setPreferences({ advancedInput: event.target.checked })
            }
          />
        </label>
        <label className="appearance-motion-row">
          <span className="appearance-choice-copy">
            <strong>Show work</strong>
            <small>Each step, tool and timing while your agent works</small>
          </span>
          <input
            type="checkbox"
            checked={preferences.showWork}
            onChange={(event) =>
              setPreferences({ showWork: event.target.checked })
            }
          />
        </label>
      </section>
    </div>
  );
}
