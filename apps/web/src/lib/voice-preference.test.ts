import { afterEach, describe, expect, it } from 'vitest';
import {
  preferredVoice,
  readPreferredVoice,
  savePreferredVoice,
} from './voice-preference';

const voices = [
  { voiceURI: 'ember', name: 'Ember' },
  { voiceURI: 'slate', name: 'Slate' },
] as SpeechSynthesisVoice[];

afterEach(() => localStorage.clear());

describe('voice preference', () => {
  it('returns the saved voice when the browser still offers it', () => {
    savePreferredVoice('slate');
    expect(readPreferredVoice()).toBe('slate');
    expect(preferredVoice({ getVoices: () => voices })?.name).toBe('Slate');
  });

  it('falls back to the default voice when none is saved or it disappeared', () => {
    expect(preferredVoice({ getVoices: () => voices })).toBeNull();
    savePreferredVoice('gone');
    expect(preferredVoice({ getVoices: () => voices })).toBeNull();
    savePreferredVoice(null);
    expect(readPreferredVoice()).toBeNull();
  });
});
