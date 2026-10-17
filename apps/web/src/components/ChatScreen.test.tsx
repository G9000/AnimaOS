import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { AgentDetail, ChatMessage } from '../lib/types';
import {
  createDaemonClient,
  type LearningReview,
  type Run,
} from '@animaOS-SWARM/sdk';
import { toChatMessage } from '../lib/daemon-api';
import { formatTime } from './ui-bits';
import { Composer, MessageList } from './ChatScreen';
import { SLASH_COMMANDS } from '../lib/slash-commands';
import {
  applyEvent,
  EMPTY_LIVE_STATE,
  emptyLiveRun,
  type LiveRun,
} from '../lib/session-events';
import { buildTranscript } from '../lib/transcript';
import { runFixture } from '../test/live';

const messages: AgentDetail['messages'] = [
  {
    id: 'user-message',
    role: 'User',
    content: { text: '**bold**' },
    created_at_ms: 1_725_000_000_000,
  },
  {
    id: 'assistant-message',
    role: 'Assistant',
    content: { text: '## Heading' },
    created_at_ms: 1_725_000_060_000,
  },
  {
    id: 'system-message',
    role: 'System',
    content: { text: '**system marker**' },
    created_at_ms: 1_725_000_120_000,
  },
  {
    id: 'tool-message',
    role: 'Tool',
    content: { text: '## tool marker' },
    created_at_ms: 1_725_000_180_000,
  },
];

const agent: AgentDetail = {
  id: 'agent-1',
  name: 'Nova',
  provider: 'openai',
  model: 'gpt-5',
  toolNames: ['search'],
  created_at_ms: 1_725_000_000_000,
  status: 'Idle',
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  messages,
};

async function openM83SearchCards() {
  for (const button of screen.queryAllByRole('button', {
    name: /^\d+ tool requests?/,
  }))
    fireEvent.click(button);
  for (const button of screen.getAllByRole('button', { name: /^web_search/ }))
    fireEvent.click(button);
}

