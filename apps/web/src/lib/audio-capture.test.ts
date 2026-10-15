import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type {
  CaptureState,
  MicrophoneCaptureController,
} from './audio-capture';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

class Track {
  kind = 'audio';
  readyState = 'live';
  listeners = new Set<() => void>();
  stop = vi.fn(() => {
    this.readyState = 'ended';
  });
  addEventListener = vi.fn((_name: string, callback: () => void) => {
    this.listeners.add(callback);
  });
  removeEventListener = vi.fn((_name: string, callback: () => void) => {
    this.listeners.delete(callback);
  });
  end() {
    this.readyState = 'ended';
    for (const callback of this.listeners) callback();
  }
}

class Source {
  channelCount = 2;
  channelCountMode = 'max';
  channelInterpretation = 'speakers';
  connect = vi.fn();
  disconnect = vi.fn();
}

class Node {
  onprocessorerror: (() => void) | null = null;
  port = {
    onmessage: null as ((event: { data: unknown }) => void) | null,
    onmessageerror: null as (() => void) | null,
    postMessage: vi.fn(),
    close: vi.fn(),
  };
  connect = vi.fn();
  disconnect = vi.fn();
  constructor(
    _context: unknown,
    public name: string,
    public options: AudioWorkletNodeOptions,
  ) {
    nodes.push(this);
  }
  message(data: unknown) {
    this.port.onmessage?.({ data });
  }
}

let permission: ReturnType<typeof deferred<MediaStream>>;
let moduleLoad: ReturnType<typeof deferred<void>>;
let resume: ReturnType<typeof deferred<void>>;
let close: ReturnType<typeof deferred<void>>;
let contexts: Context[];
let nodes: Node[];
let tracks: Track[];
let getUserMedia: ReturnType<typeof vi.fn>;
let requestedOptions: AudioContextOptions[];
let actualSampleRate: number;

class Context {
  sampleRate = actualSampleRate;
  state = 'suspended';
  destination = {};
  source = new Source();
  audioWorklet = { addModule: vi.fn(() => moduleLoad.promise) };
  resume = vi.fn(() =>
    resume.promise.then(() => {
      this.state = 'running';
    }),
  );
  close = vi.fn(() =>
    close.promise.then(() => {
      this.state = 'closed';
    }),
  );
  createMediaStreamSource = vi.fn(() => this.source);
  constructor(options: AudioContextOptions) {
    contexts.push(this);
    requestedOptions.push(options);
  }
}

function stream(owned = tracks) {
  return {
    getTracks: () => owned,
    getAudioTracks: () => owned,
  } as unknown as MediaStream;
}

// Independently construct the fixed protocol allocation and canonical PCM WAV header.
function terminal(frames = 160, limitReached = false) {
  const buffer = new ArrayBuffer(3_840_044);
  const view = new DataView(buffer);
  for (const [offset, text] of [
    [0, 'RIFF'],
    [8, 'WAVE'],
    [12, 'fmt '],
    [36, 'data'],
  ] as const) {
    for (let index = 0; index < text.length; index++)
      view.setUint8(offset + index, text.charCodeAt(index));
  }
  view.setUint32(4, 36 + frames * 2, true);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 16_000, true);
  view.setUint32(28, 32_000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  view.setUint32(40, frames * 2, true);
  return {
    version: 1,
    type: 'complete',
    buffer,
    frames,
    usedBytes: 44 + frames * 2,
    limitReached,
  };
}

async function flush() {
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
}

async function controller() {
  const api = await import('./audio-capture');
  const states: CaptureState[] = [];
  const capture = api.createMicrophoneCapture((state) => states.push(state));
  return { api, capture, states, latest: () => states.at(-1) };
}

