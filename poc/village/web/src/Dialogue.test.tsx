import { fireEvent, render, screen, cleanup } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { DialoguePanel } from './Dialogue';
import type { GameView } from './types';

afterEach(cleanup);
const view = {
  saveId: 'save-one',
  language: 'ja',
  questStatus: 'pending',
  traveler: null,
  npcId: 'tavi',
  phase: 'day',
  inventory: [],
  coins: 0,
  breadPreference: null,
  visits: 1,
  memories: [],
  journal: [],
  runtime: {
    mode: 'live',
    engine: 'anima-harness',
    provider: 'chatgpt',
    model: 'configured',
  },
  dialogue: {
    text: 'こんにちは。',
    translation: 'Hello.',
    reading: 'Konnichiwa.',
    expression: 'warm',
    source: 'model',
    suggestions: [
      {
        id: 'collect_order',
        text: 'ミラさんのパンを二つください。',
        translation: 'Two loaves for Mira, please.',
      },
    ],
  },
} satisfies GameView;

it('lets a player edit a suggestion before sending without executing it on selection', () => {
  const send = vi.fn();
  render(
    <DialoguePanel
      view={view}
      pending={false}
      stalled={false}
      error={null}
      onSend={send}
      onRecover={vi.fn()}
      onClose={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: /Two loaves for Mira/ }));
  expect(send).not.toHaveBeenCalled();
  const field = screen.getByRole('textbox', { name: /Your reply/ });
  expect(field).toHaveValue('ミラさんのパンを二つください。');
  fireEvent.change(field, { target: { value: 'パンはありますか？' } });
  fireEvent.submit(field.closest('form')!);
  expect(send).toHaveBeenCalledWith('パンはありますか？', undefined);
});

it('does not submit an Enter used to confirm Japanese composition', () => {
  const send = vi.fn();
  render(
    <DialoguePanel
      view={view}
      pending={false}
      stalled={false}
      error={null}
      onSend={send}
      onRecover={vi.fn()}
      onClose={vi.fn()}
    />,
  );
  const field = screen.getByRole('textbox', { name: /Your reply/ });
  fireEvent.change(field, { target: { value: 'パン' } });
  fireEvent.compositionStart(field);
  fireEvent.keyDown(field, { key: 'Enter', keyCode: 229, isComposing: true });
  expect(send).not.toHaveBeenCalled();
  fireEvent.compositionEnd(field);
  fireEvent.keyDown(field, { key: 'Enter', keyCode: 13, isComposing: false });
  expect(send).toHaveBeenCalledTimes(1);
});

it('offers a server-reviewed recovery after a stalled request without resubmitting free text', () => {
  const recover = vi.fn();
  render(
    <DialoguePanel
      view={view}
      pending
      stalled
      error={null}
      onSend={vi.fn()}
      onRecover={recover}
      onClose={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: /Use prepared reply/ }));
  expect(recover).toHaveBeenCalledWith(view.dialogue.suggestions[0]);
});

it('uses Mira’s portrait, title and pending text for her conversation', () => {
  render(
    <DialoguePanel
      view={{ ...view, npcId: 'mira', questStatus: 'collected' }}
      pending
      stalled={false}
      error={null}
      onSend={vi.fn()}
      onRecover={vi.fn()}
      onClose={vi.fn()}
    />,
  );
  expect(
    screen.getByRole('region', { name: 'Conversation with Mira' }),
  ).toBeInTheDocument();
  expect(screen.getByRole('heading', { name: 'Mira' })).toBeInTheDocument();
  expect(screen.getByText('Mira is thinking…')).toBeInTheDocument();
  expect(screen.getByText('THE SUPPER ORGANIZER')).toBeInTheDocument();
});

it('points back to Tavi after giving Mira the bread', () => {
  render(
    <DialoguePanel
      view={{
        ...view,
        npcId: 'mira',
        questStatus: 'delivered',
        phase: 'evening',
      }}
      pending={false}
      stalled={false}
      error={null}
      onSend={vi.fn()}
      onRecover={vi.fn()}
      onClose={vi.fn()}
    />,
  );
  expect(
    screen.getByText(
      'The bread has arrived. Return to Tavi for an evening conversation.',
    ),
  ).toBeInTheDocument();
  expect(
    screen.queryByText('Give Mira her two loaves, or ask about supper.'),
  ).not.toBeInTheDocument();
});
