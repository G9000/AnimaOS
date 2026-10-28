import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { ConnectorsView } from './ConnectorsView';
import { communityBundlesOwnerApi, mcpOwnerApi } from '../lib/daemon-api';
import { communityBundle } from '../test/community-bundles';
const api = vi.hoisted(() => ({
  calendarStatus: vi.fn(),
  driveStatus: vi.fn(),
  connectDrive: vi.fn(),
  disconnectDrive: vi.fn(),
  mailStatus: vi.fn(),
  connectMail: vi.fn(),
  connectGenericEmail: vi.fn(),
  disconnectMail: vi.fn(),
  mailMessages: vi.fn(),
  mailDrafts: vi.fn(),
  createMailDraft: vi.fn(),
  approveMailDraft: vi.fn(),
  rejectMailDraft: vi.fn(),
  calendarWrites: vi.fn(),
  approveCalendarWrite: vi.fn(),
  rejectCalendarWrite: vi.fn(),
  oauthAppStatus: vi.fn(),
  configureOauthApp: vi.fn(),
  removeOauthApp: vi.fn(),
}));
const microsoftFiles = vi.hoisted(() => ({ status: vi.fn(), connect: vi.fn(), files: vi.fn(), select: vi.fn(), disconnect: vi.fn() }));
vi.mock('@animaos/sdk', () => ({
  createDaemonClient: () => ({ connectors: api, microsoftFiles }),
}));
const connector = {
  id: 'gmail-1',
  agentId: 'main',
  type: 'gmail',
  accountLabel: 'owner@example.com',
  status: 'active',
};
const draft = {
  id: 'draft-1',
  connectorId: 'gmail-1',
  to: ['friend@example.com'],
  subject: 'Saved subject',
  body: 'Saved message',
  state: 'pending',
  error: null,
};

function emailCard() {
  return within(screen.getByRole('region', { name: 'Email' }));
}

function fillEmailSetup() {
  const card = emailCard();
  for (const [label, value] of [
    ['Email address', 'agent@example.com'],
    ['IMAP host', 'imap.example.com'],
    ['IMAP username', 'receive-user'],
    ['IMAP password', 'receiving-secret'],
    ['SMTP host', 'smtp.example.com'],
    ['SMTP username', 'send-user'],
    ['SMTP password', 'sending-secret'],
  ])
    fireEvent.change(card.getByLabelText(label), { target: { value } });
  return card;
}

it('connects generic email without OAuth, clearing secrets before setup settles', async () => {
  let finish!: (value: unknown) => void;
  api.connectGenericEmail.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  render(<ConnectorsView agentId="main" telegram={null} />);
  const card = fillEmailSetup();
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Connect Email' })).toBeEnabled(),
  );
  expect(card.getByText(/No OAuth app registration/)).toBeVisible();
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  expect(api.connectGenericEmail).toHaveBeenCalledWith('main', {
    address: 'agent@example.com',
    imap: {
      host: 'imap.example.com',
      port: 993,
      security: 'tls',
      username: 'receive-user',
    },
    smtp: {
      host: 'smtp.example.com',
      port: 465,
      security: 'tls',
      username: 'send-user',
    },
    imapPassword: 'receiving-secret',
    smtpPassword: 'sending-secret',
    autoReply: { enabled: false, allowedSenders: [], authservId: '' },
  });
  expect(card.getByLabelText('IMAP password')).toHaveValue('');
  expect(card.getByLabelText('SMTP password')).toHaveValue('');
  expect(api.connectMail).not.toHaveBeenCalled();
  await act(async () => finish({ connector: { id: 'email-1' } }));
  expect(card.getByLabelText('IMAP password')).toHaveValue('');
});

it('requires explicit allowed senders and the receiving server ID for automatic replies', async () => {
  api.connectGenericEmail.mockResolvedValue({ connector: { id: 'email-1' } });
  render(<ConnectorsView agentId="main" telegram={null} />);
  const card = fillEmailSetup();
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Connect Email' })).toBeEnabled(),
  );
  const automatic = card.getByRole('checkbox', {
    name: 'Automatically reply to allowed senders',
  });
  expect(automatic).not.toBeChecked();
  fireEvent.click(automatic);
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  expect(card.getByRole('alert')).toHaveTextContent(
    'Add at least one allowed sender',
  );
  expect(api.connectGenericEmail).not.toHaveBeenCalled();
  fireEvent.change(card.getByLabelText('Allowed senders'), {
    target: { value: 'friend@example.com, OTHER@example.com' },
  });
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  expect(card.getByRole('alert')).toHaveTextContent('Receiving mail server ID');
  fireEvent.change(card.getByLabelText('Receiving mail server ID'), {
    target: { value: 'mx.example.com' },
  });
  fireEvent.change(card.getByLabelText('SMTP security'), {
    target: { value: 'starttls' },
  });
  fireEvent.change(card.getByLabelText('SMTP port'), {
    target: { value: '587' },
  });
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  await waitFor(() =>
    expect(api.connectGenericEmail).toHaveBeenCalledWith(
      'main',
      expect.objectContaining({
        smtp: {
          host: 'smtp.example.com',
          port: 587,
          security: 'starttls',
          username: 'send-user',
        },
        autoReply: {
          enabled: true,
          allowedSenders: ['friend@example.com', 'other@example.com'],
          authservId: 'mx.example.com',
        },
      }),
    ),
  );
  expect(
    card.getByText(/Only new mail received after connecting/),
  ).toBeVisible();
});

