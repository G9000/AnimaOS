import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { MarkdownMessage } from '../MarkdownMessage';
import { WorkspaceArtifactLayout } from './WorkspaceArtifactPreview';
import {
  artifactDocument,
  storageSnapshot,
  workspaceArtifactPath,
} from './artifact-document';

const path = 'artifacts/credit-card-bill-check.html';
const source =
  '<!doctype html><html><body><h1>Actual bill page</h1><button onclick="this.textContent=2">1</button></body></html>';
const blob = () => new Blob([source], { type: 'application/octet-stream' });
function layout(
  markdown = `Created at \`${path}\`.`,
  sessionId = 'chat:one',
  clearGeneration = 0,
) {
  return (
    <WorkspaceArtifactLayout
      agentId="a"
      sessionId={sessionId}
      clearGeneration={clearGeneration}
    >
      <main>
        <MarkdownMessage>{markdown}</MarkdownMessage>
      </main>
    </WorkspaceArtifactLayout>
  );
}

async function findFrame() {
  await waitFor(() => expect(document.querySelector('iframe')).not.toBeNull());
  // Simulated frame messages must arrive after React attaches its bridge listener.
  await act(async () => {});
  return document.querySelector('iframe')!;
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  localStorage.clear();
});

async function animatedPreview() {
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockResolvedValue(blob());
  render(layout());
  const root = document.querySelector('.workspace-artifact-layout')!;
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(100, 70, 1000, 800),
  );
  fireEvent.resize(window);
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const frame = await findFrame();
  fireEvent.keyDown(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
    { key: 'ArrowRight' },
  );
  const slot = document.querySelector('.workspace-artifact-slot')!;
  const bounds = vi
    .spyOn(slot, 'getBoundingClientRect')
    .mockReturnValue(new DOMRect(620, 70, 480, 800));
  const motions: { finish: () => void; cancel: ReturnType<typeof vi.fn> }[] =
    [];
  const animate = vi.fn(() => {
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const cancel = vi.fn();
    motions.push({ finish, cancel });
    return { finished, cancel };
  });
  Object.defineProperty(slot, 'animate', {
    configurable: true,
    value: animate,
  });
  return { download, frame, bounds, motions, animate };
}

it('fades fullscreen closed in place and removes it only after the animation', async () => {
  const { frame, bounds, motions, animate, download } = await animatedPreview();
  const opener = screen.getByRole('button', { name: `Open ${path}` });
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  await act(async () => motions[0].finish());
  bounds.mockReturnValue(
    new DOMRect(0, 0, window.innerWidth, window.innerHeight),
  );
  const panel = screen.getByRole('dialog', { name: 'Artifact preview' });
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(panel).toHaveAttribute('role', 'dialog');
  expect(panel).toHaveAttribute('inert');
  expect(document.querySelector('iframe')).toBe(frame);
  expect(opener.closest('[inert]')).not.toBeNull();
  expect(animate).toHaveBeenLastCalledWith(
    [
      expect.objectContaining({
        left: '0px',
        top: '0px',
        width: `${window.innerWidth}px`,
        height: `${window.innerHeight}px`,
        opacity: '1',
      }),
      expect.objectContaining({
        left: '0px',
        top: '0px',
        width: `${window.innerWidth}px`,
        height: `${window.innerHeight}px`,
        opacity: 0,
        transform: 'scale(0.985)',
      }),
    ],
    expect.objectContaining({ duration: 220, fill: 'both' }),
  );
  await act(async () => motions[1].finish());
  expect(panel).not.toBeInTheDocument();
  expect(document.querySelector('iframe')).toBeNull();
  expect(opener.closest('[inert]')).toBeNull();
  expect(opener).toHaveFocus();
  expect(download).toHaveBeenCalledOnce();
});

