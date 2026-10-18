import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import type { ToolStep } from '../../lib/transcript';
import { ToolBlock } from './RunActivity';
import { ToolStepCard } from './ToolStepCard';

describe('m83 Search sources card', () => {
  const sources = [
    {
      title: '<img src=x onerror=alert(1)> returned title',
      url: 'https://example.com/returned?complete=yes&part=2',
    },
  ];
  const step = {
    stepId: 'run:1',
    toolCallId: 'call',
    runId: 'run',
    name: 'web_search',
    argumentsPreview: 'bounded query',
    status: 'success' as const,
    durationMs: 12,
    result: 'x'.repeat(2048),
    truncated: true,
    helper: null,
    sources,
  };

  it('renders exact source links as text outside the clipped result', () => {
    const { container } = render(<ToolStepCard step={step} />);
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.getByText('Search sources')).toBeVisible();
    const link = screen.getByRole('link', { name: sources[0].title });
    expect(link).toHaveAttribute('href', sources[0].url);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', expect.stringContaining('noreferrer'));
    expect(link.closest('pre')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText('Result shortened to 2 KiB.')).toBeVisible();
  });

  it.each(['running', 'error'] as const)(
    'never displays sources for a %s card',
    (status) => {
      render(<ToolStepCard step={{ ...step, status }} />);
      fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
      expect(screen.queryByText('Search sources')).not.toBeInTheDocument();
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    },
  );

  it('clears visible sources when the same mounted card changes status or loses its projection', () => {
    const view = render(<ToolStepCard step={step} />);
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.getByRole('link')).toHaveAttribute('href', sources[0].url);
    view.rerender(<ToolStepCard step={{ ...step, status: 'error' }} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    const legacy: ToolStep = { ...step };
    delete (legacy as ToolStep & { sources?: unknown }).sources;
    view.rerender(<ToolStepCard step={legacy} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('rejects a direct unsafe source instead of making it clickable', () => {
    render(
      <ToolStepCard
        step={{
          ...step,
          sources: [{ title: 'Unsafe', url: 'javascript:alert(1)' }],
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});

describe('app and coding agent sessions in tool steps', () => {
  const appSession = {
    kind: 'app_session',
    id: 'app-3',
    command: 'claude',
    cwd: '/Users/x/code/uploader',
    status: 'running',
    exitCode: null,
    startedAt: '2026-10-19T10:00:00Z',
    screen: { rows: ['> Fix the flaky test', '  12 passed'], cursor: [1, 0], size: [24, 80], alternate: true },
    output: '',
    truncated: false,
    outcome: 'settled',
    containment: 'process_group',
  };
  const acpSession = {
    kind: 'acp_session',
    id: 'acp-1',
    command: 'codex-acp',
    cwd: '/Users/x/code/uploader',
    agent: { name: 'Codex', version: '0.50.0' },
    sessionId: 's',
    status: 'waiting_for_you',
    stopReason: null,
    events: [
      { type: 'plan', entries: [{ content: 'Keep the byte offset', status: 'in_progress', priority: 'high' }] },
      { type: 'tool_call', id: 't1', title: 'Read src/upload.ts', toolKind: 'read', status: 'completed', locations: [] },
      { type: 'permission_asked', toolCallId: 't2', title: 'Edit src/upload.ts', toolKind: 'edit', locations: [] },
      { type: 'agent_message', text: 'It wants to make its first change.' },
    ],
    truncated: false,
  };
  const toolStep = (name: string, result: string, overrides: Partial<ToolStep> = {}): ToolStep => ({
    stepId: 'run:1',
    toolCallId: `call-${name}`,
    runId: 'run',
    name,
    argumentsPreview: '{}',
    status: 'success',
    durationMs: 5,
    result,
    truncated: false,
    helper: null,
    ...overrides,
  });

  afterEach(() => vi.restoreAllMocks());

  it('shows an app session as its terminal screen and stops it through the daemon', async () => {
    const stop = vi
      .spyOn(daemon, 'stopAppSession')
      .mockResolvedValue({ stopped: true, treeGone: true });
    render(
      <ToolStepCard step={toolStep('app_read', JSON.stringify(appSession))} agentId="agent-1" />,
    );
    fireEvent.click(screen.getByRole('button', { name: /app_read/ }));
    const card = screen.getByRole('region', { name: 'Claude Code app session' });
    expect(within(card).getByLabelText('Claude Code screen')).toHaveTextContent('> Fix the flaky test');
    expect(within(card).getByText(/^RUNNING/)).toBeVisible();
    expect(within(card).getByText('claude · ~/code/uploader')).toBeVisible();
    fireEvent.click(within(card).getByRole('button', { name: /Stop/ }));
    await waitFor(() => expect(within(card).getByText('STOPPED')).toBeVisible());
    expect(stop).toHaveBeenCalledWith('agent-1', 'app-3');
    expect(within(card).queryByRole('button', { name: /Stop/ })).toBeNull();
  });

  it('shows an ACP session with its plan, steps, message and pending ask, and cancels its turn', async () => {
    const cancel = vi.spyOn(daemon, 'cancelAcpTurn').mockResolvedValue({ cancelled: true });
    const stop = vi
      .spyOn(daemon, 'stopAcpSession')
      .mockResolvedValue({ stopped: true, treeGone: true });
    render(
      <ToolStepCard step={toolStep('acp_prompt', JSON.stringify(acpSession))} agentId="agent-1" />,
    );
    fireEvent.click(screen.getByRole('button', { name: /acp_prompt/ }));
    const card = screen.getByRole('region', { name: 'Codex coding agent' });
    expect(within(card).getByText('WAITING FOR YOU')).toBeVisible();
    expect(within(card).getByRole('list', { name: 'Plan' })).toHaveTextContent('Keep the byte offset');
    expect(within(card).getByRole('list', { name: 'Steps' })).toHaveTextContent('Read src/upload.ts');
    expect(within(card).getByText('It wants to make its first change.')).toBeVisible();
    expect(within(card).getByText('Codex wants to edit src/upload.ts')).toBeVisible();
    fireEvent.click(within(card).getByRole('button', { name: 'Cancel turn' }));
    await waitFor(() => expect(within(card).getByText('IDLE')).toBeVisible());
    expect(cancel).toHaveBeenCalledWith('agent-1', 'acp-1');
    fireEvent.click(within(card).getByRole('button', { name: /Stop/ }));
    await waitFor(() => expect(within(card).getByText('STOPPED')).toBeVisible());
    expect(stop).toHaveBeenCalledWith('agent-1', 'acp-1');
  });

  it('keeps a failed Stop visible and offers it again', async () => {
    vi.spyOn(daemon, 'stopAppSession').mockRejectedValue(new Error('daemon unreachable'));
    render(
      <ToolStepCard step={toolStep('app_read', JSON.stringify(appSession))} agentId="agent-1" />,
    );
    fireEvent.click(screen.getByRole('button', { name: /app_read/ }));
    fireEvent.click(screen.getByRole('button', { name: /Stop/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('daemon unreachable');
    expect(screen.getByRole('button', { name: /Stop/ })).toBeEnabled();
  });

  it.each([
    ['a cut-short snapshot', JSON.stringify(appSession).slice(0, 120)],
    ['an unknown kind', JSON.stringify({ ...appSession, kind: 'other' })],
    ['plain text', 'started app-3'],
  ])('falls back to the plain result for %s', (_label, result) => {
    const { container } = render(<ToolStepCard step={toolStep('app_read', result)} />);
    fireEvent.click(screen.getByRole('button', { name: /app_read/ }));
    expect(container.querySelector('.tool-step-result pre')?.textContent).toBe(result);
    expect(screen.queryByRole('region')).toBeNull();
  });

  it('offers no Stop without the agent that owns the session', () => {
    render(<ToolStepCard step={toolStep('app_read', JSON.stringify(appSession))} />);
    fireEvent.click(screen.getByRole('button', { name: /app_read/ }));
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull();
  });

  it('shows only the latest snapshot of each session under a finished block', () => {
    const older = toolStep('app_start', JSON.stringify({ ...appSession, screen: { rows: ['starting'] } }), { toolCallId: 'a' });
    const typed = toolStep('app_send', JSON.stringify({ ...appSession, screen: { rows: ['typing'] } }), { toolCallId: 'b' });
    const latest = toolStep('app_read', JSON.stringify({ ...appSession, status: 'exited', exitCode: 0 }), { toolCallId: 'c' });
    render(
      <ToolBlock
        steps={[older, typed, latest]}
        active={false}
        actions={{ sessionAgentId: 'agent-1' }}
      />,
    );
    const cards = screen.getAllByRole('region', { name: 'Claude Code app session' });
    expect(cards).toHaveLength(1);
    expect(within(cards[0]).getByText('EXITED · 0')).toBeVisible();
    expect(within(cards[0]).getByText(/Typed 1 message/)).toBeVisible();
    expect(within(cards[0]).queryByRole('button', { name: /Stop/ })).toBeNull();
  });
});
