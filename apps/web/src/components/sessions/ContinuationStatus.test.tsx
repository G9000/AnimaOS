import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { ContinuationStatus } from './ContinuationStatus';

const get = vi.fn();
vi.mock('../../lib/daemon-api', () => ({
  daemon: { runs: { get: (...args: unknown[]) => get(...args) } },
}));

beforeEach(() => get.mockReset());

it('ignores a late result from a different continuation', async () => {
  let finishFirst!: (run: { id: string; status: string }) => void;
  get.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishFirst = resolve;
      }),
  );
  get.mockResolvedValueOnce({ id: 'second', status: 'queued' });
  const view = render(<ContinuationStatus agentId="a" runId="first" />);
  view.rerender(<ContinuationStatus agentId="a" runId="second" />);
  expect(await screen.findByText('Queued')).toBeVisible();
  expect(get.mock.calls[0][2].signal.aborted).toBe(true);
  await act(async () => finishFirst({ id: 'first', status: 'completed' }));
  expect(screen.queryByText('Finished')).not.toBeInTheDocument();
});

it('keeps the continuation and retries only the status read after a failure', async () => {
  get.mockRejectedValueOnce(new Error('Connection lost'));
  get.mockResolvedValueOnce({ id: 'saved', status: 'running' });
  render(<ContinuationStatus agentId="a" runId="saved" />);
  expect(await screen.findByRole('alert')).toHaveTextContent('Connection lost');
  expect(screen.getByText(/Continuation saved/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Refresh continuation' }));
  expect(await screen.findByText('Running')).toBeVisible();
  expect(get).toHaveBeenCalledTimes(2);
  expect(get.mock.calls.map((call) => call[1])).toEqual(['saved', 'saved']);
});
