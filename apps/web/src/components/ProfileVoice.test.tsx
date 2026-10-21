import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { readPreferredVoice } from '../lib/voice-preference';
import { ProfileVoice } from './ProfileVoice';

const voices = [
  { voiceURI: 'ember', name: 'Ember', lang: 'en-US' },
  { voiceURI: 'lumen', name: 'Lumen', lang: 'en-GB' },
] as SpeechSynthesisVoice[];

function stubSpeech() {
  const speak = vi.fn();
  const cancel = vi.fn();
  vi.stubGlobal('speechSynthesis', {
    getVoices: () => voices,
    speak,
    cancel,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal(
    'SpeechSynthesisUtterance',
    class {
      voice: SpeechSynthesisVoice | null = null;
      onend: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor(public text: string) {}
    },
  );
  return { speak, cancel };
}

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe('ProfileVoice', () => {
  it('keeps the chosen voice for reading replies aloud', async () => {
    stubSpeech();
    const user = userEvent.setup();
    render(<ProfileVoice agentName="Nova" />);
    expect(screen.getByText('This computer’s default voice')).toBeVisible();
    await user.selectOptions(screen.getByLabelText('Voice'), 'lumen');
    expect(readPreferredVoice()).toBe('lumen');
    expect(screen.getByText('Lumen · en-GB')).toBeVisible();
    await user.selectOptions(screen.getByLabelText('Voice'), '');
    expect(readPreferredVoice()).toBeNull();
  });

  it('previews the chosen voice, and stops the preview', async () => {
    const { speak, cancel } = stubSpeech();
    const user = userEvent.setup();
    render(<ProfileVoice agentName="Nova" />);
    await user.selectOptions(screen.getByLabelText('Voice'), 'ember');
    await user.click(screen.getByRole('button', { name: 'Preview' }));
    expect(speak).toHaveBeenCalledOnce();
    const utterance = speak.mock.calls[0][0];
    expect(utterance.text).toBe('Hi, this is how Nova sounds.');
    expect(utterance.voice).toBe(voices[0]);
    await user.click(screen.getByRole('button', { name: 'Stop' }));
    expect(cancel).toHaveBeenCalled();
  });

  it('is not shown where the browser has no voices', () => {
    vi.stubGlobal('speechSynthesis', undefined);
    const { container } = render(<ProfileVoice agentName="Nova" />);
    expect(container).toBeEmptyDOMElement();
  });
});
