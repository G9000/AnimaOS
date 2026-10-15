import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { StrictMode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MemoryReadiness } from '@animaOS-SWARM/sdk';

import { daemon } from '../../lib/daemon-api';
import { MemorySearchStatus } from './MemorySearchStatus';

const fixture = (
  overrides: Partial<MemoryReadiness> = {},
): MemoryReadiness => ({
  passed: true,
  embeddings: {
    enabled: true,
    provider: 'local',
    model: 'small-embed',
    dimension: 384,
    vectorCount: 7,
    persisted: true,
    storageFile: 'C:\\private\\secret-index.json',
  },
  evaluation: {
    passed: true,
    totalChecks: 3,
    passedChecks: 3,
    failureMessages: [],
    cases: [],
  },
  ...overrides,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const panel = (agentId = 'agent-a', online = true, epoch = 1) => (
  <MemorySearchStatus agentId={agentId} online={online} epoch={epoch} />
);

afterEach(() => vi.restoreAllMocks());

describe('MemorySearchStatus', () => {
  it('reads only on an explicit click and projects daemon-wide status without diagnostic paths', async () => {
    const read = vi
      .spyOn(daemon, 'memoryReadiness')
      .mockResolvedValue(fixture());
    const user = userEvent.setup();
    const view = render(<StrictMode>{panel()}</StrictMode>);
    expect(read).not.toHaveBeenCalled();
    view.rerender(<StrictMode>{panel('agent-b', true, 2)}</StrictMode>);
    expect(read).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    expect(read).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText(/Daemon-wide vector index: enabled/),
    ).toBeVisible();
    expect(screen.getByText(/Configured provider: local/)).toBeVisible();
    expect(screen.getByText(/Configured model: small-embed/)).toBeVisible();
    expect(screen.getByText(/Dimension: 384/)).toBeVisible();
    expect(screen.getByText(/Vectors: 7/)).toBeVisible();
    expect(screen.getByText(/Disk cache: configured/)).toBeVisible();
    expect(
      screen.getByText(/Local fixture checks: 3 of 3 passed/),
    ).toBeVisible();
    expect(screen.queryByText(/secret-index/)).toBeNull();
    expect(screen.queryByText(/overall ready/i)).toBeNull();
  });

  it('keeps disabled and empty index separate from saved-memory and fixture-check status', async () => {
    vi.spyOn(daemon, 'memoryReadiness').mockResolvedValue(
      fixture({
        passed: false,
        embeddings: {
          ...fixture().embeddings,
          enabled: false,
          vectorCount: 0,
          persisted: false,
        },
        evaluation: {
          ...fixture().evaluation,
          passed: false,
          totalChecks: 4,
          passedChecks: 2,
        },
      }),
    );
    await userEvent.setup().click(
      render(panel()).getByRole('button', {
        name: 'Check memory search status',
      }),
    );
    expect(
      await screen.findByText(/Daemon-wide vector index: disabled/),
    ).toBeVisible();
    expect(screen.getByText(/Vectors: 0/)).toBeVisible();
    expect(screen.getByText(/Disk cache: not configured/)).toBeVisible();
    expect(
      screen.getByText(/Local fixture checks: 2 of 4 passed/),
    ).toBeVisible();
    expect(screen.getByText(/2 local fixture checks failed/)).toBeVisible();
    expect(screen.getByText(/Saved memories and keyword search/)).toBeVisible();
  });

  it('reports zero checks without calling them passed', async () => {
    vi.spyOn(daemon, 'memoryReadiness').mockResolvedValue(
      fixture({
        evaluation: {
          ...fixture().evaluation,
          totalChecks: 0,
          passedChecks: 0,
        },
      }),
    );
    await userEvent.setup().click(
      render(panel()).getByRole('button', {
        name: 'Check memory search status',
      }),
    );
    expect(
      await screen.findByText('No local fixture checks reported.'),
    ).toBeVisible();
    expect(screen.queryByText(/0 of 0 passed/)).toBeNull();
  });

  it.each(['intfloat/multilingual-e5-small', 'BAAI/bge-m3'])(
    'displays supported FastEmbed model %s as ordinary status metadata',
    async (model) => {
      vi.spyOn(daemon, 'memoryReadiness').mockResolvedValue(
        fixture({
          embeddings: {
            ...fixture().embeddings,
            provider: 'fastembed',
            model,
          },
        }),
      );
      await userEvent.setup().click(
        render(panel()).getByRole('button', {
          name: 'Check memory search status',
        }),
      );

      expect(
        await screen.findByText(`Configured model: ${model}.`),
      ).toBeVisible();
      expect(
        screen.getByText(/Daemon-wide vector index: enabled/),
      ).toBeVisible();
      expect(screen.getByText(/Configured provider: fastembed/)).toBeVisible();
      expect(screen.getByText(/Dimension: 384/)).toBeVisible();
      expect(screen.getByText(/Vectors: 7/)).toBeVisible();
      expect(screen.getByText(/Disk cache: configured/)).toBeVisible();
      expect(
        screen.getByText(/Local fixture checks: 3 of 3 passed/),
      ).toBeVisible();
      expect(
        screen.queryByText('Memory search status is unavailable.'),
      ).toBeNull();
    },
  );

  it.each([
    { embeddings: { ...fixture().embeddings, dimension: -1 } },
    {
      embeddings: { ...fixture().embeddings, provider: 'C:\\private\\secret' },
    },
    { evaluation: { ...fixture().evaluation, passedChecks: 4 } },
    {
      evaluation: {
        ...fixture().evaluation,
        totalChecks: Number.MAX_SAFE_INTEGER + 1,
      },
    },
    { embeddings: { ...fixture().embeddings, enabled: 'yes' } },
    { embeddings: { ...fixture().embeddings, model: 'x'.repeat(81) } },
    { embeddings: { ...fixture().embeddings, model: '/home/user/secret' } },
    { embeddings: { ...fixture().embeddings, model: 'C:\\private\\secret' } },
    {
      embeddings: {
        ...fixture().embeddings,
        model: 'https://example.invalid/secret',
      },
    },
    { embeddings: { ...fixture().embeddings, model: 'safe\nsecret' } },
    { evaluation: { ...fixture().evaluation, passedChecks: undefined } },
  ])(
    'shows generic unavailable for malformed projected fields %#',
    async (change) => {
      vi.spyOn(daemon, 'memoryReadiness').mockResolvedValue(
        fixture(change as unknown as Partial<MemoryReadiness>),
      );
      await userEvent.setup().click(
        render(panel()).getByRole('button', {
          name: 'Check memory search status',
        }),
      );
      expect(
        await screen.findByText('Memory search status is unavailable.'),
      ).toBeVisible();
      expect(screen.queryByText(/secret/)).toBeNull();
    },
  );

  it('hides raw failures and permits a new explicit check after settlement', async () => {
    const read = vi
      .spyOn(daemon, 'memoryReadiness')
      .mockRejectedValueOnce(new Error('C:\\private\\secret provider token'))
      .mockResolvedValueOnce(fixture());
    const user = userEvent.setup();
    render(panel());
    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    expect(
      await screen.findByText('Memory search status is unavailable.'),
    ).toBeVisible();
    expect(screen.queryByText(/secret/)).toBeNull();
    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    expect(
      await screen.findByText(/Daemon-wide vector index: enabled/),
    ).toBeVisible();
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('retains one unsettled read across unmount and enables the replacement only after settlement', async () => {
    const first = deferred<MemoryReadiness>();
    const read = vi
      .spyOn(daemon, 'memoryReadiness')
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(fixture());
    const user = userEvent.setup();
    const old = render(panel());
    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    expect(read).toHaveBeenCalledTimes(1);
    old.unmount();
    render(panel());
    const check = screen.getByRole('button', {
      name: 'Check memory search status',
    });
    expect(check).toBeDisabled();
    await user.click(check);
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => first.resolve(fixture()));
    await waitFor(() => expect(check).toBeEnabled());
    expect(screen.queryByText(/Daemon-wide vector index: enabled/)).toBeNull();
    await user.click(check);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('fences a response across committed agent, epoch, and offline changes, even after switch-back', async () => {
    const pending = deferred<MemoryReadiness>();
    vi.spyOn(daemon, 'memoryReadiness').mockReturnValue(pending.promise);
    const user = userEvent.setup();
    const view = render(panel());
    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    view.rerender(panel('agent-b'));
    view.rerender(panel('agent-a', true, 2));
    view.rerender(panel('agent-a', false, 2));
    view.rerender(panel('agent-a', true, 1));
    await act(async () => pending.resolve(fixture()));
    expect(screen.queryByText(/Daemon-wide vector index: enabled/)).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Check memory search status' }),
    ).toBeEnabled();
  });

  it('keeps the button disabled on repeated clicks while the read has not settled', async () => {
    const pending = deferred<MemoryReadiness>();
    const read = vi
      .spyOn(daemon, 'memoryReadiness')
      .mockReturnValue(pending.promise);
    const user = userEvent.setup();
    render(panel());
    const check = screen.getByRole('button', {
      name: 'Check memory search status',
    });
    await user.click(check);
    expect(check).toBeDisabled();
    await user.click(check);
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(fixture()));
  });
});
