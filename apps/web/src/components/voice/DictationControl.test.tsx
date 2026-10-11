import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DictationControl } from './DictationControl';

class Recognition {
  static instance: Recognition;
  continuous = false;
  interimResults = false;
  lang = '';
  onstart: (() => void) | null = null;
  onend: (() => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  onresult:
    | ((event: {
        resultIndex: number;
        results: Array<{ isFinal: boolean; 0: { transcript: string } }>;
      }) => void)
    | null = null;
  start = vi.fn();
  stop = vi.fn();
  abort = vi.fn();
  constructor() {
    Recognition.instance = this;
  }
}
beforeEach(() => {
  vi.stubGlobal('SpeechRecognition', Recognition);
  vi.stubGlobal('webkitSpeechRecognition', undefined);
});
afterEach(() => vi.unstubAllGlobals());

it('hides unsupported dictation and does not create a microphone request', () => {
  vi.stubGlobal('SpeechRecognition', undefined);
  render(<DictationControl onTranscript={vi.fn()} conversationKey="session" />);
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
});

it('shows interim separately, commits only final words, and shows permission errors inline', async () => {
  const transcript = vi.fn();
  render(
    <DictationControl onTranscript={transcript} conversationKey="session" />,
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Dictate message' }),
  );
  const recognition = Recognition.instance;
  act(() => recognition.onstart?.());
  expect(
    screen.getByRole('button', { name: 'Stop dictation' }),
  ).toHaveAttribute('aria-pressed', 'true');
  act(() =>
    recognition.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: false, 0: { transcript: 'preview' } }],
    }),
  );
  expect(screen.getByRole('status')).toHaveTextContent('preview');
  expect(transcript).not.toHaveBeenCalled();
  act(() =>
    recognition.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: true, 0: { transcript: 'confirmed' } }],
    }),
  );
  expect(transcript).toHaveBeenCalledWith('confirmed');
  act(() => recognition.onerror?.({ error: 'not-allowed' }));
  expect(screen.getByRole('alert')).toHaveTextContent('microphone access');
});

it('cancels a pending request and clears interim on disabled boundary', async () => {
  const transcript = vi.fn();
  const { rerender } = render(
    <DictationControl onTranscript={transcript} conversationKey="session" />,
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Dictate message' }),
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Cancel dictation' }),
  );
  expect(Recognition.instance.abort).toHaveBeenCalledOnce();
  await userEvent.click(
    screen.getByRole('button', { name: 'Dictate message' }),
  );
  act(() => Recognition.instance.onstart?.());
  rerender(
    <DictationControl
      onTranscript={transcript}
      conversationKey="session"
      disabled
    />,
  );
  expect(Recognition.instance.abort).toHaveBeenCalledOnce();
  expect(
    screen.getByRole('button', { name: 'Dictate message' }),
  ).toBeDisabled();
  expect(screen.queryByRole('status')).not.toBeInTheDocument();
});
