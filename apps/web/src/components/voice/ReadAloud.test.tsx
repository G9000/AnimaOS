import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  useReadAloud,
  type ReadAloudController,
} from '../../hooks/useReadAloud';
import { ReadAloud } from './ReadAloud';

class Utterance {
  onstart: (() => void) | null = null;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}
const synthesis = { speak: vi.fn(), cancel: vi.fn() };
beforeEach(() => {
  synthesis.speak.mockReset();
  synthesis.cancel.mockReset();
  vi.stubGlobal('speechSynthesis', synthesis);
  vi.stubGlobal('SpeechSynthesisUtterance', Utterance);
});
afterEach(() => vi.unstubAllGlobals());

function Replies({ disabled = false }: { disabled?: boolean }) {
  const controller = useReadAloud('session', disabled);
  return (
    <>
      <ReadAloud
        messageId="first"
        text="First reply"
        controller={controller}
        disabled={disabled}
      />
      <ReadAloud
        messageId="second"
        text="Second reply"
        controller={controller}
        disabled={disabled}
      />
      {controller.error && <p role="alert">{controller.error}</p>}
    </>
  );
}

it('uses one session controller and exposes read/stop labels and browser speaking state', async () => {
  render(<Replies />);
  await userEvent.click(
    screen.getAllByRole('button', { name: 'Read aloud' })[0],
  );
  expect(synthesis.speak.mock.calls[0][0].text).toBe('First reply');
  act(() => synthesis.speak.mock.calls[0][0].onstart?.());
  expect(screen.getByRole('button', { name: 'Stop reading' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await userEvent.click(screen.getByRole('button', { name: 'Read aloud' }));
  expect(synthesis.speak.mock.calls[1][0].text).toBe('Second reply');
  act(() => synthesis.speak.mock.calls[1][0].onstart?.());
  await userEvent.click(screen.getByRole('button', { name: 'Stop reading' }));
  expect(screen.getAllByRole('button', { name: 'Read aloud' })).toHaveLength(2);
});

it('hides the control when speech support is absent', () => {
  vi.stubGlobal('speechSynthesis', undefined);
  render(<Replies />);
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
});

it('hides empty replies and respects disabled state', () => {
  const controller: ReadAloudController = {
    supported: true,
    speakingMessageId: null,
    error: null,
    read: vi.fn(),
    stop: vi.fn(),
  };
  const { rerender } = render(
    <ReadAloud messageId="empty" text={' \n '} controller={controller} />,
  );
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
  rerender(
    <ReadAloud
      messageId="reply"
      text="Reply"
      controller={controller}
      disabled
    />,
  );
  expect(screen.getByRole('button', { name: 'Read aloud' })).toBeDisabled();
});