it('clears generic email passwords on lifetime and agent changes and redacts setup failures', async () => {
  api.connectGenericEmail.mockRejectedValue(
    new Error('provider echoed receiving-secret'),
  );
  const view = render(
    <ConnectorsView agentId="main" telegram={null} online epoch={1} />,
  );
  let card = fillEmailSetup();
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Connect Email' })).toBeEnabled(),
  );
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  expect(await card.findByRole('alert')).toHaveTextContent(
    'This action could not be completed',
  );
  expect(screen.queryByText(/provider echoed/)).not.toBeInTheDocument();
  fillEmailSetup();
  view.rerender(
    <ConnectorsView agentId="main" telegram={null} online={false} epoch={1} />,
  );
  expect(card.getByLabelText('IMAP password')).toHaveValue('');
  expect(card.getByLabelText('SMTP password')).toHaveValue('');
  expect(card.getByLabelText('Email address')).toHaveValue('agent@example.com');
  view.rerender(
    <ConnectorsView agentId="other" telegram={null} online epoch={2} />,
  );
  card = emailCard();
  expect(card.getByLabelText('IMAP password')).toHaveValue('');
  expect(card.getByLabelText('SMTP password')).toHaveValue('');
  expect(card.getByLabelText('Email address')).toHaveValue('');
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Connect Email' })).toBeEnabled(),
  );
});

it('does not let an obsolete email setup response alter another agent or retain passwords after remount', async () => {
  let finish!: (value: unknown) => void;
  api.connectGenericEmail.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const view = render(<ConnectorsView agentId="main" telegram={null} />);
  const card = fillEmailSetup();
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Connect Email' })).toBeEnabled(),
  );
  fireEvent.click(card.getByRole('button', { name: 'Connect Email' }));
  view.rerender(<ConnectorsView agentId="other" telegram={null} />);
  await waitFor(() =>
    expect(
      emailCard().getByRole('button', { name: 'Connect Email' }),
    ).toBeEnabled(),
  );
  const reads = api.mailStatus.mock.calls.length;
  await act(async () => finish({ connector: { id: 'obsolete-email' } }));
  expect(api.mailStatus).toHaveBeenCalledTimes(reads);
  expect(emailCard().getByLabelText('IMAP password')).toHaveValue('');
  expect(emailCard().getByLabelText('SMTP password')).toHaveValue('');
  fillEmailSetup();
  view.unmount();
  render(<ConnectorsView agentId="other" telegram={null} />);
  await waitFor(() =>
    expect(
      emailCard().getByRole('button', { name: 'Connect Email' }),
    ).toBeEnabled(),
  );
  expect(emailCard().getByLabelText('IMAP password')).toHaveValue('');
  expect(emailCard().getByLabelText('SMTP password')).toHaveValue('');
});

it('reuses email inbox and immutable draft approval without an automatic send action', async () => {
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector:
        provider === 'email'
          ? { ...connector, id: 'email-1', type: 'email' }
          : null,
    }),
  );
  api.mailMessages.mockResolvedValue([
    {
      id: 'm',
      from: 'friend@example.com',
      subject: 'Hello email',
      preview: 'Preview',
      receivedAt: 'Today',
    },
  ]);
  api.mailDrafts.mockResolvedValue([{ ...draft, connectorId: 'email-1' }]);
  api.approveMailDraft.mockResolvedValue({ ...draft, state: 'sent' });
  render(<ConnectorsView agentId="main" telegram={null} />);
  const card = emailCard();
  expect(await card.findByText('Hello email')).toBeVisible();
  expect(api.approveMailDraft).not.toHaveBeenCalled();
  fireEvent.click(card.getByRole('button', { name: 'Send saved draft' }));
  await waitFor(() =>
    expect(api.approveMailDraft).toHaveBeenCalledWith(
      'main',
      'email',
      'email-1',
      'draft-1',
    ),
  );
});

it('fp02 suppresses all connector reads and actions while offline, including timer and manual refresh', async () => {
  vi.useFakeTimers();
  try {
    render(
      <ConnectorsView
        agentId="main"
        telegram={null}
        online={false}
        epoch={1}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Refresh Gmail' }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(45_000);
    });
    expect(api.oauthAppStatus).not.toHaveBeenCalled();
    expect(api.calendarStatus).not.toHaveBeenCalled();
    expect(api.mailStatus).not.toHaveBeenCalled();
    expect(api.driveStatus).not.toHaveBeenCalled();
    expect(mcpOwnerApi.list).not.toHaveBeenCalled();
    expect(communityBundlesOwnerApi.list).not.toHaveBeenCalled();
    expect(
      screen.getByRole('button', { name: 'Save Google OAuth app' }),
    ).toBeDisabled();
  } finally {
    vi.useRealTimers();
  }
});

