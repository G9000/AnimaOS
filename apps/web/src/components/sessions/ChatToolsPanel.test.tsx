import { act, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatCapabilities } from '@animaos/sdk';
import { ChatToolsPanel } from './ChatToolsPanel';

const view = (sessionId: string): ChatCapabilities => ({ agentId: 'a', sessionId,
  basis: 'next_owner_turn', runId: null, preparing: false,
  tools: [{ name: `${sessionId}_tool`, description: 'An effective tool', origin: 'native', risk: 'exec',
    approval: 'ask', argumentDependent: true, available: true, reason: null }],
  connections: [], notice: 'Per-call checks remain required.',
});

describe('ChatToolsPanel', () => {
  it('describes actual scope and conditional approval without granting permission', async () => {
    const api = { get: vi.fn().mockResolvedValue(view('first')) };
    render(<ChatToolsPanel agentId="a" sessionId="first" api={api} />);
    expect(await screen.findByText('first_tool')).toBeTruthy();
    expect(screen.getByText(/policy: asks for approval/)).toBeTruthy();
    expect(screen.getByText(/Next owner turn/)).toBeTruthy();
  });

  it('discards old-chat responses after navigation', async () => {
    let old!: (value: ChatCapabilities) => void;
    const api = { get: vi.fn().mockImplementationOnce(() => new Promise(resolve => { old = resolve; })).mockResolvedValue(view('second')) };
    const rendered = render(<ChatToolsPanel agentId="a" sessionId="first" api={api} />);
    rendered.rerender(<ChatToolsPanel agentId="a" sessionId="second" api={api} />);
    expect(await screen.findByText('second_tool')).toBeTruthy();
    await act(async () => { old(view('first')); });
    await waitFor(() => expect(screen.queryByText('first_tool')).toBeNull());
    expect(screen.getByText('second_tool')).toBeTruthy();
  });
});
