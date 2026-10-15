import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  createMicrophoneCapture,
  isMicrophoneCaptureSupported,
  type CaptureError,
  type CaptureState,
  type MicrophoneCaptureController,
} from '../../lib/audio-capture';

const messages: Record<CaptureError, string> = {
  unsupported: 'Audio recording is unavailable in this browser.',
  busy: 'Another recording is still being cleaned up. Try again shortly.',
  permission_denied: 'Microphone access was denied.',
  device_unavailable: 'No microphone is available.',
  device_lost: 'The microphone became unavailable.',
  invalid_audio: 'The recording could not be read safely.',
  empty: 'No audio was captured.',
  capture_failed: 'Recording could not be completed.',
  cleanup_pending: 'Recording cleanup is still in progress.',
};

function formatDuration(durationMs: number): string {
  const seconds = Math.floor(durationMs / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

export function MicrophoneCapture({
  conversationKey,
  disabled,
  onAdd,
}: {
  conversationKey: string;
  disabled: boolean;
  onAdd: (files: readonly File[]) => void;
}) {
  const [state, setState] = useState<CaptureState>({ status: 'idle' });
  const [clock, setClock] = useState(Date.now());
  const controllerRef = useRef<MicrophoneCaptureController | null>(null);
  const activeRef = useRef(false);
  const scopeRef = useRef({ conversationKey, disabled, onAdd });
  const boundaryRef = useRef({ conversationKey, disabled, generation: 0 });
  if (
    boundaryRef.current.conversationKey !== conversationKey ||
    boundaryRef.current.disabled !== disabled
  ) {
    boundaryRef.current = {
      conversationKey,
      disabled,
      generation: boundaryRef.current.generation + 1,
    };
  }
  const generation = boundaryRef.current.generation;
  scopeRef.current = { conversationKey, disabled, onAdd };

  useLayoutEffect(() => {
    let live = true;
    activeRef.current = false;
    setState({ status: 'idle' });
    if (disabled || !conversationKey)
      return () => {
        live = false;
      };
    const controller = createMicrophoneCapture((next) => {
      if (
        live &&
        activeRef.current &&
        controllerRef.current === controller &&
        boundaryRef.current.generation === generation &&
        !scopeRef.current.disabled
      )
        setState(next);
    });
    controllerRef.current = controller;
    activeRef.current = true;
    return () => {
      live = false;
      activeRef.current = false;
      controller.cancel();
      controller.dispose();
      if (controllerRef.current === controller) controllerRef.current = null;
    };
  }, [conversationKey, disabled, generation]);

  useEffect(() => {
    if (state.status !== 'recording') return;
    const timer = window.setInterval(() => setClock(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [state.status]);

  const canAct = useCallback(() => {
    const scope = scopeRef.current;
    return (
      activeRef.current &&
      controllerRef.current !== null &&
      boundaryRef.current.generation === generation &&
      !scope.disabled &&
      scope.conversationKey.length > 0 &&
      scope.conversationKey === conversationKey
    );
  }, [conversationKey, generation]);

  const record = () => {
    if (!canAct()) return;
    if (!isMicrophoneCaptureSupported()) {
      setState({ status: 'error', reason: 'unsupported' });
      return;
    }
    controllerRef.current?.start();
  };
  const add = () => {
    if (!canAct() || state.status !== 'ready') return;
    const controller = controllerRef.current;
    const file = controller?.takeRecording();
    if (!file || !canAct() || controllerRef.current !== controller) return;
    setState({ status: 'idle' });
    scopeRef.current.onAdd([file]);
  };
  const discard = () => {
    if (!canAct()) return;
    controllerRef.current?.cancel();
    setState({ status: 'idle' });
  };

  const elapsed =
    state.status === 'recording'
      ? formatDuration(Math.max(0, clock - state.startedAt))
      : null;

  return (
    <section className="mt-2 px-2" aria-label="Audio recording">
      <p className="mb-2 text-xs text-ink-3">
        Recordings stay on this device until you choose Add recording.
      </p>
      {(state.status === 'idle' || state.status === 'error') && (
        <button
          type="button"
          className="studio-tool-button"
          onClick={record}
          disabled={disabled}
        >
          Record audio
        </button>
      )}
      {(state.status === 'requesting' || state.status === 'stopping') && (
        <div className="flex gap-2">
          <span role="status">
            {state.status === 'requesting'
              ? 'Requesting microphone…'
              : 'Finishing recording…'}
          </span>
          <button
            type="button"
            className="studio-tool-button"
            onClick={discard}
            disabled={disabled}
          >
            Cancel recording
          </button>
        </div>
      )}
      {state.status === 'recording' && (
        <div className="flex items-center gap-2">
          <span role="status">Recording · {elapsed}</span>
          <button
            type="button"
            className="studio-tool-button"
            onClick={() => canAct() && controllerRef.current?.stop()}
            disabled={disabled}
          >
            Stop recording
          </button>
          <button
            type="button"
            className="studio-tool-button"
            onClick={discard}
            disabled={disabled}
          >
            Cancel recording
          </button>
        </div>
      )}
      {state.status === 'ready' && (
        <div className="flex flex-wrap items-center gap-2">
          <span>
            {state.fileName} · {formatDuration(state.durationMs)} ·{' '}
            {state.sizeBytes} bytes
          </span>
          {state.limitReached && (
            <span role="status">Recording reached the 2 minute limit.</span>
          )}
          <button
            type="button"
            className="studio-tool-button"
            onClick={add}
            disabled={disabled}
          >
            Add recording
          </button>
          <button
            type="button"
            className="studio-tool-button"
            onClick={discard}
            disabled={disabled}
          >
            Discard recording
          </button>
        </div>
      )}
      {state.status === 'error' && (
        <p role="alert" className="text-xs text-danger">
          {messages[state.reason]}
        </p>
      )}
    </section>
  );
}
