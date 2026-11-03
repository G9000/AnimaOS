import { afterEach, describe, expect, it } from 'vitest';

import {
  accentHex,
  customAccentVariables,
  DEFAULT_APPEARANCE_PREFERENCES,
  readAppearancePreferences,
  saveAppearancePreferences,
} from './appearance-preferences';

afterEach(() => saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES));

describe('a custom accent', () => {
  it('sets the accent, a lighter action colour, and readable text on it', () => {
    expect(customAccentVariables('#14b8a6')).toEqual({
      '--color-accent': '#14b8a6',
      '--color-action': '#4fcabc',
      '--color-accent-fg': '#ffffff',
      '--color-accent-rgb': '20 184 166',
      '--appearance-orb-accent': '#14b8a6',
    });
    expect(customAccentVariables('#f5e6a3')['--color-accent-fg']).toBe(
      '#171614',
    );
  });

  it('is the accent in effect only while custom is chosen', () => {
    expect(accentHex({ accent: 'custom', customAccent: '#14b8a6' })).toBe(
      '#14b8a6',
    );
    expect(accentHex({ accent: 'sage', customAccent: '#14b8a6' })).toBe(
      '#7FCB8A',
    );
  });

  it('falls back to the default when a stored colour is not one', () => {
    localStorage.setItem(
      'anima.appearance',
      JSON.stringify({ accent: 'custom', customAccent: 'teal' }),
    );
    expect(readAppearancePreferences()).toMatchObject({
      accent: 'custom',
      customAccent: '#e94dab',
    });
    localStorage.clear();
  });
});
