import { useState } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type {
  BrowserPolicy,
  BrowserScope,
  BrowserProfile,
  BrowserSelection,
  UploadedAttachment,
} from '@animaos/sdk';
import {
  BrowserTaskControls,
  type BrowserControlsApi,
} from './BrowserTaskControls';

const policy: BrowserPolicy = {
  id: 'quotes',
  origin: 'http://127.0.0.1:43220',
  routes: [{ id: 'home', method: 'GET', path: '/' }],
  policyDigest: 'a'.repeat(64),
  installationId: 'edge',
  installationDigest: 'b'.repeat(64),
  workerAvailable: true,
  authenticated: false,
  interactive: false,
  allowedOrigins: ['http://127.0.0.1:43220'],
  version: 1,
  limits: {
    actions: 40,
    requests: 80,
    scopeMs: 120000,
    frameBytes: 65536,
    wireBytes: 1048576,
    observationNodes: 128,
    observationTextBytes: 8192,
    inputBytes: 16384,
    outputBytes: 16384,
    screenshotBytes: 262144,
  },
};
const scope: BrowserScope = {
  id: 'scope-a',
  agentId: 'a',
  runId: 'run-a',
  sessionId: 'chat:a',
  version: 2,
  enabled: true,
  state: 'active',
  policyId: 'quotes',
  origin: policy.origin,
  policyDigest: policy.policyDigest,
  installationId: policy.installationId,
  installationDigest: policy.installationDigest,
  selectedInputIds: [],
  expiresAtMs: 1900000000000,
  actionsUsed: 2,
  requestsUsed: 3,
  wireBytesUsed: 900,
  physicalLeaseHeld: true,
  ownerControl: false,
  profile: null,
  document: null,
  artifacts: [],
  toolNames: ['browser_navigate'],
  error: null,
};
const publicPolicy: BrowserPolicy = {
  ...policy,
  id: 'public-read',
  origin: 'https://m63-browser.anima.tools',
};
const draftAttachment: UploadedAttachment = {
  id: 'draft-file',
  name: 'draft.txt',
  mime: 'text/plain',
  size: 12,
  kind: 'text',
  path: 'draft-file',
  createdAtMs: 1,
};
const interactivePolicy: BrowserPolicy = {
  ...publicPolicy,
  id: 'website',
  interactive: true,
  authenticated: true,
  allowedOrigins: ['https://m63-browser.anima.tools'],
  routes: [{ id: 'start', method: 'GET', path: '/' }],
};
const profile: BrowserProfile = {
  id: 'account-one',
  agentId: 'a',
  policyId: interactivePolicy.id,
  origin: interactivePolicy.origin,
  revision: 2,
  digest: 'd'.repeat(64),
  ready: false,
  revoked: false,
  loginActive: false,
};

function api(): BrowserControlsApi {
  return {
    policies: vi.fn().mockResolvedValue([policy]),
    get: vi.fn().mockResolvedValue(scope),
    stop: vi.fn().mockResolvedValue({
      ...scope,
      version: 3,
      state: 'closed',
      enabled: false,
      physicalLeaseHeld: false,
    }),
    setup: vi.fn().mockResolvedValue({
      available: true,
      reason: null,
      setupCommand: 'bun run browser:setup',
    }),
    profiles: vi.fn().mockResolvedValue([]),
    createDestination: vi.fn().mockResolvedValue(interactivePolicy),
    revokeDestination: vi.fn().mockResolvedValue({ revoked: true }),
    createProfile: vi.fn().mockResolvedValue(profile),
    startProfileLogin: vi
      .fn()
      .mockResolvedValue({ ...profile, revision: 3, loginActive: true }),
    finishProfileLogin: vi
      .fn()
      .mockResolvedValue({ ...profile, revision: 3, ready: true }),
    revokeProfile: vi
      .fn()
      .mockResolvedValue({ ...profile, revision: 4, revoked: true }),
    takeover: vi.fn().mockResolvedValue({
      ...scope,
      version: 3,
      ownerControl: true,
      state: 'ownerControl',
    }),
    resume: vi.fn().mockResolvedValue({
      ...scope,
      version: 4,
      ownerControl: false,
      state: 'active',
    }),
  };
}