it('fp02 reconnect discards stale mailbox reads before starting dependent inbox reads', async () => {
  let finish!: (value: unknown) => void;
  api.mailStatus.mockImplementation((_owner, provider) =>
    provider === 'gmail'
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : Promise.resolve({ configured: false, connector: null }),
  );
  const view = render(
    <ConnectorsView agentId="main" telegram={null} online epoch={1} />,
  );
  view.rerender(
    <ConnectorsView agentId="main" telegram={null} online={false} epoch={1} />,
  );
  await act(async () => {
    finish({ configured: true, connector });
  });
  expect(api.mailMessages).not.toHaveBeenCalled();
  expect(api.mailDrafts).not.toHaveBeenCalled();
  expect(screen.queryByText('owner@example.com')).not.toBeInTheDocument();
  api.mailStatus.mockResolvedValue({ configured: false, connector: null });
  view.rerender(
    <ConnectorsView agentId="main" telegram={null} online epoch={2} />,
  );
  await waitFor(() => expect(api.mailStatus).toHaveBeenCalledTimes(6));
});

it('fp02 obsolete credential save never refetches or clears retained ordinary fields', async () => {
  let finish!: (value: unknown) => void;
  api.configureOauthApp.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const user = userEvent.setup();
  const view = render(
    <ConnectorsView agentId="main" telegram={null} online epoch={1} />,
  );
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Save Google OAuth app' }),
    ).toBeEnabled(),
  );
  fireEvent.change(screen.getByLabelText('Google client ID'), {
    target: { value: 'retained-id' },
  });
  fireEvent.change(screen.getByLabelText('Google client secret'), {
    target: { value: 'synthetic-secret' },
  });
  await user.click(
    screen.getByRole('button', { name: 'Save Google OAuth app' }),
  );
  view.rerender(
    <ConnectorsView agentId="main" telegram={null} online={false} epoch={1} />,
  );
  const reads = api.oauthAppStatus.mock.calls.length;
  await act(async () => {
    finish({ configured: true });
  });
  expect(api.oauthAppStatus).toHaveBeenCalledTimes(reads);
  expect(screen.getByLabelText('Google client ID')).toHaveValue('retained-id');
  expect(screen.getByLabelText('Google client secret')).toHaveValue('');
  expect(api.configureOauthApp).toHaveBeenCalledTimes(1);
});

it('fp02 lifetime change clears unsubmitted OAuth secrets while retaining ordinary drafts', async () => {
  const view = render(
    <ConnectorsView agentId="main" telegram={null} online epoch={1} />,
  );
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Save Google OAuth app' }),
    ).toBeEnabled(),
  );
  fireEvent.change(screen.getByLabelText('Google client ID'), {
    target: { value: 'retained-id' },
  });
  fireEvent.change(screen.getByLabelText('Google client secret'), {
    target: { value: 'synthetic-secret' },
  });
  fireEvent.change(screen.getByLabelText('Connection label'), {
    target: { value: 'Draft connection' },
  });
  view.rerender(
    <ConnectorsView agentId="main" telegram={null} online={false} epoch={1} />,
  );
  expect(screen.getByLabelText('Google client secret')).toHaveValue('');
  expect(screen.getByLabelText('Google client ID')).toHaveValue('retained-id');
  expect(screen.getByLabelText('Connection label')).toHaveValue(
    'Draft connection',
  );
});

