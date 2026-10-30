import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { beforeEach, expect, it, vi } from 'vitest';
import type { VoiceCapabilities, VoiceTranscription } from '@animaos/sdk';
import { Composer } from '../ChatScreen';
import {
  NativeTranscription,
  type NativeVoiceScope,
} from './NativeTranscription';
import type { AttachmentDraft } from '../../lib/attachments';

const api = vi.hoisted(() => ({
  capabilities: vi.fn(),
  transcribe: vi.fn(),
  cancel: vi.fn(),
}));
vi.mock('../../lib/daemon-api', async (original) => {
  const actual = await original<typeof import('../../lib/daemon-api')>();
  return { ...actual, daemon: { ...actual.daemon, voice: api } };
});
const capabilities: VoiceCapabilities = {
  version: 1,
  provider: 'openai',
  model: 'whisper-1',
  available: true,
  reason: 'available',
};
const scope: NativeVoiceScope = {
  agentId: 'agent/a',
  sessionId: 'session/b',
  ensureSession: async () => 'session/b',
};
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
const secondAudio: AttachmentDraft = {
  ...audio,
  localId: 'local-b',
  name: 'other.wav',
  attachment: { ...audio.attachment!, id: 'att-b', name: 'other.wav' },
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function result(): VoiceTranscription {
  const request = api.transcribe.mock.calls.at(-1)![2];
  return {
    version: 1,
    ...request,
    model: 'whisper-1',
    text: 'Hello from audio',
  };
}
async function selectAudio(id = 'att-a') {
  await userEvent.selectOptions(
    screen.getByRole('combobox', { name: 'Audio to transcribe' }),
    id,
  );
  return screen.findByRole('button', { name: 'Transcribe with OpenAI API' });
}
beforeEach(() => {
  vi.clearAllMocks();
  api.capabilities.mockResolvedValue(capabilities);
  api.cancel.mockResolvedValue(undefined);
  api.transcribe.mockImplementation(async () => result());
});

it('keeps the simplified composer attachment-only and makes zero transcription requests', async () => {
  const onSend = vi.fn();
  render(
    <Composer
      agentName="Nova" draft="ordinary message" setDraft={vi.fn()}
      sending={false} disabled={false} onSend={onSend}
      error={null} onDismissError={vi.fn()} conversationKey="chat-a"
      nativeVoice={scope}
      attachments={{ items: [audio], onAdd: vi.fn(), onRemove: vi.fn() }}
    />,
  );
  expect(api.transcribe).not.toHaveBeenCalled();
  expect(api.capabilities).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: 'Add' }));
  expect(screen.getByRole('menuitem', { name: /Attach files/ })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Record audio' })).not.toBeInTheDocument();
  expect(screen.queryByRole('combobox', { name: 'Audio to transcribe' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Transcribe with OpenAI API' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Send' }));
  expect(onSend).toHaveBeenCalledOnce();
  expect(api.transcribe).not.toHaveBeenCalled();
  expect(api.capabilities).not.toHaveBeenCalled();
});

it('requires exact consent and delivers edited text to the current draft only on explicit Use in a component harness', async () => {
  const pending = deferred<VoiceTranscription>();
  api.transcribe.mockReturnValue(pending.promise);
  const onSend = vi.fn();
  const onUse = vi.fn();
  function Harness() {
    const [draft, setDraft] = useState('original');
    return (
      <>
      <Composer
        agentName="Nova"
        draft={draft}
        setDraft={setDraft}
        sending={false}
        disabled={false}
        onSend={onSend}
        error={null}
        onDismissError={vi.fn()}
        conversationKey="chat-a"
        attachments={{ items: [audio], onAdd: vi.fn(), onRemove: vi.fn() }}
      />
      <NativeTranscription
        scope={scope}
        conversationKey="chat-a"
        items={[audio]}
        onUse={(text) => {
          onUse(text);
          setDraft((current) => `${current} ${text}`);
        }}
      />
      </>
    );
  }
  render(<Harness />);
  const action = await selectAudio();
  expect(screen.getByText(/sent to OpenAI.*API charges/i)).toBeVisible();
  await userEvent.click(action);
  expect(api.transcribe).toHaveBeenCalledOnce();
  const [agent, session, body, options] = api.transcribe.mock.calls[0];
  expect([agent, session]).toEqual(['agent/a', 'session/b']);
  expect(body).toEqual({
    operationId: expect.stringMatching(
      /^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/i,
    ),
    attachmentId: 'att-a',
    provider: 'openai',
    consent: true,
  });
  expect(options.signal).toBeInstanceOf(AbortSignal);
  fireEvent.change(screen.getByRole('textbox', { name: 'Message Nova' }), {
    target: { value: 'newer draft' },
  });
  await act(async () => pending.resolve(result()));
  expect(screen.getByRole('textbox', { name: 'Message Nova' })).toHaveValue(
    'newer draft',
  );
  expect(onUse).not.toHaveBeenCalled();
  const preview = screen.getByRole('textbox', { name: 'Transcript preview' });
  fireEvent.change(preview, { target: { value: 'edited transcript' } });
  await userEvent.click(screen.getByRole('button', { name: 'Use transcript' }));
  expect(screen.getByRole('textbox', { name: 'Message Nova' })).toHaveValue(
    'newer draft edited transcript',
  );
  expect(
    screen.queryByRole('textbox', { name: 'Transcript preview' }),
  ).not.toBeInTheDocument();
  expect(onUse).toHaveBeenCalledExactlyOnceWith('edited transcript');
  expect(onSend).not.toHaveBeenCalled();
});

