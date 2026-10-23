import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ChatReviewPanel } from './ChatReviewPanel';

describe('chat review panel', () => {
  it('reads durable results without starting a review and sends explicit focus once', async () => {
    const review = vi.fn().mockResolvedValue({ id: 'r', status: 'unavailable', error: 'Choose a different reviewer', findings: null, reviewer: null, evidenceLimited: true });
    render(<ChatReviewPanel agentId="a" sessionId="chat:one" api={{ reviews: vi.fn().mockResolvedValue([]), review, cancelReview: vi.fn() }} focus="Check facts" />);
    expect(await screen.findByRole('button', { name: 'Review latest answer' })).toBeInTheDocument();
    expect(review).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Review latest answer' }));
    await waitFor(() => expect(review).toHaveBeenCalledTimes(1));
    expect(review.mock.calls[0][2]).toMatchObject({ focus: 'Check facts', requestId: expect.any(String) });
    expect(await screen.findByText('Choose a different reviewer')).toBeInTheDocument();
    expect(screen.getByText(/supplied chat evidence/i)).toBeInTheDocument();
  });
  it('renders findings as text without turning them into action approval controls', async () => {
    render(<ChatReviewPanel agentId="a" sessionId="chat:one" api={{ reviews: vi.fn().mockResolvedValue([{ id: 'r', status: 'completed', reviewer: { provider: 'chatgpt', model: 'gpt-6-astra' }, findings: '<script>review</script>', error: null, evidenceLimited: true }]), review: vi.fn(), cancelReview: vi.fn() }} />);
    expect(await screen.findByText('<script>review</script>')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /approve/i })).not.toBeInTheDocument();
  });
});
