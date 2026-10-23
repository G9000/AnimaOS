import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatModelPreferencesView } from '@animaos/sdk';
import { ChatModelControls } from './ChatModelControls';

function fixture(): ChatModelPreferencesView { return { preferences: { primary: { mode: 'inherit' }, reviewer: { mode: 'inherit' } }, primary: { provider: 'chatgpt', model: 'gpt-6-astra' }, reviewer: null, reviewerUnavailableReason: 'Choose a reviewer', appliesTo: 'next_turn', clearGeneration: 3, availableModels: [{ provider: 'chatgpt', model: 'gpt-6-astra', reasoningEfforts: ['low', 'medium', 'high'] }, { provider: 'anthropic', model: 'claude-sonnet-5', reasoningEfforts: [] }] }; }

describe('chat model controls', () => {
  it('saves only the changed primary and hides unsupported effort controls', async () => {
    const view = fixture(); const update = vi.fn().mockResolvedValue(view);
    render(<ChatModelControls agentId="a" sessionId="chat:one" api={{ get: vi.fn().mockResolvedValue(view), update }} />);
    await screen.findByRole('combobox', { name: 'Chat model' });
    fireEvent.change(screen.getByRole('combobox', { name: 'Chat model' }), { target: { value: 'anthropic:claude-sonnet-5' } });
    expect(screen.queryByRole('combobox', { name: 'Chat reasoning effort' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save model settings' }));
    await waitFor(() => expect(update).toHaveBeenCalledWith('a', 'chat:one', { primary: { mode: 'override', provider: 'anthropic', model: 'claude-sonnet-5' }, clearGeneration: 3 }));
    expect(screen.getByText(/next turn/i)).toBeInTheDocument();
  });
  it('does not expose unsupported reasoning efforts for the current model', async () => {
    const view = fixture();
    render(<ChatModelControls agentId="a" sessionId="chat:one" api={{ get: vi.fn().mockResolvedValue(view), update: vi.fn() }} />);
    const reasoning = await screen.findByRole('combobox', { name: 'Chat reasoning effort' });
    expect(reasoning).toHaveTextContent('high');
    expect(reasoning).not.toHaveTextContent('none');
  });
});