it('fp02 overlapping explicit status reconciliation cannot release a pending bundle mutation lock', async () => {
  const bundle = communityBundle();
  vi.mocked(communityBundlesOwnerApi.list).mockResolvedValue([bundle]);
  vi.spyOn(communityBundlesOwnerApi, 'get').mockResolvedValue(bundle);
  let finishMutation!: (value: typeof bundle) => void;
  vi.spyOn(communityBundlesOwnerApi, 'setEnabled').mockReturnValue(
    new Promise((resolve) => {
      finishMutation = resolve;
    }),
  );
  let finishRead!: (value: []) => void;
  const canonical = vi.fn().mockResolvedValue(undefined);
  const busy = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <ConnectorsView
      agentId="main"
      telegram={null}
      onMcpGrantsChanged={canonical}
      onMcpBusyChanged={busy}
    />,
  );
  const card = within(
    await screen.findByRole('article', {
      name: 'Community bundle Violet bundle',
    }),
  );
  await user.click(card.getByRole('button', { name: 'Inspect bundle' }));
  await waitFor(() =>
    expect(card.getByRole('button', { name: 'Disable bundle' })).toBeEnabled(),
  );
  vi.mocked(mcpOwnerApi.list).mockReturnValueOnce(
    new Promise((resolve) => {
      finishRead = resolve;
    }),
  );
  await user.click(screen.getByRole('button', { name: 'Refresh status' }));
  await user.click(card.getByRole('button', { name: 'Disable bundle' }));
  await act(async () => {
    finishRead([]);
  });
  await waitFor(() => expect(canonical).toHaveBeenCalledTimes(1));
  const external = within(
    within(view.container).getByRole('region', {
      name: 'External tools (MCP)',
    }),
  );
  expect(
    external.getByRole('textbox', { name: 'Connection label' }),
  ).toBeDisabled();
  expect(busy).toHaveBeenLastCalledWith(true);
  await act(async () => {
    finishMutation({
      ...bundle,
      enabled: false,
      state: 'disabled',
      version: 4,
    });
  });
  await waitFor(() => expect(busy).toHaveBeenLastCalledWith(false));
});
beforeEach(() => {
  vi.resetAllMocks();
  microsoftFiles.status.mockResolvedValue({ configured: false, connectors: [] });
  vi.spyOn(mcpOwnerApi, 'list').mockResolvedValue([]);
  vi.spyOn(mcpOwnerApi, 'toolNames').mockResolvedValue(['read_file']);
  vi.spyOn(communityBundlesOwnerApi, 'list').mockResolvedValue([]);
  api.oauthAppStatus.mockImplementation((provider) =>
    Promise.resolve({
      provider,
      configured: false,
      source: null,
      clientIdHint: null,
      redirectUris:
        provider === 'google'
          ? [
              'http://127.0.0.1:8080/api/connectors/google/callback',
              'http://127.0.0.1:8080/api/connectors/gdrive/callback',
            ]
          : ['http://127.0.0.1:8080/api/connectors/microsoft/callback'],
      tenant: provider === 'microsoft' ? 'common' : null,
    }),
  );
  api.calendarStatus.mockResolvedValue({ configured: false, connector: null });
  api.driveStatus.mockResolvedValue({ configured: false, connector: null });
  api.mailStatus.mockResolvedValue({ configured: false, connector: null });
  api.mailMessages.mockResolvedValue([]);
  api.mailDrafts.mockResolvedValue([]);
});
it('shows mail services with honest OAuth setup state', async () => {
  render(
    <ConnectorsView agentId="main" telegram={<div>Telegram management</div>} />,
  );
  expect(screen.getByText('Telegram')).toBeVisible();
  for (const name of [
    'Google Calendar',
    'Google Drive',
    'Email',
    'Gmail',
    'Outlook',
  ])
    expect(screen.getByRole('heading', { name })).toBeVisible();
  await waitFor(() =>
    expect(screen.getAllByText('Setup required')).toHaveLength(5),
  );
  expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeDisabled();
  expect(api.approveMailDraft).not.toHaveBeenCalled();
});
it('M43_owner_connectors_expose_the_external_tools_review_section', () => {
  render(<ConnectorsView agentId="main" telegram={null} />);

  expect(
    screen.getByRole('heading', { name: 'External tools (MCP)' }),
  ).toBeVisible();
  expect(
    screen.getByRole('textbox', { name: 'Connection label' }),
  ).toBeVisible();
  expect(screen.getByRole('textbox', { name: 'Server URL' })).toBeVisible();
  expect(
    screen.getByRole('button', { name: 'Save disabled connection' }),
  ).toBeVisible();
});
it('M45_owner_connectors_expose_the_community_bundles_review_region', () => {
  render(<ConnectorsView agentId="main" telegram={null} />);

  const region = screen.getByRole('region', { name: 'Community bundles' });
  expect(region).toBeVisible();
  const bundles = within(region);
  expect(
    bundles.getByRole('textbox', { name: 'Bundle manifest' }),
  ).toBeVisible();
  expect(bundles.getByRole('button', { name: 'Review bundle' })).toBeDisabled();
});
it('M45 shares the owner lock and preserves dirty MCP fields through canonical bundle changes', async () => {
  const user = userEvent.setup();
  const connection = {
    id: 'http-1',
    agentId: 'main',
    label: 'Existing HTTP server',
    transport: 'streamableHttp' as const,
    stdio: null,
    endpoint: 'http://127.0.0.1:8089/mcp',
    enabled: false,
    version: 1,
    hasBearerCredential: false,
    reviewedTools: [],
    state: 'disconnected' as const,
    wireGeneration: 1,
    discoveryId: null,
    candidates: [],
    physicalCallsActive: false,
    error: null,
  };
  const bundle = communityBundle();
  let settle!: (value: typeof bundle) => void;
  const mutation = new Promise<typeof bundle>((resolve) => {
    settle = resolve;
  });
  vi.mocked(mcpOwnerApi.list).mockResolvedValue([connection]);
  vi.mocked(communityBundlesOwnerApi.list).mockResolvedValue([bundle]);
  vi.spyOn(communityBundlesOwnerApi, 'get').mockResolvedValue(bundle);
  vi.spyOn(communityBundlesOwnerApi, 'setEnabled').mockReturnValue(mutation);
  const canonical = vi.fn().mockResolvedValue(undefined);
  const busy = vi.fn();
  render(
    <ConnectorsView
      agentId="main"
      telegram={null}
      onMcpGrantsChanged={canonical}
      onMcpBusyChanged={busy}
    />,
  );
  const external = within(
    await screen.findByRole('article', {
      name: 'External server Existing HTTP server',
    }),
  );
  await user.click(external.getByText('Edit saved connection'));
  const label = external.getByRole('textbox', { name: 'Saved label' });
  const url = external.getByRole('textbox', { name: 'Saved server URL' });
  await user.clear(label);
  await user.type(label, 'Unsaved owner label');
  await user.clear(url);
  await user.type(url, 'http://127.0.0.1:9099/dirty');
  const owned = within(
    screen.getByRole('article', { name: 'Community bundle Violet bundle' }),
  );
  await user.click(owned.getByRole('button', { name: 'Inspect bundle' }));
  await waitFor(() =>
    expect(owned.getByRole('button', { name: 'Disable bundle' })).toBeEnabled(),
  );
  await user.click(owned.getByRole('button', { name: 'Disable bundle' }));
  expect(label).toBeDisabled();
  expect(
    external.getByRole('button', { name: 'Save connection changes' }),
  ).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Refresh status' })).toBeDisabled();
  expect(busy).toHaveBeenLastCalledWith(true);
  vi.mocked(mcpOwnerApi.list).mockResolvedValue([
    {
      ...connection,
      version: 2,
      label: 'Canonical server',
      endpoint: 'http://127.0.0.1:8089/new',
    },
  ]);
  await act(async () => {
    settle({
      ...bundle,
      version: 4,
      enabled: false,
      state: 'disabled',
      reviewedComponentStateDigest: 'component-state-4',
    });
    await mutation;
  });
  await waitFor(() => expect(busy).toHaveBeenLastCalledWith(false));
  expect(canonical).toHaveBeenCalledWith(bundle.toolNames);
  expect(
    external.getByRole('button', { name: 'Save connection changes' }),
  ).toBeDisabled();
  expect(label).toHaveValue('Unsaved owner label');
  expect(url).toHaveValue('http://127.0.0.1:9099/dirty');
  await user.click(screen.getByRole('button', { name: 'Refresh status' }));
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Save connection changes' }),
    ).toBeEnabled(),
  );
  expect(screen.getByRole('textbox', { name: 'Saved label' })).toHaveValue(
    'Unsaved owner label',
  );
  expect(screen.getByRole('textbox', { name: 'Saved server URL' })).toHaveValue(
    'http://127.0.0.1:9099/dirty',
  );
  expect(communityBundlesOwnerApi.setEnabled).toHaveBeenCalledTimes(1);
});
it('sends the reviewed saved draft only after explicit Send', async () => {
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector: provider === 'gmail' ? connector : null,
    }),
  );
  api.mailDrafts.mockResolvedValue([draft]);
  api.approveMailDraft.mockResolvedValue({ ...draft, state: 'sent' });
  render(<ConnectorsView agentId="main" telegram={null} />);
  expect(await screen.findByText('Saved message')).toBeVisible();
  expect(api.approveMailDraft).not.toHaveBeenCalled();
  await userEvent.click(
    screen.getByRole('button', { name: 'Send saved draft' }),
  );
  expect(api.approveMailDraft).toHaveBeenCalledWith(
    'main',
    'gmail',
    'gmail-1',
    'draft-1',
  );
});
it('rejects an OAuth link from an untrusted origin', async () => {
  api.mailStatus.mockResolvedValue({ configured: true, connector: null });
  api.connectMail.mockResolvedValue({
    connector,
    consentUrl: 'https://accounts.google.com.attacker.test/consent',
  });
  render(<ConnectorsView agentId="main" telegram={null} />);
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeEnabled(),
  );
  await userEvent.click(screen.getByRole('button', { name: 'Connect Gmail' }));
  expect(
    await screen.findByText(
      'The provider returned an invalid authorization link.',
    ),
  ).toBeVisible();
  expect(
    screen.queryByRole('link', { name: 'Continue to Google' }),
  ).not.toBeInTheDocument();
});
it('ignores an old account response after the main agent changes', async () => {
  let resolve!: (value: unknown) => void;
  api.mailStatus.mockImplementation((id) =>
    id === 'old'
      ? new Promise((r) => {
          resolve = r;
        })
      : Promise.resolve({ configured: false, connector: null }),
  );
  const view = render(<ConnectorsView agentId="old" telegram={null} />);
  view.rerender(<ConnectorsView agentId="new" telegram={null} />);
  resolve({ configured: true, connector });
  await waitFor(() =>
    expect(screen.getAllByText('Setup required')).toHaveLength(5),
  );
  expect(screen.queryByText('owner@example.com')).not.toBeInTheDocument();
});