describe('m83 visible search provenance', () => {
  it('renders saved returned sources and ordinary answer citations without fetching search again', async () => {
    const url = 'https://example.com/actual-result?x=1&y=2';
    const sourceMessages: ChatMessage[] = [
      {
        id: 'search-call',
        role: 'Assistant',
        content: {
          text: '',
          metadata: {
            runId: 'search-run',
            stepId: 'search-run:1',
            toolCalls: [
              {
                id: 'search-call-id',
                name: 'web_search',
                args: { query: 'bounded' },
              },
            ],
          },
        },
        created_at_ms: 1,
      },
      {
        id: 'search-result',
        role: 'Tool',
        content: {
          text: 'canonical native result',
          metadata: {
            runId: 'search-run',
            stepId: 'search-run:1',
            toolCallId: 'search-call-id',
            toolStatus: 'success',
            webSources: [{ title: 'Returned source', url }],
          },
        },
        created_at_ms: 2,
      },
      {
        id: 'search-answer',
        role: 'Assistant',
        content: {
          text: `See [Answer reference](${url}).`,
          metadata: { runId: 'search-run' },
        },
        created_at_ms: 3,
      },
    ];
    render(
      <MessageList
        agent={{ ...agent, messages: sourceMessages }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={buildTranscript({ messages: sourceMessages })}
      />,
    );
    await openM83SearchCards();
    const source = screen.getByRole('link', { name: 'Returned source' });
    expect(source).toHaveAttribute('href', url);
    expect(source.closest('pre')).toBeNull();
    expect(
      screen.getByRole('link', { name: 'Answer reference' }),
    ).toHaveAttribute('href', url);
    expect(screen.getByText('Search sources')).toBeVisible();
  });

  it('does not label an answer-only Markdown URL as a retrieved source', () => {
    const onlyAnswer: ChatMessage[] = [
      {
        id: 'answer-only',
        role: 'Assistant',
        content: { text: '[Answer link](https://example.com/answer)' },
        created_at_ms: 1,
      },
    ];
    render(
      <MessageList
        agent={{ ...agent, messages: onlyAnswer }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    expect(screen.getByRole('link', { name: 'Answer link' })).toBeVisible();
    expect(screen.queryByText('Search sources')).not.toBeInTheDocument();
  });

  it.skipIf(!process.env.ANIMA_M83_CONSOLE_ARTIFACT)(
    'm83 renders exact captured native owner-session sources through SDK and console',
    async () => {
      const { readFile } = await import('node:fs/promises');
      const artifact = JSON.parse(
        await readFile(process.env.ANIMA_M83_CONSOLE_ARTIFACT!, 'utf8'),
      );
      expect(artifact.schemaVersion).toBe(1);
      expect(artifact.endpoint).toBe('https://mcp.exa.ai/mcp');
      expect(artifact.search.status).toBe('success');
      expect(artifact.search.urls.length).toBeGreaterThan(0);
      const fetch = vi.fn(async (url: RequestInfo | URL) =>
        String(url).endsWith('/events')
          ? new Response(
              artifact.events
                .map(
                  (event: { type: string }) =>
                    `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`,
                )
                .join(''),
              { headers: { 'content-type': 'text/event-stream' } },
            )
          : Response.json(artifact.sessionMessages),
      );
      const client = createDaemonClient({ baseUrl: '', fetch });
      const page = await client.sessions.messages(
        'captured-agent',
        'captured-session',
      );
      const captured = page.messages.map(toChatMessage);
      expect(
        captured.some(
          (message) =>
            message.role === 'Assistant' &&
            artifact.search.urls.some((url: string) =>
              message.content.text.includes(url),
            ),
        ),
      ).toBe(true);
      let live = EMPTY_LIVE_STATE;
      for await (const event of client.events.stream('captured-agent'))
        live = applyEvent(live, event);
      const streamSources = Object.values(live.runs).flatMap((run) =>
        run.tools.flatMap((tool) => tool.sources ?? []),
      );
      expect([...new Set(streamSources.map((source) => source.url))]).toEqual(
        artifact.search.urls,
      );
      if (artifact.snapshot) {
        const reconnected = applyEvent(EMPTY_LIVE_STATE, artifact.snapshot);
        const restoredSources = Object.values(reconnected.runs).flatMap((run) =>
          run.tools.flatMap((tool) => tool.sources ?? []),
        );
        expect([
          ...new Set(restoredSources.map((source) => source.url)),
        ]).toEqual(artifact.search.urls);
      }
      const streamed = render(
        <MessageList
          agent={{ ...agent, messages: [] }}
          sending={false}
          scrollerRef={{ current: null }}
          onSuggestion={vi.fn()}
          items={buildTranscript({
            messages: [],
            runs: Object.values(live.runs),
            olderHistory: false,
          })}
        />,
      );
      await openM83SearchCards();
      expect([
        ...new Set(
          screen
            .getAllByRole('list', { name: 'Search sources' })
            .flatMap((list) =>
              within(list)
                .getAllByRole('link')
                .map((link) => link.getAttribute('href')),
            ),
        ),
      ]).toEqual(artifact.search.urls);
      streamed.unmount();
      render(
        <MessageList
          agent={{ ...agent, messages: captured }}
          sending={false}
          scrollerRef={{ current: null }}
          onSuggestion={vi.fn()}
          items={buildTranscript({ messages: captured })}
        />,
      );
      await openM83SearchCards();
      const lists = screen.getAllByRole('list', { name: 'Search sources' });
      const rendered = lists.flatMap((list) =>
        within(list)
          .getAllByRole('link')
          .map((link) => link.getAttribute('href')),
      );
      expect([...new Set(rendered)]).toEqual(artifact.search.urls);
      expect(fetch).toHaveBeenCalledTimes(2);
      expect(
        fetch.mock.calls.every(
          ([url]) =>
            String(url).endsWith('/messages') ||
            String(url).endsWith('/events'),
        ),
      ).toBe(true);
      for (const list of lists)
        for (const link of within(list).getAllByRole('link')) {
          expect(link.closest('pre')).toBeNull();
          expect(link).toHaveAttribute('target', '_blank');
          expect(link).toHaveAttribute(
            'rel',
            expect.stringContaining('noreferrer'),
          );
        }
    },
  );
});

describe('MessageList', () => {
  it('M38_offers_review_for_a_completed_task_with_tool_activity', async () => {
    const user = userEvent.setup();
    const completedRun = runFixture('completed-task', {
      status: 'completed',
      startedAtMs: 1_725_000_030_000,
      finishedAtMs: 1_725_000_090_000,
      toolsStarted: ['memory_search'],
      replyMessageId: 'task-reply',
      actionReceipts: [
        {
          toolCallId: 'call-1',
          name: 'memory_search',
          status: 'succeeded',
          startedAtMs: 1_725_000_040_000,
          finishedAtMs: 1_725_000_041_000,
          durationMs: 1_000,
          argumentsPreview: '{}',
          argumentsTruncated: false,
          resultPreview: 'Found one note.',
          resultTruncated: false,
          durability: 'persistent',
        },
      ],
    });
    const taskMessages: AgentDetail['messages'] = [
      {
        id: 'task-request',
        role: 'User',
        content: { text: 'Find the relevant note.' },
        created_at_ms: 1_725_000_020_000,
      },
      {
        id: 'task-reply',
        role: 'Assistant',
        content: {
          text: 'The note says to review the draft.',
          metadata: { runId: completedRun.id },
        },
        created_at_ms: 1_725_000_090_000,
      },
    ];
    const review = {
      id: 'review-1',
      agentId: 'agent-1',
      sourceRunId: completedRun.id,
      sessionId: completedRun.sessionId,
      trigger: 'manual',
      policyVersion: 1,
      evidenceDigest: 'digest',
      evidenceBrief: 'Task evidence.',
      evidenceAnchors: [completedRun.id],
      state: 'queued',
      reservedAtMs: 1,
      model: 'gpt-5.5',
      usageComplete: false,
      proposals: [],
    } satisfies LearningReview;
    const onRequestLearningReview = vi.fn(async () => review);

    render(
      <MessageList
        agent={{ ...agent, messages: taskMessages }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={buildTranscript({
          messages: taskMessages,
          runs: [emptyLiveRun(completedRun)],
        })}
        actions={{ onRequestLearningReview }}
      />,
    );

    const reviewTask = screen.getByRole('button', { name: 'Review this task' });
    expect(reviewTask).toBeVisible();
    await user.click(reviewTask);
    expect(onRequestLearningReview).toHaveBeenCalledWith(completedRun);
    expect(
      await screen.findByRole('button', { name: 'Review requested' }),
    ).toBeDisabled();
  });

  const ineligibleCompletedRuns: Array<[string, Partial<Run>]> = [
    ['legacy missing receipt data', { actionReceipts: undefined }],
    [
      'started receipt',
      {
        actionReceipts: [
          {
            toolCallId: 'call-1',
            name: 'memory_search',
            status: 'started' as const,
            startedAtMs: 1,
            finishedAtMs: null,
            durationMs: null,
            argumentsPreview: '{}',
            argumentsTruncated: false,
            resultPreview: null,
            resultTruncated: false,
            durability: 'persistent' as const,
          },
        ],
      },
    ],
    [
      'temporary receipt',
      {
        actionReceipts: [
          {
            toolCallId: 'call-1',
            name: 'memory_search',
            status: 'succeeded' as const,
            startedAtMs: 1,
            finishedAtMs: 2,
            durationMs: 1,
            argumentsPreview: '{}',
            argumentsTruncated: false,
            resultPreview: 'ok',
            resultTruncated: false,
            durability: 'memory' as const,
          },
        ],
      },
    ],
    ['failed run', { error: { code: 'failed', message: 'Failed' } }],
    ['stopped run', { stop: { requestedAtMs: 2 } }],
    ['scheduled run', { source: 'schedule' as const }],
    [
      'silent calendar follow-up',
      { source: 'api' as const, sourceRef: 'calendar-write:task' },
    ],
    ['delegated run', { parentRunId: 'parent-run' }],
  ];

  it.each(ineligibleCompletedRuns)(
    'does not offer review for a completed run with %s',
    (_label, overrides) => {
      const base = runFixture('completed-task', {
        status: 'completed',
        startedAtMs: 1,
        finishedAtMs: 3,
        toolsStarted: ['memory_search'],
        replyMessageId: 'reply',
        actionReceipts: [
          {
            toolCallId: 'call-1',
            name: 'memory_search',
            status: 'succeeded',
            startedAtMs: 1,
            finishedAtMs: 2,
            durationMs: 1,
            argumentsPreview: '{}',
            argumentsTruncated: false,
            resultPreview: 'ok',
            resultTruncated: false,
            durability: 'persistent',
          },
        ],
      });
      const messages: AgentDetail['messages'] = [
        {
          id: 'task-request',
          role: 'User',
          content: { text: 'Check the note.' },
          created_at_ms: 1,
        },
        {
          id: 'reply',
          role: 'Assistant',
          content: { text: 'Checked.', metadata: { runId: base.id } },
          created_at_ms: 3,
        },
      ];
      const run = { ...base, ...overrides };
      const items = buildTranscript({ messages, runs: [emptyLiveRun(run)] });
      expect(items.some((item) => item.kind === 'learning-review')).toBe(false);
    },
  );

  it('reveals hidden characters consistently with read-aloud text', () => {
    render(
      <MessageList
        agent={{
          ...agent,
          messages: [
            { ...messages[1], content: { text: 'Hello\u200B friend' } },
          ],
        }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    expect(screen.getByText('Hello⟨U+200B⟩ friend')).toBeVisible();
  });
  it('shows legacy attachments on an empty-text message', () => {
    render(
      <MessageList
        agent={{
          ...agent,
          messages: [
            {
              ...messages[0],
              content: {
                text: '',
                attachments: [{ type: 'file', name: 'brief.txt' }],
              },
            },
          ],
        }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    expect(screen.getByText('brief.txt')).toBeVisible();
  });
  it('preserves the reading position when new messages arrive and offers an explicit jump', async () => {
    const user = userEvent.setup();
    const scrollerRef = { current: null as HTMLDivElement | null };
    const view = render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={scrollerRef}
        onSuggestion={vi.fn()}
      />,
    );
    const scroller = screen.getByLabelText('Conversation with Nova');
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1200 },
      clientHeight: { configurable: true, value: 400 },
    });
    scroller.scrollTop = 100;
    fireEvent.scroll(scroller);
    view.rerender(
      <MessageList
        agent={{
          ...agent,
          messages: [...messages, { ...messages[0], id: 'new-message' }],
        }}
        sending={false}
        scrollerRef={scrollerRef}
        onSuggestion={vi.fn()}
      />,
    );
    expect(scroller.scrollTop).toBe(100);
    await user.click(screen.getByRole('button', { name: '↓ Jump to latest' }));
    expect(scroller.scrollTop).toBe(1200);
    expect(
      screen.queryByRole('button', { name: '↓ Jump to latest' }),
    ).not.toBeInTheDocument();
  });
  it('searches literal conversation text and reports no matches', async () => {
    const user = userEvent.setup();
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    await user.click(
      screen.getByRole('button', { name: 'Search conversation' }),
    );
    await user.type(
      screen.getByRole('searchbox', { name: 'Search messages' }),
      'Heading',
    );
    expect(screen.getByRole('status')).toHaveTextContent('1 of 1');
    await user.clear(
      screen.getByRole('searchbox', { name: 'Search messages' }),
    );
    fireEvent.change(
      screen.getByRole('searchbox', { name: 'Search messages' }),
      { target: { value: '[missing]' } },
    );
    expect(screen.getByRole('status')).toHaveTextContent('No matches');
  });

  it('copies original message Markdown', async () => {
    const user = userEvent.setup();
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    await user.click(
      screen.getAllByRole('button', { name: 'Copy message' })[0],
    );
    expect(await navigator.clipboard.readText()).toBe('**bold**');
    expect(screen.getByRole('button', { name: 'Copied' })).toBeVisible();
  });
  it('offers Save to memory on user and assistant messages only when the action is provided', async () => {
    const user = userEvent.setup();
    const onSaveToMemory = vi
      .fn()
      .mockResolvedValue({ kind: 'saved', shortened: false });
    const { rerender } = render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole('button', { name: 'Save to memory' }),
    ).not.toBeInTheDocument();

    rerender(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        actions={{ onSaveToMemory }}
      />,
    );
    // The user and the assistant message, not the system or tool ones.
    const buttons = screen.getAllByRole('button', { name: 'Save to memory' });
    expect(buttons).toHaveLength(2);

    await user.click(buttons[1]);
    expect(onSaveToMemory).toHaveBeenCalledWith(messages[1]);
    expect(
      await screen.findByRole('button', { name: '✓ Saved to memory' }),
    ).toBeDisabled();
  });

  it('does not offer Save to memory on tool or system messages', () => {
    render(
      <MessageList
        agent={{ ...agent, messages: messages.slice(2) }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        actions={{ onSaveToMemory: vi.fn() }}
      />,
    );
    expect(screen.getByText('system · **system marker**')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Save to memory' }),
    ).not.toBeInTheDocument();
  });

  it('shows a message saved before as saved when its bubble mounts again', () => {
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        actions={{
          onSaveToMemory: vi.fn(),
          savedToMemory: (id) =>
            id === 'user-message' ? { shortened: false } : null,
        }}
      />,
    );
    expect(
      screen.getAllByRole('button', { name: '✓ Saved to memory' }),
    ).toHaveLength(1);
    expect(
      screen.getAllByRole('button', { name: 'Save to memory' }),
    ).toHaveLength(1);
  });

  it('renders Markdown bubbles, literal event pills, and tool results as cards', async () => {
    const user = userEvent.setup();
    const scrollerRef = { current: null };
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={scrollerRef}
        onSuggestion={vi.fn()}
      />,
    );

    expect(screen.getByText('bold').tagName).toBe('STRONG');
    expect(
      screen.getByRole('heading', { level: 2, name: 'Heading' }),
    ).toBeVisible();
    expect(screen.getByText('system · **system marker**')).toBeVisible();
    expect(screen.getByText('system · **system marker**').tagName).toBe('SPAN');
    // Tool messages are no longer grey pills (spec §15.1).
    expect(screen.queryByText(/^tool · /)).not.toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: '1 tool request · <1s' }),
    );
    await user.click(screen.getByRole('button', { name: /^tool\b/ }));
    expect(screen.getByText('## tool marker').tagName).toBe('PRE');
    expect(
      screen.queryByRole('heading', { name: 'tool marker' }),
    ).not.toBeInTheDocument();
  });

  it('labels a stopped reply', () => {
    render(
      <MessageList
        agent={{
          ...agent,
          messages: [
            {
              id: 'stopped',
              role: 'Assistant',
              content: { text: 'Half an answer', metadata: { stopped: true } },
              created_at_ms: 1_725_000_000_000,
            },
          ],
        }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );

    expect(screen.getByText('Stopped')).toBeVisible();
  });

  it('labels an incomplete reply', () => {
    render(
      <MessageList
        agent={{
          ...agent,
          messages: [
            {
              id: 'incomplete',
              role: 'Assistant',
              content: {
                text: 'Half an answer',
                metadata: { incomplete: true },
              },
              created_at_ms: 1_725_000_000_000,
            },
          ],
        }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );

    expect(screen.getByText('Incomplete')).toBeVisible();
  });

  it('anchors a message id shared by a bubble and its tool block to the bubble alone', async () => {
    // An assistant message with both text and tool calls produces two
    // transcript items that both cite its id; only one may own it, or
    // jump/highlight are ambiguous and unmounting either item deletes the
    // other's registration.
    const calledOn: Element[] = [];
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function (this: Element) {
      calledOn.push(this);
    };
    try {
      const user = userEvent.setup();
      render(
        <MessageList
          agent={{
            ...agent,
            messages: [
              {
                id: 'a1',
                role: 'Assistant',
                content: {
                  text: 'Let me check.',
                  metadata: {
                    toolCalls: [{ id: 'call_1', name: 'calculate', args: {} }],
                  },
                },
                created_at_ms: 1_725_000_000_000,
              },
            ],
          }}
          sending={false}
          scrollerRef={{ current: null }}
          onSuggestion={vi.fn()}
        />,
      );

      const bubbleAnchor = screen
        .getByText('Let me check.')
        .closest('.studio-message-anchor');
      const toolAnchor = screen
        .getByRole('button', { name: /1 tool request/ })
        .closest('.studio-message-anchor');
      expect(bubbleAnchor).not.toBeNull();
      expect(bubbleAnchor).not.toBe(toolAnchor);

      await user.click(
        screen.getByRole('button', { name: 'Search conversation' }),
      );
      await user.type(
        screen.getByRole('searchbox', { name: 'Search messages' }),
        'Let me check',
      );
      expect(bubbleAnchor).toHaveAttribute('data-search-match', 'true');
      expect(toolAnchor).not.toHaveAttribute('data-search-match');

      fireEvent.keyDown(
        screen.getByRole('searchbox', { name: 'Search messages' }),
        { key: 'Enter' },
      );
      expect(calledOn).toContain(bubbleAnchor);
      expect(calledOn).not.toContain(toolAnchor);
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it('renders the context-trimmed divider without nesting its Compact button inside the separator', () => {
    const onCompact = vi.fn();
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={[{ kind: 'trimmed', key: 'trimmed' }]}
        actions={{ onCompact }}
      />,
    );

    const separator = screen.getByRole('separator');
    const button = screen.getByRole('button', { name: 'Compact' });
    expect(separator.contains(button)).toBe(false);
  });

  it('disables Compact and shows its progress while pending (S3b-C)', () => {
    const onCompact = vi.fn();
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={[{ kind: 'trimmed', key: 'trimmed' }]}
        actions={{ onCompact, compacting: true }}
      />,
    );

    const button = screen.getByRole('button', { name: 'Compacting…' });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onCompact).not.toHaveBeenCalled();
  });

  it('renders a helper session’s delegated turn from the delegating companion, not the owner', () => {
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={[
          {
            kind: 'delegated',
            key: 'delegated-1',
            from: 'Nova',
            message: {
              id: 'delegated-1',
              role: 'User',
              content: { text: 'Compare vendors' },
              created_at_ms: 1_725_000_000_000,
            },
          },
        ]}
      />,
    );

    expect(screen.getByText('From Nova')).toBeVisible();
    expect(screen.getByText('Compare vendors')).toBeVisible();
  });

  it('retains the conversation label and message timestamps', () => {
    const scrollerRef = { current: null };
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={scrollerRef}
        onSuggestion={vi.fn()}
      />,
    );

    expect(screen.getByLabelText('Conversation with Nova')).toBeVisible();
    expect(
      screen.getByText(formatTime(messages[0].created_at_ms)),
    ).toBeVisible();
    expect(
      screen.getByText(formatTime(messages[1].created_at_ms)),
    ).toBeVisible();
  });

  it('lets rich content shrink inside its turn instead of overflowing', () => {
    const scrollerRef = { current: null };
    render(
      <MessageList
        agent={agent}
        sending={false}
        scrollerRef={scrollerRef}
        onSuggestion={vi.fn()}
      />,
    );

    for (const message of screen.getAllByTestId('markdown-message')) {
      expect(message.parentElement).toHaveClass('min-w-0', 'max-w-full');
      // The reading width itself (68ch, or 78% for a bubble) is set in CSS.
      expect(message.parentElement?.parentElement).toHaveClass(
        'chat-turn',
        'min-w-0',
        'max-w-full',
      );
    }
  });
});

