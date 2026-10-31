import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { saveAppearancePreferences } from '../../lib/appearance-preferences';
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

const capabilityInventory = (supported = true) => ({ schemaVersion: 1, tools: [], persistence: { controlPlane: 'sqlite', memory: 'sqlite', executionJournal: true }, extensions: [], limitations: [], ...(supported ? { runtime: { images: { ledgerVersion: 1, observationVersion: 1 } } } : {}) }) as Awaited<ReturnType<typeof daemon.capabilities>>;

beforeEach(() => {
  vi.spyOn(daemon, 'capabilities').mockResolvedValue(capabilityInventory());
  vi.spyOn(daemon.images, 'list').mockImplementation(() => new Promise(() => {}));
  // The header reads the session's usage; most tests have none.
  vi.spyOn(daemon, 'getSession').mockResolvedValue(sessionFixture('room-7'));
  vi.spyOn(daemon.interactions, 'list').mockResolvedValue([]);
  vi.spyOn(daemon, 'listWorkflows').mockImplementation(
    () => new Promise(() => {}),
  );
});

describe('input controls', () => {
  const composerWith = (plan: boolean) => ({
    draft: '',
    setDraft: vi.fn(),
    sending: false,
    disabled: false,
    offline: false,
    onSend: vi.fn(),
    error: null,
    onDismissError: vi.fn(),
    mode: { plan, canPlan: true, onChange: vi.fn() },
    model: { provider: 'openai', current: 'gpt-5.4', onPick: vi.fn() },
  });
  const chip = () => screen.queryByRole('button', { name: /Change how it works/ });

  afterEach(() => saveAppearancePreferences({ advancedInput: false }));

  it('keeps Do/Plan and the model off the input unless advanced input is on', () => {
    const view = renderView({ composer: composerWith(false) });
    expect(chip()).toBeNull();
    act(() => saveAppearancePreferences({ advancedInput: true }));
    view.rerender({});
    expect(chip()).toHaveAccessibleName(/^Do, gpt-5\.4/);
  });

  it('still shows a chat left in Plan, without the model', () => {
    renderView({ composer: composerWith(true) });
    expect(chip()).toHaveAccessibleName(/^Plan, mode/);
  });

  it('offers explicit success criteria only through Advanced input and includes them with the next queued message', async () => {
    const composer = { ...composerWith(false), draft: 'Prepare the report' };
    const view = renderView({ session: sessionFixture('chat:outcome'), composer });
    expect(screen.queryByText('Success criteria')).toBeNull();
    act(() => saveAppearancePreferences({ advancedInput: true }));
    view.rerender({});
    fireEvent.click(screen.getByText('Success criteria'));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Check this result' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Required text' }), { target: { value: 'All checks passed' } });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Require my acceptance' }));
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(composer.onSend).toHaveBeenCalledWith(undefined, undefined, {
      criteria: [{ id: 'result', kind: 'response_text', contains: 'All checks passed' }],
      maxCorrections: 2, acceptance: 'owner_required',
    });
  });

  it('blocks an enabled empty criterion and ignores it after Advanced input is switched off', () => {
    saveAppearancePreferences({ advancedInput: true });
    const composer = { ...composerWith(false), draft: 'Prepare the report' };
    const view = renderView({ session: sessionFixture('chat:outcome'), composer });
    fireEvent.click(screen.getByText('Success criteria'));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Check this result' }));
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(composer.onSend).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent('Enter the text the result must contain.');
    act(() => saveAppearancePreferences({ advancedInput: false }));
    view.rerender({});
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(composer.onSend).toHaveBeenCalledWith(undefined);
  });
});

