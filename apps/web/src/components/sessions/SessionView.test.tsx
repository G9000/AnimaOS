import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { daemon } from '../../lib/daemon-api';
import { memoryFixture } from '../../test/memory';

import { emptyLiveRun } from '../../lib/session-events';
import type { AgentDetail } from '../../lib/types';
import { automationFixture } from '../../test/automations';
import { runFixture } from '../../test/live';
import { sessionFixture } from '../../test/sessions';
import { totalsFixture } from '../../test/usage';
import { SessionView, type SessionViewProps } from './SessionView';

const agent: AgentDetail = {
  id: 'agent-main',
  name: 'Nova',
  provider: 'openai',
  model: 'gpt-5.4',
  toolNames: [],
  created_at_ms: 1,
  status: 'Idle',
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  messages: [],
};
const readOnly = {
  send: false,
  steer: false,
  stop: true,
  rename: false,
  archive: true,
  delete: false,
  compact: false,
  export: true,
};

function renderView(overrides: Partial<SessionViewProps> = {}) {
  const props: SessionViewProps = {
    agent,
    session: null,
    messages: [],
    hasOlder: false,
    loadingOlder: false,
    onLoadOlder: vi.fn(),
    missing: false,
    telegramAvailable: true,
    scrollerRef: createRef<HTMLDivElement>(),
    onSuggestion: vi.fn(),
    composer: {
      draft: '',
      setDraft: vi.fn(),
      sending: false,
      disabled: false,
      offline: false,
      onSend: vi.fn(),
      error: null,
      onDismissError: vi.fn(),
    },
    onNewChat: vi.fn(),
    onOpenWork: vi.fn(),
    onRename: vi.fn().mockResolvedValue(true),
    onToggleArchived: vi.fn(),
    onExport: vi.fn(),
    ...overrides,
  };
  const { rerender } = render(<SessionView {...props} />);
  return {
    ...props,
    rerender: (next: Partial<SessionViewProps>) =>
      rerender(<SessionView {...props} {...next} />),
  };
}

