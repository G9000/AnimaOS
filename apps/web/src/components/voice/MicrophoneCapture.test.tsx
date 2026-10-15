import { StrictMode, useLayoutEffect } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MicrophoneCapture } from './MicrophoneCapture';

const captureMock = vi.hoisted(() => {
  const makeController = () => ({
    start: vi.fn(),
    stop: vi.fn(),
    cancel: vi.fn(),
    takeRecording: vi.fn(),
    dispose: vi.fn(),
  });
  return {
    supported: true,
    stateListener: null as ((state: any) => void) | null,
    controller: makeController(),
    controllers: [] as ReturnType<typeof makeController>[],
    listeners: [] as ((state: any) => void)[],
    makeController,
  };
});

vi.mock('../../lib/audio-capture', () => ({
  createMicrophoneCapture: (listener: (state: any) => void) => {
    const controller = captureMock.makeController();
    captureMock.stateListener = listener;
    captureMock.controller = controller;
    captureMock.controllers.push(controller);
    captureMock.listeners.push(listener);
    return controller;
  },
  isMicrophoneCaptureSupported: () => captureMock.supported,
}));

describe('MicrophoneCapture', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    captureMock.supported = true;
    captureMock.stateListener = null;
    captureMock.controllers.length = 0;
    captureMock.listeners.length = 0;
  });

  it('starts only on Record and transfers the ready file only on Add recording', () => {
    const onAdd = vi.fn();
    const file = new File(['wav'], 'recording.wav', { type: 'audio/wav' });
    render(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled={false}
        onAdd={onAdd}
      />,
    );
    captureMock.controller.takeRecording.mockReturnValueOnce(file);

    expect(
      screen.getByText(/stay on this device until you choose Add recording/),
    ).toBeVisible();
    expect(captureMock.controller.start).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    expect(captureMock.controller.start).toHaveBeenCalledOnce();
    expect(onAdd).not.toHaveBeenCalled();

    act(() =>
      captureMock.stateListener?.({
        status: 'ready',
        fileName: 'recording.wav',
        sizeBytes: 48,
        durationMs: 1200,
        limitReached: false,
      }),
    );
    expect(screen.getByText(/recording.wav · 0:01 · 48 bytes/)).toBeVisible();
    expect(onAdd).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Add recording' }));
    expect(captureMock.controller.takeRecording).toHaveBeenCalledOnce();
    expect(onAdd).toHaveBeenCalledWith([file]);
    expect(screen.getByRole('button', { name: 'Record audio' })).toBeVisible();
    expect(onAdd).toHaveBeenCalledOnce();
  });

  it('cancels on disabled scope and fences a stale Add callback after conversation change', () => {
    const onAdd = vi.fn();
    const view = render(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled={false}
        onAdd={onAdd}
      />,
    );
    act(() =>
      captureMock.stateListener?.({
        status: 'ready',
        fileName: 'recording.wav',
        sizeBytes: 48,
        durationMs: 1200,
        limitReached: false,
      }),
    );
    const oldController = captureMock.controller;
    const staleAdd = screen.getByRole('button', { name: 'Add recording' });
    view.rerender(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled
        onAdd={onAdd}
      />,
    );
    expect(oldController.cancel).toHaveBeenCalled();

    view.rerender(
      <MicrophoneCapture
        conversationKey="conversation-b"
        disabled={false}
        onAdd={onAdd}
      />,
    );
    fireEvent.click(staleAdd);
    expect(onAdd).not.toHaveBeenCalled();
    expect(oldController.takeRecording).not.toHaveBeenCalled();
  });

  it('begins cancellation before a disabled parent layout effect runs', () => {
    const observed: number[] = [];
    function Parent({ disabled }: { disabled: boolean }) {
      useLayoutEffect(() => {
        if (disabled)
          observed.push(captureMock.controller.cancel.mock.calls.length);
      }, [disabled]);
      return (
        <MicrophoneCapture
          conversationKey="conversation-a"
          disabled={disabled}
          onAdd={vi.fn()}
        />
      );
    }
    const view = render(<Parent disabled={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    view.rerender(<Parent disabled />);
    expect(observed).toEqual([1]);
  });

  it('begins cancellation and disposal before an unmounting parent layout effect runs', () => {
    const observed: Array<[number, number]> = [];
    function Parent({ visible }: { visible: boolean }) {
      useLayoutEffect(() => {
        if (!visible)
          observed.push([
            captureMock.controller.cancel.mock.calls.length,
            captureMock.controller.dispose.mock.calls.length,
          ]);
      }, [visible]);
      return visible ? (
        <MicrophoneCapture
          conversationKey="conversation-a"
          disabled={false}
          onAdd={vi.fn()}
        />
      ) : null;
    }
    const view = render(<Parent visible />);
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    view.rerender(<Parent visible={false} />);
    expect(observed).toEqual([[1, 1]]);
  });

  it('discards a late ready observer event after disable and does not revive it on re-enable', () => {
    const onAdd = vi.fn();
    const view = render(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled={false}
        onAdd={onAdd}
      />,
    );
    const listener = captureMock.stateListener!;
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    view.rerender(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled
        onAdd={onAdd}
      />,
    );
    act(() =>
      listener({
        status: 'ready',
        fileName: 'recording.wav',
        sizeBytes: 48,
        durationMs: 1200,
        limitReached: false,
      }),
    );
    view.rerender(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled={false}
        onAdd={onAdd}
      />,
    );
    expect(
      screen.queryByRole('button', { name: 'Add recording' }),
    ).not.toBeInTheDocument();
    expect(onAdd).not.toHaveBeenCalled();
  });

  it('keeps the second StrictMode controller live after disposing the first', () => {
    const view = render(
      <StrictMode>
        <MicrophoneCapture
          conversationKey="conversation-a"
          disabled={false}
          onAdd={vi.fn()}
        />
      </StrictMode>,
    );
    expect(captureMock.controllers).toHaveLength(2);
    expect(captureMock.controllers[0].dispose).toHaveBeenCalledOnce();
    expect(captureMock.controllers[1].dispose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    expect(captureMock.controllers[0].start).not.toHaveBeenCalled();
    expect(captureMock.controllers[1].start).toHaveBeenCalledOnce();
    view.unmount();
    act(() =>
      captureMock.listeners[1]({
        status: 'ready',
        fileName: 'recording.wav',
        sizeBytes: 48,
        durationMs: 1200,
        limitReached: false,
      }),
    );
    expect(captureMock.controllers[1].dispose).toHaveBeenCalledOnce();
  });

  it('shows a safe unsupported message only after the user asks to record', () => {
    captureMock.supported = false;
    render(
      <MicrophoneCapture
        conversationKey="conversation-a"
        disabled={false}
        onAdd={vi.fn()}
      />,
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Record audio' }));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Audio recording is unavailable in this browser.',
    );
    expect(captureMock.controller.start).not.toHaveBeenCalled();
  });
});