it('closes from the current bounds if fullscreen motion is interrupted by X', async () => {
  const { bounds, motions, animate } = await animatedPreview();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  bounds.mockReturnValue(new DOMRect(200, 20, 1280, 940));
  const panel = screen.getByRole('dialog', { name: 'Artifact preview' });
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(motions[0].cancel).toHaveBeenCalledOnce();
  expect(panel).toHaveAttribute('role', 'dialog');
  expect(animate).toHaveBeenLastCalledWith(
    [
      expect.objectContaining({
        left: '200px',
        top: '20px',
        width: '1280px',
        height: '940px',
      }),
      expect.objectContaining({
        left: '200px',
        top: '20px',
        width: '1280px',
        height: '940px',
        opacity: 0,
      }),
    ],
    expect.anything(),
  );
  await act(async () => motions[0].finish());
  expect(panel).toBeInTheDocument();
  await act(async () => motions[1].finish());
  expect(panel).not.toBeInTheDocument();
});

it('cancels a fullscreen close if the artifact is reopened before it finishes', async () => {
  const { frame, motions } = await animatedPreview();
  const opener = screen.getByRole('button', { name: `Open ${path}` });
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  await act(async () => motions[0].finish());
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  fireEvent.click(opener);
  expect(motions[1].cancel).toHaveBeenCalledOnce();
  await act(async () => motions[1].finish());
  expect(
    screen.getByRole('complementary', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
  expect(document.querySelector('iframe')).toBe(frame);
});

it('closes fullscreen immediately with reduced motion and recovers from animation failure', async () => {
  const { motions, animate } = await animatedPreview();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  await act(async () => motions[0].finish());
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(document.querySelector('iframe')).toBeNull();
  expect(animate).toHaveBeenCalledOnce();
  vi.unstubAllGlobals();
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  await findFrame();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  await act(async () => motions[1].finish());
  animate.mockImplementationOnce(() => {
    throw new Error('Animation unavailable');
  });
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  await act(async () => {});
  expect(document.querySelector('iframe')).toBeNull();
});

it('animates fullscreen bounds both ways and restores the split only after returning', async () => {
  const { download, frame, bounds, motions, animate } = await animatedPreview();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  expect(animate).toHaveBeenCalledWith(
    [
      expect.objectContaining({
        left: '620px',
        top: '70px',
        width: '480px',
        height: '800px',
      }),
      expect.objectContaining({
        left: '0px',
        top: '0px',
        width: `${window.innerWidth}px`,
        height: `${window.innerHeight}px`,
      }),
    ],
    expect.objectContaining({ duration: 280, fill: 'both' }),
  );
  await act(async () => motions[0].finish());
  bounds.mockReturnValue(
    new DOMRect(0, 0, window.innerWidth, window.innerHeight),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Exit fullscreen' }));
  expect(animate).toHaveBeenLastCalledWith(
    [
      expect.objectContaining({ left: '0px', top: '0px' }),
      expect.objectContaining({
        left: '620px',
        top: '70px',
        width: '480px',
        height: '800px',
      }),
    ],
    expect.anything(),
  );
  expect(
    screen.getByRole('dialog', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
  expect(document.querySelector('iframe')).toBe(frame);
  await act(async () => motions[1].finish());
  expect(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
  ).toHaveAttribute('aria-valuenow', '52');
  expect(document.querySelector('iframe')).toBe(frame);
  expect(download).toHaveBeenCalledOnce();
});

it('cancels interrupted fullscreen motion and ignores late completion after closing', async () => {
  const { motions } = await animatedPreview();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  fireEvent.click(screen.getByRole('button', { name: 'Exit fullscreen' }));
  expect(motions[0].cancel).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  expect(motions[1].cancel).toHaveBeenCalledOnce();
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(motions[2].cancel).toHaveBeenCalledOnce();
  await act(async () => {
    for (const motion of motions) motion.finish();
  });
  expect(screen.queryByRole('dialog', { name: 'Artifact preview' })).toBeNull();
  await waitFor(() => expect(document.querySelector('iframe')).toBeNull());
});

it('skips fullscreen animations when reduced motion is preferred', async () => {
  const { animate } = await animatedPreview();
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  fireEvent.click(screen.getByRole('button', { name: 'Exit fullscreen' }));
  expect(animate).not.toHaveBeenCalled();
  expect(
    screen.getByRole('complementary', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
});

it('still enters and restores fullscreen if the browser cannot start an animation', async () => {
  const { animate } = await animatedPreview();
  animate.mockImplementation(() => {
    throw new Error('Animation unavailable');
  });
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  await act(async () => {});
  expect(
    screen.getByRole('dialog', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Exit fullscreen' }));
  await act(async () => {});
  expect(
    screen.getByRole('complementary', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
  ).toHaveAttribute('aria-valuenow', '52');
});

it('expands the preview without reloading it and restores the chosen split', async () => {
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockResolvedValue(blob());
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const frame = await findFrame();
  fireEvent.keyDown(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
    { key: 'ArrowRight' },
  );
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  expect(
    screen.getByRole('dialog', { name: 'Artifact preview' }),
  ).toHaveAttribute('aria-modal', 'true');
  expect(
    screen.queryByRole('separator', {
      name: 'Resize chat and artifact preview',
    }),
  ).toBeNull();
  expect(document.querySelector('iframe')).toBe(frame);
  fireEvent.click(screen.getByRole('button', { name: 'Exit fullscreen' }));
  expect(
    screen.getByRole('complementary', { name: 'Artifact preview' }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
  ).toHaveAttribute('aria-valuenow', '52');
  expect(document.querySelector('iframe')).toBe(frame);
  expect(download).toHaveBeenCalledOnce();
});

it('uses Escape from the artifact to leave fullscreen before closing the preview', async () => {
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(blob());
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(
    '11111111-1111-4111-8111-111111111111',
  );
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const frame = await findFrame();
  fireEvent.click(screen.getByRole('button', { name: 'Enter fullscreen' }));
  fireEvent(
    window,
    new MessageEvent('message', {
      source: frame.contentWindow,
      data: {
        type: 'anima-artifact-close',
        nonce: '11111111-1111-4111-8111-111111111111',
      },
    }),
  );
  const panel = screen.getByRole('complementary', { name: 'Artifact preview' });
  expect(document.querySelector('iframe')).toBe(frame);
  fireEvent.keyDown(panel, { key: 'Escape' });
  await waitFor(() => expect(document.querySelector('iframe')).toBeNull());
});

it('isolates fullscreen controls and restores the background when closed', async () => {
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(blob());
  render(
    <>
      <button>Outside preview</button>
      <div inert>Already unavailable</div>
      {layout()}
    </>,
  );
  const outside = screen.getByRole('button', { name: 'Outside preview' });
  const unavailable = screen.getByText('Already unavailable');
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  await findFrame();
  const fullscreen = screen.getByRole('button', { name: 'Enter fullscreen' });
  fireEvent.click(fullscreen);
  expect(outside).toHaveAttribute('inert');
  fireEvent.keyDown(screen.getByRole('dialog', { name: 'Artifact preview' }), {
    key: 'Escape',
  });
  expect(outside).not.toHaveAttribute('inert');
  expect(unavailable).toHaveAttribute('inert');
  expect(fullscreen).toHaveFocus();
  fireEvent.click(fullscreen);
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(outside).not.toHaveAttribute('inert');
  await waitFor(() => expect(document.querySelector('iframe')).toBeNull());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  expect(
    screen.getByRole('button', { name: 'Enter fullscreen' }),
  ).toBeInTheDocument();
  expect(screen.queryByRole('dialog', { name: 'Artifact preview' })).toBeNull();
});

function openResizableLayout(width = 1000) {
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockImplementation(
    () => new Promise(() => {}),
  );
  render(layout());
  const root = document.querySelector('.workspace-artifact-layout')!;
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(100, 0, width, 900),
  );
  fireEvent.resize(window);
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const divider = screen.getByRole('separator', {
    name: 'Resize chat and artifact preview',
  });
  Object.defineProperty(divider, 'setPointerCapture', { value: vi.fn() });
  return { divider, root };
}

it('resizes both panes by dragging and stops after releasing the pointer', async () => {
  const user = userEvent.setup();
  const { divider } = openResizableLayout();
  await user.pointer([
    { keys: '[MouseLeft>]', target: divider, coords: { clientX: 600 } },
    { target: divider, coords: { clientX: 750 } },
  ]);
  expect(divider).toHaveAttribute('aria-valuenow', '65');
  expect(divider.setPointerCapture).toHaveBeenCalled();
  await user.pointer({ keys: '[/MouseLeft]', target: divider });
  await user.pointer({ target: divider, coords: { clientX: 450 } });
  expect(divider).toHaveAttribute('aria-valuenow', '65');
  fireEvent.doubleClick(divider);
  expect(divider).toHaveAttribute('aria-valuenow', '50');
});

it('supports keyboard resizing, keeps both panes usable, and remembers the split on reopen', () => {
  const { divider } = openResizableLayout();
  fireEvent.keyDown(divider, { key: 'ArrowLeft' });
  expect(divider).toHaveAttribute('aria-valuenow', '48');
  fireEvent.keyDown(divider, { key: 'Home' });
  expect(divider).toHaveAttribute('aria-valuenow', '32');
  fireEvent.keyDown(divider, { key: 'End' });
  expect(divider).toHaveAttribute('aria-valuenow', '68');
  vi.useFakeTimers();
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  act(() => vi.advanceTimersByTime(500));
  expect(
    screen.queryByRole('separator', {
      name: 'Resize chat and artifact preview',
    }),
  ).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  expect(
    screen.getByRole('separator', { name: 'Resize chat and artifact preview' }),
  ).toHaveAttribute('aria-valuenow', '68');
});

it('stops resizing when the browser loses focus', async () => {
  const user = userEvent.setup();
  const { divider } = openResizableLayout();
  await user.pointer([
    { keys: '[MouseLeft>]', target: divider, coords: { clientX: 600 } },
    { target: divider, coords: { clientX: 700 } },
  ]);
  expect(divider).toHaveAttribute('aria-valuenow', '60');
  fireEvent.blur(window);
  await user.pointer({ target: divider, coords: { clientX: 450 } });
  expect(divider).toHaveAttribute('aria-valuenow', '60');
});

it.each(['Home', 'End'])(
  'keeps the accessible value within its bounds at fractional percentages: %s',
  (key) => {
    const { divider } = openResizableLayout(960);
    fireEvent.keyDown(divider, { key });
    const value = Number(divider.getAttribute('aria-valuenow'));
    expect(value).toBeGreaterThanOrEqual(
      Number(divider.getAttribute('aria-valuemin')),
    );
    expect(value).toBeLessThanOrEqual(
      Number(divider.getAttribute('aria-valuemax')),
    );
  },
);

it('keeps the file mounted through its exit, then releases it completely', () => {
  vi.useFakeTimers();
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockImplementation(() => new Promise(() => {}));
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const panel = screen.getByRole('complementary');
  const signal = download.mock.calls[0][1]!;
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(panel).toBeInTheDocument();
  expect(panel).toHaveAttribute('aria-hidden', 'true');
  expect(signal.aborted).toBe(false);
  act(() => vi.advanceTimersByTime(500));
  expect(panel).not.toBeInTheDocument();
  expect(signal.aborted).toBe(true);
});

it('cancels a pending exit when the file is reopened', () => {
  vi.useFakeTimers();
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockImplementation(() => new Promise(() => {}));
  render(layout());
  const opener = screen.getByRole('button', { name: `Open ${path}` });
  fireEvent.click(opener);
  const panel = screen.getByRole('complementary');
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  act(() => vi.advanceTimersByTime(100));
  fireEvent.click(opener);
  act(() => vi.advanceTimersByTime(500));
  expect(screen.getByRole('complementary')).toBe(panel);
  expect(download).toHaveBeenCalledOnce();
  expect(download.mock.calls[0][1]!.aborted).toBe(false);
});

it('closes immediately when reduced motion is preferred', () => {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  );
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockImplementation(() => new Promise(() => {}));
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const panel = screen.getByRole('complementary');
  fireEvent.click(
    screen.getByRole('button', { name: 'Close artifact preview' }),
  );
  expect(panel).not.toBeInTheDocument();
  expect(download.mock.calls[0][1]!.aborted).toBe(true);
});

it.each([
  [`Created at \`${path}\`.`, {}],
  [`[Open the bill](${path})`, { ctrlKey: true }],
  [
    `[Open the bill](/api/workspace/file/download?path=${encodeURIComponent(path)})`,
    { metaKey: true },
  ],
])(
  'opens the real HTML bytes from a file reference: %s',
  async (markdown, modifiers) => {
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockResolvedValue(blob());
    const create = vi
      .spyOn(URL, 'createObjectURL')
      .mockReturnValue('blob:artifact');
    const revoke = vi
      .spyOn(URL, 'revokeObjectURL')
      .mockImplementation(() => {});
    const view = render(layout(markdown));
    expect(download).not.toHaveBeenCalled();
    expect(screen.queryByRole('complementary')).toBeNull();
    fireEvent.click(
      screen.getByRole('button', { name: `Open ${path}` }),
      modifiers,
    );
    const frame = await findFrame();
    expect(download).toHaveBeenCalledWith(path, expect.any(AbortSignal));
    const wrapper = new DOMParser().parseFromString(
      frame.getAttribute('srcdoc')!,
      'text/html',
    );
    expect(wrapper.querySelector('iframe')?.getAttribute('srcdoc')).toContain(
      source,
    );
    expect(
      wrapper.querySelector('meta[http-equiv="Content-Security-Policy"]'),
    ).not.toBeNull();
    expect(
      wrapper
        .querySelector('meta[http-equiv="Content-Security-Policy"]')
        ?.getAttribute('content'),
    ).toContain("frame-src 'none'");
    expect(frame).toHaveAttribute(
      'sandbox',
      'allow-scripts allow-downloads allow-modals',
    );
    expect(frame).toHaveAttribute('referrerpolicy', 'no-referrer');
    const panel = within(
      screen.getByRole('complementary', { name: 'Artifact preview' }),
    );
    expect(
      panel.getByRole('link', { name: 'Open in new window' }),
    ).toHaveAttribute('rel', 'noopener noreferrer');
    expect(panel.getByRole('link', { name: 'Download HTML' })).toHaveAttribute(
      'download',
      'credit-card-bill-check.html',
    );
    expect(create).toHaveBeenCalledTimes(2);
    fireEvent.click(
      panel.getByRole('button', { name: 'Close artifact preview' }),
    );
    expect(screen.queryByRole('complementary')).toBeNull();
    expect(screen.queryByText(/Show dismissed results|Pin report/)).toBeNull();
    view.rerender(layout(markdown));
    expect(screen.queryByRole('complementary')).toBeNull();
    await waitFor(() => expect(revoke).toHaveBeenCalledTimes(2));
  },
);

it('opens from the keyboard and returns focus when closed', async () => {
  const user = userEvent.setup();
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(blob());
  render(layout());
  const opener = screen.getByRole('button', { name: `Open ${path}` });
  opener.focus();
  await user.keyboard('{Enter}');
  await findFrame();
  const panel = screen.getByRole('complementary', { name: 'Artifact preview' });
  expect(panel).toHaveFocus();
  fireEvent.keyDown(panel, { key: 'Escape' });
  expect(screen.queryByRole('complementary')).toBeNull();
  expect(opener).toHaveFocus();
});

it.each(['switch', 'clear'] as const)(
  'aborts loading when the conversation changes: %s',
  async (change) => {
    let finish!: (value: Blob) => void;
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockImplementation(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
    const create = vi.spyOn(URL, 'createObjectURL');
    const view = render(layout());
    fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
    const signal = download.mock.calls[0][1]!;
    view.rerender(
      layout(
        undefined,
        change === 'switch' ? 'chat:two' : 'chat:one',
        change === 'clear' ? 1 : 0,
      ),
    );
    expect(signal.aborted).toBe(true);
    await act(async () => finish(blob()));
    expect(screen.queryByRole('complementary')).toBeNull();
    expect(create).not.toHaveBeenCalled();
  },
);

it('shows a file error and reloads only after the owner retries', async () => {
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockRejectedValueOnce(new Error('File not found'))
    .mockResolvedValueOnce(blob());
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  expect(await screen.findByRole('alert')).toHaveTextContent('File not found');
  fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
  await findFrame();
  expect(download).toHaveBeenCalledTimes(2);
});

it('does not turn fenced code or paths inside another link into nested controls', () => {
  render(
    layout(
      `[\`${path}\`](${path})\n\n[\`${path}\`](https://example.com)\n\n\`\`\`text\n${path}\n\`\`\``,
    ),
  );
  expect(screen.getAllByRole('button', { name: `Open ${path}` })).toHaveLength(
    1,
  );
  expect(screen.getByRole('link')).toHaveAttribute(
    'href',
    'https://example.com',
  );
});

it('keeps HTML references inert outside the owning file viewer', () => {
  const download = vi.spyOn(daemon, 'downloadWorkspaceFile');
  render(<MarkdownMessage>{`\`${path}\``}</MarkdownMessage>);
  expect(screen.queryByRole('button', { name: `Open ${path}` })).toBeNull();
  expect(download).not.toHaveBeenCalled();
});

it.each(['artifacts/100%.html', 'artifacts/a%20b.html'])(
  'preserves literal filename bytes: %s',
  async (filename) => {
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockResolvedValue(blob());
    render(layout(`Created at \`${filename}\`.`));
    fireEvent.click(screen.getByRole('button', { name: `Open ${filename}` }));
    await findFrame();
    expect(download).toHaveBeenCalledWith(filename, expect.any(AbortSignal));
  },
);

it('closes on Escape relayed from the current frame and ignores forged close messages', async () => {
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(blob());
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(
    '11111111-1111-4111-8111-111111111111',
  );
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const frame = await findFrame();
  const data = {
    type: 'anima-artifact-close',
    nonce: '11111111-1111-4111-8111-111111111111',
  };
  fireEvent(window, new MessageEvent('message', { data, source: window }));
  expect(screen.getByRole('complementary')).toBeVisible();
  fireEvent(
    window,
    new MessageEvent('message', { data, source: frame.contentWindow }),
  );
  expect(screen.queryByRole('complementary')).toBeNull();
});

it('persists only bounded storage messages from the selected isolated frame', async () => {
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(blob());
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(
    '11111111-1111-4111-8111-111111111111',
  );
  render(layout());
  fireEvent.click(screen.getByRole('button', { name: `Open ${path}` }));
  const frame = (await findFrame()) as HTMLIFrameElement;
  const data = {
    type: 'anima-artifact-storage',
    nonce: '11111111-1111-4111-8111-111111111111',
    snapshot: '[["rows","saved"]]',
  };
  window.dispatchEvent(new MessageEvent('message', { data, source: window }));
  expect(localStorage.length).toBe(0);
  window.dispatchEvent(
    new MessageEvent('message', { data, source: frame.contentWindow }),
  );
  await waitFor(() =>
    expect(
      localStorage.getItem(
        `anima-artifact:${JSON.stringify(['a', 'chat:one', 0, path])}`,
      ),
    ).toBe(data.snapshot),
  );
  const create = vi.spyOn(URL, 'createObjectURL');
  fireEvent.click(screen.getByRole('link', { name: 'Open in new window' }));
  expect(create).toHaveBeenCalledOnce();
  expect(await (create.mock.calls[0][0] as Blob).text()).toContain('saved');
  expect(storageSnapshot('{"consoleToken":"wrong"}')).toBeNull();
  expect(storageSnapshot('x'.repeat(256 * 1024 + 1))).toBeNull();
});

it.each([
  '../bill.html',
  '/bill.html',
  'https://example.com/bill.html',
  'file:///bill.html',
  'artifacts\\bill.html',
  'artifacts/%2e%2e/bill.html',
  'bill.html?token=private',
])('leaves invalid artifact paths inert: %s', (target) => {
  expect(workspaceArtifactPath(target)).toBeNull();
});

it('keeps artifact scripts separate from host storage and blocks remote resources', () => {
  const document = artifactDocument(
    source,
    'nonce',
    '[["value","</script><script>escape()</script>"]]',
  );
  expect(document).toContain("connect-src 'none'");
  expect(document).toContain("Object.defineProperty(window,'localStorage'");
  expect(document).not.toContain('</script><script>escape()');
});
