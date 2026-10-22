import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, SessionMessage } from '@animaos/sdk';

import type { ChatAttention } from '../lib/automation-toasts';
import { daemon } from '../lib/daemon-api';
import { useAutomationToasts } from './useAutomationToasts';

// The shared stream is replaced by one this test pushes events into.
let emit: (event: AgentEvent) => void = () => undefined;
vi.mock('./useAgentEvents', () => ({
  useAgentEvents: (_agentId: string | null, onEvent?: (e: AgentEvent) => void) => {
    emit = (event) => onEvent?.(event);
    return { status: 'open', state: {}, trimFinishedRun: () => undefined };
  },
}));

const FIRED = Date.UTC(2026, 9, 21, 14, 0);
function message(id: string, metadata: Record<string, unknown>): SessionMessage {
  return {
    id,
    role: 'assistant',
    text: 'written for the model',
    attachments: [],
    metadata,
    createdAtMs: FIRED,
  };
}
const reminder = message('m-reminder', {
  kind: 'automation',
  automationId: 'rem',
  automationName: 'Call the bank',
  reminder: true,
  body: 'Their cut-off is 3:00 PM.',
  delivery: 'main',
  runId: 'run-1',
  firedAtMs: FIRED,
});
const plain = message('m-plain', {});

function created(
  messageId: string,
  overrides: Partial<{ agentId: string; sessionId: string; role: string }> = {},
): AgentEvent {
  return {
    type: 'message.created',
    agentId: 'agent-main',
    sessionId: 'main-chat',
    seq: 1,
    at: 0,
    messageId,
    role: 'assistant',
    stepId: null,
    ...overrides,
  } as AgentEvent;
}

const away: ChatAttention = {
  viewingMainChat: false,
  atBottom: true,
  documentHidden: false,
};
const looking: ChatAttention = {
  viewingMainChat: true,
  atBottom: true,
  documentHidden: false,
};

function mount(attention: ChatAttention, notify = vi.fn()) {
  const view = renderHook(() =>
    useAutomationToasts({
      agentId: 'agent-main',
      mainSessionId: 'main-chat',
      enabled: true,
      attention: () => attention,
      notify,
    }),
  );
  return { ...view, notify };
}

beforeEach(() => {
  vi.spyOn(daemon, 'sessionMessages').mockResolvedValue({
    messages: [plain, reminder],
    nextBefore: null,
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  emit = () => undefined;
});

describe('useAutomationToasts', () => {
  it('toasts a live automation card in the main chat while the owner is elsewhere', async () => {
    const { result, notify } = mount(away);

    act(() => emit(created('m-reminder')));

    await waitFor(() => expect(result.current.toasts).toHaveLength(1));
    expect(result.current.toasts[0]).toMatchObject({
      id: 'm-reminder',
      kind: 'reminder',
    });
    expect(daemon.sessionMessages).toHaveBeenCalledWith(
      'agent-main',
      'main-chat',
      { limit: 20 },
    );
    expect(notify).toHaveBeenCalledTimes(1);
  });

  it('stays quiet while the owner looks at the bottom of the main chat', async () => {
    const { result, notify } = mount(looking);

    act(() => emit(created('m-reminder')));

    await waitFor(() => expect(daemon.sessionMessages).toHaveBeenCalled());
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.toasts).toEqual([]);
    expect(notify).not.toHaveBeenCalled();
  });

  it('toasts when the main chat is open but scrolled up, or the tab is hidden', async () => {
    const scrolled = mount({ ...looking, atBottom: false });
    act(() => emit(created('m-reminder')));
    await waitFor(() => expect(scrolled.result.current.toasts).toHaveLength(1));
    scrolled.unmount();

    const hidden = mount({ ...looking, documentHidden: true });
    act(() => emit(created('m-reminder')));
    await waitFor(() => expect(hidden.result.current.toasts).toHaveLength(1));
  });

  it('ignores other sessions, other roles, and ordinary messages', async () => {
    const { result } = mount(away);

    act(() => emit(created('m-reminder', { sessionId: 'a-side-thread' })));
    act(() => emit(created('m-reminder', { role: 'user' })));
    act(() => emit(created('m-reminder', { agentId: 'someone-else' })));
    await act(async () => {
      await Promise.resolve();
    });
    expect(daemon.sessionMessages).not.toHaveBeenCalled();

    act(() => emit(created('m-plain')));
    await waitFor(() => expect(daemon.sessionMessages).toHaveBeenCalled());
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.toasts).toEqual([]);
  });

  it('shows a message once however many times it is announced', async () => {
    const { result } = mount(away);

    act(() => emit(created('m-reminder')));
    await waitFor(() => expect(result.current.toasts).toHaveLength(1));
    act(() => emit(created('m-reminder')));
    await waitFor(() => expect(daemon.sessionMessages).toHaveBeenCalledTimes(2));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.toasts).toHaveLength(1);
  });

  it('reads again when the announcement races the committed message', async () => {
    vi.mocked(daemon.sessionMessages)
      .mockResolvedValueOnce({ messages: [plain], nextBefore: null })
      .mockResolvedValue({ messages: [plain, reminder], nextBefore: null });
    const { result } = mount(away);

    act(() => emit(created('m-reminder')));

    await waitFor(() => expect(result.current.toasts).toHaveLength(1), {
      timeout: 2_000,
    });
    expect(daemon.sessionMessages).toHaveBeenCalledTimes(2);
  });

  it('dismisses a toast', async () => {
    const { result } = mount(away);
    act(() => emit(created('m-reminder')));
    await waitFor(() => expect(result.current.toasts).toHaveLength(1));

    act(() => result.current.dismiss('m-reminder'));

    expect(result.current.toasts).toEqual([]);
  });
});
