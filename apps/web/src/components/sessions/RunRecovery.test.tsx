import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RunRecovery } from './RunRecovery';
const recovery = vi.fn();
const continueRecovery = vi.fn();
vi.mock('../../lib/daemon-api', () => ({
  daemon: {
    runs: {
      recovery: (...args: unknown[]) => recovery(...args),
      continueRecovery: (...args: unknown[]) => continueRecovery(...args),
    },
  },
}));
describe('RunRecovery', () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
  it('uses a persisted continuation after reload without offering another attempt', () => {
    render(
      <RunRecovery agentId="a" runId="source" continuationRunId="child" />,
    );
    expect(screen.getByText(/Continuation saved/)).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(recovery).not.toHaveBeenCalled();
  });
});