it('offers an official consent link without sending mail and clears it on disconnect', async () => {
  const pairing = { ...connector, status: 'pairing' };
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector: provider === 'gmail' ? pairing : null,
    }),
  );
  api.connectMail.mockResolvedValue({
    connector: pairing,
    consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=abc',
  });
  api.disconnectMail.mockResolvedValue(undefined);
  render(<ConnectorsView agentId="main" telegram={null} />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Reconnect Gmail' }),
    ).toBeEnabled(),
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Reconnect Gmail' }),
  );
  const link = await screen.findByRole('link', { name: 'Continue to Google' });
  expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  expect(link).toHaveAttribute(
    'href',
    'https://accounts.google.com/o/oauth2/v2/auth?state=abc',
  );
  expect(api.approveMailDraft).not.toHaveBeenCalled();
  await userEvent.click(
    screen.getByRole('button', { name: 'Disconnect Gmail' }),
  );
  expect(api.disconnectMail).toHaveBeenCalledWith('main', 'gmail', 'gmail-1');
  expect(
    screen.queryByRole('link', { name: 'Continue to Google' }),
  ).not.toBeInTheDocument();
});
it('saves a local draft without sending and rejects the saved draft by ID', async () => {
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector: provider === 'gmail' ? connector : null,
    }),
  );
  api.createMailDraft.mockResolvedValue(draft);
  api.mailDrafts.mockResolvedValue([draft]);
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);
  await user.type(
    await screen.findByRole('textbox', { name: 'Gmail draft recipients' }),
    'friend@example.com, other@example.com',
  );
  await user.type(
    screen.getByRole('textbox', { name: 'Gmail draft subject' }),
    'New subject',
  );
  await user.type(
    screen.getByRole('textbox', { name: 'Gmail draft message' }),
    'New unsent message',
  );
  await user.click(screen.getByRole('button', { name: 'Save local draft' }));
  expect(api.createMailDraft).toHaveBeenCalledWith('main', 'gmail', 'gmail-1', {
    to: ['friend@example.com', 'other@example.com'],
    subject: 'New subject',
    body: 'New unsent message',
  });
  expect(api.approveMailDraft).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Reject draft' }));
  expect(api.rejectMailDraft).toHaveBeenCalledWith(
    'main',
    'gmail',
    'gmail-1',
    'draft-1',
  );
});
it('shows immutable calendar write details and requires explicit approval', async () => {
  api.calendarStatus.mockResolvedValue({
    configured: true,
    connector: {
      id: 'calendar-1',
      status: 'active',
      accountLabel: 'owner@example.com',
    },
  });
  api.calendarWrites.mockResolvedValue([
    {
      id: 'write-1',
      summary: 'Create lunch',
      state: 'pending',
      operation: 'create',
      draft: {
        title: 'Lunch',
        calendarId: 'primary',
        start: '2026-09-06T12:00:00Z',
        end: '2026-09-06T13:00:00Z',
        location: 'Cafe',
        description: 'Discuss roadmap',
      },
    },
  ]);
  render(<ConnectorsView agentId="main" telegram={null} />);
  expect(await screen.findByText('Discuss roadmap')).toBeVisible();
  expect(api.approveCalendarWrite).not.toHaveBeenCalled();
  await userEvent.click(
    screen.getByRole('button', { name: 'Approve calendar change' }),
  );
  expect(api.approveCalendarWrite).toHaveBeenCalledWith(
    'main',
    'calendar-1',
    'write-1',
  );
});
it('retains account controls when inbox loading fails', async () => {
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector: provider === 'gmail' ? connector : null,
    }),
  );
  api.mailMessages.mockRejectedValue(new Error('private provider detail'));
  render(<ConnectorsView agentId="main" telegram={null} />);
  expect(
    await screen.findByRole('button', { name: 'Disconnect Gmail' }),
  ).toBeVisible();
  expect(
    screen.getByText('Could not load recent inbox. Refresh to try again.'),
  ).toBeVisible();
  expect(screen.queryByText('private provider detail')).not.toBeInTheDocument();
});