async function recording(capture: MicrophoneCaptureController) {
  capture.start();
  permission.resolve(stream());
  moduleLoad.resolve();
  resume.resolve();
  await flush();
}

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  permission = deferred();
  moduleLoad = deferred();
  resume = deferred();
  close = deferred();
  contexts = [];
  nodes = [];
  tracks = [new Track()];
  requestedOptions = [];
  actualSampleRate = 16_000;
  getUserMedia = vi.fn(() => permission.promise);
  vi.stubGlobal('isSecureContext', true);
  vi.stubGlobal('navigator', { mediaDevices: { getUserMedia } });
  vi.stubGlobal('AudioContext', Context);
  vi.stubGlobal('AudioWorkletNode', Node);
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('factory and supported checks have no acquisition side effects', async () => {
  const { api, capture } = await controller();
  expect(api.isMicrophoneCaptureSupported()).toBe(true);
  expect(contexts).toHaveLength(0);
  expect(getUserMedia).not.toHaveBeenCalled();
  expect(capture.takeRecording()).toBeNull();
  capture.dispose();
  capture.start();
  expect(getUserMedia).not.toHaveBeenCalled();
});

it.each(['isSecureContext', 'AudioContext', 'AudioWorkletNode', 'navigator'])(
  'fails closed when %s support is absent',
  async (name) => {
    const { api, capture, latest } = await controller();
    vi.stubGlobal(name, undefined);
    expect(api.isMicrophoneCaptureSupported()).toBe(false);
    capture.start();
    expect(latest()).toEqual({ status: 'error', reason: 'unsupported' });
    expect(getUserMedia).not.toHaveBeenCalled();
  },
);

it('requests 16 kHz in the click, waits for all acquisitions, and configures a silent mono graph', async () => {
  const { capture, latest } = await controller();
  capture.start();
  expect(requestedOptions).toEqual([{ sampleRate: 16_000 }]);
  expect(contexts[0].resume).toHaveBeenCalledOnce();
  expect(getUserMedia).toHaveBeenCalledOnce();
  expect(latest()).toEqual({ status: 'requesting' });
  permission.resolve(stream());
  moduleLoad.resolve();
  await flush();
  expect(nodes).toHaveLength(0);
  resume.resolve();
  await flush();
  expect(latest()).toMatchObject({
    status: 'recording',
    startedAt: expect.any(Number),
  });
  expect(nodes[0].name).toBe('anima-pcm-capture-v1');
  expect(nodes[0].options).toEqual({
    numberOfInputs: 1,
    numberOfOutputs: 1,
    outputChannelCount: [1],
    channelCount: 1,
    channelCountMode: 'explicit',
    channelInterpretation: 'speakers',
  });
  expect(contexts[0].source).toMatchObject({
    channelCount: 1,
    channelCountMode: 'explicit',
    channelInterpretation: 'speakers',
  });
  expect(contexts[0].source.connect).toHaveBeenCalledWith(nodes[0]);
  expect(nodes[0].connect).toHaveBeenCalledWith(contexts[0].destination);
  expect(contexts[0].audioWorklet.addModule.mock.calls[0]).toHaveLength(1);
  capture.cancel();
});

it('rejects an actual unsupported rate without requesting permission and observes close', async () => {
  actualSampleRate = 48_000;
  const { capture, latest, api } = await controller();
  capture.start();
  expect(getUserMedia).not.toHaveBeenCalled();
  expect(latest()).toEqual({ status: 'error', reason: 'unsupported' });
  const otherStates: CaptureState[] = [];
  api.createMicrophoneCapture((state) => otherStates.push(state)).start();
  expect(otherStates.at(-1)).toEqual({ status: 'error', reason: 'busy' });
  expect(contexts[0].close).toHaveBeenCalledOnce();
  close.resolve();
  await flush();
});

it.each(['cancel', 'dispose'] as const)(
  'retains the global lease after %s until a late grant and every pending operation settle',
  async (action) => {
    const { capture, api } = await controller();
    capture.start();
    capture[action]();
    const otherStates: CaptureState[] = [];
    const other = api.createMicrophoneCapture((state) =>
      otherStates.push(state),
    );
    close.resolve();
    await flush();
    other.start();
    expect(otherStates.at(-1)).toEqual({ status: 'error', reason: 'busy' });
    expect(getUserMedia).toHaveBeenCalledOnce();
    permission.resolve(stream());
    await flush();
    expect(tracks[0].stop).toHaveBeenCalledOnce();
    other.start();
    expect(getUserMedia).toHaveBeenCalledOnce();
    moduleLoad.resolve();
    await flush();
    other.start();
    expect(getUserMedia).toHaveBeenCalledOnce();
    resume.reject(new Error('closed while resuming'));
    await flush();
    other.start();
    expect(getUserMedia).toHaveBeenCalledTimes(2);
    expect(nodes).toHaveLength(0);
    other.cancel();
  },
);