beforeEach(() => {
  // The header reads the session's usage; most tests have none.
  vi.spyOn(daemon, 'getSession').mockResolvedValue(sessionFixture('room-7'));
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('SessionView', () => {
  it('refreshes the context estimate while historical messages and visible runs stay fixed', async () => {
    const initialContext = {
      estimatedTokens: 800,
      budgetTokens: 1000,
      replyReserveTokens: 200,
    };
    const getSession = vi
      .mocked(daemon.getSession)
      .mockResolvedValueOnce(
        sessionFixture('chat:plans', { contextUsage: initialContext }),
      )
      .mockResolvedValueOnce(
        sessionFixture('chat:plans', {
          contextUsage: { ...initialContext, estimatedTokens: 900 },
        }),
      );
    const historyRead = vi
      .spyOn(daemon, 'sessionMessages')
      .mockResolvedValue({ messages: [], nextBefore: null });
    const fixedMessages = [
      {
        id: 'saved-message',
        role: 'User' as const,
        content: { text: 'A fixed historical message' },
        created_at_ms: 1,
      },
    ];
    const view = renderView({
      session: sessionFixture('chat:plans'),
      messages: fixedMessages,
      runs: [],
      historyAnchor: {
        messageId: 'saved-message',
        loading: false,
        error: null,
        onReturnToLatest: vi.fn(),
      },
      contextRefreshKey: 'activity:1',
    });
    await waitFor(() =>
      expect(screen.getByRole('progressbar')).toHaveAttribute('value', '800'),
    );
    expect(getSession).toHaveBeenCalledTimes(1);
    view.rerender({ contextRefreshKey: 'activity:2' });
    await waitFor(() =>
      expect(screen.getByRole('progressbar')).toHaveAttribute('value', '900'),
    );
    expect(getSession).toHaveBeenCalledTimes(2);
    expect(historyRead).not.toHaveBeenCalled();
    expect(screen.getByText('A fixed historical message')).toBeVisible();
  });

  it('waits for a successful history window before jumping to a message already in the transcript', () => {
    const scrollIntoView = vi.fn();
    const previous = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      const historyAnchor = {
        messageId: 'saved-message',
        loading: true,
        error: null,
        onReturnToLatest: vi.fn(),
      };
      const view = renderView({
        session: sessionFixture('chat:plans'),
        messages: [
          {
            id: 'saved-message',
            role: 'User',
            content: { text: 'An already loaded match' },
            created_at_ms: 1,
          },
        ],
        historyAnchor,
      });
      expect(scrollIntoView).not.toHaveBeenCalled();
      view.rerender({
        historyAnchor: {
          ...historyAnchor,
          loading: false,
          error: 'Read unavailable',
        },
      });
      expect(scrollIntoView).not.toHaveBeenCalled();
      view.rerender({ historyAnchor: { ...historyAnchor, loading: false } });
      expect(scrollIntoView).toHaveBeenCalledTimes(1);
    } finally {
      Element.prototype.scrollIntoView = previous;
    }
  });

  it('shows bounded-history loading, failure and an explicit return to latest', async () => {
    const onReturnToLatest = vi.fn();
    const historyAnchor = {
      messageId: 'saved-message',
      loading: true,
      error: null,
      onReturnToLatest,
    };
    const view = renderView({
      session: sessionFixture('chat:plans'),
      historyAnchor,
    });
    expect(screen.getByText('Loading saved message…')).toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Return to latest messages' }),
    );
    expect(onReturnToLatest).toHaveBeenCalledOnce();
    view.rerender({
      historyAnchor: {
        ...historyAnchor,
        loading: false,
        error: 'This saved message is no longer available.',
      },
    });
    expect(screen.getByRole('alert')).toHaveTextContent(
      'This saved message is no longer available.',
    );
    expect(
      screen.queryByText('Loading saved message…'),
    ).not.toBeInTheDocument();
  });

  it('says the chat is getting long only near the budget, with a truthful over-budget reading', () => {
    const usage = (estimatedTokens: number) => ({
      estimatedTokens,
      budgetTokens: 1000,
      replyReserveTokens: 200,
    });
    const view = renderView({
      session: sessionFixture('chat:plans', { contextUsage: usage(500) }),
    });
    // Half full is not news.
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();

    view.rerender({
      session: sessionFixture('chat:plans', { contextUsage: usage(820) }),
    });
    const meter = screen.getByRole('progressbar', {
      name: 'Estimated conversation context',
    });
    expect(meter).toHaveAttribute('max', '1000');
    expect(meter).toHaveAttribute('value', '820');
    expect(screen.getByText(/getting long · 82% of Nova’s working memory/)).toBeVisible();
    for (const excluded of ['instructions', 'tool schemas', 'files', 'draft', 'reply'])
      expect(meter).toHaveAttribute('title', expect.stringContaining(excluded));

    view.rerender({
      session: sessionFixture('chat:plans', { contextUsage: usage(1500) }),
    });
    expect(meter).toHaveAttribute('value', '1000');
    expect(screen.getByText(/150% of Nova’s working memory/)).toBeVisible();
  });

  it('says nothing about context for old or invalid readings', () => {
    const view = renderView({ session: sessionFixture('old-daemon') });
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(screen.queryByText(/working memory/)).not.toBeInTheDocument();
    for (const contextUsage of [
      { estimatedTokens: 0, budgetTokens: 0, replyReserveTokens: 0 },
      { estimatedTokens: -1, budgetTokens: 1000, replyReserveTokens: 200 },
      {
        estimatedTokens: Number.NaN,
        budgetTokens: 1000,
        replyReserveTokens: 200,
      },
      { estimatedTokens: 900, budgetTokens: 1000, replyReserveTokens: -1 },
    ]) {
      view.rerender({ session: sessionFixture('invalid', { contextUsage }) });
      expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    }
  });

  it('leaves the main chat’s header to the top bar', () => {
    renderView({
      session: sessionFixture('chat:main', { mainChat: true }),
    });
    expect(
      screen.queryByRole('button', { name: 'Rename' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Export' }),
    ).not.toBeInTheDocument();
  });

  it('keeps the saved summary reachable once when its boundary is outside the loaded history', async () => {
    const summary = {
      text: 'Keep **Saturday free**. [Unsafe](javascript:alert(1))',
      throughMessageId: 'unloaded-message',
      createdAtMs: 1_725_000_000_000,
      sourceMessageCount: 12,
    };
    renderView({
      session: sessionFixture('chat:plans', {
        summary,
        contextTrimmed: {
          droppedThroughMessageId: 'unloaded-message',
          atMs: 1,
        },
      }),
      messages: [
        {
          id: 'latest-message',
          role: 'Assistant',
          content: { text: 'A newer reply' },
          created_at_ms: 2,
        },
      ],
    });

    const disclosure = screen.getByText('What anima kept');
    expect(screen.getAllByText('What anima kept')).toHaveLength(1);
    expect(screen.getByText('Saturday free')).not.toBeVisible();
    await userEvent.click(disclosure);
    expect(screen.getByText('Saturday free')).toBeVisible();
    expect(screen.getByText('Saturday free').tagName).toBe('STRONG');
    expect(screen.getByText(/12 messages/)).toBeVisible();
    expect(screen.getByText('Unsafe')).not.toHaveAttribute(
      'href',
      expect.stringMatching(/^javascript:/),
    );
  });

  it('updates an open saved summary and retains it after a later compaction error', async () => {
    const first = sessionFixture('chat:plans', {
      summary: {
        text: 'The first saved summary.',
        throughMessageId: 'old-1',
        createdAtMs: 1,
        sourceMessageCount: 2,
      },
    });
    const view = renderView({ session: first });
    await userEvent.click(screen.getByText('What anima kept'));
    view.rerender({
      session: {
        ...first,
        summary: {
          ...first.summary!,
          text: 'The updated saved summary.',
          sourceMessageCount: 4,
        },
        compactionError: { message: 'model unavailable', atMs: 2 },
      },
    });
    expect(screen.getByText('The updated saved summary.')).toBeVisible();
    expect(
      screen.queryByText('The first saved summary.'),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/could not be summarized: model unavailable/),
    ).toBeVisible();
    expect(screen.getAllByText('What anima kept')).toHaveLength(1);
  });

  it('omits an absent summary and collapses the disclosure when the session changes', async () => {
    const view = renderView({ session: sessionFixture('chat:first') });
    expect(screen.queryByText('What anima kept')).not.toBeInTheDocument();
    const summary = {
      text: 'Saved context.',
      throughMessageId: 'old-1',
      createdAtMs: 1,
      sourceMessageCount: 2,
    };
    view.rerender({ session: sessionFixture('chat:first', { summary }) });
    await userEvent.click(screen.getByText('What anima kept'));
    expect(screen.getByText('Saved context.')).toBeVisible();
    view.rerender({ session: sessionFixture('chat:second', { summary }) });
    expect(screen.getByText('Saved context.')).not.toBeVisible();
  });

  it('offers native speech for a saved assistant reply without starting playback', () => {
    const ensureSession = vi.fn().mockResolvedValue('room-7');
    const audio = vi.fn();
    vi.stubGlobal('Audio', audio);
    try {
      renderView({
        session: sessionFixture('room-7'),
        messages: [
          {
            id: 'assistant-saved',
            role: 'Assistant',
            content: { text: 'A saved reply to speak.' },
            created_at_ms: 1,
          },
        ],
        composer: {
          draft: '',
          setDraft: vi.fn(),
          sending: false,
          disabled: false,
          offline: false,
          onSend: vi.fn(),
          error: null,
          onDismissError: vi.fn(),
          nativeVoice: {
            agentId: 'agent-main',
            sessionId: 'room-7',
            ensureSession,
          },
        },
      });

      expect(
        screen.getByRole('button', {
          name: 'Generate speech with OpenAI API',
        }),
      ).toBeVisible();
      expect(ensureSession).not.toHaveBeenCalled();
      expect(audio).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('uses a single read-aloud controller for assistant replies and cancels on session changes', async () => {
    class Utterance {
      onstart: (() => void) | null = null;
      onend: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor(public text: string) {}
    }
    const speak = vi.fn();
    const cancel = vi.fn();
    vi.stubGlobal('SpeechSynthesisUtterance', Utterance);
    vi.stubGlobal('speechSynthesis', { speak, cancel });
    try {
      const message = (
        id: string,
        role: 'User' | 'Assistant' | 'Tool' | 'System',
        text: string,
      ) => ({ id, role, content: { text }, created_at_ms: 1 });
      const view = renderView({
        session: sessionFixture('first'),
        messages: [
          message('user', 'User', 'User text'),
          message('assistant', 'Assistant', 'Reply\u200B'),
          message('tool', 'Tool', 'Tool secret'),
          message('system', 'System', 'System secret'),
          message('empty', 'Assistant', ''),
        ],
      });
      expect(
        screen.getAllByRole('button', { name: 'Read aloud' }),
      ).toHaveLength(1);
      await userEvent.click(screen.getByRole('button', { name: 'Read aloud' }));
      expect(speak.mock.calls[0][0].text).toBe('Reply⟨U+200B⟩');
      act(() => speak.mock.calls[0][0].onstart?.());
      expect(
        screen.getByRole('button', { name: 'Stop reading' }),
      ).toBeVisible();
      await act(async () => {
        view.rerender({ session: sessionFixture('second') });
        await Promise.resolve();
      });
      expect(cancel).toHaveBeenCalledTimes(2);
      expect(speak.mock.calls[0][0].onend).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });
  it('saves a message to the viewed session’s agent as a memory', async () => {
    const user = userEvent.setup();
    const saveMemory = vi
      .spyOn(daemon, 'saveMemory')
      .mockResolvedValue(memoryFixture('mem-1'));
    renderView({
      agent: { ...agent, id: 'helper-7', name: 'Researcher' },
      session: sessionFixture('room-9', {
        agentId: 'helper-7',
        kind: 'helper',
      }),
      messages: [
        {
          id: 'm1',
          role: 'User',
          content: { text: 'Found the answer' },
          created_at_ms: 1,
        },
      ],
    });

    await user.click(screen.getByRole('button', { name: 'Save to memory' }));
    expect(
      await screen.findByRole('button', { name: '✓ Saved to memory' }),
    ).toBeDisabled();
    expect(saveMemory).toHaveBeenCalledWith(
      expect.objectContaining({
        agentId: 'helper-7',
        agentName: 'Researcher',
        content: 'Found the answer',
      }),
    );
    expect(saveMemory.mock.calls[0]?.[0]).not.toHaveProperty('sessionId');
  });

  it('saves a helper’s message with the session’s own agent when the agent shown is the companion', async () => {
    const user = userEvent.setup();
    const saveMemory = vi
      .spyOn(daemon, 'saveMemory')
      .mockResolvedValue(memoryFixture('mem-2'));
    // The helper is not in the agents list, so the page passes the companion.
    renderView({
      session: sessionFixture('room-10', {
        agentId: 'helper-9',
        kind: 'helper',
      }),
      messages: [
        {
          id: 'm1',
          role: 'User',
          content: { text: 'Found it' },
          created_at_ms: 1,
        },
      ],
    });

    await user.click(screen.getByRole('button', { name: 'Save to memory' }));
    expect(
      await screen.findByRole('button', { name: '✓ Saved to memory' }),
    ).toBeDisabled();
    const saved = saveMemory.mock.calls[0][0];
    expect(saved.agentId).toBe('helper-9');
    expect(saved.agentName).not.toBe('Nova');
    expect(saved.agentName).toBeTruthy();
  });

  it('the header shows the session’s usage line and hides it when there are no calls', async () => {
    vi.mocked(daemon.getSession).mockResolvedValue(
      sessionFixture('room-7', {
        usage: totalsFixture({
          calls: 2,
          totalTokens: 12_300,
          costMicros: 40_000,
        }),
      }),
    );
    renderView({ session: sessionFixture('room-7') });
    expect(await screen.findByText('12.3k tokens · $0.04')).toBeVisible();
    expect(daemon.getSession).toHaveBeenCalledWith('agent-main', 'room-7');
  });

  it('shows no usage line for a session without calls', async () => {
    vi.mocked(daemon.getSession).mockResolvedValue(
      sessionFixture('room-7', { usage: totalsFixture() }),
    );
    renderView({ session: sessionFixture('room-7') });
    await waitFor(() => expect(daemon.getSession).toHaveBeenCalled());
    expect(screen.queryByText(/tokens ·/)).not.toBeInTheDocument();
  });

  it('the line refreshes when a run finishes', async () => {
    const working = runFixture('run_1', { status: 'working' });
    const done = runFixture('run_1', { status: 'completed' });
    vi.mocked(daemon.getSession).mockResolvedValue(
      sessionFixture('room-7', {
        usage: totalsFixture({
          calls: 1,
          totalTokens: 100,
          costMicros: 10_000,
        }),
      }),
    );
    const props = renderView({
      session: sessionFixture('room-7'),
      runs: [emptyLiveRun(working)],
    });
    expect(await screen.findByText('100 tokens · $0.01')).toBeVisible();
    expect(daemon.getSession).toHaveBeenCalledTimes(1);

    vi.mocked(daemon.getSession).mockResolvedValue(
      sessionFixture('room-7', {
        usage: totalsFixture({
          calls: 2,
          totalTokens: 250,
          costMicros: 20_000,
        }),
      }),
    );
    props.rerender({ runs: [emptyLiveRun(done)] });
    expect(await screen.findByText('250 tokens · $0.02')).toBeVisible();
    expect(daemon.getSession).toHaveBeenCalledTimes(2);
  });

  it('opens a new chat on the welcome screen with the companion composer', () => {
    renderView();
    expect(
      screen.getByRole('heading', { name: 'Say something to Nova' }),
    ).toBeVisible();
    expect(screen.getByPlaceholderText('Message Nova…')).toBeVisible();
  });

  it('shows a chat with its header, older history, rename, archive, and export', async () => {
    const user = userEvent.setup();
    const props = renderView({
      session: sessionFixture('chat:plans', { title: 'Plans' }),
      messages: [
        {
          id: 'm1',
          role: 'Assistant',
          content: { text: 'Here is the plan' },
          created_at_ms: 1,
        },
      ],
      hasOlder: true,
    });

    expect(screen.getByRole('heading', { name: 'Plans' })).toBeVisible();
    expect(
      screen.getByText('Chat', { selector: '.session-kind-badge' }),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Load older messages' }),
    );
    expect(props.onLoadOlder).toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Rename' }));
    const title = screen.getByRole('textbox', { name: 'Session title' });
    await user.clear(title);
    await user.type(title, 'Offsite{Enter}');
    expect(props.onRename).toHaveBeenCalledWith('Offsite');
    await user.click(screen.getByRole('button', { name: 'Archive' }));
    expect(props.onToggleArchived).toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Export' }));
    expect(props.onExport).toHaveBeenCalled();
  });

  it('replies in a Telegram session through the Telegram composer', () => {
    renderView({
      session: sessionFixture('telegram:tg-1', {
        kind: 'telegram',
        title: 'Telegram · @nova_bot',
      }),
    });
    expect(screen.getByPlaceholderText('Reply on Telegram…')).toBeVisible();
  });

  it('replies to a check-in through its own composer', () => {
    renderView({
      session: sessionFixture('schedule:daily', {
        kind: 'checkin',
        origin: 'schedule',
        title: 'Check-in · goals',
      }),
    });
    expect(
      screen.getByPlaceholderText('Reply to this check-in…'),
    ).toBeVisible();
  });

  it('labels native Discord sessions and uses the ordinary session composer', () => {
    renderView({
      session: sessionFixture('discord:dc-1', {
        kind: 'discord',
        origin: 'discord',
        title: 'Discord · private-chat',
      }),
      discordAvailable: true,
    });
    expect(
      screen.getByText('Discord', { selector: '.session-kind-badge' }),
    ).toBeVisible();
    expect(screen.getByPlaceholderText('Reply on Discord…')).toBeVisible();
  });

  it('blocks Discord replies when its selected connector cannot deliver', () => {
    renderView({
      session: sessionFixture('discord:dc-1', {
        kind: 'discord',
        origin: 'discord',
      }),
      discordAvailable: false,
    });
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('note')).toHaveTextContent(
      'Check its status in Connectors',
    );
  });

  it.each(['degraded', 'loading', 'missing credentials'])(
    'keeps Stop available while Discord delivery is unavailable (%s)',
    async () => {
      const user = userEvent.setup();
      const stop = vi.fn();
      renderView({
        session: sessionFixture('discord:dc-1', {
          kind: 'discord',
          origin: 'discord',
        }),
        discordAvailable: false,
        composer: {
          draft: 'Do not send',
          setDraft: vi.fn(),
          sending: false,
          disabled: false,
          offline: false,
          onSend: vi.fn(),
          error: null,
          onDismissError: vi.fn(),
          runActive: true,
          onStop: stop,
        },
      });
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: 'Send' }),
      ).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Stop' }));
      expect(stop).toHaveBeenCalledOnce();
    },
  );

  it('shows a message that is still on its way', () => {
    renderView({
      session: sessionFixture('chat:plans', { title: 'Plans' }),
      pending: [
        {
          key: 'k1',
          text: 'Book the train',
          createdAtMs: 1,
          status: 'retrying',
        },
      ],
    });
    expect(screen.getByText('Book the train')).toBeVisible();
    expect(screen.getByText('Not delivered yet · retrying…')).toBeVisible();
  });

  it('shows a read-only note instead of a composer for jobs', async () => {
    const props = renderView({
      session: sessionFixture('job:1', {
        kind: 'job',
        title: 'Job · Report',
        capabilities: readOnly,
      }),
    });

    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('note')).toHaveTextContent(
      'Job sessions are read-only',
    );
    expect(screen.getByText('No messages yet.')).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Open Work' }));
    expect(props.onOpenWork).toHaveBeenCalled();
  });

  it('shows the reply in progress, outcomes, and where the companion’s view begins', async () => {
    const user = userEvent.setup();
    const onCompact = vi.fn();
    const onSendAgain = vi.fn();
    const failed = runFixture('run_f', {
      sessionId: 'chat:plans',
      status: 'failed',
      error: { code: 'model_error', message: 'provider unavailable' },
    });
    const working = runFixture('run_w', {
      sessionId: 'chat:plans',
      status: 'running',
      createdAtMs: 5,
      startedAtMs: Date.now(),
      input: { text: 'And Sunday?', attachmentIds: [], skill: null },
    });
    renderView({
      session: sessionFixture('chat:plans', {
        title: 'Plans',
        activeRuns: 1,
        contextTrimmed: { droppedThroughMessageId: 'm1', atMs: 1 },
      }),
      messages: [
        {
          id: 'm1',
          role: 'User',
          content: { text: 'Plan Saturday', metadata: { runId: 'run_f' } },
          created_at_ms: 1,
        },
      ],
      runs: [
        emptyLiveRun(failed),
        {
          ...emptyLiveRun(working),
          steps: [{ stepId: 'run_w:1', text: 'Sunday is free', textOffset: 0 }],
        },
      ],
      actions: { onCompact, onSendAgain },
    });

    expect(
      screen.getByText(
        'Older messages are outside anima’s working memory · they stay here in the chat',
      ),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Compact' }));
    expect(onCompact).toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onSendAgain).toHaveBeenCalledWith(failed);
    expect(screen.getByText('And Sunday?')).toBeVisible();
    expect(screen.getByText('Sunday is free')).toBeVisible();
    // The live run speaks for itself: no second "thinking" indicator.
    expect(screen.queryByText('Nova is thinking')).not.toBeInTheDocument();
  });

  it('announces a finished reply and says when earlier messages could not be summarized', () => {
    renderView({
      session: sessionFixture('chat:plans', {
        title: 'Plans',
        compactionError: { message: 'model unavailable', atMs: 2 },
      }),
      announcement: 'Nova replied.',
    });

    expect(screen.getByText('Nova replied.')).toHaveAttribute(
      'aria-live',
      'polite',
    );
    const note = screen.getByText(
      'Earlier messages could not be summarized: model unavailable',
    );
    expect(note).toBeVisible();
    // The note sits inside a status region that stays mounted, so its text
    // is announced when it arrives (S3b-E).
    expect(note.closest('[role="status"]')).not.toBeNull();
  });

  it('still says the companion is thinking about a run it has no view of', () => {
    renderView({
      session: sessionFixture('chat:plans', { title: 'Plans', activeRuns: 1 }),
      runs: [
        emptyLiveRun(
          runFixture('run_old', {
            sessionId: 'chat:plans',
            status: 'completed',
            finishedAtMs: 1,
          }),
        ),
      ],
    });

    expect(screen.getByText('Nova is thinking')).toBeVisible();
  });

  it('offers the main chat when the session was deleted elsewhere', async () => {
    const props = renderView({
      session: sessionFixture('chat:gone'),
      missing: true,
    });
    expect(screen.getByText('This session was deleted.')).toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Return to main chat' }),
    );
    expect(props.onNewChat).toHaveBeenCalled();
  });

  it('names a check-in’s schedule and opens its automation', async () => {
    const user = userEvent.setup();
    const automation = automationFixture('daily');
    const props = renderView({
      session: sessionFixture('schedule:daily', {
        kind: 'checkin',
        origin: 'schedule',
        title: 'Check-in · goals',
      }),
      automation,
      onEditAutomation: vi.fn(),
    });

    expect(screen.getByText('Check-in · every 30 min')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Edit automation' }));
    expect(props.onEditAutomation).toHaveBeenCalledWith(automation);
  });
});
