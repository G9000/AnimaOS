import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { MemoryPublication, MemoryPublicationsView } from '@animaos/sdk';
import { GardenPublicationsPanel } from './GardenPublicationsPanel';

const item = (state: MemoryPublication['state'], extra: Partial<MemoryPublication> = {}): MemoryPublication => ({
  id: state, operationId: state, revision: 3, state, title: `Synthetic ${state}`, localId: 'local', attempts: state === 'queued' ? 0 : 1,
  createdAtMs: 1, updatedAtMs: 2, nextAttemptAtMs: 3, reason: null, proposalId: null, canonical: null, ...extra,
});
const view = (...items: MemoryPublication[]): MemoryPublicationsView => ({ available: true, reason: null, items, counts: Object.fromEntries(items.map(row => [row.state, 1])), omitted: 0 });

describe('selected garden shares', () => {
  it('loads only when expanded and distinguishes pending review, uncertain and saved', async () => {
    const load = vi.fn().mockResolvedValue(view(item('submitted_for_review'), item('unknown'), item('saved', { canonical: { id: 'canonical', revision: 2, ownerEdited: true } })));
    render(<GardenPublicationsPanel agentId="a" load={load} />);
    expect(load).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Show selected shares' }));
    expect(await screen.findByText('Awaiting Mother review')).toBeInTheDocument();
    expect(screen.getByText('Outcome unconfirmed')).toBeInTheDocument();
    expect(screen.getByText('Saved in Mother')).toBeInTheDocument();
    expect(screen.getByText('Owner edited the approved memory.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Approve/ })).not.toBeInTheDocument();
  });

  it('only offers cancellation for never-sent items and carries the current revision', async () => {
    const load = vi.fn().mockResolvedValue(view(item('queued'), item('unknown')));
    const control = vi.fn().mockResolvedValue(item('cancelled', { id: 'queued' }));
    render(<GardenPublicationsPanel agentId="a" load={load} control={control} />);
    fireEvent.click(screen.getByRole('button', { name: 'Show selected shares' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel Synthetic queued' }));
    await waitFor(() => expect(control).toHaveBeenCalledWith('a', 'queued', { expectedRevision: 3, action: 'cancel' }));
    expect(screen.queryByRole('button', { name: 'Cancel Synthetic unknown' })).not.toBeInTheDocument();
  });

  it('requires rereading after an uncertain control response and does not replay', async () => {
    const load = vi.fn().mockResolvedValue(view(item('blocked', { reason: 'approval_required' })));
    const control = vi.fn().mockRejectedValue(new TypeError('Lost synthetic response'));
    render(<GardenPublicationsPanel agentId="a" load={load} control={control} />);
    fireEvent.click(screen.getByRole('button', { name: 'Show selected shares' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Resume Synthetic blocked' }));
    expect(await screen.findByText(/could not be confirmed/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Resume Synthetic blocked' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh selected shares' }));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    expect(control).toHaveBeenCalledTimes(1);
  });

  it('does not show a previous agent response after switching agents', async () => {
    let resolve!: (value: MemoryPublicationsView) => void;
    const load = vi.fn((agentId: string) => agentId === 'a' ? new Promise<MemoryPublicationsView>(done => { resolve = done; }) : Promise.resolve(view()));
    const { rerender } = render(<GardenPublicationsPanel agentId="a" load={load} />);
    fireEvent.click(screen.getByRole('button', { name: 'Show selected shares' }));
    rerender(<GardenPublicationsPanel agentId="b" load={load} />);
    await act(async () => resolve(view(item('saved'))));
    expect(screen.queryByText('Synthetic saved')).not.toBeInTheDocument();
  });

  it('keeps unavailable storage distinct from an empty queue', async () => {
    render(<GardenPublicationsPanel agentId="a" load={async () => ({ ...view(), available: false, reason: 'persistent_storage_required' })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Show selected shares' }));
    expect(await screen.findByText(/Persistent local storage is required/)).toBeInTheDocument();
    expect(screen.queryByText('No selected shares.')).not.toBeInTheDocument();
  });
});
