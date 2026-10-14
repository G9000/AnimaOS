import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { BundleSummary } from '../../lib/daemon-api';
import {
  communityBundle,
  communityManifest,
  communityReview,
} from '../../test/community-bundles';
import {
  CommunityBundlesPanel,
  type CommunityBundlesOwnerApi,
} from './CommunityBundlesPanel';

function api(rows: BundleSummary[] = []): CommunityBundlesOwnerApi {
  return {
    list: vi.fn().mockResolvedValue(rows),
    get: vi.fn().mockResolvedValue(communityBundle()),
    review: vi.fn().mockResolvedValue(communityReview()),
    install: vi.fn().mockResolvedValue(communityBundle()),
    update: vi.fn().mockResolvedValue(communityBundle({ version: 4 })),
    setEnabled: vi
      .fn()
      .mockResolvedValue(
        communityBundle({ enabled: false, state: 'disabled', version: 4 }),
      ),
    remove: vi.fn().mockResolvedValue({
      bundleId: 'bundle-1',
      removed: true,
      trashPaths: ['.trash/owned'],
      toolNames: ['read_file'],
    }),
    toolNames: vi.fn().mockResolvedValue(['read_file', 'unknown_custom']),
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { resolve, promise };
}

async function loaded() {
  await waitFor(() =>
    expect(
      screen.queryByText('Reading saved bundles…'),
    ).not.toBeInTheDocument(),
  );
}

async function enter(
  user: ReturnType<typeof userEvent.setup>,
  manifest = communityManifest(),
) {
  fireEvent.change(screen.getByRole('textbox', { name: 'Bundle manifest' }), {
    target: { value: JSON.stringify(manifest) },
  });
  if (manifest.components.some((part) => part.kind === 'mcpStdio')) {
    fireEvent.change(
      screen.getByRole('textbox', { name: 'Executable for peer' }),
      { target: { value: 'C:\\synthetic\\peer.exe' } },
    );
    await user.click(
      screen.getByRole('checkbox', {
        name: 'I trust these local programs and literal arguments',
      }),
    );
  }
}

async function inspect(
  user: ReturnType<typeof userEvent.setup>,
  name = 'Violet bundle',
) {
  const card = within(
    screen.getByRole('article', { name: `Community bundle ${name}` }),
  );
  await user.click(card.getByRole('button', { name: 'Inspect bundle' }));
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Remove bundle' })).toBeEnabled(),
  );
  return card;
}

