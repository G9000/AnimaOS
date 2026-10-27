import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { McpConnection, McpOAuthStatus } from '../../lib/daemon-api';
import { McpOAuthAccount, type McpOAuthApi } from './McpOAuthAccount';

const row: McpConnection = {
  id: 'server-1',
  agentId: 'main',
  label: 'Account server',
  endpoint: 'https://mcp.example/tools',
  transport: 'streamableHttp',
  stdio: null,
  enabled: true,
  version: 3,
  hasBearerCredential: false,
  reviewedTools: [],
  state: 'disconnected',
  wireGeneration: 0,
  discoveryId: null,
  candidates: [],
  physicalCallsActive: false,
  error: null,
};
function api(): McpOAuthApi {
  return {
    startOAuth: vi.fn().mockResolvedValue({
      authorizationUrl: 'https://accounts.example/authorize?state=synthetic',
      attemptId: 'attempt-1',
      expiresAt: Math.floor(Date.now() / 1000) + 600,
    }),
    getOAuthStatus: vi.fn().mockResolvedValue({ state: 'signed_out' }),
    cancelOAuth: vi.fn().mockResolvedValue(row),
    signOutOAuth: vi.fn().mockResolvedValue({ ...row, version: 4 }),
  };
}
const runAction = async (action: () => Promise<void>) => action();
async function loaded() {
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled(),
  );
}

