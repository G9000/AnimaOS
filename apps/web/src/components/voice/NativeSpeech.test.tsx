import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { NativeSpeech } from './NativeSpeech';

const messages = [
  {
    id: 'reply-1',
    role: 'Assistant' as const,
    content: { text: 'Hello there' },
    created_at_ms: 1,
  },
];
const capability = {
  version: 1 as const,
  provider: 'openai' as const,
  model: 'gpt-4o-mini-tts' as const,
  voice: 'alloy' as const,
  format: 'wav' as const,
  available: true,
  reason: 'available' as const,
  maxInputChars: 2048 as const,
  maxInputBytes: 8192 as const,
  maxAudioBytes: 5760044 as const,
  maxDurationMs: 120000 as const,
};
const props = {
  agentId: 'agent-1',
  sessionId: 'session-1',
  conversationKey: 'first',
  messages,
  disabled: false,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

afterEach(() => vi.restoreAllMocks());

describe('NativeSpeech', () => {
  it('makes no request or media object until explicit actions', async () => {
    const cap = vi
      .spyOn(daemon.speech, 'capabilities')
      .mockResolvedValue(capability);
    const generate = vi
      .spyOn(daemon.speech, 'generate')
      .mockResolvedValue(new Blob(['audio']));
    vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const audio = vi.fn();
    vi.stubGlobal('Audio', audio);
    try {
      const view = render(<NativeSpeech {...props} />);
      expect(cap).not.toHaveBeenCalled();
      expect(generate).not.toHaveBeenCalled();
      expect(audio).not.toHaveBeenCalled();
      await userEvent.selectOptions(
        screen.getByRole('combobox', { name: 'Assistant reply for speech' }),
        'reply-1',
      );
      expect(cap).not.toHaveBeenCalled();
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await waitFor(() => expect(generate).toHaveBeenCalledOnce());
      expect(generate.mock.calls[0]?.[2]).toMatchObject({
        messageId: 'reply-1',
        text: 'Hello there',
        consent: true,
      });
      expect(audio).not.toHaveBeenCalled();
      await screen.findByRole('button', { name: 'Play speech' });
      view.unmount();
      await act(async () => {
        await Promise.resolve();
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('cancels a dispatched generation on source change and ignores its late result', async () => {
    let finish!: (blob: Blob) => void;
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    vi.spyOn(daemon.speech, 'generate').mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const cancel = vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const view = render(<NativeSpeech {...props} />);
    await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
    await userEvent.click(
      screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
    );
    await waitFor(() => expect(daemon.speech.generate).toHaveBeenCalledOnce());
    view.rerender(<NativeSpeech {...props} messages={[]} />);
    expect(cancel).toHaveBeenCalledOnce();
    await act(async () => finish(new Blob(['late'])));
    expect(
      screen.queryByRole('button', { name: 'Play speech' }),
    ).not.toBeInTheDocument();
  });

  it('creates media only on Play and cleans up once on Stop while retaining replay', async () => {
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    vi.spyOn(daemon.speech, 'generate').mockResolvedValue(new Blob(['audio']));
    vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const createUrl = vi.fn(() => 'blob:speech-1');
    const revokeUrl = vi.fn();
    const pause = vi.fn();
    const load = vi.fn();
    const removeAttribute = vi.fn();
    const play = vi.fn(async () => undefined);
    class FakeAudio {
      src = '';
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      pause = pause;
      load = load;
      removeAttribute = removeAttribute;
      play = play;
    }
    class FakeURL extends URL {
      static createObjectURL = createUrl;
      static revokeObjectURL = revokeUrl;
    }
    vi.stubGlobal('Audio', FakeAudio);
    vi.stubGlobal('URL', FakeURL);
    try {
      const view = render(<NativeSpeech {...props} />);
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await screen.findByRole('button', { name: 'Play speech' });
      expect(createUrl).not.toHaveBeenCalled();
      await userEvent.click(
        screen.getByRole('button', { name: 'Play speech' }),
      );
      expect(play).toHaveBeenCalledOnce();
      await userEvent.click(
        screen.getByRole('button', { name: 'Stop speech' }),
      );
      expect(pause).toHaveBeenCalledOnce();
      expect(removeAttribute).toHaveBeenCalledWith('src');
      expect(load).toHaveBeenCalledOnce();
      expect(revokeUrl).toHaveBeenCalledExactlyOnceWith('blob:speech-1');
      expect(screen.getByRole('button', { name: 'Play speech' })).toBeEnabled();
      view.unmount();
      await act(async () => {
        await Promise.resolve();
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('retains the page owner until a dispatched DELETE settles even after its abort timer fires', async () => {
    const deleting = deferred<void>();
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    vi.spyOn(daemon.speech, 'generate').mockResolvedValue(new Blob(['audio']));
    const cancel = vi
      .spyOn(daemon.speech, 'cancel')
      .mockReturnValue(deleting.promise);
    class FakeAudio {
      src = '';
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      pause() {}
      removeAttribute(_name: string) {}
      load() {}
      play() {
        return Promise.resolve();
      }
    }
    class FakeURL extends URL {
      static createObjectURL() {
        return 'blob:pending-delete';
      }
      static revokeObjectURL(_url: string) {}
    }
    vi.stubGlobal('Audio', FakeAudio);
    vi.stubGlobal('URL', FakeURL);
    const view = render(<NativeSpeech {...props} />);
    try {
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await screen.findByRole('button', { name: 'Play speech' });
      await userEvent.click(
        screen.getByRole('button', { name: 'Play speech' }),
      );
      await waitFor(() =>
        expect(
          screen.getByRole('button', { name: 'Stop speech' }),
        ).toBeVisible(),
      );
      await act(async () => {
        await Promise.resolve();
      });
      vi.useFakeTimers();
      fireEvent.click(screen.getByRole('button', { name: 'Discard speech' }));
      expect(cancel).toHaveBeenCalledOnce();
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeDisabled();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2100);
      });
      expect(cancel.mock.calls[0]?.[3]?.signal?.aborted).toBe(true);
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeDisabled();
      expect(screen.getByText(/Speech cleanup pending/)).toBeVisible();
      await act(async () => deleting.resolve(undefined));
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeEnabled();
    } finally {
      deleting.resolve(undefined);
      vi.useRealTimers();
      view.unmount();
      vi.unstubAllGlobals();
    }
  });

  it('aborts a pending capability check without DELETE or POST', async () => {
    const checking = deferred<typeof capability>();
    vi.spyOn(daemon.speech, 'capabilities').mockReturnValue(checking.promise);
    const generate = vi
      .spyOn(daemon.speech, 'generate')
      .mockResolvedValue(new Blob(['audio']));
    const cancel = vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const view = render(<NativeSpeech {...props} />);
    try {
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      const signal = vi.mocked(daemon.speech.capabilities).mock.calls[0]?.[2]
        ?.signal;
      await userEvent.click(
        screen.getByRole('button', { name: 'Stop generating speech' }),
      );
      expect(signal?.aborted).toBe(true);
      await act(async () => checking.resolve(capability));
      expect(generate).not.toHaveBeenCalled();
      expect(cancel).not.toHaveBeenCalled();
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeEnabled();
    } finally {
      checking.resolve(capability);
      view.unmount();
    }
  });

  it('cancels one dispatched POST with an independent DELETE signal and waits for both settlements', async () => {
    const posting = deferred<Blob>();
    const deleting = deferred<void>();
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    const generate = vi
      .spyOn(daemon.speech, 'generate')
      .mockReturnValue(posting.promise);
    const cancel = vi
      .spyOn(daemon.speech, 'cancel')
      .mockReturnValue(deleting.promise);
    const view = render(<NativeSpeech {...props} />);
    try {
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await waitFor(() => expect(generate).toHaveBeenCalledOnce());
      const postSignal = generate.mock.calls[0]?.[3]?.signal;
      await userEvent.click(
        screen.getByRole('button', { name: 'Stop generating speech' }),
      );
      expect(postSignal?.aborted).toBe(true);
      expect(cancel).toHaveBeenCalledOnce();
      expect(cancel.mock.calls[0]?.[3]?.signal).not.toBe(postSignal);
      expect(cancel.mock.calls[0]?.[3]?.signal?.aborted).toBe(false);
      await act(async () => posting.resolve(new Blob(['late'])));
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeDisabled();
      expect(
        screen.queryByRole('button', { name: 'Play speech' }),
      ).not.toBeInTheDocument();
      await act(async () => deleting.resolve(undefined));
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeEnabled();
    } finally {
      posting.resolve(new Blob(['late']));
      deleting.resolve(undefined);
      view.unmount();
    }
  });

  it('wakes a replacement panel when a retired capability request settles', async () => {
    const checking = deferred<typeof capability>();
    vi.spyOn(daemon.speech, 'capabilities').mockReturnValue(checking.promise);
    const generate = vi
      .spyOn(daemon.speech, 'generate')
      .mockResolvedValue(new Blob(['audio']));
    const cancel = vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const replacement = { ...props, conversationKey: 'second' };
    const view = render(
      <>
        <div key="old" data-testid="old">
          <NativeSpeech {...props} />
        </div>
        <div key="new" data-testid="new">
          <NativeSpeech {...replacement} />
        </div>
      </>,
    );
    try {
      await userEvent.selectOptions(
        within(screen.getByTestId('old')).getByRole('combobox'),
        'reply-1',
      );
      await userEvent.selectOptions(
        within(screen.getByTestId('new')).getByRole('combobox'),
        'reply-1',
      );
      await userEvent.click(
        within(screen.getByTestId('old')).getByRole('button', {
          name: 'Generate speech with OpenAI API',
        }),
      );
      view.rerender(
        <div key="new" data-testid="new">
          <NativeSpeech {...replacement} />
        </div>,
      );
      expect(
        within(screen.getByTestId('new')).getByRole('button', {
          name: 'Generate speech with OpenAI API',
        }),
      ).toBeDisabled();
      await act(async () => checking.resolve(capability));
      expect(generate).not.toHaveBeenCalled();
      expect(cancel).not.toHaveBeenCalled();
      await waitFor(() =>
        expect(
          within(screen.getByTestId('new')).getByRole('button', {
            name: 'Generate speech with OpenAI API',
          }),
        ).toBeEnabled(),
      );
    } finally {
      checking.resolve(capability);
      view.unmount();
    }
  });

  it('does not revive playback when a stopped play promise settles after scope change', async () => {
    const playing = deferred<void>();
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    const generate = vi
      .spyOn(daemon.speech, 'generate')
      .mockResolvedValue(new Blob(['audio']));
    vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const revoke = vi.fn();
    const play = vi.fn(() => playing.promise);
    class FakeAudio {
      src = '';
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      pause() {}
      removeAttribute(_name: string) {}
      load() {}
      play = play;
    }
    class FakeURL extends URL {
      static createObjectURL() {
        return 'blob:late-play';
      }
      static revokeObjectURL = revoke;
    }
    vi.stubGlobal('Audio', FakeAudio);
    vi.stubGlobal('URL', FakeURL);
    const view = render(<NativeSpeech {...props} />);
    try {
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await screen.findByRole('button', { name: 'Play speech' });
      await userEvent.click(
        screen.getByRole('button', { name: 'Play speech' }),
      );
      await userEvent.click(
        screen.getByRole('button', { name: 'Stop speech' }),
      );
      view.rerender(<NativeSpeech {...props} conversationKey="second" />);
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:late-play');
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeDisabled();
      await act(async () => playing.resolve(undefined));
      expect(play).toHaveBeenCalledOnce();
      expect(generate).toHaveBeenCalledOnce();
      expect(
        screen.queryByRole('button', { name: 'Play speech' }),
      ).not.toBeInTheDocument();
      expect(revoke).toHaveBeenCalledTimes(1);
    } finally {
      playing.resolve(undefined);
      view.unmount();
      vi.unstubAllGlobals();
    }
  });

  it('keeps uncertain media cleanup busy and discards its candidate when event detachment throws', async () => {
    vi.spyOn(daemon.speech, 'capabilities').mockResolvedValue(capability);
    vi.spyOn(daemon.speech, 'generate').mockResolvedValue(new Blob(['audio']));
    vi.spyOn(daemon.speech, 'cancel').mockResolvedValue();
    const pause = vi.fn(() => {
      throw new Error('pause failed');
    });
    const remove = vi.fn(() => {
      throw new Error('remove failed');
    });
    const load = vi.fn(() => {
      throw new Error('load failed');
    });
    const revoke = vi.fn(() => {
      throw new Error('revoke failed');
    });
    class FakeAudio {
      src = '';
      private ended: (() => void) | null = null;
      private errored: (() => void) | null = null;
      get onended() {
        return this.ended;
      }
      set onended(value: (() => void) | null) {
        if (value === null) throw new Error('detach ended failed');
        this.ended = value;
      }
      get onerror() {
        return this.errored;
      }
      set onerror(value: (() => void) | null) {
        if (value === null) throw new Error('detach error failed');
        this.errored = value;
      }
      pause = pause;
      removeAttribute = remove;
      load = load;
      play() {
        return Promise.resolve();
      }
    }
    class FakeURL extends URL {
      static createObjectURL() {
        return 'blob:uncertain';
      }
      static revokeObjectURL = revoke;
    }
    vi.stubGlobal('Audio', FakeAudio);
    vi.stubGlobal('URL', FakeURL);
    const view = render(<NativeSpeech {...props} />);
    try {
      await userEvent.selectOptions(screen.getByRole('combobox'), 'reply-1');
      await userEvent.click(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      );
      await screen.findByRole('button', { name: 'Play speech' });
      await userEvent.click(
        screen.getByRole('button', { name: 'Play speech' }),
      );
      await waitFor(() =>
        expect(
          screen.getByRole('button', { name: 'Stop speech' }),
        ).toBeVisible(),
      );
      fireEvent.click(screen.getByRole('button', { name: 'Stop speech' }));
      expect(pause).toHaveBeenCalledOnce();
      expect(remove).toHaveBeenCalledWith('src');
      expect(load).toHaveBeenCalledOnce();
      expect(revoke).toHaveBeenCalledOnce();
      expect(
        screen.queryByRole('button', { name: 'Play speech' }),
      ).not.toBeInTheDocument();
      expect(screen.getByText(/Speech cleanup pending/)).toBeVisible();
      expect(
        screen.getByRole('button', { name: 'Generate speech with OpenAI API' }),
      ).toBeDisabled();
    } finally {
      view.unmount();
      vi.unstubAllGlobals();
    }
  });
});