describe('owner community bundles', () => {
  it('loads saved metadata only and leaves empty review and installation inactive', async () => {
    const mock = api();
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    expect(
      screen.getByRole('region', { name: 'Community bundles' }),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Review bundle' }),
    ).toBeDisabled();
    expect(mock.list).toHaveBeenCalledTimes(1);
    for (const method of [
      mock.review,
      mock.install,
      mock.update,
      mock.setEnabled,
      mock.remove,
      mock.toolNames,
    ])
      expect(method).not.toHaveBeenCalled();
  });

  it('reviews exact raw text and owner bindings with singleton empty argv without installing or adding authority', async () => {
    const user = userEvent.setup();
    const manifest = communityManifest({
      components: [
        {
          key: 'peer',
          kind: 'mcpStdio',
          label: 'Peer',
          programRef: 'peer',
          args: [''],
        },
      ],
    });
    const mock = api();
    vi.mocked(mock.review).mockResolvedValue(communityReview(manifest));
    const uncertain = vi.fn();
    render(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        onPermissionsUncertain={uncertain}
      />,
    );
    await loaded();
    await enter(user, manifest);
    expect(screen.getByText('peer: [""]')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    expect(mock.review).toHaveBeenCalledWith('main', {
      manifestText: JSON.stringify(manifest),
      programBindings: { peer: 'C:\\synthetic\\peer.exe' },
    });
    expect(screen.getByText('Arguments: [""]')).toBeVisible();
    expect(mock.install).not.toHaveBeenCalled();
    expect(uncertain).not.toHaveBeenCalled();
  });

  it('treats program references that match object properties as plain binding keys', async () => {
    const user = userEvent.setup();
    const mock = api();
    const manifest = communityManifest({
      components: [
        {
          key: 'peer',
          kind: 'mcpStdio',
          label: 'Peer',
          programRef: 'constructor',
          args: [],
        },
      ],
    });
    vi.mocked(mock.review).mockResolvedValue(communityReview(manifest));
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    fireEvent.change(screen.getByRole('textbox', { name: 'Bundle manifest' }), {
      target: { value: JSON.stringify(manifest) },
    });
    const binding = screen.getByRole('textbox', {
      name: 'Executable for constructor',
    });
    expect(binding).toHaveValue('');
    await user.click(
      screen.getByRole('checkbox', {
        name: 'I trust these local programs and literal arguments',
      }),
    );
    expect(
      screen.getByRole('button', { name: 'Review bundle' }),
    ).toBeDisabled();
    fireEvent.change(binding, { target: { value: 'C:\\synthetic\\peer.exe' } });
    await user.click(
      screen.getByRole('checkbox', {
        name: 'I trust these local programs and literal arguments',
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    expect(mock.review).toHaveBeenCalledWith('main', {
      manifestText: JSON.stringify(manifest),
      programBindings: { constructor: 'C:\\synthetic\\peer.exe' },
    });
  });

  it('installs only a reviewed receipt and keeps new connections disabled and skill adoption separate with canonical owner grants', async () => {
    const user = userEvent.setup();
    const mock = api();
    const changed = vi.fn();
    const uncertain = vi.fn();
    render(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
        onPermissionsUncertain={uncertain}
      />,
    );
    await loaded();
    await enter(user);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await user.click(
      await screen.findByRole('button', { name: 'Install reviewed bundle' }),
    );
    const card = within(
      await screen.findByRole('article', {
        name: 'Community bundle Violet bundle',
      }),
    );
    expect(mock.install).toHaveBeenCalledWith('main', {
      reviewId: 'review-1',
      reviewDigest: 'review-digest',
    });
    await waitFor(() =>
      expect(changed).toHaveBeenCalledWith(['read_file', 'unknown_custom']),
    );
    expect(uncertain).toHaveBeenCalledTimes(1);
    expect(
      card.getByText(/disconnected · disabled · connection-1/),
    ).toBeVisible();
    expect(card.getByText(/pending · cb-owned-procedure/)).toBeVisible();
    expect(
      card.getByRole('link', { name: 'Review skill drafts' }),
    ).toHaveAttribute('href', '#/skills');
    expect(
      card.getByRole('link', { name: 'Manage external tools' }),
    ).toHaveAttribute('href', '#/connectors');
    expect(mock.setEnabled).not.toHaveBeenCalled();
    expect(mock.update).not.toHaveBeenCalled();
  });

  it('renders original/canonical skills, argv and attribution as plain evidence without publisher links or markup', async () => {
    const user = userEvent.setup();
    const hostile = '<img src="https://hostile.test/track" onerror="run()">';
    const manifest = communityManifest({
      name: hostile,
      attribution: [
        {
          component: 'example',
          source: hostile,
          revision: 'v1',
          license: 'claim',
          notice: hostile,
        },
      ],
      components: [
        {
          key: 'peer',
          kind: 'mcpStdio',
          label: 'Peer',
          programRef: 'peer',
          args: [hostile],
        },
        {
          key: 'procedure',
          kind: 'skill',
          skillFile: `---\nname: Plain evidence\ndescription: Inspect plain evidence\n---\n\n${hostile}\n`,
        },
      ],
    });
    const mock = api();
    vi.mocked(mock.review).mockResolvedValue(communityReview(manifest));
    const view = render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    await enter(user, manifest);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    const preview = await screen.findByRole('region', {
      name: 'Reviewed bundle',
    });
    expect(
      within(preview).getByText('Canonical hash: canonical-hash'),
    ).toBeVisible();
    expect(within(preview).getByText('Source hash: source-hash')).toBeVisible();
    expect(view.container.querySelector('img, iframe, a')).toBeNull();
    expect(preview.textContent).toContain(hostile);
    expect(mock.install).not.toHaveBeenCalled();
  });

  it('invalidates exact review and local trust on manifest, binding or trust edits while preserving entered paths', async () => {
    const user = userEvent.setup();
    const mock = api();
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    await enter(user);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    const path = screen.getByRole('textbox', { name: 'Executable for peer' });
    fireEvent.change(screen.getByRole('textbox', { name: 'Bundle manifest' }), {
      target: {
        value: JSON.stringify(communityManifest({ version: '1.0.1' })),
      },
    });
    expect(
      screen.queryByRole('region', { name: 'Reviewed bundle' }),
    ).not.toBeInTheDocument();
    expect(path).toHaveValue('C:\\synthetic\\peer.exe');
    const trust = screen.getByRole('checkbox', {
      name: 'I trust these local programs and literal arguments',
    });
    expect(trust).not.toBeChecked();
    await user.click(trust);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    await user.click(trust);
    expect(
      screen.queryByRole('button', { name: 'Install reviewed bundle' }),
    ).not.toBeInTheDocument();
    await user.click(trust);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    fireEvent.change(path, { target: { value: 'C:\\synthetic\\changed.exe' } });
    expect(trust).not.toBeChecked();
    expect(
      screen.queryByRole('region', { name: 'Reviewed bundle' }),
    ).not.toBeInTheDocument();
    expect(mock.install).not.toHaveBeenCalled();
  });

  it('bounds decoded UTF-8 manifest and component counts before requesting a review', async () => {
    const mock = api();
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    const field = screen.getByRole('textbox', { name: 'Bundle manifest' });
    fireEvent.change(field, { target: { value: 'é'.repeat(65_537) } });
    expect(
      screen.getByText('The bundle manifest exceeds 128 KiB.'),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Review bundle' }),
    ).toBeDisabled();
    fireEvent.change(field, {
      target: {
        value: JSON.stringify(
          communityManifest({
            components: Array.from({ length: 5 }, (_, i) => ({
              key: `skill-${i}`,
              kind: 'skill' as const,
              skillFile: 'content',
            })),
          }),
        ),
      },
    });
    expect(screen.getByText(/Use at most four components/)).toBeVisible();
    expect(mock.review).not.toHaveBeenCalled();
  });

  it('pins an explicit update target/version and sends the reviewed receipt without overwriting dirty input', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    vi.mocked(mock.review).mockResolvedValue(
      communityReview(communityManifest(), {
        mode: 'update',
        expectedBundleVersion: 3,
      }),
    );
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    const card = await inspect(user);
    const edited = JSON.stringify(communityManifest({ version: '1.0.1' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Bundle manifest' }), {
      target: { value: edited },
    });
    await user.click(card.getByRole('button', { name: 'Review update' }));
    expect(
      screen.getByRole('textbox', { name: 'Bundle manifest' }),
    ).toHaveValue(edited);
    fireEvent.change(
      screen.getByRole('textbox', { name: 'Executable for peer' }),
      { target: { value: 'C:\\synthetic\\peer.exe' } },
    );
    await user.click(
      screen.getByRole('checkbox', {
        name: 'I trust these local programs and literal arguments',
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    expect(mock.review).toHaveBeenCalledWith('main', {
      manifestText: edited,
      programBindings: { peer: 'C:\\synthetic\\peer.exe' },
      bundleId: 'bundle-1',
      expectedBundleVersion: 3,
    });
    await user.click(
      await screen.findByRole('button', { name: 'Save reviewed update' }),
    );
    expect(mock.update).toHaveBeenCalledWith('main', 'bundle-1', {
      expectedVersion: 3,
      reviewId: 'review-1',
      reviewDigest: 'review-digest',
    });
    expect(mock.install).not.toHaveBeenCalled();
  });

  it.each([
    { version: 4, reviewedComponentStateDigest: 'component-state-4' },
    { version: 3, reviewedComponentStateDigest: 'component-state-changed' },
  ])(
    'invalidates an update receipt when inspection observes a changed target $version/$reviewedComponentStateDigest',
    async (changed) => {
      const user = userEvent.setup();
      const mock = api([communityBundle()]);
      vi.mocked(mock.review).mockResolvedValue(
        communityReview(communityManifest(), {
          mode: 'update',
          expectedBundleVersion: 3,
        }),
      );
      render(<CommunityBundlesPanel agentId="main" api={mock} />);
      await loaded();
      const card = await inspect(user);
      await user.click(card.getByRole('button', { name: 'Review update' }));
      await enter(user);
      await user.click(screen.getByRole('button', { name: 'Review bundle' }));
      await screen.findByRole('region', { name: 'Reviewed bundle' });
      expect(
        screen.getByRole('button', { name: 'Save reviewed update' }),
      ).toBeEnabled();

      vi.mocked(mock.get).mockResolvedValue(communityBundle(changed));
      await inspect(user);
      expect(
        screen.queryByRole('region', { name: 'Reviewed bundle' }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: 'Save reviewed update' }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Review bundle' }),
      ).toBeDisabled();
      expect(mock.update).not.toHaveBeenCalled();
      expect(mock.install).not.toHaveBeenCalled();
    },
  );

  it('settles a detail read before allowing a review to invalidate its read generation', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    const detail =
      deferred<Awaited<ReturnType<CommunityBundlesOwnerApi['get']>>>();
    vi.mocked(mock.get).mockReturnValue(detail.promise);
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    await enter(user);
    await user.click(screen.getByRole('button', { name: 'Inspect bundle' }));
    expect(
      screen.getByRole('button', { name: 'Review bundle' }),
    ).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Refresh bundle status' }),
    ).toBeDisabled();
    await act(async () => {
      detail.resolve(communityBundle());
      await detail.promise;
    });
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Review bundle' }),
      ).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    expect(
      screen.getByRole('button', { name: 'Inspect bundle' }),
    ).toBeEnabled();
    expect(mock.review).toHaveBeenCalledTimes(1);
  });

  it('uses acknowledged lifecycle versions and exact component-state digests without grants or starts', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    vi.mocked(mock.setEnabled)
      .mockResolvedValueOnce(
        communityBundle({
          version: 4,
          state: 'disabled',
          enabled: false,
          reviewedComponentStateDigest: 'state-4',
        }),
      )
      .mockResolvedValueOnce(
        communityBundle({
          version: 5,
          reviewedComponentStateDigest: 'state-5',
        }),
      );
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    const card = await inspect(user);
    await user.click(card.getByRole('button', { name: 'Disable bundle' }));
    await user.click(
      await card.findByRole('button', { name: 'Re-enable bundle' }),
    );
    expect(mock.setEnabled).toHaveBeenNthCalledWith(1, 'main', 'bundle-1', {
      expectedVersion: 3,
      reviewedComponentStateDigest: 'component-state-3',
      enabled: false,
    });
    expect(mock.setEnabled).toHaveBeenNthCalledWith(2, 'main', 'bundle-1', {
      expectedVersion: 4,
      reviewedComponentStateDigest: 'state-4',
      enabled: true,
    });
    expect(mock.review).not.toHaveBeenCalled();
    expect(mock.install).not.toHaveBeenCalled();
  });

  it('invalidates removal confirmation when component digest changes even without a bundle version change', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    const card = await inspect(user);
    await user.click(card.getByRole('button', { name: 'Remove bundle' }));
    const changed = communityBundle({
      reviewedComponentStateDigest: 'changed-state',
    });
    vi.mocked(mock.list).mockResolvedValue([changed]);
    vi.mocked(mock.get).mockResolvedValue(changed);
    await user.click(
      screen.getByRole('button', { name: 'Refresh bundle status' }),
    );
    await loaded();
    expect(
      card.getByRole('button', { name: 'Confirm bundle removal' }),
    ).toBeDisabled();
    expect(card.getByText(/The component state changed/)).toBeVisible();
    await user.click(card.getByRole('button', { name: 'Inspect bundle' }));
    await waitFor(() =>
      expect(card.getByRole('button', { name: 'Remove bundle' })).toBeEnabled(),
    );
    await user.click(card.getByRole('button', { name: 'Remove bundle' }));
    await user.click(
      card.getByRole('button', { name: 'Confirm bundle removal' }),
    );
    expect(mock.remove).toHaveBeenCalledWith('main', 'bundle-1', {
      expectedVersion: 3,
      reviewedComponentStateDigest: 'changed-state',
    });
  });

  it('retains unknown removal state and entered manifest until explicit reads reconcile, without retry or raw error disclosure', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    const uncertain = vi.fn();
    vi.mocked(mock.remove).mockRejectedValue({
      status: 503,
      body: { error: 'private argv and root' },
    });
    render(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        onPermissionsUncertain={uncertain}
      />,
    );
    await loaded();
    const card = await inspect(user);
    const text = JSON.stringify(communityManifest());
    fireEvent.change(screen.getByRole('textbox', { name: 'Bundle manifest' }), {
      target: { value: text },
    });
    await user.click(card.getByRole('button', { name: 'Remove bundle' }));
    await user.click(
      card.getByRole('button', { name: 'Confirm bundle removal' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not be confirmed',
    );
    expect(screen.queryByText('private argv and root')).not.toBeInTheDocument();
    expect(
      screen.getByRole('article', { name: 'Community bundle Violet bundle' }),
    ).toBeVisible();
    expect(card.getByRole('button', { name: 'Disable bundle' })).toBeDisabled();
    expect(
      screen.getByRole('textbox', { name: 'Bundle manifest' }),
    ).toHaveValue(text);
    await user.click(
      screen.getByRole('button', { name: 'Refresh bundle status' }),
    );
    await loaded();
    expect(mock.toolNames).toHaveBeenCalledTimes(1);
    expect(mock.remove).toHaveBeenCalledTimes(1);
    expect(uncertain).toHaveBeenCalledTimes(1);
  });

  it('keeps acknowledged deletion after a parent canonical-read failure and refreshes only reads', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    const changed = vi
      .fn()
      .mockRejectedValueOnce(new Error('Read failed'))
      .mockResolvedValue(undefined);
    render(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    await loaded();
    const card = await inspect(user);
    await user.click(card.getByRole('button', { name: 'Remove bundle' }));
    await user.click(
      card.getByRole('button', { name: 'Confirm bundle removal' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not be confirmed',
    );
    expect(
      screen.queryByRole('article', { name: 'Community bundle Violet bundle' }),
    ).not.toBeInTheDocument();
    vi.mocked(mock.list).mockResolvedValue([]);
    vi.mocked(mock.toolNames).mockResolvedValue(['read_file']);
    await user.click(
      screen.getByRole('button', { name: 'Refresh bundle status' }),
    );
    await loaded();
    expect(changed).toHaveBeenNthCalledWith(1, ['read_file']);
    expect(changed).toHaveBeenNthCalledWith(2, ['read_file']);
    expect(mock.remove).toHaveBeenCalledTimes(1);
    expect(mock.install).not.toHaveBeenCalled();
  });

  it('fences old owner reads and clears manifest, bindings, review and trust on owner change', async () => {
    const user = userEvent.setup();
    const mock = api();
    const old = deferred<BundleSummary[]>();
    vi.mocked(mock.list).mockReturnValueOnce(old.promise).mockResolvedValue([]);
    const view = render(<CommunityBundlesPanel agentId="old" api={mock} />);
    view.rerender(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    await act(async () => {
      old.resolve([communityBundle({ agentId: 'old', name: 'Old bundle' })]);
      await old.promise;
    });
    expect(screen.queryByText('Old bundle')).not.toBeInTheDocument();
    await enter(user);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    await screen.findByRole('region', { name: 'Reviewed bundle' });
    view.rerender(<CommunityBundlesPanel agentId="beta" api={mock} />);
    await loaded();
    expect(
      screen.getByRole('textbox', { name: 'Bundle manifest' }),
    ).toHaveValue('');
    expect(
      screen.queryByRole('region', { name: 'Reviewed bundle' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(mock.install).not.toHaveBeenCalled();
  });

  it('retains the parent uncertainty fence for an unmounted removal and never publishes its late acknowledgement', async () => {
    const user = userEvent.setup();
    const mock = api([communityBundle()]);
    const removal =
      deferred<Awaited<ReturnType<CommunityBundlesOwnerApi['remove']>>>();
    vi.mocked(mock.remove).mockReturnValue(removal.promise);
    const changed = vi.fn();
    const uncertain = vi.fn();
    const busy = vi.fn();
    const view = render(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
        onPermissionsUncertain={uncertain}
        onBusyChanged={busy}
      />,
    );
    await loaded();
    const card = await inspect(user);
    await user.click(card.getByRole('button', { name: 'Remove bundle' }));
    await user.click(
      card.getByRole('button', { name: 'Confirm bundle removal' }),
    );
    expect(uncertain).toHaveBeenCalledTimes(1);
    view.unmount();
    await act(async () => {
      removal.resolve({
        bundleId: 'bundle-1',
        removed: true,
        trashPaths: [],
        toolNames: ['read_file'],
      });
      await removal.promise;
    });
    expect(changed).not.toHaveBeenCalled();
    expect(busy).toHaveBeenLastCalledWith(false);
  });

  it('refuses an expired receipt and keeps restricted writes inactive while explicit refresh stays read-only', async () => {
    const user = userEvent.setup();
    const mock = api();
    vi.mocked(mock.review).mockResolvedValue(
      communityReview(communityManifest(), { expiresAtMs: Date.now() - 1 }),
    );
    const view = render(<CommunityBundlesPanel agentId="main" api={mock} />);
    await loaded();
    await enter(user);
    await user.click(screen.getByRole('button', { name: 'Review bundle' }));
    expect(
      await screen.findByText(
        'This review expired. Review the exact inputs again.',
      ),
    ).toBeVisible();
    expect(mock.install).not.toHaveBeenCalled();
    view.rerender(
      <CommunityBundlesPanel
        agentId="main"
        api={mock}
        disabled
        permissionsUncertain
      />,
    );
    expect(
      screen.getByRole('textbox', { name: 'Bundle manifest' }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole('button', { name: 'Refresh bundle status' }),
    );
    await loaded();
    expect(mock.toolNames).toHaveBeenCalledTimes(1);
    expect(mock.install).not.toHaveBeenCalled();
  });
});