describe('MCP account sign-in', () => {
  beforeEach(() => {
    vi.spyOn(window, 'open').mockReturnValue(null);
  });

  it('discards the browser link when another panel replaces the pending attempt', async () => {
    vi.useFakeTimers();
    try {
      const mock = api();
      vi.mocked(mock.getOAuthStatus)
        .mockResolvedValueOnce({ state: 'signed_out' })
        .mockResolvedValue({ state: 'pending', attemptId: 'attempt-2' });
      render(
        <McpOAuthAccount
          connection={row}
          api={mock}
          lifetime={{}}
          blocked={false}
          stale={false}
          runAction={runAction}
          onSettled={vi.fn()}
        />,
      );
      await act(async () => {
        await Promise.resolve();
      });
      fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
      await act(async () => {
        await Promise.resolve();
      });
      expect(
        screen.getByRole('link', { name: 'Continue sign-in in browser' }),
      ).toBeInTheDocument();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Cancel sign-in' }));
      await act(async () => {
        await Promise.resolve();
      });
      expect(mock.cancelOAuth).toHaveBeenCalledWith('main', 'server-1', {
        expectedVersion: 3,
        attemptId: 'attempt-2',
      });
    } finally {
      vi.useRealTimers();
    }
  });
  it('polls an explicit attempt until it settles and then stops polling', async () => {
    vi.useFakeTimers();
    try {
      const mock = api();
      vi.mocked(mock.getOAuthStatus)
        .mockResolvedValueOnce({ state: 'signed_out' })
        .mockResolvedValue({ state: 'connected' });
      const settled = vi.fn();
      render(
        <McpOAuthAccount
          connection={row}
          api={mock}
          lifetime={{}}
          blocked={false}
          stale={false}
          runAction={runAction}
          onSettled={settled}
        />,
      );
      await act(async () => {
        await Promise.resolve();
      });
      fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
      await act(async () => {
        await Promise.resolve();
      });
      expect(
        screen.getByText('Waiting for browser sign-in'),
      ).toBeInTheDocument();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(screen.getByText('Account signed in')).toBeInTheDocument();
      expect(settled).toHaveBeenCalledTimes(1);
      const reads = vi.mocked(mock.getOAuthStatus).mock.calls.length;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20000);
      });
      expect(mock.getOAuthStatus).toHaveBeenCalledTimes(reads);
    } finally {
      vi.useRealTimers();
    }
  });

  it('starts only on a click, exposes a safe browser link, and sends the optional public client ID', async () => {
    const mock = api();
    render(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    await loaded();
    expect(mock.startOAuth).not.toHaveBeenCalled();
    expect(window.open).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Client ID (optional)'), {
      target: { value: 'native-public-client' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    const link = await screen.findByRole('link', {
      name: 'Continue sign-in in browser',
    });
    expect(link).toHaveAttribute(
      'href',
      'https://accounts.example/authorize?state=synthetic',
    );
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(window.open).toHaveBeenCalledWith('about:blank', '_blank');
    expect(mock.startOAuth).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 3,
      clientId: 'native-public-client',
    });
    expect(screen.queryByLabelText('Bearer token')).not.toBeInTheDocument();
  });

  it('cancels exactly the pending attempt and offers sign-out separately from server disconnect', async () => {
    const mock = api();
    vi.mocked(mock.getOAuthStatus).mockResolvedValue({
      state: 'pending',
      attemptId: 'restored-attempt',
    });
    const settled = vi.fn();
    const view = render(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={settled}
      />,
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Cancel sign-in' }),
    );
    await waitFor(() =>
      expect(mock.cancelOAuth).toHaveBeenCalledWith('main', 'server-1', {
        expectedVersion: 3,
        attemptId: 'restored-attempt',
      }),
    );
    expect(settled).toHaveBeenCalled();
    vi.mocked(mock.getOAuthStatus).mockResolvedValue({ state: 'connected' });
    view.rerender(
      <McpOAuthAccount
        connection={{ ...row, version: 4, hasOAuthCredential: true }}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={settled}
      />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Sign out' }));
    await waitFor(() =>
      expect(mock.signOutOAuth).toHaveBeenCalledWith('main', 'server-1', {
        expectedVersion: 4,
      }),
    );
  });

  it('rejects an unsafe authorization link and does not retry start', async () => {
    const mock = api();
    vi.mocked(mock.startOAuth).mockResolvedValue({
      authorizationUrl: 'javascript:alert(1)',
      attemptId: 'attempt-1',
      expiresAt: 9999999999,
    });
    render(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Refresh status',
    );
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(mock.startOAuth).toHaveBeenCalledTimes(1);
  });

  it('ignores a status response from an obsolete panel lifetime', async () => {
    let resolve!: (status: McpOAuthStatus) => void;
    const pending = new Promise<McpOAuthStatus>((done) => {
      resolve = done;
    });
    const mock = api();
    vi.mocked(mock.getOAuthStatus).mockReturnValueOnce(pending);
    const view = render(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    const oldSignal = vi.mocked(mock.getOAuthStatus).mock.calls[0][2]?.signal;
    view.rerender(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked
        stale
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    await act(async () => {
      resolve({ state: 'connected' });
    });
    expect(oldSignal?.aborted).toBe(true);
    expect(screen.queryByText('Account signed in')).not.toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('closes an unopened consent tab when its start response belongs to an old agent', async () => {
    const mock = api();
    let resolve!: (started: {
      authorizationUrl: string;
      attemptId: string;
      expiresAt: number;
    }) => void;
    vi.mocked(mock.startOAuth).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const popup = {
      opener: window,
      document: document.implementation.createHTMLDocument(),
      close: vi.fn(),
      location: { replace: vi.fn() },
    };
    vi.mocked(window.open).mockReturnValue(popup as unknown as Window);
    const view = render(
      <McpOAuthAccount
        connection={row}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(popup.opener).toBe(null);
    expect(
      popup.document
        .querySelector('meta[name="referrer"]')
        ?.getAttribute('content'),
    ).toBe('no-referrer');
    view.rerender(
      <McpOAuthAccount
        connection={{ ...row, id: 'other-server', agentId: 'other' }}
        api={mock}
        lifetime={{}}
        blocked={false}
        stale={false}
        runAction={runAction}
        onSettled={vi.fn()}
      />,
    );
    await act(async () => {
      resolve({
        authorizationUrl: 'https://accounts.example/authorize',
        attemptId: 'obsolete-attempt',
        expiresAt: 9999999999,
      });
    });
    expect(popup.close).toHaveBeenCalledTimes(1);
    expect(popup.location.replace).not.toHaveBeenCalled();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});
