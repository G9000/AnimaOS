import workletUrl from './pcm-capture.worklet.js?url&no-inline';

export type CaptureError =
  | 'unsupported'
  | 'busy'
  | 'permission_denied'
  | 'device_unavailable'
  | 'device_lost'
  | 'invalid_audio'
  | 'empty'
  | 'capture_failed'
  | 'cleanup_pending';

export type CaptureState =
  | { status: 'idle' }
  | { status: 'requesting' }
  | { status: 'recording'; startedAt: number }
  | { status: 'stopping' }
  | {
      status: 'ready';
      fileName: 'recording.wav';
      sizeBytes: number;
      durationMs: number;
      limitReached: boolean;
    }
  | { status: 'error'; reason: CaptureError };

export interface MicrophoneCaptureController {
  start(): void;
  stop(): void;
  cancel(): void;
  takeRecording(): File | null;
  dispose(): void;
}

const RATE = 16_000;
const MAX_FRAMES = 1_920_000;
const ALLOCATION_BYTES = 3_840_044;
const WALL_LIMIT_MS = 120_000;
const STOP_TIMEOUT_MS = 2_000;

type Candidate = {
  file: File;
  metadata: Extract<CaptureState, { status: 'ready' }>;
};
type Outcome = Candidate | null;
type OwnedTrack = { ended: (() => void) | null; stopped: boolean };
type Owner = {
  context: AudioContext | null;
  stream: MediaStream | null;
  source: MediaStreamAudioSourceNode | null;
  node: AudioWorkletNode | null;
  tracks: Map<MediaStreamTrack, OwnedTrack>;
  pending: number;
  permissionReady: boolean;
  moduleReady: boolean;
  resumeReady: boolean;
  retiring: boolean;
  terminal: boolean;
  closeStarted: boolean;
  closeDone: boolean;
  uncertain: boolean;
  cleanupReported: boolean;
  outcome: Outcome;
  watchdog: ReturnType<typeof setTimeout> | null;
  stopTimeout: ReturnType<typeof setTimeout> | null;
};

// A cancelled permission prompt is still a live acquisition. This lease is never
// released by a timeout, controller disposal, or a dropped promise reference.
let retainedOwner: Owner | null = null;

export function isMicrophoneCaptureSupported(): boolean {
  try {
    return (
      globalThis.isSecureContext === true &&
      typeof navigator !== 'undefined' &&
      typeof navigator.mediaDevices?.getUserMedia === 'function' &&
      typeof AudioContext === 'function' &&
      typeof AudioWorkletNode === 'function'
    );
  } catch {
    return false;
  }
}

function permissionError(error: unknown): CaptureError {
  const name =
    error && typeof error === 'object' && 'name' in error ? error.name : '';
  if (name === 'NotSupportedError') return 'unsupported';
  if (name === 'NotAllowedError' || name === 'SecurityError')
    return 'permission_denied';
  if (
    name === 'NotFoundError' ||
    name === 'NotReadableError' ||
    name === 'OverconstrainedError'
  )
    return 'device_unavailable';
  return 'capture_failed';
}

