import {
  APPEARANCE_ACCENTS,
  APPEARANCE_DENSITIES,
  APPEARANCE_TEXT_SIZES,
  APPEARANCE_THEMES,
  isHexColour,
  readAppearancePreferences,
  saveAppearancePreferences,
  type AccentChoice,
  type AppearanceDensity,
  type AppearanceTextSize,
  type AppearanceTheme,
} from './appearance-preferences';
import { daemon } from './daemon-api';

/** How the owner's console looks, as the daemon keeps it (`/settings/look`)
 *  so asking in chat can change it. This browser keeps a copy so the page
 *  draws at once; the daemon's record wins when the two differ. */
export interface OwnerLook {
  theme: AppearanceTheme;
  accent: AccentChoice;
  /** `#rrggbb`, used when `accent` is `custom`. */
  customAccent: string;
  textSize: AppearanceTextSize;
  density: AppearanceDensity;
  reduceMotion: boolean;
  advancedInput: boolean;
  showWork: boolean;
}

export const LOOK_KEYS = [
  'theme',
  'accent',
  'customAccent',
  'textSize',
  'density',
  'reduceMotion',
  'advancedInput',
  'showWork',
] as const satisfies readonly (keyof OwnerLook)[];

/** The look this browser shows now. */
export function localLook(): OwnerLook {
  return readAppearancePreferences();
}

export function sameLook(a: OwnerLook, b: OwnerLook): boolean {
  return LOOK_KEYS.every((key) => a[key] === b[key]);
}

function isOneOf<T extends string>(value: unknown, values: readonly T[]): value is T {
  return typeof value === 'string' && values.includes(value as T);
}

/** A look from the daemon or a tool result, or null when anything in it is
 *  not a choice this console knows. */
export function parseLook(value: unknown): OwnerLook | null {
  if (typeof value !== 'object' || value === null) return null;
  const look = value as Record<string, unknown>;
  if (
    !isOneOf(look.theme, APPEARANCE_THEMES) ||
    !(look.accent === 'custom' || isOneOf(look.accent, APPEARANCE_ACCENTS)) ||
    !isHexColour(look.customAccent) ||
    !isOneOf(look.textSize, APPEARANCE_TEXT_SIZES) ||
    !isOneOf(look.density, APPEARANCE_DENSITIES) ||
    typeof look.reduceMotion !== 'boolean' ||
    typeof look.advancedInput !== 'boolean' ||
    typeof look.showWork !== 'boolean'
  )
    return null;
  return {
    theme: look.theme,
    accent: look.accent,
    customAccent: look.customAccent.toLowerCase(),
    textSize: look.textSize,
    density: look.density,
    reduceMotion: look.reduceMotion,
    advancedInput: look.advancedInput,
    showWork: look.showWork,
  };
}

let applying = false;

/** True while a look from the daemon is being shown here: those changes are
 *  not sent back to it. */
export function applyingDaemonLook(): boolean {
  return applying;
}

/** Shows a look the daemon already holds. */
export function applyLook(look: OwnerLook): void {
  if (sameLook(look, localLook())) return;
  applying = true;
  try {
    saveAppearancePreferences(look);
  } finally {
    applying = false;
  }
}

/** Shows a look here and saves it with the daemon (an Undo). */
export async function changeLook(look: OwnerLook): Promise<void> {
  applyLook(look);
  await daemon.setLook(look);
}

/** Loads the daemon's look and shows it. The first time, when the daemon
 *  has none, this browser's choices become the owner's look. */
export async function syncLookFromDaemon(): Promise<void> {
  const saved = parseLook(await daemon.look());
  if (saved) applyLook(saved);
  else await daemon.setLook(localLook());
}

/** A `change_look` call's result: the look it set, the one before, and the
 *  names of what changed. Null for a read, a failure, or anything else. */
export interface LookChange {
  look: OwnerLook;
  previous: OwnerLook;
  changed: (keyof OwnerLook)[];
}

export function lookChangeResult(
  name: string,
  status: string,
  result: string | null,
): LookChange | null {
  if (name !== 'change_look' || status !== 'success' || !result) return null;
  try {
    const parsed = JSON.parse(result) as Record<string, unknown>;
    const look = parseLook(parsed.look);
    const previous = parseLook(parsed.previous);
    const changed = Array.isArray(parsed.changed)
      ? parsed.changed.filter((key): key is keyof OwnerLook =>
          (LOOK_KEYS as readonly unknown[]).includes(key),
        )
      : [];
    return look && previous && changed.length > 0
      ? { look, previous, changed }
      : null;
  } catch {
    return null;
  }
}

const THEME_NAMES: Record<AppearanceTheme, string> = {
  dark: 'Dark theme',
  light: 'Light theme',
  system: 'Theme matches your computer',
};

function capitalized(value: string): string {
  return value[0].toUpperCase() + value.slice(1);
}

/** What a change did, in a few words: "Advanced view · Light theme". */
export function describeLookChange({ look, changed }: LookChange): string {
  const parts: string[] = [];
  const view =
    changed.includes('showWork') &&
    changed.includes('advancedInput') &&
    look.showWork === look.advancedInput;
  if (view) parts.push(look.showWork ? 'Advanced view' : 'Simple view');
  for (const key of changed) {
    if (view && (key === 'showWork' || key === 'advancedInput')) continue;
    if (key === 'theme') parts.push(THEME_NAMES[look.theme]);
    if (key === 'accent' || key === 'customAccent') {
      // A custom colour names itself once, however both fields moved.
      if (key === 'customAccent' && changed.includes('accent')) continue;
      if (key === 'customAccent' && look.accent !== 'custom') continue;
      parts.push(
        look.accent === 'custom'
          ? `Accent ${look.customAccent}`
          : `${capitalized(look.accent)} accent`,
      );
    }
    if (key === 'textSize')
      parts.push(look.textSize === 'default' ? 'Default text size' : `${capitalized(look.textSize)} text`);
    if (key === 'density') parts.push(`${capitalized(look.density)} spacing`);
    if (key === 'reduceMotion')
      parts.push(look.reduceMotion ? 'Reduced motion' : 'Full motion');
    if (key === 'showWork')
      parts.push(look.showWork ? 'Showing work' : 'Work hidden');
    if (key === 'advancedInput')
      parts.push(look.advancedInput ? 'Advanced input on' : 'Advanced input off');
  }
  return parts.join(' · ');
}
