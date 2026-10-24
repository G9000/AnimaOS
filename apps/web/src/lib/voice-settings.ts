import { useEffect, useState } from 'react';

export interface VoiceSettings {
  pushToTalk: boolean;
  readRepliesAfterSpeech: boolean;
}

const STORAGE_KEY = 'anima.voiceSettings';
const SPOKEN_PREFIX = 'anima.spoken-input:';
const CHANGE_EVENT = 'anima:voice-settings-changed';
const DEFAULTS: VoiceSettings = {
  pushToTalk: false,
  readRepliesAfterSpeech: false,
};

export function readVoiceSettings(): VoiceSettings {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    if (typeof value !== 'object' || value === null) return DEFAULTS;
    const settings = value as Partial<VoiceSettings>;
    return {
      pushToTalk: settings.pushToTalk === true,
      readRepliesAfterSpeech: settings.readRepliesAfterSpeech === true,
    };
  } catch {
    return DEFAULTS;
  }
}

export function saveVoiceSettings(patch: Partial<VoiceSettings>): void {
  const next = { ...readVoiceSettings(), ...patch };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // In-memory preference remains usable for this app session.
  }
  window.dispatchEvent(new CustomEvent(CHANGE_EVENT));
}

export function useVoiceSettings(): VoiceSettings {
  const [settings, setSettings] = useState(readVoiceSettings);
  useEffect(() => {
    const update = () => setSettings(readVoiceSettings());
    const storage = (event: StorageEvent) => {
      if (event.key === STORAGE_KEY) update();
    };
    window.addEventListener(CHANGE_EVENT, update);
    window.addEventListener('storage', storage);
    return () => {
      window.removeEventListener(CHANGE_EVENT, update);
      window.removeEventListener('storage', storage);
    };
  }, []);
  return settings;
}

export function markSpokenInput(conversationKey: string | null | undefined): void {
  if (!conversationKey) return;
  try {
    sessionStorage.setItem(
      `${SPOKEN_PREFIX}${conversationKey}`,
      String(Date.now()),
    );
  } catch {
    // Read-aloud can remain manual when browser storage is unavailable.
  }
}

export function consumeRecentSpokenInput(
  conversationKey: string | null | undefined,
): boolean {
  if (!conversationKey) return false;
  try {
    const key = `${SPOKEN_PREFIX}${conversationKey}`;
    const timestamp = Number(sessionStorage.getItem(key));
    sessionStorage.removeItem(key);
    return Number.isFinite(timestamp) && Date.now() - timestamp < 30 * 60_000;
  } catch {
    return false;
  }
}
