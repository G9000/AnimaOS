import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { AgentDetail } from '../lib/types';
import type { LearningReview, Run } from '@animaOS-SWARM/sdk';
import { formatTime } from './ui-bits';
import { Composer, MessageList } from './ChatScreen';
import { SLASH_COMMANDS } from '../lib/slash-commands';
import { emptyLiveRun } from '../lib/session-events';
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

  it('allows rich-content bubbles to shrink within their width cap', () => {
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
      expect(message.parentElement?.parentElement).toHaveClass(
        'max-w-[85%]',
        'min-w-0',
      );
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
