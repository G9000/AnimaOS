import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { approvalFixture } from '../../test/live';
import { DocumentEditCard } from './DocumentEditCard';

const hash = 'a'.repeat(64);
const sourceHash = 'b'.repeat(64);
const inventory = { version: 1, format: 'docx', targets: [{ id: 'p1', label: 'Body', editable: true, value: { kind: 'text', text: '<img src=x>\u202e' } }], retained_hyperlinks: [{ relationship_id: 'r1', target: 'https://example.test' }], limitations: ['No layout preview'] };
const review = { version: 1, proposal_id: 'proposal', review_sha256: hash, source: { kind: 'attachment', display_name: 'source.docx' }, source_sha256: sourceHash, file_path: 'edited.docx', expected_destination: { state: 'absent' }, expires_at_ms: 4_000_000_000_000, preview_quality: 'structural_content', before: inventory, after: inventory, changes: [{ operation_index: 0, label: 'Body', before: 'Old', after: '<script>new</script>', derived: false }], preservation: { unchanged_parts: 3, edited_parts: 1, unchanged_bytes_verified: true, container_normalized: true } };
const args = { _anima_exact_document_edit: true, proposal_id: 'proposal', review_sha256: hash, candidate_sha256: 'c'.repeat(64), source_sha256: sourceHash, file_path: 'edited.docx', expected_destination: { state: 'absent' } };
function approval(overrides: Record<string, unknown> = {}) {
  return approvalFixture('approval', { tool: 'apply_document_edit', arguments: JSON.stringify(args), expiresAtMs: 4_000_000_000_000, ...overrides });
}

describe('document edit approval', () => {
  it('requires the full matching review, displays plain text, and only offers one-time approval or denial', async () => {
    const load = vi.fn().mockResolvedValue(review);
    const decide = vi.fn().mockResolvedValue(null);
    const { container } = render(<DocumentEditCard approval={approval()} load={load} onDecide={decide} />);
    expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
    expect(await screen.findByText('After: <script>new</script>')).toBeVisible();
    expect(container.querySelector('script, img, a')).toBeNull();
    expect(screen.getAllByText('Retained link: https://example.test')).toHaveLength(2);
    expect(screen.getAllByText('No layout preview')).toHaveLength(2);
    expect(screen.queryByRole('button', { name: /always|session/i })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Apply these changes' }));
    expect(decide).toHaveBeenCalledWith(expect.objectContaining({ id: 'approval' }), { decision: 'allow_once' });
  });

  it.each([
    { ...review, proposal_id: 'different' },
    { ...review, review_sha256: 'd'.repeat(64) },
    { ...review, source_sha256: 'd'.repeat(64) },
    { ...review, file_path: 'other.docx' },
    { ...review, changes: undefined },
    { ...review, preservation: { ...review.preservation, unchanged_bytes_verified: false } },
  ])('keeps apply disabled for mismatched or incomplete review', async (value) => {
    render(<DocumentEditCard approval={approval()} load={vi.fn().mockResolvedValue(value)} onDecide={vi.fn()} />);
    expect(await screen.findByRole('alert')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
  });

  it('does not fetch or allow decisions on a read-only surface', async () => {
    const load = vi.fn().mockResolvedValue(review);
    render(<DocumentEditCard approval={approval()} load={load} />);
    await waitFor(() => expect(load).not.toHaveBeenCalled());
    expect(screen.queryByRole('button', { name: 'Apply these changes' })).toBeNull();
  });

  it('shows spreadsheet formula status and automatic changes without claiming layout fidelity', async () => {
    const sheet = { ...inventory, format: 'xlsx', targets: [{ id: 'cell', label: 'Total', editable: true, value: { kind: 'cell', sheet_id: 'sheet', address: 'B2', value: { type: 'number', value: 4 }, formula: 'SUM(A1:A2)', calculation: 'needs_external_recalculation' } }] };
    const worksheet = { ...review, file_path: 'edited.xlsx', before: sheet, after: sheet, changes: [{ operation_index: null, label: 'Formula cache', before: 'stored', after: 'recalculate', derived: true }] };
    const request = approval({ arguments: JSON.stringify({ ...args, file_path: 'edited.xlsx' }) });
    render(<DocumentEditCard approval={request} load={vi.fn().mockResolvedValue(worksheet)} onDecide={vi.fn()} />);
    expect(await screen.findByText('Formula cache')).toBeVisible();
    expect(screen.getAllByText(/needs external recalculation/)).toHaveLength(2);
    expect(screen.getByText(/automatic change/)).toBeVisible();
    expect(screen.getByText(/Content preview only/)).toBeVisible();
  });

  it.each([{ sessionId: 'other-session' }, { agentId: 'other-agent' }])('ignores a late review after its scope changes', async (change) => {
    let finish!: (value: unknown) => void;
    const load = vi.fn().mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValueOnce({ ...review, proposal_id: 'other' });
    const { rerender } = render(<DocumentEditCard approval={approval()} load={load} onDecide={vi.fn()} />);
    rerender(<DocumentEditCard approval={approval(change)} load={load} onDecide={vi.fn()} />);
    finish(review);
    expect(await screen.findByRole('alert')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
  });

  it('does not retry an approval after a lost acknowledgement', async () => {
    const decide = vi.fn().mockResolvedValue('Could not reach your companion. Try again.');
    render(<DocumentEditCard approval={approval()} load={vi.fn().mockResolvedValue(review)} onDecide={decide} />);
    const button = screen.getByRole('button', { name: 'Apply these changes' });
    await waitFor(() => expect(button).toBeEnabled());
    await userEvent.click(button);
    expect(await screen.findByRole('alert')).toHaveTextContent(/refresh/i);
    expect(button).toBeDisabled();
    expect(decide).toHaveBeenCalledTimes(1);
  });

  it('does not apply a late decision result to a different approval', async () => {
    let finish!: (value: string | null) => void;
    const decide = vi.fn().mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const load = vi.fn().mockResolvedValue(review);
    const { rerender } = render(<DocumentEditCard approval={approval()} load={load} onDecide={decide} />);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeEnabled());
    await userEvent.click(screen.getByRole('button', { name: 'Apply these changes' }));
    rerender(<DocumentEditCard approval={approval({ id: 'next', agentId: 'next-agent' })} load={load} onDecide={decide} />);
    finish(null);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeEnabled());
    expect(screen.queryByText('Decision sent.')).toBeNull();
  });
});
