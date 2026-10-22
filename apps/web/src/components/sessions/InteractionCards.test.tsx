import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { ClarificationRequest } from '@animaos/sdk';
import { ClarificationCard, SideQuestionCard } from './InteractionCards';

const request: ClarificationRequest = {
  id: 'ask_1',
  agentId: 'a',
  sessionId: 's',
  runId: 'r',
  toolCallId: 'tc',
  revision: 7,
  createdAtMs: 1,
  status: 'pending',
  response: null,
  questions: [
    {
      id: 'q0',
      question: 'Which output?',
      choices: [
        { id: 'c0', label: 'A <img>' },
        { id: 'c1', label: 'B' },
      ],
      multiSelect: false,
    },
  ],
};
describe('interaction cards', () => {
  it('submits stable choice IDs and the request revision without recommendation text', async () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const { container } = render(
      <ClarificationCard request={request} onAnswer={onAnswer} />,
    );
    expect(screen.getByText('Which output?')).toBeVisible();
    expect(container.querySelector('img')).toBeNull();
    await user.click(
      screen.getByRole('radio', { name: 'A <img> (Recommended)' }),
    );
    await user.click(screen.getByRole('button', { name: 'Submit answers' }));
    expect(onAnswer).toHaveBeenCalledWith(request, {
      revision: 7,
      answers: [{ questionId: 'q0', choiceIds: ['c0'], text: null }],
    });
  });

  it('accepts an Other answer and renders an answered request after a remount', async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <ClarificationCard request={request} onAnswer={onAnswer} />,
    );
    await user.type(
      screen.getByLabelText('Other answer: Which output?'),
      '  Markdown  ',
    );
    await user.click(screen.getByRole('button', { name: 'Submit answers' }));
    expect(onAnswer).toHaveBeenCalledWith(request, {
      revision: 7,
      answers: [{ questionId: 'q0', choiceIds: [], text: 'Markdown' }],
    });
    rerender(
      <ClarificationCard
        request={{
          ...request,
          status: 'answered',
          revision: 8,
          response: {
            answeredAtMs: 2,
            answers: [{ questionId: 'q0', choiceIds: [], text: 'Markdown' }],
          },
        }}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.getByText('Markdown')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Submit answers' })).toBeNull();
  });

  it('keeps the main task wording and side answer distinct', () => {
    render(
      <SideQuestionCard
        question="Why?"
        answer={{
          answer: 'Because of the saved setting.',
          snapshotAtMs: 2,
          snapshotThroughMessageId: 'm',
        }}
      />,
    );
    expect(
      screen.getByRole('region', { name: 'Side question' }),
    ).toHaveTextContent('Because of the saved setting.');
    expect(
      screen.getByText(
        'The main task continues. This answer uses a conversation snapshot.',
      ),
    ).toBeVisible();
  });
});
