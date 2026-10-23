import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import type { Session } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import * as attachmentTransport from '../lib/attachments';
import { ViewHarness } from '../ViewHarness';
import { deferred, snapshot } from './view-harness';
import { sessionFixture } from './sessions';
import {
  runFixture,
  runEvent,
  snapshotEvent,
  snapshotRun,
  type scriptedAgentEvents,
} from './live';

/** Existing attachment/recovery/voice journeys retain the parent suite's setup. */
export function registerAttachmentJourneys({
  openLiveSession,
  mockProviders,
  routes,
}: {
  openLiveSession: (overrides?: Partial<Session>) => Promise<{
    input: HTMLElement;
    stream: ReturnType<typeof scriptedAgentEvents>['streams'][number];
    events: ReturnType<typeof scriptedAgentEvents>;
  }>;
  mockProviders: () => void;
  routes: { sessions: Session[] };
}) {
  it('retains known file metadata when SSE shows the queued run before HTTP acceptance', async () => {
    const answer = deferred<Awaited<ReturnType<typeof daemon.startRun>>>();
    vi.mocked(daemon.startRun).mockReturnValue(answer.promise);
    vi.spyOn(
      attachmentTransport,
      'uploadAttachmentWithProgress',
    ).mockResolvedValue({
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 3,
      kind: 'text',
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    });
    const user = userEvent.setup();
    const { stream } = await openLiveSession();
    act(() => stream.push(snapshotEvent([])));
    await user.upload(
      screen.getByLabelText('Choose attachments'),
      new File(['txt'], 'brief.txt'),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    const [[, , , key]] = vi.mocked(daemon.startRun).mock.calls;
    const run = runFixture('run_files', {
      sessionId: 'room-7',
      idempotencyKey: key,
      input: { text: '', attachmentIds: ['att_1'], skill: null },
    });
    act(() => stream.push(runEvent('run.queued', run, 2)));
    expect(await screen.findByText('Queued')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Download brief.txt' }),
    ).toBeVisible();
    expect(screen.queryByText('Sending…')).not.toBeInTheDocument();
    await act(async () => answer.resolve({ run }));
    expect(
      screen.getAllByRole('button', { name: 'Download brief.txt' }),
    ).toHaveLength(1);
  });

  it('replays raw whitespace and the original request key after API sendAgain fails and is restored', async () => {
    const user = userEvent.setup();
    const raw = ' \n raw API request \t ';
    const { input, stream } = await openLiveSession();
    const run = runFixture('raw_api', {
      sessionId: 'room-7',
      source: 'api',
      status: 'failed',
      error: { code: 'failed', message: 'Failed earlier' },
      input: { text: raw, attachmentIds: ['att_1'], skill: 'notes' },
    });
    act(() => stream.push(snapshotEvent([snapshotRun(run)])));
    vi.mocked(daemon.startRun).mockRejectedValueOnce(
      new Error('Rejected retry'),
    );
    await user.click(await screen.findByRole('button', { name: 'Retry' }));
    await user.click(
      await screen.findByRole('button', { name: 'Restore message' }),
    );
    expect(input).toHaveValue(raw);
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() => expect(daemon.startRun).toHaveBeenCalledTimes(2));
    const calls = vi.mocked(daemon.startRun).mock.calls;
    expect(calls[1][2]).toEqual({
      text: raw,
      mode: 'queue',
      timeZone: expect.any(String),
      skill: 'notes',
      attachmentIds: ['att_1'],
    });
    expect(calls[1][3]).toBe(calls[0][3]);
  });

  it.each(['Work', 'Settings'])(
    'cancels both voice controllers when %s hides or inerts the conversation',
    async (destination) => {
      class Recognition {
        static current: Recognition;
        continuous = false;
        interimResults = false;
        lang = '';
        onstart: (() => void) | null = null;
        onend: (() => void) | null = null;
        onerror: ((event: { error: string }) => void) | null = null;
        onresult: ((event: unknown) => void) | null = null;
        start = vi.fn();
        stop = vi.fn();
        abort = vi.fn();
        constructor() {
          Recognition.current = this;
        }
      }
      class Utterance {
        onstart: (() => void) | null = null;
        onend: (() => void) | null = null;
        onerror: (() => void) | null = null;
        constructor(public text: string) {}
      }
      const speak = vi.fn();
      const cancel = vi.fn();
      vi.stubGlobal('SpeechRecognition', Recognition);
      vi.stubGlobal('speechSynthesis', { speak, cancel });
      vi.stubGlobal('SpeechSynthesisUtterance', Utterance);
      vi.mocked(daemon.sessionMessages).mockResolvedValue({
        messages: [
          {
            id: 'reply',
            role: 'assistant',
            text: 'Visible assistant reply',
            attachments: [],
            metadata: {},
            createdAtMs: 1,
          },
        ],
        nextBefore: null,
      });
      try {
        const user = userEvent.setup();
        const { input } = await openLiveSession();
        await user.click(
          screen.getAllByRole('button', { name: 'Read aloud' })[0],
        );
        act(() => speak.mock.calls[0][0].onstart?.());
        await user.click(
          screen.getByRole('button', { name: 'Dictate message' }),
        );
        const recognition = Recognition.current;
        act(() => recognition.onstart?.());
        const lateResult = recognition.onresult;
        const lateStart = speak.mock.calls[0][0].onstart;
        if (destination === 'Work') {
          // Pages live behind the command menu and settings, not a sidebar.
          await user.click(
            screen.getByRole('button', { name: 'Open command menu' }),
          );
          await user.type(
            screen.getByRole('combobox', { name: 'Search commands' }),
            'Go to Work',
          );
          await user.keyboard('{Enter}');
        } else {
          // Settings open from the agent's menu in the top bar.
          await user.click(screen.getByRole('button', { name: /, menu$/ }));
          await user.click(
            screen.getByRole('menuitem', { name: /'s settings$/ }),
          );
        }
        expect(recognition.abort).toHaveBeenCalledOnce();
        expect(cancel).toHaveBeenCalledTimes(2);
        expect(recognition.onresult).toBeNull();
        expect(speak.mock.calls[0][0].onend).toBeNull();
        act(() => {
          lateResult?.({
            resultIndex: 0,
            results: [{ isFinal: true, 0: { transcript: 'late words' } }],
          });
          lateStart?.();
        });
        expect(input).not.toHaveValue('late words');
      } finally {
        vi.unstubAllGlobals();
      }
    },
  );

  it('keeps known file metadata after attachment-only acceptance before transcript history arrives', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'health').mockResolvedValue({ status: 'ok' });
    vi.spyOn(daemon, 'listAgents').mockResolvedValue({
      agents: [snapshot('agent-a', 'Alpha', 1)],
    });
    mockProviders();
    vi.spyOn(
      attachmentTransport,
      'uploadAttachmentWithProgress',
    ).mockResolvedValue({
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 3,
      kind: 'text',
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    });
    vi.mocked(daemon.startRun).mockImplementation(
      async (agentId, sessionId, input, idempotencyKey) => ({
        run: runFixture('queued-files', {
          agentId,
          sessionId,
          idempotencyKey,
          input: {
            text: input.text,
            attachmentIds: input.attachmentIds ?? [],
            skill: null,
          },
        }),
      }),
    );
    render(<ViewHarness />);
    await screen.findByPlaceholderText('Message Alpha…');
    await user.upload(
      screen.getByLabelText('Choose attachments'),
      new File(['txt'], 'brief.txt'),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    expect(
      await screen.findByRole('button', { name: 'Download brief.txt' }),
    ).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Remove brief.txt' }),
    ).not.toBeInTheDocument();
  });

  it('keeps recovery and the current draft intact when recovered files exceed available slots', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'health').mockResolvedValue({ status: 'ok' });
    vi.spyOn(daemon, 'listAgents').mockResolvedValue({
      agents: [snapshot('agent-a', 'Alpha', 1)],
    });
    mockProviders();
    routes.sessions.push(sessionFixture('room-7', { agentId: 'agent-a' }));
    window.history.replaceState(null, '', '/#/s/room-7');
    sessionStorage.setItem(
      'animaos.pendingSends',
      JSON.stringify([
        {
          key: 'recovered-key',
          text: 'raw recovered message',
          conversation: 'agent-a\u0000session:room-7',
          createdAtMs: 1,
          agentId: 'agent-a',
          sessionId: 'room-7',
          attachmentIds: Array.from({ length: 10 }, (_, i) => `recovered_${i}`),
          attachments: [],
        },
      ]),
    );
    vi.spyOn(
      attachmentTransport,
      'uploadAttachmentWithProgress',
    ).mockImplementation(async (_agent, _session, file) => ({
      id: file.name,
      name: file.name,
      mime: 'text/plain',
      size: 3,
      kind: 'text',
      path: `attachments/${file.name}`,
      createdAtMs: 1,
    }));
    render(<ViewHarness />);
    const field = await screen.findByPlaceholderText('Message Alpha…');
    await user.type(field, 'keep this draft');
    await user.upload(
      screen.getByLabelText('Choose attachments'),
      Array.from(
        { length: 9 },
        (_, i) => new File(['txt'], `existing_${i}.txt`),
      ),
    );
    await waitFor(() =>
      expect(
        screen.getAllByRole('button', { name: /Remove existing_/ }),
      ).toHaveLength(9),
    );
    await user.click(screen.getByRole('button', { name: 'Restore message' }));
    expect(field).toHaveValue('keep this draft');
    expect(
      screen.getByRole('button', { name: 'Restore message' }),
    ).toBeVisible();
    expect(
      screen.getAllByRole('button', { name: /Remove existing_/ }),
    ).toHaveLength(9);
  });

  it('keeps an in-flight upload when returning to the already-open main chat', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'health').mockResolvedValue({ status: 'ok' });
    vi.spyOn(daemon, 'listAgents').mockResolvedValue({
      agents: [snapshot('agent-a', 'Alpha', 1)],
    });
    mockProviders();
    const pending =
      deferred<
        Awaited<
          ReturnType<typeof attachmentTransport.uploadAttachmentWithProgress>
        >
      >();
    const upload = vi
      .spyOn(attachmentTransport, 'uploadAttachmentWithProgress')
      .mockReturnValue(pending.promise);
    render(<ViewHarness />);
    await waitFor(() => expect(window.location.hash).toBe('#/s/chat%3Anew-1'));
    await user.upload(
      screen.getByLabelText('Choose attachments'),
      new File(['one'], 'ongoing.txt'),
    );
    await waitFor(() => expect(upload).toHaveBeenCalledOnce());
    await user.click(screen.getByRole('button', { name: /, menu$/ }));
    await user.click(
      screen.getByRole('menuitem', { name: 'Other conversations' }),
    );
    await user.click(
      within(screen.getByRole('dialog', { name: 'History' })).getByRole(
        'button',
        { name: 'Main chat' },
      ),
    );
    expect(window.location.hash).toBe('#/s/chat%3Anew-1');
    await act(async () =>
      pending.resolve({
        id: 'att_ongoing',
        name: 'ongoing.txt',
        mime: 'text/plain',
        size: 3,
        kind: 'text',
        path: 'attachments/ongoing.txt',
        createdAtMs: 1,
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() =>
      expect(daemon.startRun).toHaveBeenCalledWith(
        'agent-a',
        'chat:new-1',
        {
          text: '',
          mode: 'queue',
          attachmentIds: ['att_ongoing'],
          timeZone: expect.any(String),
        },
        expect.any(String),
      ),
    );
    expect(daemon.ensureMainSession).toHaveBeenCalledOnce();
    expect(daemon.createSession).not.toHaveBeenCalled();
  });

  it('uploads a batch into the durable main session and sends attachment-only input with those IDs', async () => {
    const user = userEvent.setup();
    const alpha = snapshot('agent-a', 'Alpha', 1);
    vi.spyOn(daemon, 'health').mockResolvedValue({ status: 'ok' });
    vi.spyOn(daemon, 'listAgents').mockResolvedValue({ agents: [alpha] });
    mockProviders();
    const upload = vi
      .spyOn(attachmentTransport, 'uploadAttachmentWithProgress')
      .mockImplementation(async (_agent, _session, file) => ({
        id: file.name,
        name: file.name,
        mime: 'text/plain',
        size: 3,
        kind: 'text',
        path: `attachments/${file.name}`,
        createdAtMs: 1,
      }));
    render(<ViewHarness />);
    await screen.findByPlaceholderText('Message Alpha…');
    const first = new File(['one'], 'one.txt');
    const second = new File(['two'], 'two.txt');
    await waitFor(() =>
      expect(screen.getByLabelText('Choose attachments')).toBeEnabled(),
    );
    await user.upload(screen.getByLabelText('Choose attachments'), [
      first,
      second,
    ]);
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled(),
    );
    expect(daemon.ensureMainSession).toHaveBeenCalledExactlyOnceWith('agent-a');
    expect(upload.mock.calls.map((call) => call[1])).toEqual([
      'chat:new-1',
      'chat:new-1',
    ]);
    expect(daemon.createSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() =>
      expect(daemon.startRun).toHaveBeenCalledWith(
        'agent-a',
        upload.mock.calls[0][1],
        {
          text: '',
          mode: 'queue',
          attachmentIds: ['one.txt', 'two.txt'],
          timeZone: expect.any(String),
        },
        expect.any(String),
      ),
    );
    expect(daemon.ensureMainSession).toHaveBeenCalledOnce();
    expect(daemon.createSession).not.toHaveBeenCalled();
  });

  it('restores a rejected attachment-only message manually with its metadata, IDs and original retry key', async () => {
    const user = userEvent.setup();
    const alpha = snapshot('agent-a', 'Alpha', 1);
    vi.spyOn(daemon, 'health').mockResolvedValue({ status: 'ok' });
    vi.spyOn(daemon, 'listAgents').mockResolvedValue({ agents: [alpha] });
    mockProviders();
    vi.spyOn(
      attachmentTransport,
      'uploadAttachmentWithProgress',
    ).mockResolvedValue({
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 3,
      kind: 'text',
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    });
    vi.mocked(daemon.startRun).mockRejectedValueOnce(new Error('Rejected'));
    render(<ViewHarness />);
    await screen.findByPlaceholderText('Message Alpha…');
    await waitFor(() =>
      expect(screen.getByLabelText('Choose attachments')).toBeEnabled(),
    );
    await user.upload(
      screen.getByLabelText('Choose attachments'),
      new File(['one'], 'brief.txt'),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await user.click(
      await screen.findByRole('button', { name: 'Restore message' }),
    );
    expect(
      screen.getByRole('button', { name: 'Remove brief.txt' }),
    ).toBeVisible();
    expect(daemon.startRun).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() => expect(daemon.startRun).toHaveBeenCalledTimes(2));
    const calls = vi.mocked(daemon.startRun).mock.calls;
    expect(calls[1][2].attachmentIds).toEqual(['att_1']);
    expect(calls[1][3]).toBe(calls[0][3]);
  });
}
