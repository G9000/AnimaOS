import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RunRecovery } from './RunRecovery';
const recovery = vi.fn();
const continueRecovery = vi.fn();
const getRun = vi.fn();
vi.mock('../../lib/daemon-api', () => ({
  daemon: {
    runs: {
      recovery: (...args: unknown[]) => recovery(...args),
      continueRecovery: (...args: unknown[]) => continueRecovery(...args),
      get: (...args: unknown[]) => getRun(...args),
    },
  },
}));
describe('RunRecovery', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getRun.mockResolvedValue({ id: 'child', status: 'queued' });
  });
  it('requires acknowledgement of possible effects and sends the viewed evidence hash', async () => {
    recovery.mockResolvedValue({
      sourceRunId: 'source',
      fingerprint: 'a'.repeat(64),
      eligible: true,
      reviewRequired: true,
      unknownActionCount: 1,
      remainingAttempts: 3,
      continuationRunId: null,
    });
    continueRecovery.mockResolvedValue({ id: 'child' });
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    const button = await screen.findByRole('button', {
      name: 'Continue remaining work',
    });
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox'));
    fireEvent.click(button);
    await waitFor(() =>
      expect(continueRecovery).toHaveBeenCalledWith('a', 'source', {
        fingerprint: 'a'.repeat(64),
        reviewed: true,
      }),
    );
    expect(await screen.findByText(/Continuation saved/)).toBeInTheDocument();
  });
  it('uses a persisted continuation after reload without offering another attempt', async () => {
    render(
      <RunRecovery agentId="a" runId="source" continuationRunId="child" />,
    );
    expect(screen.getByText(/Continuation saved/)).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Continue remaining work' }),
    ).not.toBeInTheDocument();
    expect(recovery).not.toHaveBeenCalled();
    expect(await screen.findByText('Queued')).toBeVisible();
  });

  it('shows recorded evidence before acknowledging uncertain effects', async () => {
    recovery.mockResolvedValue({
      sourceRunId: 'source',
      fingerprint: 'b'.repeat(64),
      eligible: true,
      reviewRequired: true,
      unknownActionCount: 1,
      remainingAttempts: 2,
      continuationRunId: null,
      evidence: {
        actionReceipts: [
          {
            toolCallId: 'send-1',
            name: 'send_message',
            status: 'started',
            startedAtMs: 1,
            finishedAtMs: null,
            durationMs: null,
            argumentsPreview: 'Message preview',
            argumentsTruncated: false,
            resultPreview: null,
            resultTruncated: false,
            durability: 'persistent',
          },
        ],
        unreceiptedTools: [],
        omittedUnreceiptedToolCount: 0,
        checkpoint: {
          sequence: 1,
          phase: 'before_model',
          modelCalls: 1,
          excerpts: [{ role: 'assistant', text: 'Saved draft excerpt' }],
          intendedTools: [],
        },
      },
    });
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    expect(await screen.findByText('Saved draft excerpt')).toBeVisible();
    expect(screen.getByText(/Outcome unknown/)).toBeVisible();
    expect(screen.getByText(/2.*attempt/)).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Continue remaining work' }),
    ).toBeDisabled();
  });

  it('checks the saved successor without treating acceptance as completion', async () => {
    recovery.mockResolvedValue({
      sourceRunId: 'source',
      fingerprint: 'c'.repeat(64),
      eligible: true,
      reviewRequired: false,
      unknownActionCount: 0,
      remainingAttempts: 3,
      continuationRunId: null,
    });
    continueRecovery.mockResolvedValue({ id: 'child', status: 'queued' });
    getRun.mockResolvedValue({ id: 'child', status: 'running' });
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Continue remaining work' }),
    );
    expect(await screen.findByText(/Running/)).toBeVisible();
    expect(getRun).toHaveBeenCalledWith('a', 'child', expect.any(Object));
    expect(continueRecovery).toHaveBeenCalledTimes(1);
  });

  it('finds a saved continuation after a lost response without resubmitting it', async () => {
    const summary = {
      sourceRunId: 'source',
      fingerprint: 'd'.repeat(64),
      eligible: true,
      reviewRequired: false,
      unknownActionCount: 0,
      remainingAttempts: 3,
      continuationRunId: null,
    };
    recovery.mockResolvedValueOnce(summary).mockResolvedValueOnce({
      ...summary,
      continuationRunId: 'child',
    });
    continueRecovery.mockRejectedValueOnce(new Error('Connection lost'));
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Continue remaining work' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Connection lost',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Refresh recovery' }));
    expect(await screen.findByText(/Continuation saved/)).toBeVisible();
    await waitFor(() => expect(getRun).toHaveBeenCalled());
    expect(continueRecovery).toHaveBeenCalledTimes(1);
  });

  it('does not offer continuation when its saved binding is stale', async () => {
    recovery.mockResolvedValue({
      sourceRunId: 'source',
      fingerprint: 'e'.repeat(64),
      eligible: true,
      reviewRequired: true,
      unknownActionCount: 0,
      remainingAttempts: 2,
      continuationRunId: null,
      recoveryState: 'invalid_or_stale',
    });
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    expect(await screen.findByText(/cannot be continued/)).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Continue remaining work' }),
    ).not.toBeInTheDocument();
    expect(continueRecovery).not.toHaveBeenCalled();
  });

  it('shows the changed project notice and requires its review', async () => {
    recovery.mockResolvedValue({
      sourceRunId: 'source',
      fingerprint: 'a'.repeat(64),
      eligible: true,
      reviewRequired: true,
      unknownActionCount: 0,
      remainingAttempts: 2,
      continuationRunId: null,
      projectContextNotice:
        'The shared project has changed since this work stopped.',
    });
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    expect(await screen.findByText(/shared project has changed/)).toBeVisible();
    const button = screen.getByRole('button', {
      name: 'Continue remaining work',
    });
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox'));
    expect(button).toBeEnabled();
  });

  it('clears the owner acknowledgement before a failed evidence refresh', async () => {
    recovery
      .mockResolvedValueOnce({
        sourceRunId: 'source',
        fingerprint: 'f'.repeat(64),
        eligible: true,
        reviewRequired: true,
        unknownActionCount: 1,
        remainingAttempts: 2,
        continuationRunId: null,
      })
      .mockRejectedValueOnce(new Error('Refresh failed'));
    render(<RunRecovery agentId="a" runId="source" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Review interrupted work' }),
    );
    const checkbox = await screen.findByRole('checkbox');
    fireEvent.click(checkbox);
    expect(checkbox).toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh recovery' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Refresh failed',
    );
    expect(checkbox).not.toBeChecked();
    expect(
      screen.getByRole('button', { name: 'Continue remaining work' }),
    ).toBeDisabled();
  });
});
