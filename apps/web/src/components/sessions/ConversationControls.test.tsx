import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { sessionFixture } from '../../test/sessions';
import { runFixture } from '../../test/live';
import { ConversationControls } from './ConversationControls';

describe('ConversationControls', () => {
  it('offers safe revisions and a link to the source', () => {
    const revise = vi.fn();
    const openSource = vi.fn();
    render(
      <ConversationControls
        session={sessionFixture('revision', { parentSessionId: 'source' })}
        messages={[]}
        runs={[]}
        enabled
        onRevise={revise}
        onOpenSource={openSource}
      />,
    );
    fireEvent.click(screen.getByText('Conversation controls'));
    fireEvent.click(
      screen.getByRole('button', { name: 'Retry latest prompt' }),
    );
    expect(revise).toHaveBeenCalledWith('retry');
    expect(
      screen.getByText(/cannot reverse completed actions/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open original chat' }));
    expect(openSource).toHaveBeenCalledWith('source');
  });

  it('edits and reorders ordinary waiting prompts without exposing protected inputs', async () => {
    const patch = vi.fn().mockResolvedValue(true);
    const first = runFixture('first', {
      sessionId: 'room-7',
      status: 'queued',
      queueOrder: 0,
      source: 'web',
    });
    const second = runFixture('second', {
      sessionId: 'room-7',
      status: 'queued',
      queueOrder: 1,
      source: 'api',
    });
    const system = runFixture('system', {
      sessionId: 'room-7',
      status: 'queued',
      source: 'schedule',
    });
    render(
      <ConversationControls
        session={sessionFixture('room-7')}
        messages={[]}
        runs={[second, system, first]}
        enabled
        onQueuePatch={patch}
      />,
    );
    await act(async () =>
      fireEvent.click(
        screen.getByRole('button', { name: 'Move queued message 2 up' }),
      ),
    );
    expect(patch).toHaveBeenCalledWith(second, { beforeRunId: 'first' });
    expect(
      screen.queryByRole('button', { name: 'Edit queued message 3' }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('button', { name: 'Edit queued message 1' }),
    );
    fireEvent.change(
      screen.getByRole('textbox', { name: 'Edit waiting message 1' }),
      { target: { value: 'Changed waiting instruction' } },
    );
    await act(async () =>
      fireEvent.click(
        screen.getByRole('button', { name: 'Save waiting message' }),
      ),
    );
    expect(patch).toHaveBeenLastCalledWith(first, {
      text: 'Changed waiting instruction',
    });
  });
});