it.each(['NotAllowedError', 'NotFoundError', 'NotReadableError', 'OtherError'])(
  'maps permission failure %s without leaking details',
  async (name) => {
    const { capture, latest } = await controller();
    capture.start();
    permission.reject({ name, message: 'private device detail' });
    moduleLoad.resolve();
    resume.resolve();
    close.resolve();
    await flush();
    expect(latest()).toEqual({
      status: 'error',
      reason:
        name === 'NotAllowedError'
          ? 'permission_denied'
          : name === 'OtherError'
            ? 'capture_failed'
            : 'device_unavailable',
    });
    expect(contexts[0].close).toHaveBeenCalledOnce();
  },
);

it.each(['module', 'resume'] as const)(
  'handles %s rejection and still stops late tracks',
  async (failure) => {
    const { capture, latest } = await controller();
    capture.start();
    (failure === 'module' ? moduleLoad : resume).reject(new Error('private'));
    (failure === 'module' ? resume : moduleLoad).resolve();
    await flush();
    expect(latest()).toEqual({ status: 'error', reason: 'capture_failed' });
    permission.resolve(stream());
    close.resolve();
    await flush();
    expect(tracks[0].stop).toHaveBeenCalledOnce();
    expect(nodes).toHaveLength(0);
  },
);

it('publishes only metadata after observed close and transfers one bounded File exactly once', async () => {
  const { capture, latest, states } = await controller();
  await recording(capture);
  capture.stop();
  expect(nodes[0].port.postMessage).toHaveBeenCalledWith({
    version: 1,
    type: 'stop',
  });
  const staleMessage = nodes[0].port.onmessage!;
  nodes[0].message(terminal());
  expect(latest()).toEqual({ status: 'stopping' });
  expect(capture.takeRecording()).toBeNull();
  expect(tracks[0].stop).toHaveBeenCalledOnce();
  expect(contexts[0].source.disconnect).toHaveBeenCalledOnce();
  expect(nodes[0].disconnect).toHaveBeenCalledOnce();
  expect(nodes[0].port.close).toHaveBeenCalledOnce();
  expect(contexts[0].close).toHaveBeenCalledOnce();
  close.resolve();
  await flush();
  expect(latest()).toEqual({
    status: 'ready',
    fileName: 'recording.wav',
    sizeBytes: 364,
    durationMs: 10,
    limitReached: false,
  });
  expect(states.some((state) => 'file' in state || 'buffer' in state)).toBe(
    false,
  );
  staleMessage({ data: terminal() });
  const file = capture.takeRecording();
  expect(file).toBeInstanceOf(File);
  expect(file).toMatchObject({
    name: 'recording.wav',
    type: 'audio/wav',
    size: 364,
  });
  expect(capture.takeRecording()).toBeNull();
  expect(latest()).toEqual({ status: 'idle' });
  capture.cancel();
  capture.dispose();
  expect(tracks[0].stop).toHaveBeenCalledOnce();
});

it('preserves the bounded two-minute candidate and limit notice', async () => {
  const { capture, latest } = await controller();
  await recording(capture);
  nodes[0].message(terminal(1_920_000, true));
  close.resolve();
  await flush();
  expect(latest()).toEqual({
    status: 'ready',
    fileName: 'recording.wav',
    sizeBytes: 3_840_044,
    durationMs: 120_000,
    limitReached: true,
  });
});