it('retains calendar controls when approvals cannot be loaded', async () => {
  api.calendarStatus.mockResolvedValue({
    configured: true,
    connector: { id: 'cal-1', status: 'active' },
  });
  api.calendarWrites.mockRejectedValue(new Error('private detail'));
  render(<ConnectorsView agentId="main" telegram={null} />);
  expect(
    await screen.findByRole('button', { name: 'Disconnect Google Calendar' }),
  ).toBeVisible();
  expect(
    screen.getByText(
      'Could not load calendar approvals. Refresh to try again.',
    ),
  ).toBeVisible();
});
it('does not reveal old consent after changing agents during connection', async () => {
  let resolve!: (value: unknown) => void;
  api.mailStatus.mockResolvedValue({ configured: true, connector: null });
  api.connectMail.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const view = render(<ConnectorsView agentId="old" telegram={null} />);
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeEnabled(),
  );
  await userEvent.click(screen.getByRole('button', { name: 'Connect Gmail' }));
  view.rerender(<ConnectorsView agentId="new" telegram={null} />);
  resolve({
    connector,
    consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=old',
  });
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeEnabled(),
  );
  expect(
    screen.queryByRole('link', { name: 'Continue to Google' }),
  ).not.toBeInTheDocument();
});

it('does not misreport a failed status request as missing OAuth setup', async () => {
  api.calendarStatus.mockRejectedValue(new Error('offline'));
  api.mailStatus.mockRejectedValue(new Error('offline'));
  render(<ConnectorsView agentId="main" telegram={null} />);
  await waitFor(() =>
    expect(screen.getAllByText('Status unavailable')).toHaveLength(4),
  );
  expect(screen.queryByText(/OAuth is not configured/)).not.toBeInTheDocument();
});

