import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Session } from '@animaOS-SWARM/sdk';

import { daemon } from '../lib/daemon-api';
import {
  SLASH_COMMANDS,
  SKILL_NOT_IN_TELEGRAM_CHAT,
} from '../lib/slash-commands';
import { runFixture } from '../test/live';
import { sessionFixture } from '../test/sessions';
import {
  useSessionCommands,
  type SessionCommandOptions,
} from './useSessionCommands';

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const CHAT_KEY = 'agent-main\u0000session:room-7';
const SKILLFUL = [
  ...SLASH_COMMANDS,
  {
    name: 'notes',
    description: 'Take notes',
    placeholder: '<request>',
    skill: 'notes',
  },
];
const readOnly = {
  send: false,
  steer: false,
  stop: false,
  rename: false,
  archive: false,
  delete: false,
  compact: false,
  export: false,
};

function setup(overrides: Partial<SessionCommandOptions> = {}) {
  const session = sessionFixture('room-7', { title: 'Weekend plans' });
  const options: SessionCommandOptions = {
    companionId: 'agent-main',
    canSend: () => true,
    routeSessionId: 'room-7',
    session,
    chatKey: CHAT_KEY,
    draft: '',
    resend: null,
    telegramReady: false,
    activeRun: null,
    updateChat: vi.fn(),
    startChat: vi.fn(),
    queueSend: vi.fn(),
    refreshRuns: vi.fn(),
    setError: vi.fn(),
    navigate: vi.fn(),
    listedSessions: [session],
    upsertSession: vi.fn(),
    setKnownSession: vi.fn(),
    newChat: vi.fn(),
    showCommands: vi.fn(),
    search: vi.fn(),
    chooseModel: vi.fn(),
    rename: vi.fn().mockResolvedValue(true),
    archive: vi.fn().mockResolvedValue(true),
    exportSession: vi.fn().mockResolvedValue(undefined),
    slashCommands: SKILLFUL,
    ...overrides,
  };
  const view = renderHook(
    (props: SessionCommandOptions) => useSessionCommands(props),
    { initialProps: options },
  );
  return { ...view, options };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useSessionCommands', () => {
  it('sends learn and its complete source requirements as an ordinary queued turn', () => {
    const { result, options } = setup({
      draft: '/learn docs/api.md focus on errors',
    });
    act(() => result.current.send());
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      '/learn docs/api.md focus on errors',
      expect.any(String),
      'queue',
    );
    expect(options.showCommands).not.toHaveBeenCalled();
  });
  it('queues learn rather than steering an existing task', () => {
    const { result, options } = setup({
      draft: '/learn',
      activeRun: runFixture('active', { status: 'running' }),
    });
    act(() => result.current.steer());
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      '/learn',
      expect.any(String),
      'queue',
    );
  });
  it('replays an unchanged recovered raw API input before trimming or interpreting its slash command', () => {
    const text = ' \n /help raw request \t ';
    const { result, options, rerender } = setup({
      draft: text,
      attachmentIds: ['att_2', 'att_1'],
      resend: {
        text,
        idempotencyKey: 'failed-send-again',
        attachmentIds: ['att_2', 'att_1'],
        skill: 'notes',
      },
    });
    act(() => result.current.send());
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      text,
      'failed-send-again',
      'queue',
      'notes',
      { attachmentIds: ['att_2', 'att_1'], attachments: [] },
    );
    expect(options.showCommands).not.toHaveBeenCalled();
    rerender({ ...options, draft: 'changed input' });
    act(() => result.current.send());
    const second = vi.mocked(options.queueSend).mock.calls[1];
    expect(second[3]).not.toBe('failed-send-again');
    expect(second[5]).toBeUndefined();
  });
  it('retains the explicit skill of a recovered raw message', () => {
    const { result, options } = setup({
      draft: 'raw request',
      resend: {
        text: 'raw request',
        idempotencyKey: 'original',
        skill: 'notes',
      },
    });
    act(() => result.current.send());
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      'raw request',
      'original',
      'queue',
      'notes',
    );
  });
  it('sends attachment-only input and keeps restored IDs with their original retry key', () => {
    const attachment = {
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 4,
      kind: 'text' as const,
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    };
    const clearAttachments = vi.fn();
    const { result, options } = setup({
      attachments: [attachment],
      clearAttachments,
      resend: {
        text: '',
        idempotencyKey: 'original',
        attachmentIds: ['att_1'],
      },
    });
    act(() => result.current.send());
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      '',
      'original',
      'queue',
      undefined,
      { attachmentIds: ['att_1'], attachments: [attachment] },
    );
    expect(clearAttachments).toHaveBeenCalledOnce();
  });

  it('blocks unfinished files and keeps chips for local slash commands', () => {
    const attachment = {
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 4,
      kind: 'text' as const,
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    };
    const clearAttachments = vi.fn();
    const { result, options, rerender } = setup({
      draft: '/new',
      attachments: [attachment],
      clearAttachments,
    });
    act(() => result.current.send());
    expect(options.newChat).not.toHaveBeenCalled();
    expect(clearAttachments).not.toHaveBeenCalled();
    expect(options.updateChat).toHaveBeenCalledWith(
      CHAT_KEY,
      expect.objectContaining({
        error: expect.stringContaining('attachments'),
      }),
    );
    rerender({ ...options, draft: 'hello', attachmentsBlocked: true });
    act(() => result.current.send());
    expect(options.queueSend).not.toHaveBeenCalled();
  });

  it('retains run input IDs on sendAgain and refuses a different session', () => {
    const { result, options } = setup();
    const run = runFixture('retry', {
      sessionId: 'room-7',
      input: {
        text: 'raw input',
        attachmentIds: ['att_2', 'att_1'],
        skill: null,
      },
    });
    act(() => {
      expect(result.current.sendAgain(run)).toBe(true);
    });
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      'raw input',
      expect.any(String),
      'queue',
      undefined,
      { attachmentIds: ['att_2', 'att_1'] },
    );
    act(() => {
      expect(result.current.sendAgain({ ...run, sessionId: 'foreign' })).toBe(
        false,
      );
    });
    expect(options.queueSend).toHaveBeenCalledOnce();
  });
  it('queues the draft, keeping a restored message’s key', () => {
    const { result, options, rerender } = setup({
      draft: ' Book the train ',
      resend: { text: 'Book the train', idempotencyKey: 'key-old' },
    });
    act(() => result.current.send());
    expect(options.updateChat).toHaveBeenCalledWith(CHAT_KEY, {
      draft: '',
      error: null,
      resend: null,
      delivery: null,
    });
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      'Book the train',
      'key-old',
      'queue',
    );

    rerender({ ...options, draft: 'Something else' });
    act(() => result.current.send());
    const [, , , key] = vi.mocked(options.queueSend).mock.calls[1];
    expect(key).not.toBe('key-old');
  });

  it('steers only a session whose reply is in progress and can take it', () => {
    const run = runFixture('run_7', { sessionId: 'room-7', status: 'running' });
    const { result, options, rerender } = setup({
      draft: 'also check trains',
      activeRun: run,
    });
    act(() => result.current.steer());
    expect(vi.mocked(options.queueSend).mock.calls[0][4]).toBe('steer');

    rerender({ ...options, activeRun: null });
    act(() => result.current.steer());
    expect(vi.mocked(options.queueSend).mock.calls[1][4]).toBe('queue');
  });

  it('starts a chat from a new conversation and sends nothing it cannot', () => {
    const { result, options, rerender } = setup({
      routeSessionId: null,
      session: null,
      chatKey: 'agent-main\u0000home',
      draft: 'Hello',
    });
    act(() => result.current.send());
    expect(options.startChat).toHaveBeenCalledWith('Hello');

    rerender({ ...options, canSend: () => false });
    act(() => result.current.send());
    expect(options.startChat).toHaveBeenCalledTimes(1);

    const telegram = sessionFixture('telegram:tg-1', { kind: 'telegram' });
    rerender({
      ...options,
      routeSessionId: telegram.id,
      session: telegram,
      telegramReady: false,
    });
    act(() => result.current.send());
    expect(options.queueSend).not.toHaveBeenCalled();
  });

  it('runs the commands the session allows and says why the others cannot run', async () => {
    const compacted: Session = sessionFixture('room-7', {
      title: 'Weekend plans',
      summary: 'Plans so far',
    });
    vi.spyOn(daemon, 'compactSession').mockResolvedValue(compacted);
    const { result, options } = setup();

    await act(async () => result.current.send('/compact'));
    expect(options.upsertSession).toHaveBeenCalledWith(compacted);
    act(() => result.current.send('/rename Offsite'));
    expect(options.rename).toHaveBeenCalledWith(options.session, 'Offsite');
    act(() => result.current.send('/archive'));
    expect(options.archive).toHaveBeenCalledWith(options.session, true);
    act(() => result.current.send('/export'));
    expect(options.exportSession).toHaveBeenCalledWith(options.session);
    act(() => result.current.send('/search trains'));
    expect(options.search).toHaveBeenCalledWith('trains');
    act(() => result.current.send('/help'));
    expect(options.showCommands).toHaveBeenCalled();
    act(() => result.current.send('/model'));
    expect(options.chooseModel).toHaveBeenCalled();
    act(() => result.current.send('/new'));
    expect(options.newChat).toHaveBeenCalled();

    act(() => result.current.send('/stop'));
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/stop',
      error: '/stop is not available here.',
    });
    act(() => result.current.send('/rename'));
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/rename',
      error: 'Add a title after /rename.',
    });
    expect(options.queueSend).not.toHaveBeenCalled();
  });

  it('uses native Discord text runs, blocks unavailable delivery, skills and attachments, and stops through the ordinary control', async () => {
    const session = sessionFixture('discord:dc-1', {
      kind: 'discord',
      origin: 'discord',
    });
    const stop = vi
      .spyOn(daemon, 'stopRun')
      .mockResolvedValue(runFixture('run_1', { status: 'cancelled' }));
    const { result, options, rerender } = setup({
      session,
      routeSessionId: session.id,
      discordReady: false,
      draft: 'Hello',
    });
    act(() => result.current.send());
    expect(options.queueSend).not.toHaveBeenCalled();
    rerender({ ...options, discordReady: true });
    act(() => result.current.send());
    expect(options.queueSend).toHaveBeenCalledWith(
      session,
      CHAT_KEY,
      'Hello',
      expect.any(String),
      'queue',
    );
    vi.mocked(options.queueSend).mockClear();
    rerender({ ...options, discordReady: true, draft: '/notes write this' });
    act(() => result.current.send());
    expect(options.queueSend).not.toHaveBeenCalled();
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/notes write this',
      error:
        'Skills are unavailable in Discord sessions. Use a web chat to run a skill.',
    });
    rerender({ ...options, discordReady: true, attachmentIds: ['att_1'] });
    act(() => result.current.send());
    expect(options.queueSend).not.toHaveBeenCalled();
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      error:
        'Discord sessions accept text only. Use a web chat for attachments.',
    });
    await act(async () => {
      await result.current.stopRun({ agentId: 'agent-main', id: 'run_1' });
    });
    expect(stop).toHaveBeenCalledWith('agent-main', 'run_1');
  });

  it('/usage opens the usage page', () => {
    const { result, options } = setup();
    act(() => result.current.send('/usage'));
    expect(options.navigate).toHaveBeenCalledWith({
      kind: 'page',
      page: 'usage',
    });
    expect(options.queueSend).not.toHaveBeenCalled();
  });

  it('shows Compact pending while a manual compaction is in flight (S3b-C)', async () => {
    const pending = deferred<Session>();
    vi.spyOn(daemon, 'compactSession').mockReturnValue(pending.promise);
    const { result, options } = setup();

    expect(result.current.compacting).toBe(false);
    act(() => void result.current.compactSession(options.session!));
    expect(result.current.compacting).toBe(true);

    await act(async () =>
      pending.resolve(sessionFixture('room-7', { summary: 'Plans so far' })),
    );
    expect(result.current.compacting).toBe(false);
  });

  it('clears Compact pending even when the compaction fails', async () => {
    vi.spyOn(daemon, 'compactSession').mockRejectedValue(
      new Error('model unavailable'),
    );
    const { result, options } = setup();

    await act(async () => result.current.compactSession(options.session!));
    expect(result.current.compacting).toBe(false);
    expect(options.setError).toHaveBeenCalledWith('model unavailable');
  });

  it('keeps the draft when a command that takes no text gets some (S3b-D)', () => {
    const { result, options } = setup({ draft: '/new write a poem' });
    act(() => result.current.send());
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/new write a poem',
      error: '/new takes no text.',
    });
    expect(options.newChat).not.toHaveBeenCalled();

    act(() => result.current.send('/help me'));
    expect(options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/help me',
      error: '/help takes no text.',
    });
    expect(options.showCommands).not.toHaveBeenCalled();
  });

  it('offers no session command a read-only session lacks', () => {
    const { result, options } = setup({
      session: sessionFixture('job:1', { kind: 'job', capabilities: readOnly }),
      activeRun: runFixture('run_j', { sessionId: 'job:1', status: 'running' }),
    });
    for (const command of ['/stop', '/compact', '/archive', '/export'])
      act(() => result.current.send(command));
    expect(
      vi
        .mocked(options.updateChat)
        .mock.calls.filter(([, patch]) => 'error' in patch && patch.error)
        .map(([, patch]) => patch.error),
    ).toEqual([
      '/stop is not available here.',
      '/compact is not available here.',
      '/archive is not available here.',
      '/export is not available here.',
    ]);
  });

  it('stops a run, reads the ledger again, and shows a failure', async () => {
    const stop = vi
      .spyOn(daemon, 'stopRun')
      .mockResolvedValueOnce(runFixture('run_7', { status: 'cancelled' }))
      .mockRejectedValueOnce(new Error('already finished'));
    const run = runFixture('run_7', { sessionId: 'room-7', status: 'running' });
    const { result, options } = setup({ activeRun: run });

    await act(async () => result.current.send('/stop'));
    expect(stop).toHaveBeenCalledWith('agent-main', 'run_7');
    expect(options.refreshRuns).toHaveBeenCalled();
    await act(async () => result.current.stopRun(run));
    expect(options.setError).toHaveBeenCalledWith('already finished');
  });

  it('sends a run’s message again under a new key, only in its own session', () => {
    const { result, options } = setup();
    act(() =>
      result.current.sendAgain(
        runFixture('run_f', {
          sessionId: 'room-7',
          input: { text: 'Clean the logs', attachmentIds: [], skill: null },
        }),
      ),
    );
    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      'Clean the logs',
      expect.any(String),
    );
    act(() =>
      result.current.sendAgain(runFixture('run_o', { sessionId: 'chat:b' })),
    );
    expect(options.queueSend).toHaveBeenCalledTimes(1);
  });

  it('never sends again a message the owner did not write', () => {
    const { result, options } = setup();
    let sent = true;
    act(() => {
      sent = result.current.sendAgain(
        runFixture('run_t', {
          sessionId: 'room-7',
          source: 'schedule',
          sourceRef: 'schedule-1',
          status: 'failed',
        }),
      );
    });
    expect(sent).toBe(false);
    expect(options.queueSend).not.toHaveBeenCalled();
  });

  it('routes to another agent’s session by its agent', () => {
    const { result, options } = setup();
    result.current.openTarget({ agentId: 'helper-7', sessionId: 'room-9' });
    result.current.openTarget({ agentId: 'agent-main', sessionId: 'room-7' });
    expect(vi.mocked(options.navigate).mock.calls).toEqual([
      [{ kind: 'session', sessionId: 'room-9', agentId: 'helper-7' }],
      [{ kind: 'session', sessionId: 'room-7' }],
    ]);
  });
});

