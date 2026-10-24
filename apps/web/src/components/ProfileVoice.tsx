import { useEffect, useState } from 'react';
import {
  readPreferredVoice,
  savePreferredVoice,
} from '../lib/voice-preference';
import { saveVoiceSettings, useVoiceSettings } from '../lib/voice-settings';
import { PenIcon } from './PenIcon';
import { StopIcon } from './icons';

function synthesisOrNull(): SpeechSynthesis | null {
  if (
    typeof window === 'undefined' ||
    typeof window.speechSynthesis?.getVoices !== 'function' ||
    typeof window.SpeechSynthesisUtterance !== 'function'
  )
    return null;
  return window.speechSynthesis;
}

/**
 * The voice replies are read aloud in: one of this computer's voices, kept in
 * this browser. Nothing here asks the daemon.
 */
export function ProfileVoice({
  agentName,
  inheritedFrom,
}: {
  agentName: string;
  inheritedFrom?: string;
}) {
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>(
    () => synthesisOrNull()?.getVoices() ?? [],
  );
  const [chosen, setChosen] = useState(() => readPreferredVoice() ?? '');
  const [playing, setPlaying] = useState(false);
  const voiceSettings = useVoiceSettings();

  useEffect(() => {
    const synthesis = synthesisOrNull();
    if (!synthesis) return;
    const update = () => setVoices(synthesis.getVoices());
    update();
    synthesis.addEventListener?.('voiceschanged', update);
    return () => {
      synthesis.removeEventListener?.('voiceschanged', update);
      synthesis.cancel();
    };
  }, []);

  const synthesis = synthesisOrNull();
  const voice = voices.find((item) => item.voiceURI === chosen) ?? null;
  const defaultVoice = voices.find((item) => item.default) ?? null;
  const voiceSummary = voice
    ? `${voice.name} · ${voice.lang}`
    : defaultVoice
      ? `${defaultVoice.name} · ${defaultVoice.lang}`
      : 'This computer’s default voice';

  const preview = () => {
    if (!synthesis) return;
    if (playing) {
      synthesis.cancel();
      setPlaying(false);
      return;
    }
    synthesis.cancel();
    const utterance = new window.SpeechSynthesisUtterance(
      `Hi, this is how ${agentName} sounds.`,
    );
    if (voice) utterance.voice = voice;
    const done = () => setPlaying(false);
    utterance.onend = done;
    utterance.onerror = done;
    setPlaying(true);
    synthesis.speak(utterance);
  };

  return (
    <>
      <div className="settings-item profile-setting-row">
        <span className="settings-item-icon" aria-hidden="true">
          <PenIcon name="speaker-high-fill" size={16} />
        </span>
        <div className="settings-item-text">
          <label htmlFor="settings-voice">Voice</label>
          {inheritedFrom ? (
          <small>
            Same as {inheritedFrom} · {voiceSummary}
          </small>
          ) : synthesis && voices.length > 0 ? (
            <select
              id="settings-voice"
              className="profile-voice-select"
              value={voice ? chosen : ''}
              onChange={(event) => {
                setChosen(event.target.value);
                savePreferredVoice(event.target.value || null);
              }}
            >
              <option value="">
                {defaultVoice
                  ? `${defaultVoice.name} · ${defaultVoice.lang}`
                  : 'This computer’s default voice'}
              </option>
              {voices.map((item) => (
                <option key={item.voiceURI} value={item.voiceURI}>
                  {item.name} · {item.lang}
                </option>
              ))}
            </select>
          ) : (
            <small>
              {synthesis
                ? 'This computer’s default voice'
                : 'Speech playback is unavailable in this browser'}
            </small>
          )}
        </div>
        {!inheritedFrom && (
          <button
            type="button"
            className="settings-item-action profile-voice-preview"
            disabled={!synthesis}
            onClick={preview}
          >
            {playing ? (
              <StopIcon size={12} />
            ) : (
              <PenIcon name="play-fill" size={12} />
            )}
            {playing ? 'Stop' : 'Preview'}
          </button>
        )}
      </div>
      {!inheritedFrom && (
        <div className="settings-item settings-preference-row profile-setting-row">
          <span className="settings-item-icon" aria-hidden="true">
            <PenIcon name="microphone-fill" size={16} />
          </span>
          <span className="settings-item-text">
            <strong>Push to talk</strong>
            <small>Hold to talk, let go to send</small>
          </span>
          <button
            type="button"
            className="settings-preference-value profile-keycap"
            aria-pressed={voiceSettings.pushToTalk}
            aria-label="Push to talk"
            onClick={() =>
              saveVoiceSettings({ pushToTalk: !voiceSettings.pushToTalk })
            }
          >
            Space
          </button>
        </div>
      )}
      {!inheritedFrom && (
        <label className="settings-item settings-preference-row profile-setting-row">
          <span className="settings-item-icon" aria-hidden="true">
            <PenIcon name="speaker-simple-high-fill" size={16} />
          </span>
          <span className="settings-item-text">
            <strong>Read replies aloud</strong>
            <small>Only when you spoke first</small>
          </span>
          <input
            type="checkbox"
            role="switch"
            checked={voiceSettings.readRepliesAfterSpeech}
            onChange={(event) =>
              saveVoiceSettings({
                readRepliesAfterSpeech: event.target.checked,
              })
            }
            aria-label="Read replies aloud, only when you spoke first"
          />
        </label>
      )}
    </>
  );
}