it('refreshes inbox from the provider only on explicit Refresh', async () => {
  api.mailStatus.mockImplementation((_id, provider) =>
    Promise.resolve({
      configured: true,
      connector: provider === 'gmail' ? connector : null,
    }),
  );
  render(<ConnectorsView agentId="main" telegram={null} />);
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Refresh Gmail' })).toBeEnabled(),
  );
  expect(api.mailMessages).not.toHaveBeenCalledWith(
    'main',
    'gmail',
    'gmail-1',
    { refresh: true },
  );
  await userEvent.click(screen.getByRole('button', { name: 'Refresh Gmail' }));
  await waitFor(() =>
    expect(api.mailMessages).toHaveBeenCalledWith('main', 'gmail', 'gmail-1', {
      refresh: true,
    }),
  );
});

it('shows OAuth redirect URIs and accessible credential fields', async () => {
  render(<ConnectorsView agentId="main" telegram={null} />);

  fireEvent.click(screen.getByText('OAuth app setup'));
  expect(screen.getByText('OAuth app setup')).toBeVisible();
  expect(
    screen.getByText(
      'One OAuth app enables Gmail, Google Calendar, and selected Google Drive files.',
    ),
  ).toBeVisible();
  expect(screen.getByText('Enables Outlook and Microsoft files.')).toBeVisible();
  expect(
    await screen.findByText(
      'http://127.0.0.1:8080/api/connectors/google/callback',
    ),
  ).toBeVisible();
  expect(
    screen.getByText('http://127.0.0.1:8080/api/connectors/microsoft/callback'),
  ).toBeVisible();
  expect(
    screen.getByText('http://127.0.0.1:8080/api/connectors/gdrive/callback'),
  ).toBeVisible();
  expect(screen.getByLabelText('Google client secret')).toHaveAttribute(
    'type',
    'password',
  );
  expect(screen.getByLabelText('Google client secret')).toHaveAttribute(
    'autocomplete',
    'new-password',
  );
  expect(screen.getByLabelText('Microsoft tenant')).toHaveValue('common');
});

it('saves Google credentials without connecting and refreshes both Google services', async () => {
  let googleConfigured = false;
  api.oauthAppStatus.mockImplementation((provider) =>
    Promise.resolve({
      provider,
      configured: false,
      source: null,
      clientIdHint: null,
      redirectUris: [`https://daemon.test/${provider}/callback`],
      tenant: provider === 'microsoft' ? 'common' : null,
    }),
  );
  api.configureOauthApp.mockImplementation((provider, input) => {
    expect(provider).toBe('google');
    expect(input).toEqual({
      clientId: 'google-id',
      clientSecret: 'top-secret',
    });
    googleConfigured = true;
    return Promise.resolve({
      provider,
      configured: true,
      source: 'vault',
      clientIdHint: 'goo…-id',
      redirectUris: ['https://daemon.test/google/callback'],
      tenant: null,
    });
  });
  api.mailStatus.mockImplementation((_id, provider) => {
    return Promise.resolve({
      configured: provider === 'gmail' && googleConfigured,
      connector: null,
    });
  });
  api.calendarStatus.mockImplementation(() =>
    Promise.resolve({
      configured: googleConfigured,
      connector: null,
    }),
  );
  api.driveStatus.mockImplementation(() =>
    Promise.resolve({ configured: googleConfigured, connector: null }),
  );
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);
  await user.type(
    await screen.findByLabelText('Google client ID'),
    'google-id',
  );
  await user.type(screen.getByLabelText('Google client secret'), 'top-secret');
  await user.click(
    screen.getByRole('button', { name: 'Save Google OAuth app' }),
  );

  await waitFor(() =>
    expect(api.configureOauthApp).toHaveBeenCalledWith('google', {
      clientId: 'google-id',
      clientSecret: 'top-secret',
    }),
  );
  expect(screen.getByLabelText('Google client secret')).toHaveValue('');
  expect(screen.getByLabelText('Google client ID')).toHaveValue('');
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeEnabled(),
  );
  expect(
    screen.getByRole('button', { name: 'Connect Google Calendar' }),
  ).toBeEnabled();
  expect(api.connectMail).not.toHaveBeenCalled();
  expect(screen.queryByText('top-secret')).not.toBeInTheDocument();
});

it('saves Microsoft credentials with tenant and enables Outlook independently', async () => {
  let outlookCalls = 0;
  api.configureOauthApp.mockResolvedValue({
    provider: 'microsoft',
    configured: true,
    source: 'vault',
    clientIdHint: 'mic…-id',
    redirectUris: ['https://daemon.test/microsoft/callback'],
    tenant: 'organizations',
  });
  api.mailStatus.mockImplementation((_id, provider) => {
    if (provider === 'outlook') outlookCalls++;
    return Promise.resolve({
      configured: provider === 'outlook' && outlookCalls > 1,
      connector: null,
    });
  });
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);
  await user.type(
    await screen.findByLabelText('Microsoft client ID'),
    'microsoft-id',
  );
  await user.type(screen.getByLabelText('Microsoft client secret'), 'secret');
  const tenant = screen.getByLabelText('Microsoft tenant');
  await user.clear(tenant);
  await user.type(tenant, 'organizations');
  await user.click(
    screen.getByRole('button', { name: 'Save Microsoft OAuth app' }),
  );

  expect(api.configureOauthApp).toHaveBeenCalledWith('microsoft', {
    clientId: 'microsoft-id',
    clientSecret: 'secret',
    tenant: 'organizations',
  });
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Connect Outlook' }),
    ).toBeEnabled(),
  );
  expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeDisabled();
});