function candidateFromMessage(value: unknown): Candidate | null {
  if (!value || typeof value !== 'object') return null;
  const data = value as Record<string, unknown>;
  const { buffer, frames, usedBytes, limitReached } = data;
  if (
    data.version !== 1 ||
    data.type !== 'complete' ||
    !(buffer instanceof ArrayBuffer) ||
    buffer.byteLength !== ALLOCATION_BYTES ||
    typeof frames !== 'number' ||
    !Number.isSafeInteger(frames) ||
    frames <= 0 ||
    frames > MAX_FRAMES ||
    typeof usedBytes !== 'number' ||
    usedBytes !== 44 + frames * 2 ||
    usedBytes > 4 * 1024 * 1024 ||
    typeof limitReached !== 'boolean' ||
    limitReached !== (frames === MAX_FRAMES)
  )
    return null;

  const view = new DataView(buffer);
  if (
    view.getUint32(0, false) !== 0x52494646 || // RIFF
    view.getUint32(4, true) !== usedBytes - 8 ||
    view.getUint32(8, false) !== 0x57415645 || // WAVE
    view.getUint32(12, false) !== 0x666d7420 || // fmt
    view.getUint32(16, true) !== 16 ||
    view.getUint16(20, true) !== 1 ||
    view.getUint16(22, true) !== 1 ||
    view.getUint32(24, true) !== RATE ||
    view.getUint32(28, true) !== RATE * 2 ||
    view.getUint16(32, true) !== 2 ||
    view.getUint16(34, true) !== 16 ||
    view.getUint32(36, false) !== 0x64617461 || // data
    view.getUint32(40, true) !== frames * 2
  )
    return null;

  // The view does not copy. File snapshots just the checked prefix, avoiding a
  // second full-size ArrayBuffer allocation; the terminal buffer is not retained.
  const file = new File(
    [new Uint8Array(buffer, 0, usedBytes)],
    'recording.wav',
    { type: 'audio/wav' },
  );
  return {
    file,
    metadata: {
      status: 'ready',
      fileName: 'recording.wav',
      sizeBytes: usedBytes,
      durationMs: (frames * 1000) / RATE,
      limitReached,
    },
  };
}

