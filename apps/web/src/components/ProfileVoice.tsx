import { useEffect, useState } from 'react';
import {
  readPreferredVoice,
  savePreferredVoice,
} from '../lib/voice-preference';
import { PlayIcon, StopIcon } from './icons';

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
export function ProfileVoice({ agentName }: { agentName: string }) {
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>(
    () => synthesisOrNull()?.getVoices() ?? [],
  );
  const [chosen, setChosen] = useState(() => readPreferredVoice() ?? '');
  const [playing, setPlaying] = useState(false);

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
  if (!synthesis || voices.length === 0) return null;
  const voice = voices.find((item) => item.voiceURI === chosen) ?? null;

  const preview = () => {
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
    <div className="settings-item">
      <div className="settings-item-text">
        <label htmlFor="settings-voice">Voice</label>
        <small>
          {voice
            ? `${voice.name} · ${voice.lang}`
            : 'This computer’s default voice'}
        </small>
      </div>
      <select
        id="settings-voice"
        className="settings-item-select"
        value={voice ? chosen : ''}
        onChange={(event) => {
          setChosen(event.target.value);
          savePreferredVoice(event.target.value || null);
        }}
      >
        <option value="">System default</option>
        {voices.map((item) => (
          <option key={item.voiceURI} value={item.voiceURI}>
            {item.name}
          </option>
        ))}
      </select>
      <button type="button" className="settings-item-action" onClick={preview}>
        {playing ? <StopIcon size={12} /> : <PlayIcon size={12} />}
        {playing ? 'Stop' : 'Preview'}
      </button>
    </div>
  );
}