it('omits a blank Microsoft tenant so the daemon can use its default', async () => {
  api.configureOauthApp.mockResolvedValue({
    provider: 'microsoft',
    configured: true,
    source: 'vault',
    clientIdHint: 'mic…-id',
    redirectUris: ['https://daemon.test/microsoft/callback'],
    tenant: 'common',
  });
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);

  await user.type(
    await screen.findByLabelText('Microsoft client ID'),
    'microsoft-id',
  );
  await user.type(screen.getByLabelText('Microsoft client secret'), 'secret');
  const tenant = screen.getByLabelText('Microsoft tenant');
  await user.clear(tenant);
  await user.type(tenant, '   ');
  await user.click(
    screen.getByRole('button', { name: 'Save Microsoft OAuth app' }),
  );

  expect(api.configureOauthApp).toHaveBeenCalledWith('microsoft', {
    clientId: 'microsoft-id',
    clientSecret: 'secret',
  });
});

it('clears a submitted secret before a failed configuration request settles', async () => {
  api.configureOauthApp.mockRejectedValue(
    new Error('rejected response containing never-render-this-secret'),
  );
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);
  fireEvent.click(screen.getByText('OAuth app setup'));

  await user.type(await screen.findByLabelText('Google client ID'), '  id  ');
  await user.type(
    screen.getByLabelText('Google client secret'),
    '  never-render-this-secret  ',
  );
  await user.click(
    screen.getByRole('button', { name: 'Save Google OAuth app' }),
  );

  expect(api.configureOauthApp).toHaveBeenCalledWith('google', {
    clientId: 'id',
    clientSecret: 'never-render-this-secret',
  });
  expect(screen.getByLabelText('Google client secret')).toHaveValue('');
  expect(
    await screen.findByText(
      'This action could not be completed. Refresh the connection before trying again.',
    ),
  ).toBeVisible();
  expect(screen.getByLabelText('Google client secret')).toHaveValue('');
  expect(
    screen.queryByText(/never-render-this-secret/),
  ).not.toBeInTheDocument();
});

it('keeps provider failures independent and validates credentials locally', async () => {
  api.oauthAppStatus.mockImplementation((provider) =>
    provider === 'google'
      ? Promise.reject(new Error('provider secret'))
      : Promise.resolve({
          provider,
          configured: false,
          source: null,
          clientIdHint: null,
          redirectUris: ['https://daemon.test/microsoft/callback'],
          tenant: 'common',
        }),
  );
  const user = userEvent.setup();
  render(
    <ConnectorsView agentId="main" telegram={<div>Telegram management</div>} />,
  );
  fireEvent.click(screen.getByText('OAuth app setup'));

  expect(
    await screen.findByText('Could not refresh this connection. Try again.'),
  ).toBeVisible();
  expect(screen.getByText('Microsoft OAuth app')).toBeVisible();
  expect(screen.getByText('Telegram')).toBeVisible();
  await user.type(screen.getByLabelText('Microsoft client ID'), 'id-only');
  await user.click(
    screen.getByRole('button', { name: 'Save Microsoft OAuth app' }),
  );
  expect(
    await screen.findByText('Client ID and client secret are required.'),
  ).toBeVisible();
  expect(api.configureOauthApp).not.toHaveBeenCalled();
  expect(screen.queryByText('provider secret')).not.toBeInTheDocument();
});

it('removes vault credentials and treats environment credentials as managed outside the UI', async () => {
  api.oauthAppStatus.mockImplementation((provider) =>
    Promise.resolve({
      provider,
      configured: true,
      source: provider === 'google' ? 'vault' : 'environment',
      clientIdHint: provider === 'google' ? 'goo…123' : 'mic…456',
      redirectUris: [`https://daemon.test/${provider}/callback`],
      tenant: provider === 'microsoft' ? 'common' : null,
    }),
  );
  api.removeOauthApp.mockRejectedValue(new Error('connector_conflict secret'));
  const user = userEvent.setup();
  render(<ConnectorsView agentId="main" telegram={null} />);
  fireEvent.click(screen.getByText('OAuth app setup'));

  expect(await screen.findByText('goo…123')).toBeVisible();
  expect(screen.getByText('mic…456')).toBeVisible();
  expect(
    screen.getByText('Managed through the daemon environment.'),
  ).toBeVisible();
  expect(
    screen.queryByRole('button', { name: 'Remove Microsoft OAuth app' }),
  ).not.toBeInTheDocument();
  expect(screen.getByLabelText('Microsoft client ID')).toBeDisabled();
  await user.click(
    screen.getByRole('button', { name: 'Remove Google OAuth app' }),
  );
  expect(api.removeOauthApp).toHaveBeenCalledWith('google');
  expect(
    await screen.findByText(
      'This action could not be completed. Refresh the connection before trying again.',
    ),
  ).toBeVisible();
  expect(
    screen.queryByText(/connector_conflict secret/),
  ).not.toBeInTheDocument();
});