const corruptions: [string, (value: ReturnType<typeof terminal>) => void][] = [
  [
    'version',
    (value: ReturnType<typeof terminal>) => {
      value.version = 2;
    },
  ],
  [
    'allocation',
    (value: ReturnType<typeof terminal>) => {
      value.buffer = value.buffer.slice(0, value.usedBytes);
    },
  ],
  [
    'used bytes',
    (value: ReturnType<typeof terminal>) => {
      value.usedBytes++;
    },
  ],
  [
    'frames',
    (value: ReturnType<typeof terminal>) => {
      value.frames = 0;
    },
  ],
  [
    'frame cap',
    (value: ReturnType<typeof terminal>) => {
      value.frames = 1_920_001;
    },
  ],
  [
    'fraction',
    (value: ReturnType<typeof terminal>) => {
      value.frames = 160.5;
    },
  ],
  [
    'limit flag',
    (value: ReturnType<typeof terminal>) => {
      value.limitReached = true;
    },
  ],
  ...[0, 4, 8, 12, 16, 20, 22, 24, 28, 32, 34, 36, 40].map(
    (offset) =>
      [
        `header ${offset}`,
        (value: ReturnType<typeof terminal>) => {
          new DataView(value.buffer).setUint8(offset, 255);
        },
      ] as const,
  ),
];
it.each(corruptions)(
  'rejects malformed %s before creating a candidate',
  async (_name, corrupt) => {
    const { capture, latest } = await controller();
    await recording(capture);
    const value = terminal();
    corrupt(value);
    nodes[0].message(value);
    close.resolve();
    await flush();
    expect(latest()).toEqual({ status: 'error', reason: 'invalid_audio' });
    expect(capture.takeRecording()).toBeNull();
  },
);

it.each(['empty', 'invalid_audio'] as const)(
  'accepts safe terminal worklet error %s',
  async (reason) => {
    const { capture, latest } = await controller();
    await recording(capture);
    nodes[0].message({ version: 1, type: 'error', reason });
    close.resolve();
    await flush();
    expect(latest()).toEqual({ status: 'error', reason });
  },
);

it.each(['track', 'processor', 'message'] as const)(
  'discards on %s failure and fences saved callbacks',
  async (failure) => {
    const { capture, latest } = await controller();
    await recording(capture);
    const saved = nodes[0].port.onmessage!;
    if (failure === 'track') tracks[0].end();
    else if (failure === 'processor') nodes[0].onprocessorerror?.();
    else nodes[0].port.onmessageerror?.();
    close.resolve();
    await flush();
    const before = latest();
    saved({ data: terminal() });
    expect(latest()).toEqual(before);
    expect(latest()).toMatchObject({ status: 'error' });
    expect(capture.takeRecording()).toBeNull();
  },
);

it('requests stop at the wall limit and times out without releasing a pending close', async () => {
  const { capture, latest, api } = await controller();
  await recording(capture);
  vi.advanceTimersByTime(120_000);
  expect(latest()).toEqual({ status: 'stopping' });
  expect(nodes[0].port.postMessage).toHaveBeenCalledWith({
    version: 1,
    type: 'stop',
  });
  vi.advanceTimersByTime(2_000);
  expect(latest()).toEqual({ status: 'error', reason: 'capture_failed' });
  expect(tracks[0].stop).toHaveBeenCalledOnce();
  const states: CaptureState[] = [];
  const other = api.createMicrophoneCapture((state) => states.push(state));
  other.start();
  expect(states.at(-1)).toEqual({ status: 'error', reason: 'busy' });
  close.resolve();
  await flush();
  other.start();
  expect(getUserMedia).toHaveBeenCalledTimes(2);
  other.cancel();
});

it.each(['track stop', 'disconnect', 'port close', 'context close'] as const)(
  'retains the lease and discards a candidate if %s cleanup fails',
  async (failure) => {
    const { capture, latest, api } = await controller();
    await recording(capture);
    const fail = () => {
      throw new Error('private cleanup details');
    };
    if (failure === 'track stop') tracks[0].stop.mockImplementation(fail);
    if (failure === 'disconnect') nodes[0].disconnect.mockImplementation(fail);
    if (failure === 'port close') nodes[0].port.close.mockImplementation(fail);
    nodes[0].message(terminal());
    if (failure === 'context close') close.reject(new Error('private'));
    else close.resolve();
    await flush();
    expect(latest()).toEqual({ status: 'error', reason: 'cleanup_pending' });
    expect(capture.takeRecording()).toBeNull();
    capture.cancel();
    capture.dispose();
    const states: CaptureState[] = [];
    api.createMicrophoneCapture((state) => states.push(state)).start();
    expect(states.at(-1)).toEqual({ status: 'error', reason: 'busy' });
    expect(getUserMedia).toHaveBeenCalledOnce();
    expect(tracks[0].stop).toHaveBeenCalledOnce();
    expect(nodes[0].port.close).toHaveBeenCalledOnce();
    expect(contexts[0].close).toHaveBeenCalledOnce();
  },
);