export function createMicrophoneCapture(
  onState: (state: CaptureState) => void,
): MicrophoneCaptureController {
  let observer: typeof onState | null = onState;
  let state: CaptureState = { status: 'idle' };
  let owned: Owner | null = null;
  let recordingFile: File | null = null;
  let disposed = false;

  function publish(next: CaptureState) {
    state = next;
    // Observers cannot interrupt resource retirement by throwing.
    try {
      observer?.(next);
    } catch {
      /* No application/audio details are logged. */
    }
  }

  function live(owner: Owner) {
    return (
      !disposed && owned === owner && retainedOwner === owner && !owner.retiring
    );
  }

  function clearTimers(owner: Owner) {
    if (owner.watchdog !== null) clearTimeout(owner.watchdog);
    if (owner.stopTimeout !== null) clearTimeout(owner.stopTimeout);
    owner.watchdog = null;
    owner.stopTimeout = null;
  }

  function attempt(owner: Owner, action: () => void) {
    try {
      action();
    } catch {
      owner.uncertain = true;
    }
  }

  function settled(owner: Owner) {
    if (!owner.retiring) return;
    if (owner.uncertain) {
      owner.outcome = null;
      if (!owner.cleanupReported) {
        owner.cleanupReported = true;
        if (!disposed && owned === owner)
          publish({ status: 'error', reason: 'cleanup_pending' });
      }
      return;
    }
    if (owner.pending !== 0 || !owner.closeDone) return;
    if (retainedOwner === owner) retainedOwner = null;
    if (owned !== owner) return;
    owned = null;
    const outcome = owner.outcome;
    owner.outcome = null;
    owner.stream = null;
    owner.tracks.clear();
    if (!disposed && outcome) {
      recordingFile = outcome.file;
      publish(outcome.metadata);
    }
  }

  // Both handlers attach in the same call that obtains each browser promise.
  // Pending resume/module work remains counted even after context.close resolves.
  function observe<T>(
    owner: Owner,
    promise: Promise<T>,
    success: (value: T) => void,
    failure: (error: unknown) => void,
  ) {
    owner.pending++;
    const complete = (action: () => void) => {
      try {
        action();
      } catch {
        if (owner.retiring) owner.uncertain = true;
        else fail(owner, 'capture_failed');
      }
      owner.pending--;
      if (live(owner)) activate(owner);
      settled(owner);
    };
    void promise.then(
      (value) => complete(() => success(value)),
      (error: unknown) => complete(() => failure(error)),
    );
  }

  function cleanup(owner: Owner) {
    clearTimers(owner);
    for (const [track, entry] of owner.tracks) {
      if (entry.ended) {
        const ended = entry.ended;
        entry.ended = null;
        attempt(owner, () => track.removeEventListener('ended', ended));
      }
      if (!entry.stopped) {
        entry.stopped = true;
        attempt(owner, () => {
          track.stop();
          if (track.readyState !== 'ended') owner.uncertain = true;
        });
      }
    }
    const source = owner.source;
    owner.source = null;
    if (source) attempt(owner, () => source.disconnect());
    const node = owner.node;
    owner.node = null;
    if (node) {
      attempt(owner, () => {
        node.onprocessorerror = null;
      });
      attempt(owner, () => {
        node.port.onmessage = null;
        node.port.onmessageerror = null;
      });
      if (!owner.terminal)
        attempt(owner, () =>
          node.port.postMessage({ version: 1, type: 'cancel' }),
        );
      attempt(owner, () => node.disconnect());
      attempt(owner, () => node.port.close());
    }
    const context = owner.context;
    if (context && !owner.closeStarted) {
      owner.closeStarted = true;
      try {
        observe(
          owner,
          context.close(),
          () => {
            owner.closeDone = context.state === 'closed';
            if (!owner.closeDone) owner.uncertain = true;
            owner.context = null;
          },
          () => {
            owner.uncertain = true;
          },
        );
      } catch {
        owner.uncertain = true;
      }
    } else if (!context && !owner.closeStarted) {
      owner.closeDone = true;
    }
    settled(owner);
  }

  function retire(owner: Owner, outcome: Outcome) {
    owner.retiring = true;
    owner.outcome = outcome;
    cleanup(owner);
  }

  function fail(owner: Owner, reason: CaptureError) {
    if (owner.retiring) return;
    retire(owner, null);
    if (!disposed && owned === owner && !owner.uncertain)
      publish({ status: 'error', reason });
    // Synchronous constructor failures can retire without a pending operation.
    else if (!disposed && owned === null && retainedOwner === null)
      publish({ status: 'error', reason });
  }

  function receive(owner: Owner, data: unknown) {
    if (!live(owner) || owner.terminal) return;
    try {
      if (data && typeof data === 'object') {
        const message = data as Record<string, unknown>;
        if (
          message.version === 1 &&
          message.type === 'error' &&
          (message.reason === 'empty' || message.reason === 'invalid_audio')
        ) {
          owner.terminal = true;
          fail(owner, message.reason);
          return;
        }
      }
      const candidate = candidateFromMessage(data);
      if (!candidate) {
        fail(owner, 'invalid_audio');
        return;
      }
      owner.terminal = true;
      // Set retiring before notifying a reentrant observer. Its cancellation
      // must be able to discard this candidate, including during pending close.
      owner.retiring = true;
      owner.outcome = candidate;
      publish({ status: 'stopping' });
      cleanup(owner);
    } catch {
      fail(owner, 'invalid_audio');
    }
  }

  function activate(owner: Owner) {
    if (
      !live(owner) ||
      owner.node ||
      !owner.permissionReady ||
      !owner.moduleReady ||
      !owner.resumeReady
    )
      return;
    try {
      const context = owner.context;
      if (!context || context.state !== 'running' || !owner.stream) {
        fail(owner, 'capture_failed');
        return;
      }
      for (const track of owner.tracks.keys()) {
        if (track.readyState !== 'live') {
          fail(owner, 'device_lost');
          return;
        }
      }
      const source = context.createMediaStreamSource(owner.stream);
      owner.source = source;
      source.channelCount = 1;
      source.channelCountMode = 'explicit';
      source.channelInterpretation = 'speakers';
      const node = new AudioWorkletNode(context, 'anima-pcm-capture-v1', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
        channelCount: 1,
        channelCountMode: 'explicit',
        channelInterpretation: 'speakers',
      });
      owner.node = node;
      node.onprocessorerror = () => {
        if (live(owner)) fail(owner, 'capture_failed');
      };
      node.port.onmessage = (event: MessageEvent<unknown>) =>
        receive(owner, event.data);
      node.port.onmessageerror = () => {
        if (live(owner)) fail(owner, 'invalid_audio');
      };
      source.connect(node);
      // The worklet writes only zeros to its single output. This connection
      // drives processing without routing input audio to the speakers.
      node.connect(context.destination);
      owner.watchdog = setTimeout(() => {
        if (live(owner)) stop();
      }, WALL_LIMIT_MS);
      publish({ status: 'recording', startedAt: Date.now() });
    } catch {
      fail(owner, 'capture_failed');
    }
  }

  function start() {
    if (disposed || (owned && !owned.retiring)) return;
    recordingFile = null;
    if (retainedOwner) {
      if (owned) owned.outcome = null;
      publish({ status: 'error', reason: 'busy' });
      return;
    }
    if (!isMicrophoneCaptureSupported()) {
      publish({ status: 'error', reason: 'unsupported' });
      return;
    }
    const owner: Owner = {
      context: null,
      stream: null,
      source: null,
      node: null,
      tracks: new Map(),
      pending: 0,
      permissionReady: false,
      moduleReady: false,
      resumeReady: false,
      retiring: false,
      terminal: false,
      closeStarted: false,
      closeDone: false,
      uncertain: false,
      cleanupReported: false,
      outcome: null,
      watchdog: null,
      stopTimeout: null,
    };
    owned = owner;
    retainedOwner = owner;
    publish({ status: 'requesting' });
    if (!live(owner)) return;
    try {
      const context = new AudioContext({ sampleRate: RATE });
      owner.context = context;
      if (
        context.sampleRate !== RATE ||
        typeof context.audioWorklet?.addModule !== 'function'
      ) {
        fail(owner, 'unsupported');
        return;
      }
      observe(
        owner,
        context.resume(),
        () => {
          owner.resumeReady = true;
        },
        () => {
          fail(owner, 'capture_failed');
        },
      );
      observe(
        owner,
        context.audioWorklet.addModule(workletUrl),
        () => {
          owner.moduleReady = true;
        },
        () => {
          fail(owner, 'capture_failed');
        },
      );
      observe(
        owner,
        navigator.mediaDevices.getUserMedia({
          audio: { channelCount: { ideal: 1 }, sampleRate: { ideal: RATE } },
          video: false,
        }),
        (stream) => {
          owner.stream = stream;
          let tracks: MediaStreamTrack[];
          try {
            tracks = stream.getTracks();
          } catch {
            // Acquired stream ownership cannot be proved retired if enumeration fails.
            owner.uncertain = true;
            fail(owner, 'capture_failed');
            cleanup(owner);
            return;
          }
          for (const track of tracks)
            owner.tracks.set(track, { ended: null, stopped: false });
          if (!live(owner)) {
            cleanup(owner);
            return;
          }
          if (tracks.length !== 1 || tracks[0].kind !== 'audio') {
            fail(owner, 'device_unavailable');
            return;
          }
          for (const [track, entry] of owner.tracks) {
            entry.ended = () => {
              if (live(owner)) fail(owner, 'device_lost');
            };
            track.addEventListener('ended', entry.ended);
            if (track.readyState !== 'live') {
              fail(owner, 'device_lost');
              return;
            }
          }
          owner.permissionReady = true;
        },
        (error) => {
          if (live(owner)) fail(owner, permissionError(error));
        },
      );
    } catch (error) {
      fail(owner, permissionError(error));
    }
  }

  function stop() {
    const owner = owned;
    if (!owner || !live(owner) || state.status !== 'recording' || !owner.node)
      return;
    publish({ status: 'stopping' });
    if (!live(owner)) return;
    if (owner.watchdog !== null) clearTimeout(owner.watchdog);
    owner.watchdog = null;
    owner.stopTimeout = setTimeout(() => {
      if (live(owner)) fail(owner, 'capture_failed');
    }, STOP_TIMEOUT_MS);
    try {
      owner.node.port.postMessage({ version: 1, type: 'stop' });
    } catch {
      fail(owner, 'capture_failed');
    }
  }

  function cancel() {
    if (disposed) return;
    recordingFile = null;
    if (owned) retire(owned, null);
    publish({ status: 'idle' });
  }

  return {
    start,
    stop,
    cancel,
    takeRecording() {
      if (disposed || state.status !== 'ready' || !recordingFile) return null;
      const file = recordingFile;
      recordingFile = null;
      publish({ status: 'idle' });
      return file;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      observer = null;
      recordingFile = null;
      if (owned) retire(owned, null);
    },
  };
}