it('discards a preview without insertion', async () => {
  const onUse = vi.fn();
  render(
    <NativeTranscription
      scope={scope}
      conversationKey="a"
      items={[audio]}
      onUse={onUse}
    />,
  );
  await userEvent.click(await selectAudio());
  expect(
    await screen.findByRole('textbox', { name: 'Transcript preview' }),
  ).toHaveValue('Hello from audio');
  await userEvent.click(screen.getByRole('button', { name: 'Discard' }));
  expect(onUse).not.toHaveBeenCalled();
  expect(
    screen.queryByRole('textbox', { name: 'Transcript preview' }),
  ).not.toBeInTheDocument();
});

it.each([
  'disabled',
  'missing_credentials',
  'unsupported_configuration',
  'unavailable',
] as const)(
  'shows safe %s capability explanation without a transfer action',
  async (reason) => {
    api.capabilities.mockResolvedValue({
      ...capabilities,
      available: false,
      reason,
    });
    render(
      <NativeTranscription
        scope={scope}
        conversationKey="a"
        items={[audio]}
        onUse={vi.fn()}
      />,
    );
    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Audio to transcribe' }),
      'att-a',
    );
    expect(
      await screen.findByText(/Transcription is unavailable/),
    ).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Transcribe with OpenAI API' }),
    ).not.toBeInTheDocument();
    expect(api.transcribe).not.toHaveBeenCalled();
  },
);

it('Stop dispatches owner DELETE before independent local abort and fences a late result even if DELETE fails', async () => {
  const pending = deferred<VoiceTranscription>();
  const order: string[] = [];
  api.transcribe.mockImplementation((_a, _s, _body, { signal }) => {
    signal.addEventListener('abort', () => order.push('abort'));
    return pending.promise;
  });
  api.cancel.mockImplementation(async (_a, _s, _id, options) => {
    expect(options?.signal).toBeUndefined();
    order.push('delete');
    throw new Error('synthetic failure');
  });
  const onUse = vi.fn();
  render(
    <NativeTranscription
      scope={scope}
      conversationKey="a"
      items={[audio]}
      onUse={onUse}
    />,
  );
  await userEvent.click(await selectAudio());
  const operationId = api.transcribe.mock.calls[0][2].operationId;
  await userEvent.click(
    screen.getByRole('button', { name: 'Stop transcription' }),
  );
  expect(api.cancel).toHaveBeenCalledWith('agent/a', 'session/b', operationId);
  expect(order).toEqual(['delete', 'abort']);
  expect(
    screen.getByText(/Remote processing and API charges may still occur/),
  ).toBeVisible();
  await act(async () => pending.resolve(result()));
  expect(
    screen.queryByRole('textbox', { name: 'Transcript preview' }),
  ).not.toBeInTheDocument();
  expect(onUse).not.toHaveBeenCalled();
});

it.each([
  'conversation',
  'agent',
  'session',
  'removed',
  'selection',
  'disabled',
  'unmount',
] as const)('fences late transcription on %s boundary', async (boundary) => {
  const pending = deferred<VoiceTranscription>();
  api.transcribe.mockReturnValue(pending.promise);
  const onUse = vi.fn();
  const props = {
    scope,
    conversationKey: 'chat-a',
    items: [audio, secondAudio],
    onUse,
  };
  const view = render(<NativeTranscription {...props} />);
  await userEvent.click(await selectAudio());
  const request = api.transcribe.mock.calls[0];
  if (boundary === 'unmount') view.unmount();
  else if (boundary === 'selection')
    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Audio to transcribe' }),
      'att-b',
    );
  else
    view.rerender(
      <NativeTranscription
        {...props}
        {...(boundary === 'conversation'
          ? { conversationKey: 'chat-b' }
          : boundary === 'agent'
            ? { scope: { ...scope, agentId: 'other' } }
            : boundary === 'session'
              ? { scope: { ...scope, sessionId: 'other' } }
              : boundary === 'removed'
                ? { items: [] }
                : { disabled: true })}
      />,
    );
  await waitFor(() => expect(request[3].signal.aborted).toBe(true));
  expect(api.cancel).toHaveBeenCalledWith(
    'agent/a',
    'session/b',
    request[2].operationId,
  );
  await act(async () => pending.resolve(result()));
  expect(
    screen.queryByRole('textbox', { name: 'Transcript preview' }),
  ).not.toBeInTheDocument();
  expect(onUse).not.toHaveBeenCalled();
});

it('never offers uploading audio and fences late capabilities after a selection change', async () => {
  const pending = deferred<VoiceCapabilities>();
  api.capabilities.mockReturnValue(pending.promise);
  const props = { scope, conversationKey: 'a', items: [audio], onUse: vi.fn() };
  const view = render(
    <NativeTranscription
      {...props}
      items={[{ ...audio, status: 'uploading' }]}
    />,
  );
  expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  view.rerender(<NativeTranscription {...props} />);
  await userEvent.selectOptions(screen.getByRole('combobox'), 'att-a');
  view.rerender(<NativeTranscription {...props} items={[]} />);
  await act(async () => pending.resolve(capabilities));
  expect(
    screen.queryByRole('button', { name: 'Transcribe with OpenAI API' }),
  ).not.toBeInTheDocument();
  expect(api.transcribe).not.toHaveBeenCalled();
});
