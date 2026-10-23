import { startTransition, Suspense, useState } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { VoiceCapabilities, VoiceTranscription } from '@animaos/sdk';
import type { CaptureState } from '../../lib/audio-capture';
import type { AttachmentDraft } from '../../lib/attachments';
import { daemon } from '../../lib/daemon-api';
import { MicrophoneCapture } from './MicrophoneCapture';
import { NativeTranscription } from './NativeTranscription';

const capture = vi.hoisted(() => ({
  listener: null as ((state: CaptureState) => void) | null,
}));
vi.mock('../../lib/audio-capture', () => ({
  isMicrophoneCaptureSupported: () => true,
  createMicrophoneCapture: (listener: (state: CaptureState) => void) => {
    capture.listener = listener;
    return {
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      dispose: vi.fn(),
      takeRecording: vi.fn(),
    };
  },
}));

const suspended = new Promise<void>(() => {});
function RenderGate({ blocked }: { blocked: boolean }) {
  if (blocked) throw suspended;
  return null;
}

const audio: AttachmentDraft = {
  localId: 'local-a',
  name: 'voice.wav',
  kind: 'audio',
  status: 'ready',
  progress: 100,
  attachment: {
    id: 'att-a',
    name: 'voice.wav',
    mime: 'audio/wav',
    size: 44,
    kind: 'audio',
    path: 'attachments/a.wav',
    createdAtMs: 1,
  },
};
const scope = {
  agentId: 'agent-a',
  sessionId: 'session-a',
  ensureSession: async () => 'session-a',
};
const capabilities: VoiceCapabilities = {
  version: 1,
  provider: 'openai',
  model: 'whisper-1',
  available: true,
  reason: 'available',
};

beforeEach(() => {
  capture.listener = null;
});
afterEach(() => vi.restoreAllMocks());

function MicrophoneHarness() {
  const [next, setNext] = useState(false);
  return (
    <>
      <button onClick={() => startTransition(() => setNext(true))}>
        Switch conversation
      </button>
      <Suspense fallback="Loading conversation">
        <MicrophoneCapture
          conversationKey={next ? 'next' : 'current'}
          disabled={false}
          onAdd={vi.fn()}
        />
        <RenderGate blocked={next} />
      </Suspense>
    </>
  );
}

function TranscriptionHarness() {
  const [next, setNext] = useState(false);
  return (
    <>
      <button onClick={() => startTransition(() => setNext(true))}>
        Switch conversation
      </button>
      <Suspense fallback="Loading conversation">
        <NativeTranscription
          scope={scope}
          conversationKey={next ? 'next' : 'current'}
          items={[audio]}
          onUse={vi.fn()}
        />
        <RenderGate blocked={next} />
      </Suspense>
    </>
  );
}

it('keeps the committed microphone observer live while another conversation render suspends', async () => {
  render(<MicrophoneHarness />);
  const listener = capture.listener;
  await act(async () => {
    fireEvent.click(
      screen.getByRole('button', { name: 'Switch conversation' }),
    );
  });
  expect(screen.queryByText('Loading conversation')).not.toBeInTheDocument();
  act(() => {
    listener?.({
      status: 'ready',
      fileName: 'current.wav',
      sizeBytes: 48,
      durationMs: 1000,
      limitReached: false,
    });
  });
  expect(screen.getByRole('button', { name: 'Add recording' })).toBeVisible();
});

it('accepts the committed transcription result while another conversation render suspends', async () => {
  let finish!: (value: VoiceTranscription) => void;
  const pending = new Promise<VoiceTranscription>((resolve) => {
    finish = resolve;
  });
  vi.spyOn(daemon.voice, 'capabilities').mockResolvedValue(capabilities);
  vi.spyOn(daemon.voice, 'transcribe').mockReturnValue(pending);
  vi.spyOn(daemon.voice, 'cancel').mockResolvedValue();
  render(<TranscriptionHarness />);
  await act(async () => {
    fireEvent.change(screen.getByRole('combobox'), {
      target: { value: 'att-a' },
    });
  });
  fireEvent.click(
    await screen.findByRole('button', { name: 'Transcribe with OpenAI API' }),
  );
  await act(async () => {
    fireEvent.click(
      screen.getByRole('button', { name: 'Switch conversation' }),
    );
  });
  expect(screen.queryByText('Loading conversation')).not.toBeInTheDocument();
  await act(async () => {
    finish({
      version: 1,
      operationId: '12345678-1234-4234-9234-123456789abc',
      attachmentId: 'att-a',
      provider: 'openai',
      model: 'whisper-1',
      text: 'Current conversation transcript',
    });
  });
  expect(
    screen.getByRole('textbox', { name: 'Transcript preview' }),
  ).toHaveValue('Current conversation transcript');
});