describe('artifact conversation layout', () => {
  it('returns the chosen saved image to the composer with its original identity and no automatic send', async () => {
    const attachment = { id: 'saved-image', name: 'Saved picture.png', mime: 'image/png', size: 20, kind: 'image' as const, path: 'attachments/saved.png', createdAtMs: 2 };
    vi.mocked(daemon.images.list).mockResolvedValue({ clearGeneration: 0, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: [{ id: 'op', prompt: 'A bird', sourceAttachmentId: null, createdAtMs: 1, status: 'succeeded', version: { id: 'v', attachment, parentAttachmentId: null, width: 1024, height: 1024, model: 'gpt-image-2.5-sunburst', createdAtMs: 2 } }] });
    vi.spyOn(daemon, 'downloadAttachment').mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:saved');
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const onUseSaved = vi.fn().mockReturnValue(true);
    const composer = { draft: '', setDraft: vi.fn(), sending: false, disabled: false, offline: false, onSend: vi.fn(), error: null, onDismissError: vi.fn(), attachments: { items: [], onAdd: vi.fn(), onRemove: vi.fn(), onUseSaved } };
    renderView({ session: sessionFixture('chat:images'), composer });
    fireEvent.click(await screen.findByRole('button', { name: 'Open image version 1' }));
    await screen.findByRole('img', { name: 'Saved picture.png' });
    fireEvent.click(screen.getByRole('button', { name: 'Edit in chat' }));
    expect(onUseSaved).toHaveBeenCalledWith(attachment);
    expect(composer.onSend).not.toHaveBeenCalled();
    expect(composer.setDraft).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message Nova' })).toHaveFocus());
  });
  it('requires removing another image instead of silently changing the selected edit input', async () => {
    const attachment = { id: 'saved-image', name: 'Saved picture.png', mime: 'image/png', size: 20, kind: 'image' as const, path: 'attachments/saved.png', createdAtMs: 2 };
    vi.mocked(daemon.images.list).mockResolvedValue({ clearGeneration: 0, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: [{ id: 'op', prompt: 'A bird', sourceAttachmentId: null, createdAtMs: 1, status: 'succeeded', version: { id: 'v', attachment, parentAttachmentId: null, width: 1024, height: 1024, model: 'gpt-image-2.5-sunburst', createdAtMs: 2 } }] });
    vi.spyOn(daemon, 'downloadAttachment').mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:saved');
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const onUseSaved = vi.fn().mockReturnValue(true);
    const onSend = vi.fn();
    const onRemove = vi.fn();
    renderView({ session: sessionFixture('chat:images'), composer: { draft: '', setDraft: vi.fn(), sending: false, disabled: false, offline: false, onSend, error: null, onDismissError: vi.fn(), attachments: { items: [{ localId: 'other', name: 'other.png', kind: 'image', status: 'ready', progress: 100, attachment: { ...attachment, id: 'other-image' } }], onAdd: vi.fn(), onRemove, onUseSaved } } });
    fireEvent.click(await screen.findByRole('button', { name: 'Open image version 1' }));
    await screen.findByRole('img', { name: 'Saved picture.png' });
    fireEvent.click(screen.getByRole('button', { name: 'Edit in chat' }));
    expect(onUseSaved).not.toHaveBeenCalled();
    expect(onRemove).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByText('Remove the other selected image before choosing this version to edit.')).toBeVisible();
  });
  it('keeps a selected document target beside an editable draft and sends it only explicitly', async () => {
    const path = 'artifacts/report.docx';
    const sha256 = 'a'.repeat(64);
    vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockResolvedValue({ version: 1, source_scope: 'd'.repeat(64), format: 'docx', source_sha256: sha256, source: { kind: 'workspace', display_name: path }, targets: [{ id: 'paragraph:one', label: 'Paragraph 1', editable: true, value: { kind: 'text', text: 'Keep this exact source' } }], coverage: { total_targets: 1, returned_targets: 1, complete: true }, retained_hyperlink_count: 0, limitations: [], next_cursor: null });
    const composer = { draft: 'Make it clearer', setDraft: vi.fn(), sending: false, disabled: false, offline: false, onSend: vi.fn(), error: null, onDismissError: vi.fn() };
    renderView({ session: sessionFixture('chat:edit'), composer, messages: [{ id: 'file', role: 'Assistant', content: { text: `Created \`${path}\`.` }, created_at_ms: 2 }] });
    fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
    fireEvent.click(await screen.findByRole('button', { name: 'Select Paragraph 1' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add selection to draft' }));
    expect(composer.setDraft).not.toHaveBeenCalled();
    expect(composer.onSend).not.toHaveBeenCalled();
    expect(screen.getByRole('region', { name: 'Selected content for edit' })).toHaveTextContent('Paragraph 1');
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message Nova' })).toHaveFocus());
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(composer.onSend).toHaveBeenCalledOnce();
    expect(composer.onSend.mock.calls[0][0]).toBe('Make it clearer');
    expect(composer.onSend.mock.calls[0][1]).toEqual({ version: 1, source: { kind: 'workspace', path, sha256, scope: 'd'.repeat(64) }, selection: { kind: 'office_target', format: 'docx', target_id: 'paragraph:one', label: 'Paragraph 1', value: { kind: 'text', text: 'Keep this exact source' } } });
  });

  it('clears the selection on reload while preserving the instruction draft', async () => {
    const path = 'report.docx';
    vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockResolvedValue({ version: 1, source_scope: 'd'.repeat(64), format: 'docx', source_sha256: 'a'.repeat(64), source: { kind: 'workspace', display_name: path }, targets: [{ id: 'one', label: 'Paragraph 1', editable: true, value: { kind: 'text', text: 'Source' } }], coverage: { total_targets: 1, returned_targets: 1, complete: true }, retained_hyperlink_count: 0, limitations: [], next_cursor: null });
    renderView({ session: sessionFixture('chat:edit'), composer: { draft: 'Rewrite this', setDraft: vi.fn(), sending: false, disabled: false, offline: false, onSend: vi.fn(), error: null, onDismissError: vi.fn() }, messages: [{ id: 'file', role: 'Assistant', content: { text: `\`${path}\`` }, created_at_ms: 2 }] });
    fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
    fireEvent.click(await screen.findByRole('button', { name: 'Select Paragraph 1' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add selection to draft' }));
    expect(screen.getByRole('region', { name: 'Selected content for edit' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    await screen.findByRole('button', { name: 'Select Paragraph 1' });
    expect(screen.queryByRole('region', { name: 'Selected content for edit' })).toBeNull();
    expect(screen.getByRole('textbox', { name: 'Message Nova' })).toHaveValue('Rewrite this');
  });

  it('keeps the composer with the conversation and preserves its draft while previewing', async () => {
    vi.spyOn(daemon, 'downloadWorkspaceFile').mockImplementation(
      () => new Promise(() => {}),
    );
    const view = renderView({
      session: {
        ...sessionFixture('chat:artifact'),
        title: 'Artifact conversation',
        mainChat: true,
      },
      messages: [
        {
          id: 'artifact-message',
          role: 'Assistant',
          content: { text: 'Created `artifacts/credit-card-bill-check.html`.' },
          created_at_ms: 2,
        },
      ],
    });
    view.rerender({ composer: { ...view.composer, draft: 'Keep this draft' } });
    const conversation = screen.getByRole('region', {
      name: 'Artifact conversation',
    });
    const composer = screen.getByRole('textbox', {
      name: 'Message Nova',
    }) as HTMLTextAreaElement;
    composer.setSelectionRange(5, 9);
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Open artifacts/credit-card-bill-check.html',
      }),
    );
    const preview = await screen.findByRole('complementary', {
      name: 'Artifact preview',
    });
    expect(conversation).toContainElement(composer);
    expect(conversation).not.toContainElement(preview);
    expect(screen.getByRole('textbox', { name: 'Message Nova' })).toBe(
      composer,
    );
    expect(composer).toHaveValue('Keep this draft');
    expect([composer.selectionStart, composer.selectionEnd]).toEqual([5, 9]);
    fireEvent.click(
      screen.getByRole('button', { name: 'Close artifact preview' }),
    );
    expect(
      screen.queryByRole('complementary', { name: 'Artifact preview' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Message Nova' })).toBe(
      composer,
    );
    expect(composer).toHaveValue('Keep this draft');
  });
});

describe('chat workflows integration', () => {
  it('sends a literal retry /btw draft without creating a side question', () => {
    const onSend = vi.fn();
    const ask = vi.spyOn(daemon.interactions, 'sideQuestion');
    renderView({
      session: sessionFixture('room-7'),
      composer: {
        draft: '/btw quoted original input',
        literalDraft: true,
        setDraft: vi.fn(),
        sending: false,
        disabled: false,
        offline: false,
        onSend,
        error: null,
        onDismissError: vi.fn(),
      },
    });
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Message Nova' }), {
      key: 'Enter',
    });
    expect(onSend).toHaveBeenCalledOnce();
    expect(ask).not.toHaveBeenCalled();
  });
  it.each([null, sessionFixture('helper-chat')])(
    'does not mount workflows for a helper agent with session %s',
    async (session) => {
      renderView({ agent: { ...agent, workspaceRole: 'helper' }, session });
      await act(async () => {});
      expect(
        screen.queryByRole('region', { name: 'Active workflows' }),
      ).not.toBeInTheDocument();
      expect(daemon.listWorkflows).not.toHaveBeenCalled();
    },
  );
  it('places active workflows inside the chat scroller and keeps its composer available', async () => {
    vi.mocked(daemon.listWorkflows).mockResolvedValue([
      {
        id: 'active-workflow',
        agentId: agent.id,
        title: 'Waiting for owner',
        requestKey: 'test',
        status: 'awaiting_approval',
        revision: 1,
        currentStep: 0,
        steps: [],
        decisions: [],
        error: null,
        createdAtMs: 1,
        updatedAtMs: 1,
      },
    ]);
    const scrollerRef = createRef<HTMLDivElement>();
    renderView({ scrollerRef });
    const workflows = await screen.findByRole('region', {
      name: 'Active workflows',
    });
    expect(scrollerRef.current).toContainElement(workflows);
    expect(screen.getByRole('textbox', { name: /Message/ })).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Create workflow' }),
    ).not.toBeInTheDocument();
  });

  it('refreshes the initially empty workflow list after new messages and run status activity', async () => {
    const list = vi.mocked(daemon.listWorkflows);
    list.mockResolvedValue([]);
    const view = renderView({ session: sessionFixture('chat:workflows') });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByRole('region', { name: 'Active workflows' }),
    ).not.toBeInTheDocument();
    view.rerender({
      messages: [
        {
          id: 'created',
          role: 'Assistant',
          content: { text: 'Created your workflow' },
          created_at_ms: 2,
        },
      ],
    });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    const liveRun = emptyLiveRun(
      runFixture('workflow-chat-run', { status: 'running' }),
    );
    view.rerender({ runs: [liveRun] });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(3));
    view.rerender({
      runs: [{ ...liveRun, run: { ...liveRun.run, status: 'completed' } }],
    });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(4));
    view.rerender({ contextRefreshKey: 'activity-outside-history' });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(5));
  });

  it.each([
    'helper',
    'job',
    'checkin',
    'telegram',
    'discord',
    'email',
  ] as const)('does not mount workflows in %s sessions', async (kind) => {
    renderView({ session: sessionFixture('other', { kind }) });
    await act(async () => {});
    expect(
      screen.queryByRole('region', { name: 'Active workflows' }),
    ).not.toBeInTheDocument();
    expect(daemon.listWorkflows).not.toHaveBeenCalled();
  });

  it('does not load workflows for a deleted or mismatched agent session', async () => {
    const view = renderView({
      session: sessionFixture('deleted'),
      missing: true,
    });
    expect(daemon.listWorkflows).not.toHaveBeenCalled();
    view.rerender({
      missing: false,
      session: sessionFixture('wrong', { agentId: 'helper-other' }),
    });
    await act(async () => {});
    expect(
      screen.queryByRole('region', { name: 'Active workflows' }),
    ).not.toBeInTheDocument();
    expect(daemon.listWorkflows).not.toHaveBeenCalled();
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('SessionView', () => {
  it('answers a side question alongside the active task and removes it when the chat resets', async () => {
    const ask = vi
      .spyOn(daemon.interactions, 'sideQuestion')
      .mockResolvedValue({
        answer: 'The report covers three sources.',
        snapshotThroughMessageId: null,
        snapshotAtMs: 1,
      });
    const send = vi.fn();
    const stop = vi.fn();
    const session = sessionFixture('chat:side');
    const view = renderView({
      session,
      composer: {
        draft: '/btw What does the report cover?',
        setDraft: vi.fn(),
        sending: false,
        disabled: false,
        offline: false,
        onSend: send,
        onStop: stop,
        runActive: true,
        error: null,
        onDismissError: vi.fn(),
      },
    });
    await userEvent.click(
      screen.getByRole('button', { name: 'Ask side question' }),
    );
    await waitFor(() =>
      expect(ask).toHaveBeenCalledWith(
        session.agentId,
        session.id,
        'What does the report cover?',
        expect.anything(),
      ),
    );
    expect(
      await screen.findByText('The report covers three sources.'),
    ).toBeInTheDocument();
    expect(send).not.toHaveBeenCalled();
    expect(stop).not.toHaveBeenCalled();
    view.rerender({ session: { ...session, clearGeneration: 1 } });
    expect(
      screen.queryByText('The report covers three sources.'),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText('What does the report cover?'),
    ).not.toBeInTheDocument();
  });
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
    expect(
      screen.getByText(/getting long · 82% of Nova’s working memory/),
    ).toBeVisible();
    for (const excluded of [
      'instructions',
      'tool schemas',
      'files',
      'draft',
      'reply',
    ])
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

    const disclosure = screen.getByRole('button', { name: 'Show' });
    expect(screen.getAllByText('What anima kept')).toHaveLength(1);
    expect(screen.queryByText('Saturday free')).not.toBeInTheDocument();
    await userEvent.click(disclosure);
    expect(screen.getByText('Saturday free')).toBeVisible();
    expect(screen.getByText('Saturday free').tagName).toBe('STRONG');
    expect(screen.getByText(/12 earlier messages/)).toBeVisible();
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
    await userEvent.click(screen.getByRole('button', { name: 'Show' }));
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
    await userEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(screen.getByText('Saved context.')).toBeVisible();
    view.rerender({ session: sessionFixture('chat:second', { summary }) });
    expect(screen.queryByText('Saved context.')).not.toBeInTheDocument();
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

      expect(screen.queryByText('Spoken replies')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Generate speech with OpenAI API' })).not.toBeInTheDocument();
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

  it('directs email owner replies through draft review in Connectors', () => {
    renderView({
      session: sessionFixture('email:connector:sender', {
        kind: 'email',
        origin: 'email',
        title: 'Email',
        capabilities: readOnly,
      }),
    });
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('note')).toHaveTextContent(
      'Review and send email drafts in Connectors',
    );
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

describe('image backend compatibility', () => {
  it('keeps an unused old-backend chat quiet without requesting the missing image endpoint', async () => {
    vi.mocked(daemon.capabilities).mockResolvedValue(capabilityInventory(false));
    renderView({ session: sessionFixture('chat:1'), messages: [{ id: 'prompt', role: 'User', content: { text: 'generate_image could be useful later' }, created_at_ms: 1 }] });
    await act(async () => {});
    expect(daemon.images.list).not.toHaveBeenCalled();
    expect(screen.queryByText(/running backend does not advertise image history/)).toBeNull();
  });

  it('reports missing backend support for actual native image activity in this conversation', async () => {
    vi.mocked(daemon.capabilities).mockResolvedValue(capabilityInventory(false));
    const imageRun = runFixture('image-run', { toolsStarted: ['generate_image'], status: 'failed', finishedAtMs: 2, error: { code: 'unsupported', message: 'Image tool unavailable' } });
    renderView({ session: sessionFixture('chat:1'), runs: [emptyLiveRun(imageRun)] });
    expect(await screen.findByText(/running backend does not advertise image history/)).toBeVisible();
    expect(daemon.images.list).not.toHaveBeenCalled();
  });

  it('does not show another conversation image work in an unused old-backend chat', async () => {
    vi.mocked(daemon.capabilities).mockResolvedValue(capabilityInventory(false));
    const imageRun = runFixture('foreign-image-run', { sessionId: 'chat:other', toolsStarted: ['edit_image'] });
    renderView({ session: sessionFixture('chat:1'), runs: [emptyLiveRun(imageRun)] });
    await act(async () => {});
    expect(screen.queryByText(/running backend does not advertise image history/)).toBeNull();
  });
});
