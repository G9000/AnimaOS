const STORAGE_KEY = 'anima.readAloudVoice';

export function readPreferredVoice(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function savePreferredVoice(voiceURI: string | null): void {
  try {
    if (voiceURI) localStorage.setItem(STORAGE_KEY, voiceURI);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* Storage can be unavailable in private windows; reading aloud still works. */
  }
}

export function preferredVoice(
  synthesis: Pick<SpeechSynthesis, 'getVoices'>,
): SpeechSynthesisVoice | null {
  const uri = readPreferredVoice();
  if (!uri || typeof synthesis.getVoices !== 'function') return null;
  return synthesis.getVoices().find((voice) => voice.voiceURI === uri) ?? null;
}