describe('M53 accepted budget wrap-up notice', () => {
  it('shows the tool limit beside its committed saved reply without a loaded run', () => {
    render(
      <MessageList
        agent={{
          ...agent,
          messages: [
            {
              ...messages[1],
              id: 'm53-saved-reply',
              content: {
                text: 'I checked the draft and saved the current reply.',
                metadata: { budgetWrapup: true },
              },
            },
          ],
        }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );

    const reply = screen.getByText(
      'I checked the draft and saved the current reply.',
    );
    expect(reply).toBeVisible();
    const anchor = reply.closest<HTMLElement>('.studio-message-anchor');
    expect(anchor).not.toBeNull();
    const heading = within(anchor!).getByRole('heading', {
      name: 'Tool limit reached',
      exact: true,
    });
    expect(heading).toBeVisible();
    expect(
      within(anchor!).getByText(
        'This saved reply could not run further tools. Review the tool results and any remaining work.',
        { exact: true },
      ),
    ).toBeVisible();
    expect(
      reply.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('keeps an ordinary saved assistant reply quiet', () => {
    render(
      <MessageList
        agent={{ ...agent, messages: [messages[1]] }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
      />,
    );

    expect(screen.getByRole('heading', { name: 'Heading' })).toBeVisible();
    expect(
      screen.queryByRole('heading', { name: 'Tool limit reached' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        'This saved reply could not run further tools. Review the tool results and any remaining work.',
        { exact: true },
      ),
    ).not.toBeInTheDocument();
  });

  const excludedReplies: Array<
    [string, ChatMessage['role'], Record<string, unknown>]
  > = [
    ['user metadata', 'User', { budgetWrapup: true }],
    ['tool metadata', 'Tool', { budgetWrapup: true }],
    ['stopped content', 'Assistant', { budgetWrapup: true, stopped: true }],
    [
      'incomplete content',
      'Assistant',
      { budgetWrapup: true, incomplete: true },
    ],
    [
      'assistant tool calls',
      'Assistant',
      {
        budgetWrapup: true,
        toolCalls: [{ id: 'm53-call', name: 'search', args: {} }],
      },
    ],
  ];

  it.each(excludedReplies)(
    'does not show a notice for %s',
    (_, role, metadata) => {
      render(
        <MessageList
          agent={{
            ...agent,
            messages: [
              {
                ...messages[1],
                role,
                content: { text: 'Saved content', metadata },
              },
            ],
          }}
          sending={false}
          scrollerRef={{ current: null }}
          onSuggestion={vi.fn()}
        />,
      );
      expect(
        screen.queryByRole('heading', {
          name: 'Tool limit reached',
          hidden: true,
        }),
      ).not.toBeInTheDocument();
    },
  );

  it('keeps revised, live and pending paths quiet even with lookalike content', () => {
    const revised: ChatMessage = {
      ...messages[1],
      id: 'm53-revised',
      content: {
        text: 'Earlier saved draft',
        metadata: { budgetWrapup: true },
      },
    };
    const live = {
      ...emptyLiveRun(
        runFixture('m53-live', { status: 'running', startedAtMs: 1 }),
      ),
      steps: [
        {
          stepId: 'm53-live:1',
          text: 'Tool limit reached. budgetWrapup: true',
          textOffset: 0,
        },
      ],
    };
    render(
      <MessageList
        agent={{ ...agent, messages: [] }}
        sending={false}
        scrollerRef={{ current: null }}
        onSuggestion={vi.fn()}
        items={[
          { kind: 'revised', key: revised.id, message: revised },
          ...buildTranscript({
            messages: [],
            runs: [live],
            pending: [
              {
                key: 'm53-pending',
                text: 'budgetWrapup: true',
                createdAtMs: 2,
                status: 'sending',
              },
            ],
          }),
        ]}
      />,
    );

    expect(screen.getByText('Earlier draft (revised)')).toBeVisible();
    expect(
      screen.getByText('Tool limit reached. budgetWrapup: true'),
    ).toBeVisible();
    expect(screen.getByText('Sending…')).toBeVisible();
    expect(
      screen.queryByRole('heading', {
        name: 'Tool limit reached',
        hidden: true,
      }),
    ).not.toBeInTheDocument();
  });

  it.each(['run-first', 'message-first'])(
    'keeps one saved reply and notice through %s hydration and repeated snapshots',
    (arrival) => {
      const run = runFixture('m53-hydration', {
        status: 'completed',
        startedAtMs: 1,
        finishedAtMs: 2,
        replyMessageId: 'm53-hydrated-reply',
      });
      const live = {
        ...emptyLiveRun(run),
        steps: [
          {
            stepId: `${run.id}:1`,
            text: 'The current draft is saved.',
            textOffset: 0,
          },
        ],
      };
      const saved: ChatMessage = {
        ...messages[1],
        id: run.replyMessageId!,
        content: {
          text: 'The current draft is saved.',
          metadata: { runId: run.id, budgetWrapup: true },
        },
      };
      const list = (history: ChatMessage[], runs: LiveRun[]) => (
        <MessageList
          agent={{ ...agent, messages: history }}
          sending={false}
          scrollerRef={{ current: null }}
          onSuggestion={vi.fn()}
          items={buildTranscript({ messages: history, runs })}
        />
      );
      const view = render(
        arrival === 'run-first' ? list([], [live]) : list([saved], []),
      );
      expect(
        screen.queryAllByRole('heading', { name: 'Tool limit reached' }),
      ).toHaveLength(arrival === 'run-first' ? 0 : 1);
      view.rerender(list([saved], [live]));
      const notice = screen.getByRole('heading', {
        name: 'Tool limit reached',
      });
      view.rerender(list([structuredClone(saved)], [structuredClone(live)]));

      expect(screen.getAllByText('The current draft is saved.')).toHaveLength(
        1,
      );
      expect(
        screen.getAllByRole('heading', { name: 'Tool limit reached' }),
      ).toEqual([notice]);
    },
  );

  it('keeps copy, read-aloud and memory actions on the original text without automatic effects', async () => {
    const user = userEvent.setup();
    const saved: ChatMessage = {
      ...messages[1],
      id: 'm53-original',
      content: {
        text: '**Finished**, but the draft contents remain unconfirmed.',
        metadata: { budgetWrapup: true },
      },
    };
    const before = JSON.stringify(saved);
    const onSaveToMemory = vi
      .fn()
      .mockResolvedValue({ kind: 'saved', shortened: false });
    const onRequestLearningReview = vi.fn();
    const onOpenSession = vi.fn();
    const readAloud = {
      supported: true,
      speakingMessageId: null,
      error: null,
      read: vi.fn(),
      stop: vi.fn(),
    };
    const fetch = vi
      .spyOn(globalThis, 'fetch')
      .mockRejectedValue(new Error('Unexpected network request'));
    const props = {
      agent: { ...agent, messages: [saved] },
      sending: false,
      scrollerRef: { current: null },
      onSuggestion: vi.fn(),
      actions: { onSaveToMemory, onRequestLearningReview, onOpenSession },
      readAloud,
    };
    try {
      const view = render(<MessageList {...props} />);
      view.rerender(
        <MessageList
          {...props}
          agent={{ ...props.agent, messages: [saved] }}
        />,
      );
      const notice = screen.getByRole('heading', {
        name: 'Tool limit reached',
      }).parentElement!;
      expect(within(notice).queryByRole('button')).not.toBeInTheDocument();
      expect(
        notice.closest('[aria-live], [role="status"], [role="alert"]'),
      ).toBeNull();
      expect(fetch).not.toHaveBeenCalled();
      expect(onSaveToMemory).not.toHaveBeenCalled();
      expect(readAloud.read).not.toHaveBeenCalled();
      expect(onRequestLearningReview).not.toHaveBeenCalled();
      expect(onOpenSession).not.toHaveBeenCalled();

      await user.click(screen.getByRole('button', { name: 'Copy message' }));
      expect(await navigator.clipboard.readText()).toBe(saved.content.text);
      await user.click(screen.getByRole('button', { name: 'Read aloud' }));
      expect(readAloud.read).toHaveBeenCalledWith(saved.id, saved.content.text);
      await user.click(screen.getByRole('button', { name: 'Save to memory' }));
      expect(onSaveToMemory.mock.calls[0][0]).toBe(saved);
      expect(
        await screen.findByRole('button', { name: '✓ Saved to memory' }),
      ).toBeDisabled();
      expect(JSON.stringify(saved)).toBe(before);
      expect(fetch).not.toHaveBeenCalled();
    } finally {
      fetch.mockRestore();
    }
  });
});

describe('composer attachments', () => {
  const props = {
    agentName: 'Nova',
    draft: '',
    setDraft: vi.fn(),
    sending: false,
    disabled: false,
    onSend: vi.fn(),
    error: null,
    onDismissError: vi.fn(),
  };
  it('allows attachment-only sends once ready but blocks upload/errors while Stop stays usable', async () => {
    const attachment = {
      id: 'att_1',
      name: 'brief.txt',
      mime: 'text/plain',
      size: 3,
      kind: 'text' as const,
      path: 'attachments/brief.txt',
      createdAtMs: 1,
    };
    const item = {
      localId: 'local',
      name: 'brief.txt',
      status: 'ready' as const,
      progress: 100,
      attachment,
    };
    const attachments = { items: [item], onAdd: vi.fn(), onRemove: vi.fn() };
    const { rerender } = render(
      <Composer {...props} attachments={attachments} />,
    );
    expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
    await userEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(props.onSend).toHaveBeenCalled();
    rerender(
      <Composer
        {...props}
        draft="hello"
        attachments={{
          ...attachments,
          items: [{ ...item, status: 'uploading' }],
        }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    const stop = vi.fn();
    rerender(
      <Composer
        {...props}
        draft="hello"
        attachments={{ ...attachments, items: [{ ...item, status: 'error' }] }}
        runActive
        onStop={stop}
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(stop).toHaveBeenCalledOnce();
  });
  it('uses local drop and paste handlers while ordinary text stays available', () => {
    const onAdd = vi.fn();
    render(
      <Composer
        {...props}
        attachments={{ items: [], onAdd, onRemove: vi.fn() }}
      />,
    );
    const file = new File(['x'], 'brief.txt');
    const field = screen.getByRole('textbox');
    fireEvent.drop(field, { dataTransfer: { files: [file] } });
    expect(onAdd).toHaveBeenCalledWith([file]);
    fireEvent.paste(field, {
      clipboardData: { items: [], files: [], getData: () => 'normal text' },
    });
    expect(onAdd).toHaveBeenCalledOnce();
  });
});

describe('Composer keyboard safety', () => {
  it('does not send while composing IME text or while another send is pending', () => {
    const onSend = vi.fn();
    const props = {
      agentName: 'Nova',
      draft: 'Hello',
      setDraft: vi.fn(),
      sending: false,
      disabled: false,
      onSend,
      error: null,
      onDismissError: vi.fn(),
    };
    const view = render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
    expect(onSend).not.toHaveBeenCalled();
    view.rerender(<Composer {...props} sending />);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSend).not.toHaveBeenCalled();
    view.rerender(<Composer {...props} />);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSend).toHaveBeenCalledTimes(1);
  });
});

describe('Composer commands and live replies', () => {
  function composerProps(
    overrides: Partial<Parameters<typeof Composer>[0]> = {},
  ) {
    return {
      agentName: 'Nova',
      draft: '',
      setDraft: vi.fn(),
      sending: false,
      disabled: false,
      onSend: vi.fn(),
      error: null,
      onDismissError: vi.fn(),
      commands: SLASH_COMMANDS,
      ...overrides,
    };
  }

  it('keeps extra inputs behind + and shows no hint while idle', async () => {
    const user = userEvent.setup();
    render(
      <Composer
        {...composerProps({
          attachments: { items: [], onAdd: vi.fn(), onRemove: vi.fn() },
          tools: <p>Browser task controls</p>,
        })}
      />,
    );
    const more = screen.getByRole('button', { name: 'More input options' });
    const tray = document.getElementById(more.getAttribute('aria-controls')!)!;
    expect(more).toHaveAttribute('aria-expanded', 'false');
    expect(tray).not.toHaveAttribute('data-open');
    expect(within(tray).getByText('Browser task controls')).toBeInTheDocument();
    expect(
      within(tray).getByRole('button', { name: 'Attach files' }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/send · ⇧⏎/)).toBeNull();

    await user.click(more);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    expect(tray).toHaveAttribute('data-open');
    within(tray).getByRole('button', { name: 'Attach files' }).focus();
    await user.keyboard('{Escape}');
    expect(tray).not.toHaveAttribute('data-open');
    expect(more).toHaveFocus();
  });

  it('shows Record audio for an attach-capable composer without requesting microphone access', () => {
    const getUserMedia = vi.fn();
    vi.stubGlobal('navigator', {
      ...navigator,
      mediaDevices: { getUserMedia },
    });
    try {
      render(
        <Composer
          {...composerProps({
            attachments: { items: [], onAdd: vi.fn(), onRemove: vi.fn() },
          })}
        />,
      );

      expect(
        screen.getByRole('button', { name: 'Record audio' }),
      ).toBeVisible();
      expect(getUserMedia).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('shows an inert Record control without a known conversation and enables it only in an active scoped composer', () => {
    const onAdd = vi.fn();
    const onSend = vi.fn();
    const attachments = { items: [], onAdd, onRemove: vi.fn() };
    const props = composerProps({ attachments, onSend });
    const view = render(<Composer {...props} />);
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeDisabled();

    view.rerender(<Composer {...props} conversationKey="conversation-a" />);
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeEnabled();

    view.rerender(
      <Composer {...props} conversationKey="conversation-a" offline />,
    );
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeDisabled();
    view.rerender(
      <Composer {...props} conversationKey="conversation-a" sending />,
    );
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeDisabled();
    view.rerender(
      <Composer {...props} conversationKey="conversation-a" disabled />,
    );
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeDisabled();
    view.rerender(
      <Composer {...props} conversationKey="conversation-a" voiceDisabled />,
    );
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeDisabled();
    expect(onAdd).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
  });

  it('offers the matching commands and runs one with Enter', () => {
    const props = composerProps({ draft: '/co' });
    render(<Composer {...props} />);

    const menu = screen.getByRole('listbox', { name: 'Commands' });
    expect(within(menu).getAllByRole('option')).toHaveLength(1);
    expect(
      within(menu).getByRole('option', { selected: true }),
    ).toHaveTextContent('/compact');
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Message Nova' }), {
      key: 'Enter',
    });
    expect(props.onSend).toHaveBeenCalledWith('/compact');
  });

  it('completes a command that needs more text instead of running it', () => {
    const props = composerProps({ draft: '/re' });
    const view = render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    fireEvent.keyDown(input, { key: 'Enter' });
    expect(props.setDraft).toHaveBeenCalledWith('/rename ');
    expect(props.onSend).not.toHaveBeenCalled();

    view.rerender(<Composer {...props} draft="/n" />);
    fireEvent.keyDown(input, { key: 'Tab' });
    expect(props.setDraft).toHaveBeenLastCalledWith('/new');
  });

  it('moves through commands with the arrow keys, picks by click, and closes with Escape', async () => {
    const props = composerProps({ draft: '/' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { selected: true })).toHaveTextContent(
      '/stop',
    );
    expect(input).toHaveAttribute(
      'aria-activedescendant',
      screen.getByRole('option', { selected: true }).id,
    );
    await userEvent.click(screen.getByRole('option', { name: /\/export/ }));
    expect(props.onSend).toHaveBeenCalledWith('/export');
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('shows no command menu without commands', () => {
    render(
      <Composer {...composerProps({ draft: '/', commands: undefined })} />,
    );
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('turns Send into Stop and steers with Ctrl+Enter while a reply runs', async () => {
    const props = composerProps({
      draft: 'also check flights',
      runActive: true,
      onStop: vi.fn(),
      onSteer: vi.fn(),
    });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    expect(
      screen.queryByRole('button', { name: 'Send' }),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(props.onStop).toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    expect(props.onSteer).toHaveBeenCalledTimes(1);
    expect(props.onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(props.onSend).toHaveBeenCalledTimes(1);
    // jsdom reports no Mac platform, so the hint names Ctrl+Enter (S3b-E).
    expect(
      screen.getByText('⏎ queue · Ctrl+Enter steer · ⇧⏎ new line'),
    ).toBeVisible();
  });

  it('shows the ⌘ steer hint on a Mac', () => {
    vi.stubGlobal('navigator', { ...navigator, platform: 'MacIntel' });
    try {
      render(
        <Composer
          {...composerProps({
            draft: 'and Sunday?',
            runActive: true,
            onStop: vi.fn(),
            onSteer: vi.fn(),
          })}
        />,
      );
      expect(
        screen.getByText('⏎ queue · ⌘⏎ steer · ⇧⏎ new line'),
      ).toBeVisible();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('does not steer or send while composing IME text', () => {
    const props = composerProps({
      draft: 'more context',
      commands: undefined,
      runActive: true,
      onStop: vi.fn(),
      onSteer: vi.fn(),
    });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    fireEvent.keyDown(input, {
      key: 'Enter',
      ctrlKey: true,
      isComposing: true,
    });
    expect(props.onSteer).not.toHaveBeenCalled();
    expect(props.onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it('wires the input to the open command menu, and picking an option keeps focus in the input', async () => {
    const props = composerProps({ draft: '/co' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });
    const listbox = screen.getByRole('listbox');
    const option = screen.getByRole('option', { selected: true });

    // A textbox takes aria-autocomplete, aria-controls and
    // aria-activedescendant, but not aria-expanded (S3b-E).
    expect(input).not.toHaveAttribute('aria-expanded');
    expect(input).toHaveAttribute('aria-autocomplete', 'list');
    expect(input).toHaveAttribute('aria-controls', listbox.id);
    expect(input).toHaveAttribute('aria-activedescendant', option.id);

    input.focus();
    expect(document.activeElement).toBe(input);
    await userEvent.click(option);
    expect(document.activeElement).toBe(input);
  });

  it('drops the menu wiring once no command matches', () => {
    const props = composerProps({ draft: 'hello' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    expect(input).not.toHaveAttribute('aria-expanded');
    expect(input).not.toHaveAttribute('aria-controls');
    expect(input).not.toHaveAttribute('aria-activedescendant');
  });

  it('lets Shift+Tab move focus away normally instead of completing a command', () => {
    const props = composerProps({ draft: '/re' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    const notPrevented = fireEvent.keyDown(input, {
      key: 'Tab',
      shiftKey: true,
    });
    expect(notPrevented).toBe(true);
    expect(props.setDraft).not.toHaveBeenCalled();
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it('does not send a picked no-argument command while a send is already in flight', async () => {
    const props = composerProps({ draft: '/st', sending: true });
    render(<Composer {...props} />);

    await userEvent.click(screen.getByRole('option', { name: /\/stop/ }));
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it('falls back to a normal send with Ctrl+Enter when a run is active but there is no onSteer', () => {
    const props = composerProps({
      draft: 'keep going',
      runActive: true,
      onStop: vi.fn(),
      onSteer: undefined,
    });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    expect(props.onSend).toHaveBeenCalledTimes(1);
  });

  it('wraps the active suggestion at both ends of the list with the arrow keys', () => {
    const props = composerProps({ draft: '/' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });
    const last = SLASH_COMMANDS[SLASH_COMMANDS.length - 1];

    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(screen.getByRole('option', { selected: true })).toHaveTextContent(
      `/${last.name}`,
    );
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { selected: true })).toHaveTextContent(
      `/${SLASH_COMMANDS[0].name}`,
    );
  });

  it('completes a skill command for its request instead of sending it', () => {
    const props = composerProps({
      draft: '/no',
      commands: [
        ...SLASH_COMMANDS,
        {
          name: 'notes',
          description: 'Take notes',
          placeholder: '<request>',
          skill: 'notes',
        },
      ],
    });
    render(<Composer {...props} />);

    expect(screen.getByRole('option', { selected: true })).toHaveTextContent(
      '/notes <request>',
    );
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Message Nova' }), {
      key: 'Enter',
    });
    expect(props.setDraft).toHaveBeenCalledWith('/notes ');
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it('closes the menu when the textarea loses focus and reopens it on refocus with a matching draft', () => {
    const props = composerProps({ draft: '/co' });
    render(<Composer {...props} />);
    const input = screen.getByRole('textbox', { name: 'Message Nova' });

    expect(screen.getByRole('listbox')).toBeInTheDocument();
    fireEvent.blur(input);
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
    fireEvent.focus(input);
    expect(screen.getByRole('listbox')).toBeInTheDocument();
  });
});
