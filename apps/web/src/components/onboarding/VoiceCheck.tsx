import { useEffect, useRef, useState } from 'react';

type MicState = 'idle' | 'starting' | 'live' | 'blocked' | 'missing' | 'unsupported';

const METER_SEGMENTS = 10;

function speechSynthesisOrNull(): SpeechSynthesis | null {
  if (
    typeof window === 'undefined' ||
    typeof window.speechSynthesis?.getVoices !== 'function' ||
    typeof window.SpeechSynthesisUtterance !== 'function'
  )
    return null;
  return window.speechSynthesis;
}

function useBrowserVoices(): SpeechSynthesisVoice[] {
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>(
    () => speechSynthesisOrNull()?.getVoices() ?? [],
  );
  useEffect(() => {
    const synthesis = speechSynthesisOrNull();
    if (!synthesis) return;
    const update = () => setVoices(synthesis.getVoices());
    update();
    synthesis.addEventListener?.('voiceschanged', update);
    return () => synthesis.removeEventListener?.('voiceschanged', update);
  }, []);
  return voices;
}

function shortlist(voices: SpeechSynthesisVoice[]): SpeechSynthesisVoice[] {
  const language = (
    typeof navigator === 'undefined' ? 'en' : navigator.language || 'en'
  )
    .slice(0, 2)
    .toLowerCase();
  const local = voices.filter((voice) =>
    voice.lang.toLowerCase().startsWith(language),
  );
  return (local.length ? local : voices).slice(0, 4);
}

export interface VoiceCheckProps {
  voiceURI: string | null;
  onVoiceChange(voiceURI: string | null): void;
  sample: string;
  onSpeakingChange?(voiceName: string | null): void;
}

export function VoiceCheck({
  voiceURI,
  onVoiceChange,
  sample,
  onSpeakingChange,
}: VoiceCheckProps) {
  const voices = shortlist(useBrowserVoices());
  const [playing, setPlaying] = useState<string | null>(null);
  const [mic, setMic] = useState<MicState>('idle');
  const [level, setLevel] = useState(0);
  const micRef = useRef<{
    stream: MediaStream;
    context: AudioContext;
    frame: number;
  } | null>(null);
  const speakingRef = useRef(onSpeakingChange);
  speakingRef.current = onSpeakingChange;

  useEffect(
    () => () => {
      speechSynthesisOrNull()?.cancel();
      const owned = micRef.current;
      micRef.current = null;
      if (owned) {
        cancelAnimationFrame(owned.frame);
        owned.stream.getTracks().forEach((track) => track.stop());
        void owned.context.close().catch(() => undefined);
      }
    },
    [],
  );

  const preview = (voice: SpeechSynthesisVoice) => {
    const synthesis = speechSynthesisOrNull();
    if (!synthesis) return;
    synthesis.cancel();
    if (playing === voice.voiceURI) {
      setPlaying(null);
      speakingRef.current?.(null);
      return;
    }
    const utterance = new window.SpeechSynthesisUtterance(sample);
    utterance.voice = voice;
    const done = () => {
      setPlaying((current) => (current === voice.voiceURI ? null : current));
      speakingRef.current?.(null);
    };
    utterance.onend = done;
    utterance.onerror = done;
    setPlaying(voice.voiceURI);
    speakingRef.current?.(voice.name);
    onVoiceChange(voice.voiceURI);
    synthesis.speak(utterance);
  };

  const checkMicrophone = async () => {
    if (micRef.current || mic === 'starting') return;
    if (
      typeof navigator === 'undefined' ||
      typeof navigator.mediaDevices?.getUserMedia !== 'function' ||
      typeof window.AudioContext !== 'function'
    ) {
      setMic('unsupported');
      return;
    }
    setMic('starting');
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const context = new window.AudioContext();
      const analyser = context.createAnalyser();
      analyser.fftSize = 512;
      context.createMediaStreamSource(stream).connect(analyser);
      const samples = new Uint8Array(analyser.fftSize);
      const tick = () => {
        analyser.getByteTimeDomainData(samples);
        let sum = 0;
        for (const value of samples) sum += ((value - 128) / 128) ** 2;
        setLevel(Math.sqrt(sum / samples.length));
        if (micRef.current) micRef.current.frame = requestAnimationFrame(tick);
      };
      micRef.current = { stream, context, frame: 0 };
      micRef.current.frame = requestAnimationFrame(tick);
      setMic('live');
    } catch (caught) {
      const name = caught instanceof DOMException ? caught.name : '';
      setMic(name === 'NotFoundError' ? 'missing' : 'blocked');
    }
  };

  const lit = Math.min(METER_SEGMENTS, Math.round(level * 40));
  const micStatus: Record<MicState, string> = {
    idle: 'Check that anima can hear you.',
    starting: 'Asking for your microphone…',
    live: lit > 0 ? 'Hearing you.' : 'Listening. Say something.',
    blocked: 'Microphone access is blocked. Allow it in your browser to talk.',
    missing: 'No microphone found.',
    unsupported: 'This browser can’t use a microphone here.',
  };

  return (
    <>
      <div className="onboarding__section">
        <span id="onboarding-voice-label" className="onboarding__label">
          Voice for reading replies aloud
        </span>
        {voices.length ? (
          <div
            role="radiogroup"
            aria-labelledby="onboarding-voice-label"
            className="onboarding__options"
          >
            {voices.map((voice) => (
              <div key={voice.voiceURI} className="onboarding__option">
                <label className="onboarding__option-text">
                  <input
                    type="radio"
                    name="onboarding-voice"
                    value={voice.voiceURI}
                    checked={voiceURI === voice.voiceURI}
                    onChange={() => onVoiceChange(voice.voiceURI)}
                  />
                  <span className="onboarding__option-title">{voice.name}</span>
                  <span className="onboarding__option-desc">{voice.lang}</span>
                </label>
                <button
                  type="button"
                  className="onboarding__button onboarding__button--secondary onboarding__button--small"
                  aria-label={`${playing === voice.voiceURI ? 'Stop' : 'Play'} ${voice.name}`}
                  onClick={() => preview(voice)}
                >
                  {playing === voice.voiceURI ? 'Stop' : 'Play'}
                </button>
                <span className="onboarding__mark" aria-hidden="true" />
              </div>
            ))}
          </div>
        ) : (
          <p className="onboarding__hint">
            This browser has no voices to read replies aloud. You can still
            type and talk.
          </p>
        )}
      </div>
      <div className="onboarding__section">
        <span className="onboarding__label">Microphone</span>
        <div className="onboarding__row">
          <button
            type="button"
            className="onboarding__button onboarding__button--secondary"
            disabled={mic === 'starting' || mic === 'live'}
            onClick={() => void checkMicrophone()}
          >
            {mic === 'live' ? 'Microphone on' : 'Check microphone'}
          </button>
          <div
            className="onboarding__meter"
            role="meter"
            aria-label="Microphone level"
            aria-valuemin={0}
            aria-valuemax={METER_SEGMENTS}
            aria-valuenow={lit}
          >
            {Array.from({ length: METER_SEGMENTS }, (_, index) => (
              <span
                key={index}
                data-on={index < lit}
                data-hot={index >= 6}
              />
            ))}
          </div>
        </div>
        <p role="status" className="onboarding__hint">
          {micStatus[mic]}
        </p>
      </div>
    </>
  );
}
