import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { MicIcon, PauseIcon, PlayIcon } from '../icons';

type MicState =
  | 'idle'
  | 'starting'
  | 'live'
  | 'blocked'
  | 'missing'
  | 'unsupported';

const METER_TICKS = 38;
const LIT_FULL = 20;

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

/** A small stable bar shape per voice, so each row reads as its own sound. */
function waveHeights(seed: string, count = 16): number[] {
  let state = 0;
  for (const char of seed) state = (state * 31 + char.charCodeAt(0)) >>> 0;
  return Array.from({ length: count }, () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return 2 + ((state >>> 24) % 12);
  });
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
  const speakingRef = useRef(onSpeakingChange);
  speakingRef.current = onSpeakingChange;

  useEffect(
    () => () => {
      speechSynthesisOrNull()?.cancel();
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

  return voices.length ? (
    <div
      role="radiogroup"
      aria-label="Voice for reading replies aloud"
      className="onboarding__options"
    >
      {voices.map((voice) => {
        const active = playing === voice.voiceURI;
        return (
          <div
            key={voice.voiceURI}
            className="onboarding__choice"
            data-checked={voiceURI === voice.voiceURI || undefined}
          >
            <div className="onboarding__option">
              <button
                type="button"
                className="onboarding__play"
                aria-label={`${active ? 'Stop' : 'Play'} ${voice.name}`}
                onClick={() => preview(voice)}
              >
                {active ? <PauseIcon size={14} /> : <PlayIcon size={14} />}
              </button>
              <label className="onboarding__option-text">
                <input
                  type="radio"
                  name="onboarding-voice"
                  value={voice.voiceURI}
                  checked={voiceURI === voice.voiceURI}
                  onChange={() => onVoiceChange(voice.voiceURI)}
                />
                <span className="onboarding__option-title onboarding__option-title--clip">
                  {voice.name}
                </span>
                <span className="onboarding__option-desc">{voice.lang}</span>
              </label>
              <span
                className="ob-wave ob-wave--small"
                data-live={active || undefined}
                aria-hidden="true"
              >
                {waveHeights(voice.voiceURI).map((height, index) => (
                  <i
                    key={index}
                    style={{ '--h': height, '--i': index } as CSSProperties}
                  />
                ))}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  ) : (
    <p className="onboarding__hint">
      This browser has no voices to read replies aloud. You can still type and
      talk.
    </p>
  );
}

/** The microphone check: a card on the stage with a live level. */
export function MicCheck() {
  const [mic, setMic] = useState<MicState>('idle');
  const [level, setLevel] = useState(0);
  const [device, setDevice] = useState('');
  const micRef = useRef<{
    stream: MediaStream;
    context: AudioContext;
    frame: number;
  } | null>(null);

  useEffect(
    () => () => {
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
      setDevice(stream.getAudioTracks()[0]?.label ?? '');
      setMic('live');
    } catch (caught) {
      const name = caught instanceof DOMException ? caught.name : '';
      setMic(name === 'NotFoundError' ? 'missing' : 'blocked');
    }
  };

  const lit = Math.min(METER_TICKS, Math.round((level * METER_TICKS) / 0.25));
  const status: Record<MicState, string> = {
    idle: 'Check that anima can hear you.',
    starting: 'Asking for your microphone…',
    live: lit > 0 ? 'Hearing you.' : 'Listening. Say something.',
    blocked: 'Microphone access is blocked. Allow it in your browser to talk.',
    missing: 'No microphone found.',
    unsupported: 'This browser can’t use a microphone here.',
  };

  return (
    <div
      className="ob-float ob-mic"
      style={{ '--x': 760, '--y': 640, '--w': 340 } as CSSProperties}
    >
      <div className="ob-mic__row">
        <span className="ob-mic__device">
          <MicIcon size={14} aria-hidden="true" />
          <span>{device || 'Microphone'}</span>
        </span>
        <button
          type="button"
          className="ob-mic__check"
          disabled={mic === 'starting' || mic === 'live'}
          onClick={() => void checkMicrophone()}
        >
          {mic === 'live' ? 'On' : 'Check'}
        </button>
      </div>
      <div
        className="ob-mic__level"
        role="meter"
        aria-label="Microphone level"
        aria-valuemin={0}
        aria-valuemax={METER_TICKS}
        aria-valuenow={lit}
      >
        {Array.from({ length: METER_TICKS }, (_, index) => (
          <i key={index} data-on={index < lit} data-hot={index >= LIT_FULL} />
        ))}
      </div>
      <p aria-live="polite" className="ob-mic__status">
        {status[mic]}
      </p>
    </div>
  );
}