it('cancel while close is pending discards terminal output and suppresses late ready', async () => {
  const { capture, latest } = await controller();
  await recording(capture);
  nodes[0].message(terminal());
  capture.cancel();
  close.resolve();
  await flush();
  expect(latest()).toEqual({ status: 'idle' });
  expect(capture.takeRecording()).toBeNull();
});

it('observer reentrancy cannot acquire after cancellation or resurrect a disposed controller', async () => {
  const { api } = await controller();
  let capture!: MicrophoneCaptureController;
  capture = api.createMicrophoneCapture((state) => {
    if (state.status === 'requesting') capture.dispose();
  });
  capture.start();
  expect(contexts).toHaveLength(0);
  expect(getUserMedia).not.toHaveBeenCalled();
});

it('keeps an ignored permission acquisition busy indefinitely without requesting a second stream', async () => {
  const { capture, api } = await controller();
  capture.start();
  capture.dispose();
  moduleLoad.resolve();
  resume.reject(new Error('closed'));
  close.resolve();
  await flush();
  vi.advanceTimersByTime(60 * 60 * 1000);
  const states: CaptureState[] = [];
  const other = api.createMicrophoneCapture((state) => states.push(state));
  for (let attempt = 0; attempt < 5; attempt++) other.start();
  expect(states.at(-1)).toEqual({ status: 'error', reason: 'busy' });
  expect(getUserMedia).toHaveBeenCalledOnce();
  expect(contexts).toHaveLength(1);
});

it('stops every late-granted track even when a cancelled grant has unexpected channels', async () => {
  const { capture } = await controller();
  capture.start();
  capture.cancel();
  const lateTracks = [new Track(), new Track(), new Track()];
  permission.resolve(stream(lateTracks));
  moduleLoad.resolve();
  resume.reject(new Error('closed'));
  close.resolve();
  await flush();
  for (const track of lateTracks) expect(track.stop).toHaveBeenCalledOnce();
  expect(nodes).toHaveLength(0);
});

it('retains ownership if an acquired stream cannot enumerate its tracks', async () => {
  const { capture, latest, api } = await controller();
  capture.start();
  permission.resolve({
    getTracks: () => {
      throw new Error('unknown tracks');
    },
  } as unknown as MediaStream);
  moduleLoad.resolve();
  resume.resolve();
  close.resolve();
  await flush();
  expect(latest()).toEqual({ status: 'error', reason: 'cleanup_pending' });
  const states: CaptureState[] = [];
  api.createMicrophoneCapture((state) => states.push(state)).start();
  expect(states.at(-1)).toEqual({ status: 'error', reason: 'busy' });
});

it('keeps a track whose stop did not settle from being treated as physically retired', async () => {
  const { capture, latest } = await controller();
  await recording(capture);
  tracks[0].stop.mockImplementation(() => undefined);
  nodes[0].message(terminal());
  close.resolve();
  await flush();
  expect(latest()).toEqual({ status: 'error', reason: 'cleanup_pending' });
  expect(capture.takeRecording()).toBeNull();
});

it('fences old port and processor callbacks after a fresh generation starts', async () => {
  const { capture, latest } = await controller();
  await recording(capture);
  const message = nodes[0].port.onmessage!;
  const processorError = nodes[0].onprocessorerror!;
  const ended = [...tracks[0].listeners][0];
  capture.cancel();
  close.resolve();
  await flush();
  permission = deferred();
  moduleLoad = deferred();
  resume = deferred();
  close = deferred();
  tracks = [new Track()];
  await recording(capture);
  message({ data: terminal() });
  processorError();
  ended();
  expect(latest()).toMatchObject({ status: 'recording' });
  expect(tracks[0].stop).not.toHaveBeenCalled();
  expect(capture.takeRecording()).toBeNull();
  capture.cancel();
});

it('a second Start while retirement is pending cannot later publish the old candidate', async () => {
  const { capture, latest } = await controller();
  await recording(capture);
  nodes[0].message(terminal());
  capture.start();
  expect(latest()).toEqual({ status: 'error', reason: 'busy' });
  close.resolve();
  await flush();
  expect(latest()).toEqual({ status: 'error', reason: 'busy' });
  expect(capture.takeRecording()).toBeNull();
});