describe('skill messages', () => {
  it('sends a /skill message whole, queued, with its skill', () => {
    const run = runFixture('run_7', { sessionId: 'room-7', status: 'running' });
    const { result, options } = setup({
      draft: '/notes plan the week',
      activeRun: run,
    });

    act(() => result.current.steer());

    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      '/notes plan the week',
      expect.any(String),
      'queue',
      'notes',
    );
  });

  it('starts a new chat with the skill and refuses one in a Telegram session', () => {
    const fresh = setup({
      routeSessionId: null,
      session: null,
      chatKey: 'agent-main\u0000home',
      draft: '/notes hi',
    });
    act(() => fresh.result.current.send());
    expect(fresh.options.startChat).toHaveBeenCalledWith('/notes hi', 'notes');

    const telegram = setup({
      session: sessionFixture('telegram:conn-1', { kind: 'telegram' }),
      telegramReady: true,
      draft: '/notes hi',
    });
    act(() => telegram.result.current.send());
    expect(telegram.options.queueSend).not.toHaveBeenCalled();
    expect(telegram.options.updateChat).toHaveBeenLastCalledWith(CHAT_KEY, {
      draft: '/notes hi',
      error: SKILL_NOT_IN_TELEGRAM_CHAT,
    });
  });

  it('sends a skill run again with its skill', () => {
    const { result, options } = setup();
    const run = runFixture('run_1', {
      sessionId: 'room-7',
      status: 'failed',
      input: { text: '/notes go', attachmentIds: [], skill: 'notes' },
    });

    act(() => {
      result.current.sendAgain(run);
    });

    expect(options.queueSend).toHaveBeenCalledWith(
      options.session,
      CHAT_KEY,
      '/notes go',
      expect.any(String),
      'queue',
      'notes',
    );
  });
});
