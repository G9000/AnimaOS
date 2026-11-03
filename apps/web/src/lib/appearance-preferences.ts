import { useSyncExternalStore } from 'react';

export const APPEARANCE_THEMES = ['dark', 'light', 'system'] as const;
export type AppearanceTheme = (typeof APPEARANCE_THEMES)[number];

export const APPEARANCE_ACCENTS = [
  'pink',
  'lavender',
  'amber',
  'sage',
  'coral',
  'paper',
] as const;
export type AppearanceAccent = (typeof APPEARANCE_ACCENTS)[number];
export const APPEARANCE_ACCENT_HEX: Record<AppearanceAccent, string> = {
  pink: '#E94DAB',
  lavender: '#8C93FF',
  amber: '#E8B04B',
  sage: '#7FCB8A',
  coral: '#FF6B5B',
  paper: '#ECE6DA',
};

/** The preset accents, or `custom` for the owner's own colour. */
export type AccentChoice = AppearanceAccent | 'custom';
export const DEFAULT_CUSTOM_ACCENT = '#e94dab';

/** `#` and six hex digits. */
export function isHexColour(value: unknown): value is string {
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value);
}

/** The accent in effect, as `#rrggbb`. */
export function accentHex({
  accent,
  customAccent,
}: Pick<AppearancePreferences, 'accent' | 'customAccent'>): string {
  return accent === 'custom' ? customAccent : APPEARANCE_ACCENT_HEX[accent];
}

/** The colour variables a custom accent sets: the accent, a lighter action
 *  colour, readable text on it, and its RGB channels. */
export function customAccentVariables(hex: string): Record<string, string> {
  const channels = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));
  const [r, g, b] = channels;
  const lighter = channels
    .map((value) => Math.round(value + (255 - value) * 0.25))
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('');
  // Perceived brightness: light accents carry dark text.
  const light = (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.6;
  return {
    '--color-accent': hex,
    '--color-action': `#${lighter}`,
    '--color-accent-fg': light ? '#171614' : '#ffffff',
    '--color-accent-rgb': `${r} ${g} ${b}`,
    '--appearance-orb-accent': hex,
  };
}

export const APPEARANCE_TEXT_SIZES = ['small', 'default', 'large'] as const;
export type AppearanceTextSize = (typeof APPEARANCE_TEXT_SIZES)[number];

export const APPEARANCE_DENSITIES = ['comfortable', 'compact'] as const;
export type AppearanceDensity = (typeof APPEARANCE_DENSITIES)[number];

export interface AppearancePreferences {
  theme: AppearanceTheme;
  accent: AccentChoice;
  /** The owner's own accent as `#rrggbb`, kept while a preset is chosen. */
  customAccent: string;
  textSize: AppearanceTextSize;
  density: AppearanceDensity;
  reduceMotion: boolean;
  /** Do/Plan and the model picker on the chat input. Off keeps the input
   *  to attachments; the model lives in Settings. */
  advancedInput: boolean;
  /** Each tool step, its timing and results while the agent works. Off
   *  shows one plain line of what it is doing and then only the answer. */
  showWork: boolean;
}

export const DEFAULT_APPEARANCE_PREFERENCES: AppearancePreferences = {
  theme: 'dark',
  accent: 'pink',
  customAccent: DEFAULT_CUSTOM_ACCENT,
  textSize: 'default',
  density: 'comfortable',
  reduceMotion: false,
  advancedInput: false,
  showWork: false,
};

const STORAGE_KEY = 'anima.appearance';
/** Fired in this tab when the preferences change. */
export const APPEARANCE_CHANGED = 'anima:appearance';
const CHANGED = APPEARANCE_CHANGED;
let remembered: AppearancePreferences | null = null;
let storageWriteFailed = false;
let cachedSerialized: string | null | undefined;
let cachedPreferences = DEFAULT_APPEARANCE_PREFERENCES;

function isOneOf<T extends string>(value: unknown, values: readonly T[]): value is T {
  return typeof value === 'string' && values.includes(value as T);
}

function sanitize(value: unknown): AppearancePreferences {
  if (typeof value !== 'object' || value === null) {
    return DEFAULT_APPEARANCE_PREFERENCES;
  }
  const stored = value as Partial<AppearancePreferences>;
  return {
    theme: isOneOf(stored.theme, APPEARANCE_THEMES)
      ? stored.theme
      : DEFAULT_APPEARANCE_PREFERENCES.theme,
    accent:
      stored.accent === 'custom' || isOneOf(stored.accent, APPEARANCE_ACCENTS)
        ? stored.accent
        : DEFAULT_APPEARANCE_PREFERENCES.accent,
    customAccent: isHexColour(stored.customAccent)
      ? stored.customAccent.toLowerCase()
      : DEFAULT_APPEARANCE_PREFERENCES.customAccent,
    textSize: isOneOf(stored.textSize, APPEARANCE_TEXT_SIZES)
      ? stored.textSize
      : DEFAULT_APPEARANCE_PREFERENCES.textSize,
    density: isOneOf(stored.density, APPEARANCE_DENSITIES)
      ? stored.density
      : DEFAULT_APPEARANCE_PREFERENCES.density,
    reduceMotion:
      typeof stored.reduceMotion === 'boolean'
        ? stored.reduceMotion
        : DEFAULT_APPEARANCE_PREFERENCES.reduceMotion,
    advancedInput:
      typeof stored.advancedInput === 'boolean'
        ? stored.advancedInput
        : DEFAULT_APPEARANCE_PREFERENCES.advancedInput,
    showWork:
      typeof stored.showWork === 'boolean'
        ? stored.showWork
        : DEFAULT_APPEARANCE_PREFERENCES.showWork,
  };
}

export function readAppearancePreferences(): AppearancePreferences {
  if (storageWriteFailed && remembered) return remembered;
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved !== null) {
      if (saved === cachedSerialized) return cachedPreferences;
      cachedSerialized = saved;
      cachedPreferences = sanitize(JSON.parse(saved));
      return cachedPreferences;
    }
  } catch {
    // Private browsing or malformed older data falls back to defaults.
  }
  return remembered ?? DEFAULT_APPEARANCE_PREFERENCES;
}

export function saveAppearancePreferences(
  patch: Partial<AppearancePreferences>,
): void {
  remembered = sanitize({ ...readAppearancePreferences(), ...patch });
  const serialized = JSON.stringify(remembered);
  cachedSerialized = serialized;
  cachedPreferences = remembered;
  try {
    localStorage.setItem(STORAGE_KEY, serialized);
    storageWriteFailed = false;
  } catch {
    storageWriteFailed = true;
  }
  window.dispatchEvent(new Event(CHANGED));
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener(CHANGED, onChange);
  window.addEventListener('storage', onChange);
  return () => {
    window.removeEventListener(CHANGED, onChange);
    window.removeEventListener('storage', onChange);
  };
}

export function useAppearancePreferences(): [
  AppearancePreferences,
  (patch: Partial<AppearancePreferences>) => void,
] {
  const preferences = useSyncExternalStore(
    subscribe,
    readAppearancePreferences,
    () => DEFAULT_APPEARANCE_PREFERENCES,
  );
  return [preferences, saveAppearancePreferences];
}