async function openControls() {
  const details = screen.getByText('Browser task').closest('details')!;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  await screen.findByLabelText('Browser policy for next message');
}

async function openWebsites() {
  const details = screen.getByText('Websites and sign-in').closest('details')!;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  await screen.findByRole('button', { name: 'Refresh websites and accounts' });
}

describe('browser task controls', () => {
  it('refreshes website policy and active status after an uncertain destination create', async () => {
    const mock = api();
    let savedPolicies = [policy];
    vi.mocked(mock.policies).mockImplementation(async () => savedPolicies);
    vi.mocked(mock.createDestination).mockImplementationOnce(async () => {
      savedPolicies = [policy, interactivePolicy];
      throw new Error('Response lost after save');
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId="run-a"
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    await screen.findByText(
      'Browser is ready. Sign-in uses a separate browser account.',
    );
    fireEvent.change(screen.getByLabelText('Starting website URL'), {
      target: { value: 'https://example.com/' },
    });
    fireEvent.click(
      screen.getByLabelText('Allow this browser task to use these websites'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add website' }));
    await screen.findByRole('alert');
    expect(
      screen.queryByRole('option', {
        name: `${interactivePolicy.origin} (sign-in)`,
      }),
    ).not.toBeInTheDocument();
    const before = vi.mocked(mock.get).mock.calls.length;
    fireEvent.click(
      screen.getByRole('button', { name: 'Refresh websites and accounts' }),
    );
    await screen.findByRole('option', {
      name: `${interactivePolicy.origin} (sign-in)`,
    });
    expect(vi.mocked(mock.get).mock.calls.length).toBeGreaterThan(before);
    expect(mock.createDestination).toHaveBeenCalledTimes(1);
  });

  it('keeps invalid website permissions out of the owner API', async () => {
    const mock = api();
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    await screen.findByText(
      'Browser is ready. Sign-in uses a separate browser account.',
    );
    fireEvent.change(screen.getByLabelText('Starting website URL'), {
      target: { value: 'https://example.com/' },
    });
    fireEvent.change(screen.getByLabelText('Additional allowed origins'), {
      target: { value: 'https://example.com/private' },
    });
    fireEvent.click(
      screen.getByLabelText('Allow this browser task to use these websites'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add website' }));
    await screen.findByRole('alert');
    expect(mock.createDestination).not.toHaveBeenCalled();
  });

  it('ignores a destination create response after switching owner', async () => {
    const mock = api();
    let resolve!: (value: BrowserPolicy) => void;
    vi.mocked(mock.createDestination).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const onSelection = vi.fn();
    const props = {
      runId: null,
      disabled: false,
      attachments: [],
      selection: null,
      onSelection,
      api: mock,
    };
    const view = render(<BrowserTaskControls {...props} agentId="a" />);
    await openControls();
    await openWebsites();
    await screen.findByText(
      'Browser is ready. Sign-in uses a separate browser account.',
    );
    fireEvent.change(screen.getByLabelText('Starting website URL'), {
      target: { value: 'https://example.com/' },
    });
    fireEvent.click(
      screen.getByLabelText('Allow this browser task to use these websites'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add website' }));
    await waitFor(() =>
      expect(mock.createDestination).toHaveBeenCalledTimes(1),
    );
    view.rerender(<BrowserTaskControls {...props} agentId="b" />);
    resolve({ ...interactivePolicy, id: 'old-owner-website' });
    await waitFor(() =>
      expect(mock.policies).toHaveBeenCalledWith('b', expect.anything()),
    );
    expect(onSelection).not.toHaveBeenCalled();
    expect(
      screen.queryByRole('option', { name: /old-owner-website/ }),
    ).not.toBeInTheDocument();
  });

  it('offers bounded browser documents using their trusted file kinds', async () => {
    const mock = api();
    vi.mocked(mock.get).mockResolvedValue({
      ...scope,
      artifacts: [
        { id: 'file-pdf', kind: 'pdf', bytes: 32, sha256: 'c'.repeat(64) },
        { id: 'file-json', kind: 'json', bytes: 12, sha256: 'd'.repeat(64) },
        { id: 'file-text', kind: 'text', bytes: 9, sha256: 'e'.repeat(64) },
      ],
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId="run-a"
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    expect(
      screen.getByRole('button', { name: 'Download browser-output.pdf' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Download browser-output.json' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Download browser-output.txt' }),
    ).toBeInTheDocument();
  });

  it('requires account refresh after an uncertain sign-in and then uses the refreshed revision', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    vi.mocked(mock.profiles)
      .mockResolvedValueOnce([profile])
      .mockResolvedValueOnce([{ ...profile, revision: 4, loginActive: true }]);
    vi.mocked(mock.startProfileLogin).mockRejectedValueOnce({
      status: 409,
      message: '/secret/path',
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={{ policyId: interactivePolicy.id, selectedInputIds: [] }}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }));
    await screen.findByRole('alert');
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeDisabled();
    expect(mock.startProfileLogin).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('/secret/path')).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('button', { name: 'Refresh websites and accounts' }),
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Finish sign-in' }),
    );
    await waitFor(() =>
      expect(mock.finishProfileLogin).toHaveBeenCalledExactlyOnceWith(
        'a',
        profile.id,
        { expectedRevision: 4 },
      ),
    );
  });
  it('reports missing runtime without starting setup or a browser', async () => {
    const mock = api();
    vi.mocked(mock.setup).mockResolvedValue({
      available: false,
      reason: 'runtime_missing',
      setupCommand: 'bun run browser:setup',
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    await screen.findByText('Browser setup is needed on this computer.');
    expect(screen.getByText('bun run browser:setup')).toBeInTheDocument();
    expect(
      screen.getByText(/Restart AnimaOS, then refresh/),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add website' })).toBeDisabled();
    expect(mock.createDestination).not.toHaveBeenCalled();
    expect(mock.startProfileLogin).not.toHaveBeenCalled();
  });

  it('creates and selects only an explicitly reviewed website and revokes it with its current version', async () => {
    const mock = api();
    const onSelection = vi.fn();
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={onSelection}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    fireEvent.change(screen.getByLabelText('Starting website URL'), {
      target: { value: 'https://example.com/form?q=one' },
    });
    fireEvent.change(screen.getByLabelText('Additional allowed origins'), {
      target: { value: 'https://static.example.com\nhttps://auth.example.com' },
    });
    fireEvent.click(screen.getByLabelText('This website needs sign-in'));
    fireEvent.click(
      screen.getByLabelText('Allow this browser task to use these websites'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add website' }));
    await waitFor(() =>
      expect(mock.createDestination).toHaveBeenCalledExactlyOnceWith('a', {
        startUrl: 'https://example.com/form?q=one',
        allowedOrigins: [
          'https://example.com',
          'https://static.example.com',
          'https://auth.example.com',
        ],
        authenticated: true,
      }),
    );
    await waitFor(() =>
      expect(onSelection).toHaveBeenCalledWith({
        policyId: interactivePolicy.id,
        selectedInputIds: [],
      }),
    );
    expect(
      await screen.findByRole('option', {
        name: `${interactivePolicy.origin} (sign-in)`,
      }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Remove website/ }));
    await waitFor(() =>
      expect(mock.revokeDestination).toHaveBeenCalledExactlyOnceWith(
        'a',
        interactivePolicy.id,
        { expectedVersion: 1 },
      ),
    );
  });

  it('keeps a later website choice when an earlier destination create finishes', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([policy, publicPolicy]);
    let resolve!: (value: BrowserPolicy) => void;
    vi.mocked(mock.createDestination).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const onSelection = vi.fn();
    const props = {
      agentId: 'a',
      runId: null,
      disabled: false,
      attachments: [],
      onSelection,
      api: mock,
    };
    const view = render(
      <BrowserTaskControls
        {...props}
        selection={{ policyId: policy.id, selectedInputIds: [] }}
      />,
    );
    await openControls();
    await openWebsites();
    await screen.findByText(
      'Browser is ready. Sign-in uses a separate browser account.',
    );
    fireEvent.change(screen.getByLabelText('Starting website URL'), {
      target: { value: 'https://example.com/' },
    });
    fireEvent.click(
      screen.getByLabelText('Allow this browser task to use these websites'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add website' }));
    await waitFor(() =>
      expect(mock.createDestination).toHaveBeenCalledTimes(1),
    );
    view.rerender(
      <BrowserTaskControls
        {...props}
        selection={{ policyId: publicPolicy.id, selectedInputIds: [] }}
      />,
    );
    resolve(interactivePolicy);
    await screen.findByRole('option', {
      name: `${interactivePolicy.origin} (sign-in)`,
    });
    expect(onSelection).not.toHaveBeenCalled();
  });

  it('keeps a later account choice when another account finishes sign-in', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    vi.mocked(mock.profiles).mockResolvedValue([
      { ...profile, loginActive: true },
      { ...profile, id: 'account-other', ready: true },
    ]);
    let resolve!: (value: BrowserProfile) => void;
    vi.mocked(mock.finishProfileLogin).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const onSelection = vi.fn();
    const props = {
      agentId: 'a',
      runId: null,
      disabled: false,
      attachments: [],
      onSelection,
      api: mock,
    };
    const view = render(
      <BrowserTaskControls
        {...props}
        selection={{ policyId: interactivePolicy.id, selectedInputIds: [] }}
      />,
    );
    await openControls();
    await openWebsites();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Finish sign-in' }),
    );
    await waitFor(() =>
      expect(mock.finishProfileLogin).toHaveBeenCalledTimes(1),
    );
    view.rerender(
      <BrowserTaskControls
        {...props}
        selection={{
          policyId: interactivePolicy.id,
          selectedInputIds: [],
          profileId: 'account-other',
        }}
      />,
    );
    resolve({ ...profile, ready: true });
    await waitFor(() =>
      expect(
        screen.queryByRole('button', { name: 'Finish sign-in' }),
      ).not.toBeInTheDocument(),
    );
    expect(onSelection).not.toHaveBeenCalled();
  });

  it('creates an account and uses exact sign-in revisions before selecting a ready profile', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    const onSelection = vi.fn();
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={{ policyId: interactivePolicy.id, selectedInputIds: [] }}
        onSelection={onSelection}
        api={mock}
      />,
    );
    await openControls();
    await openWebsites();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Create dedicated account' }),
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }));
    await waitFor(() =>
      expect(mock.startProfileLogin).toHaveBeenCalledExactlyOnceWith(
        'a',
        profile.id,
        { expectedRevision: 2, routeId: 'start' },
      ),
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Finish sign-in' }),
    );
    await waitFor(() =>
      expect(mock.finishProfileLogin).toHaveBeenCalledExactlyOnceWith(
        'a',
        profile.id,
        { expectedRevision: 3 },
      ),
    );
    await waitFor(() =>
      expect(onSelection).toHaveBeenCalledWith({
        policyId: interactivePolicy.id,
        selectedInputIds: [],
        profileId: profile.id,
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Revoke account' }));
    await waitFor(() =>
      expect(mock.revokeProfile).toHaveBeenCalledExactlyOnceWith(
        'a',
        profile.id,
        { expectedRevision: 3 },
      ),
    );
  });

  it('takes over and resumes the same scope using acknowledged versions', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    vi.mocked(mock.get).mockResolvedValue({
      ...scope,
      policyId: interactivePolicy.id,
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId="run-a"
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    await openControls();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Take control' }),
    );
    await screen.findByText('You control the browser. The agent is paused.');
    expect(mock.takeover).toHaveBeenCalledExactlyOnceWith('a', 'run-a', {
      expectedVersion: 2,
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Return control to agent' }),
    );
    await waitFor(() =>
      expect(mock.resume).toHaveBeenCalledExactlyOnceWith('a', 'run-a', {
        expectedVersion: 3,
      }),
    );
  });

  it('requires refresh after a takeover conflict and ignores late control responses after changing agent', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    vi.mocked(mock.get).mockResolvedValue({
      ...scope,
      policyId: interactivePolicy.id,
    });
    vi.mocked(mock.takeover).mockRejectedValueOnce({ status: 409 });
    const props = {
      runId: 'run-a',
      disabled: false,
      attachments: [],
      selection: null,
      onSelection: vi.fn(),
      api: mock,
    };
    const view = render(<BrowserTaskControls {...props} agentId="a" />);
    await openControls();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Take control' }),
    );
    await screen.findByRole('alert');
    expect(screen.getByRole('button', { name: 'Take control' })).toBeDisabled();
    expect(mock.takeover).toHaveBeenCalledTimes(1);
    fireEvent.click(
      screen.getByRole('button', { name: 'Refresh browser status' }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Take control' }),
      ).toBeEnabled(),
    );
    let resolve!: (value: BrowserScope) => void;
    vi.mocked(mock.takeover).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Take control' }));
    view.rerender(<BrowserTaskControls {...props} agentId="b" />);
    resolve({ ...scope, agentId: 'a', version: 4, ownerControl: true });
    await waitFor(() =>
      expect(mock.get).toHaveBeenCalledWith('b', 'run-a', expect.anything()),
    );
    expect(
      screen.queryByText('You control the browser. The agent is paused.'),
    ).not.toBeInTheDocument();
  });
  it('keeps controls usable after returning to an owner while an old control request settles', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([interactivePolicy]);
    vi.mocked(mock.get).mockResolvedValue({
      ...scope,
      policyId: interactivePolicy.id,
    });
    let resolve!: (value: BrowserScope) => void;
    vi.mocked(mock.takeover).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const props = {
      runId: 'run-a',
      disabled: false,
      attachments: [],
      selection: null,
      onSelection: vi.fn(),
      api: mock,
    };
    const view = render(<BrowserTaskControls {...props} agentId="a" />);
    await openControls();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Take control' }),
    );
    view.rerender(<BrowserTaskControls {...props} agentId="b" />);
    await waitFor(() =>
      expect(mock.get).toHaveBeenCalledWith('b', 'run-a', expect.anything()),
    );
    view.rerender(<BrowserTaskControls {...props} agentId="a" />);
    resolve({
      ...scope,
      policyId: interactivePolicy.id,
      ownerControl: true,
      version: 3,
    });
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Refresh browser status' }),
      ).toBeEnabled(),
    );
    expect(
      screen.queryByText('You control the browser. The agent is paused.'),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole('button', { name: 'Take control' }),
    ).toBeEnabled();
  });

  it('does not offer local file sharing for a public HTTPS policy', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([publicPolicy]);
    render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[draftAttachment]}
        selection={{
          policyId: publicPolicy.id,
          selectedInputIds: ['draft-file'],
        }}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await screen.findByRole('option', { name: /public-read/ });
    expect(
      screen.getByText('Uploaded files aren’t shared with public pages.'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.queryByText(/Share draft\.txt/)).not.toBeInTheDocument();
  });

  it('clears browser file selection when switching to public while retaining the chat attachment', async () => {
    const mock = api();
    vi.mocked(mock.policies).mockResolvedValue([policy, publicPolicy]);
    const onSelection = vi.fn();
    const attachments = [draftAttachment];
    function Harness() {
      const [selection, setSelection] = useState<BrowserSelection | null>({
        policyId: policy.id,
        selectedInputIds: [draftAttachment.id],
      });
      return (
        <>
          <div aria-label="Chat attachments">Draft attachment: draft.txt</div>
          <BrowserTaskControls
            agentId="a"
            runId={null}
            disabled={false}
            attachments={attachments}
            selection={selection}
            onSelection={(next) => {
              onSelection(next);
              setSelection(next);
            }}
            api={mock}
          />
        </>
      );
    }
    render(<Harness />);
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await screen.findByRole('option', { name: /public-read/ });
    expect(
      screen.getByRole('checkbox', { name: /Share draft\.txt/ }),
    ).toBeChecked();

    fireEvent.change(screen.getByLabelText('Browser policy for next message'), {
      target: { value: publicPolicy.id },
    });
    expect(onSelection).toHaveBeenLastCalledWith({
      policyId: publicPolicy.id,
      selectedInputIds: [],
    });
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.getByText('Draft attachment: draft.txt')).toBeInTheDocument();
    expect(attachments).toEqual([draftAttachment]);

    fireEvent.change(screen.getByLabelText('Browser policy for next message'), {
      target: { value: policy.id },
    });
    expect(onSelection).toHaveBeenLastCalledWith({
      policyId: policy.id,
      selectedInputIds: [],
    });
    const share = screen.getByRole('checkbox', { name: /Share draft\.txt/ });
    expect(share).not.toBeChecked();
    fireEvent.click(share);
    expect(onSelection).toHaveBeenLastCalledWith({
      policyId: policy.id,
      selectedInputIds: [draftAttachment.id],
    });
    expect(screen.getByText('Draft attachment: draft.txt')).toBeInTheDocument();
  });

  it('loads metadata only on opening and selects only installed policies and ready inputs', async () => {
    const mock = api();
    const onSelection = vi.fn();
    const view = render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={onSelection}
        api={mock}
      />,
    );
    expect(mock.policies).not.toHaveBeenCalled();
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await screen.findByRole('option', { name: /quotes/ });
    fireEvent.change(screen.getByLabelText('Browser policy for next message'), {
      target: { value: 'quotes' },
    });
    expect(onSelection).toHaveBeenLastCalledWith({
      policyId: 'quotes',
      selectedInputIds: [],
    });
    expect(mock.get).not.toHaveBeenCalled();
    expect(mock.stop).not.toHaveBeenCalled();
    view.unmount();
  });

  it('uses current server CAS for Stop and retains a physical cleanup warning on conflict', async () => {
    const mock = api();
    vi.mocked(mock.stop).mockRejectedValue({
      status: 409,
      message: '/private/path',
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId="run-a"
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    const stop = await screen.findByRole('button', { name: 'Stop browser' });
    await waitFor(() => expect(stop).toBeEnabled());
    fireEvent.click(stop);
    await screen.findByRole('alert');
    expect(mock.stop).toHaveBeenCalledExactlyOnceWith('a', 'run-a', {
      expectedVersion: 2,
    });
    expect(
      screen.getByText(/Physical browser cleanup is pending/),
    ).toBeInTheDocument();
    expect(screen.queryByText('/private/path')).not.toBeInTheDocument();
  });

  it('ignores an old agent response after the owner changes and aborts its read', async () => {
    const mock = api();
    let resolve!: (value: BrowserPolicy[]) => void;
    vi.mocked(mock.policies).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const props = {
      runId: null,
      disabled: false,
      attachments: [],
      selection: null,
      onSelection: vi.fn(),
      api: mock,
    };
    const view = render(<BrowserTaskControls {...props} agentId="a" />);
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await waitFor(() => expect(mock.policies).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(mock.policies).mock.calls[0][1]?.signal;
    view.rerender(<BrowserTaskControls {...props} agentId="b" />);
    resolve([{ ...policy, id: 'foreign-policy' }]);
    await waitFor(() => expect(signal?.aborted).toBe(true));
    expect(
      screen.queryByRole('option', { name: /foreign-policy/ }),
    ).not.toBeInTheDocument();
    expect(mock.stop).not.toHaveBeenCalled();
  });
});
