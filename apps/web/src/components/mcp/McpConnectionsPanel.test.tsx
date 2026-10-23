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
import type { McpConnection, McpToolDescriptor } from '../../lib/daemon-api';
import { McpConnectionsPanel, type McpOwnerApi } from './McpConnectionsPanel';

const digest = 'a'.repeat(64);
const tool: McpToolDescriptor = {
  remoteName: 'lookup',
  description: 'Remote lookup',
  inputSchema: { type: 'object' },
  outputSchema: null,
  descriptorDigest: digest,
};
const alias = `mcp_${'1'.repeat(32)}_${'a'.repeat(16)}`;
function connection(patch: Partial<McpConnection> = {}): McpConnection {
  return {
    id: 'server-1',
    agentId: 'main',
    label: 'Local server',
    transport: 'streamableHttp',
    stdio: null,
    endpoint: 'http://127.0.0.1:8089/mcp',
    enabled: false,
    version: 1,
    hasBearerCredential: false,
    reviewedTools: [],
    state: 'disconnected',
    wireGeneration: 1,
    discoveryId: null,
    candidates: [],
    physicalCallsActive: false,
    error: null,
    ...patch,
  };
}
function ready(patch: Partial<McpConnection> = {}) {
  return connection({
    enabled: true,
    state: 'ready',
    discoveryId: 'discovery-1',
    candidates: [tool],
    reviewedTools: [{ ...tool, alias }],
    ...patch,
  });
}
function api(
  rows: McpConnection[] = [],
  names: string[] = ['read_file', 'unknown_custom'],
): McpOwnerApi {
  return {
    list: vi.fn().mockResolvedValue(rows),
    toolNames: vi.fn().mockResolvedValue(names),
    create: vi.fn(),
    update: vi.fn(),
    connect: vi.fn(),
    discover: vi.fn(),
    review: vi.fn(),
    disconnect: vi.fn(),
    remove: vi.fn(),
    saveBearerCredential: vi.fn(),
    clearBearerCredential: vi.fn(),
    setGrant: vi.fn(),
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function loaded() {
  await waitFor(() =>
    expect(
      screen.queryByText('Reading saved connections and permissions…'),
    ).not.toBeInTheDocument(),
  );
}

describe('owner external tools', () => {
  it('navigation releases a same-lifetime status-reconciliation busy claim without publishing late rows', async () => {
    const mock = api();
    const reconciliation = deferred<void>();
    const changed = vi.fn().mockReturnValue(reconciliation.promise);
    const busy = vi.fn();
    const view = render(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        online
        epoch={1}
        onGrantsChanged={changed}
        onBusyChanged={busy}
      />,
    );
    await loaded();
    await userEvent
      .setup()
      .click(screen.getByRole('button', { name: 'Refresh status' }));
    expect(busy).toHaveBeenLastCalledWith(true);
    view.unmount();
    await act(async () => {
      reconciliation.resolve(undefined);
    });
    expect(changed).toHaveBeenCalledTimes(1);
    expect(busy).toHaveBeenLastCalledWith(false);
  });

  it('fences obsolete connection lists and mutation callbacks across offline and epoch changes', async () => {
    const first = deferred<McpConnection[]>();
    const mock = api();
    vi.mocked(mock.list).mockReturnValueOnce(first.promise);
    const busy = vi.fn();
    const view = render(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        online
        epoch={1}
        onBusyChanged={busy}
      />,
    );
    view.rerender(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        online={false}
        epoch={1}
        onBusyChanged={busy}
      />,
    );
    await act(async () => {
      first.resolve([ready()]);
    });
    expect(
      screen.queryByText('Connected — reviewed tools ready'),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh status' }));
    expect(mock.list).toHaveBeenCalledTimes(1);
    vi.mocked(mock.list).mockResolvedValue([connection()]);
    view.rerender(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        online
        epoch={2}
        onBusyChanged={busy}
      />,
    );
    await loaded();
    const pending = deferred<McpConnection>();
    vi.mocked(mock.update).mockReturnValue(pending.promise);
    await userEvent
      .setup()
      .click(screen.getByRole('button', { name: 'Enable connection' }));
    expect(mock.update).toHaveBeenCalledTimes(1);
    view.rerender(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        online={false}
        epoch={2}
        onBusyChanged={busy}
      />,
    );
    busy.mockClear();
    await act(async () => {
      pending.resolve(connection({ enabled: true, version: 2 }));
    });
    expect(busy).not.toHaveBeenCalled();
    expect(mock.update).toHaveBeenCalledTimes(1);
  });

  it('offers a password action for HTTPS and local loopback HTTP connections', async () => {
    const mock = api([
      connection({ endpoint: 'https://example.test/mcp' }),
      connection({
        id: 'loopback',
        label: 'Loopback',
        endpoint: 'http://localhost:5173/mcp',
      }),
      connection({
        id: 'ipv6',
        label: 'IPv6',
        endpoint: 'http://[::1]:5173/mcp',
      }),
      connection({
        id: 'remote',
        label: 'Remote HTTP',
        endpoint: 'http://example.test/mcp',
      }),
      connection({
        id: 'local',
        label: 'Local process',
        transport: 'stdio',
        stdio: { executable: 'C:\\synthetic\\mcp.exe', args: [] },
        endpoint: '',
      }),
    ]);
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    const https = within(
      screen.getByRole('article', { name: 'External server Local server' }),
    );
    const token = https.getByLabelText('Bearer token');
    expect(token).toHaveAttribute('type', 'password');
    expect(
      https.getByRole('button', { name: 'Save bearer credential' }),
    ).toBeDisabled();
    expect(https.getByText('No bearer credential saved')).toBeVisible();
    expect(
      within(
        screen.getByRole('article', { name: 'External server Loopback' }),
      ).queryByLabelText('Bearer token'),
    ).toHaveAttribute('type', 'password');
    expect(
      within(
        screen.getByRole('article', { name: 'External server IPv6' }),
      ).getByLabelText('Bearer token'),
    ).toHaveAttribute('type', 'password');
    expect(
      within(
        screen.getByRole('article', { name: 'External server Remote HTTP' }),
      ).queryByLabelText('Bearer token'),
    ).not.toBeInTheDocument();
    expect(
      within(
        screen.getByRole('article', { name: 'External server Local process' }),
      ).queryByLabelText('Bearer token'),
    ).not.toBeInTheDocument();
    expect(mock.saveBearerCredential).not.toHaveBeenCalled();
    expect(mock.clearBearerCredential).not.toHaveBeenCalled();
    expect(mock.connect).not.toHaveBeenCalled();
  });

  it('saves and replaces only on clicks, clears the password after settlement, and never echoes it', async () => {
    const user = userEvent.setup();
    const token = 'synthetic-owner-secret';
    const mock = api([connection({ endpoint: 'https://example.test/mcp' })]);
    vi.mocked(mock.saveBearerCredential)
      .mockResolvedValueOnce(
        connection({
          endpoint: 'https://example.test/mcp',
          version: 2,
          hasBearerCredential: true,
        }),
      )
      .mockResolvedValueOnce(
        connection({
          endpoint: 'https://example.test/mcp',
          version: 3,
          hasBearerCredential: true,
        }),
      );
    vi.mocked(mock.clearBearerCredential).mockResolvedValue(
      connection({ endpoint: 'https://example.test/mcp', version: 4 }),
    );
    const { container } = render(
      <McpConnectionsPanel agentId="main" api={mock} />,
    );
    await loaded();
    const card = within(
      screen.getByRole('article', { name: 'External server Local server' }),
    );
    const password = card.getByLabelText('Bearer token');
    await user.type(password, token);
    expect(mock.saveBearerCredential).not.toHaveBeenCalled();
    await user.click(
      card.getByRole('button', { name: 'Save bearer credential' }),
    );
    expect(mock.saveBearerCredential).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 1,
      bearerToken: token,
    });
    await waitFor(() => expect(password).toHaveValue(''));
    expect(card.getByText('Bearer credential saved')).toBeVisible();
    expect(container.textContent).not.toContain(token);
    await user.type(password, 'replacement-token');
    await user.click(
      card.getByRole('button', { name: 'Replace bearer credential' }),
    );
    expect(mock.saveBearerCredential).toHaveBeenLastCalledWith(
      'main',
      'server-1',
      {
        expectedVersion: 2,
        bearerToken: 'replacement-token',
      },
    );
    await waitFor(() => expect(password).toHaveValue(''));
    await user.click(
      card.getByRole('button', { name: 'Clear bearer credential' }),
    );
    expect(mock.clearBearerCredential).toHaveBeenCalledWith(
      'main',
      'server-1',
      {
        expectedVersion: 3,
      },
    );
    expect(await card.findByText('No bearer credential saved')).toBeVisible();
    expect(mock.connect).not.toHaveBeenCalled();
  });

  it('discards a failed password, shows only a static error, and requires explicit refresh before re-entry', async () => {
    const user = userEvent.setup();
    const token = 'synthetic-failed-token';
    const mock = api([connection({ endpoint: 'https://example.test/mcp' })]);
    vi.mocked(mock.saveBearerCredential).mockRejectedValue({
      status: 409,
      body: { error: token },
    });
    const { container } = render(
      <McpConnectionsPanel agentId="main" api={mock} />,
    );
    await loaded();
    const card = within(
      screen.getByRole('article', { name: 'External server Local server' }),
    );
    const password = card.getByLabelText('Bearer token');
    await user.type(password, token);
    await user.click(
      card.getByRole('button', { name: 'Save bearer credential' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This connection changed',
    );
    expect(password).toHaveValue('');
    expect(container.textContent).not.toContain(token);
    expect(
      card.getByRole('button', { name: 'Save bearer credential' }),
    ).toBeDisabled();
    expect(mock.saveBearerCredential).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Refresh status' }));
    await loaded();
    expect(mock.saveBearerCredential).toHaveBeenCalledTimes(1);
  });

  it('locks other mutations while a credential write is pending and fences an old agent completion', async () => {
    const user = userEvent.setup();
    const pending = deferred<McpConnection>();
    const mock = api();
    vi.mocked(mock.list)
      .mockResolvedValueOnce([
        connection({ endpoint: 'https://example.test/mcp' }),
      ])
      .mockResolvedValueOnce([
        connection({
          id: 'beta-server',
          agentId: 'beta',
          label: 'Beta server',
          endpoint: 'https://beta.test/mcp',
        }),
      ]);
    vi.mocked(mock.saveBearerCredential).mockReturnValue(pending.promise);
    const view = render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    const card = within(
      screen.getByRole('article', { name: 'External server Local server' }),
    );
    await user.type(
      card.getByLabelText('Bearer token'),
      'synthetic-pending-token',
    );
    await user.click(
      card.getByRole('button', { name: 'Save bearer credential' }),
    );
    expect(
      screen.getByRole('button', { name: 'Refresh status' }),
    ).toBeDisabled();
    expect(
      card.getByRole('button', { name: 'Remove saved connection' }),
    ).toBeDisabled();
    view.rerender(<McpConnectionsPanel agentId="beta" api={mock} />);
    await screen.findByRole('heading', { name: 'Beta server' });
    await act(async () => {
      pending.resolve(
        connection({
          endpoint: 'https://example.test/mcp',
          version: 2,
          hasBearerCredential: true,
        }),
      );
      await pending.promise;
    });
    expect(screen.queryByText('Local server')).not.toBeInTheDocument();
    expect(
      screen.getByRole('article', { name: 'External server Beta server' }),
    ).toBeVisible();
    expect(mock.saveBearerCredential).toHaveBeenCalledTimes(1);
    expect(mock.connect).not.toHaveBeenCalled();
  });

  it('owner_explicitly_selects_connection_type_before_configuring_local_server', async () => {
    const mock = api();
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    expect(
      screen.getByRole('combobox', { name: 'Connection type' }),
    ).toHaveValue('streamableHttp');
    expect(mock.create).not.toHaveBeenCalled();
    expect(mock.connect).not.toHaveBeenCalled();
    expect(mock.discover).not.toHaveBeenCalled();
  });

  it('confirms trusted immutable launch, saves disabled, enables separately and starts only on an explicit click', async () => {
    const user = userEvent.setup();
    const mock = api();
    const stdio = {
      executable: 'C:\\synthetic\\mcp-server.exe',
      args: ['--mode', 'one argument with spaces', '"literal quotes"'],
    };
    const local = connection({ transport: 'stdio', stdio, endpoint: '' });
    vi.mocked(mock.create).mockResolvedValue(local);
    vi.mocked(mock.update)
      .mockResolvedValueOnce({ ...local, enabled: true, version: 2 })
      .mockResolvedValueOnce({
        ...local,
        enabled: true,
        label: 'Renamed local',
        version: 3,
      });
    vi.mocked(mock.connect).mockResolvedValue({
      ...local,
      label: 'Renamed local',
      enabled: true,
      version: 3,
      state: 'needs_review',
      discoveryId: 'local-discovery',
      candidates: [tool],
    });
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Connection type' }),
      'stdio',
    );
    expect(
      screen.queryByRole('textbox', { name: 'Server URL' }),
    ).not.toBeInTheDocument();
    await user.type(
      screen.getByRole('textbox', { name: 'Connection label' }),
      'Local server',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Absolute executable path' }),
      stdio.executable,
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Arguments — one per line' }),
      stdio.args.join('\n'),
    );
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole('checkbox', {
        name: 'I trust this local program and its arguments',
      }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    );
    expect(mock.create).toHaveBeenCalledWith('main', {
      label: 'Local server',
      stdio,
    });
    const card = within(
      await screen.findByRole('article', {
        name: 'External server Local server',
      }),
    );
    expect(card.getByText(stdio.executable)).toBeVisible();
    expect(
      card.getByRole('button', { name: 'Start local server' }),
    ).toBeDisabled();
    expect(mock.connect).not.toHaveBeenCalled();
    await user.click(card.getByRole('button', { name: 'Enable connection' }));
    await waitFor(() =>
      expect(
        card.getByRole('button', { name: 'Start local server' }),
      ).toBeEnabled(),
    );
    expect(mock.update).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 1,
      enabled: true,
    });
    expect(mock.connect).not.toHaveBeenCalled();
    await user.click(card.getByText('Edit saved connection'));
    expect(
      card.queryByRole('textbox', { name: 'Saved server URL' }),
    ).not.toBeInTheDocument();
    expect(
      card.queryByRole('textbox', { name: 'Absolute executable path' }),
    ).not.toBeInTheDocument();
    const label = card.getByRole('textbox', { name: 'Saved label' });
    await user.clear(label);
    await user.type(label, 'Renamed local');
    await user.click(
      card.getByRole('button', { name: 'Save connection changes' }),
    );
    expect(mock.update).toHaveBeenLastCalledWith('main', 'server-1', {
      expectedVersion: 2,
      label: 'Renamed local',
    });
    await waitFor(() =>
      expect(
        card.getByRole('button', { name: 'Start local server' }),
      ).toBeEnabled(),
    );
    await user.click(card.getByRole('button', { name: 'Start local server' }));
    expect(mock.connect).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 3,
    });
    expect(
      await card.findByRole('button', { name: 'Review lookup' }),
    ).toBeEnabled();
    expect(mock.review).not.toHaveBeenCalled();
    expect(mock.setGrant).not.toHaveBeenCalled();
    expect(mock.discover).not.toHaveBeenCalled();
  });

  it('explicitly distinguishes singleton empty argv from untouched zero argv', async () => {
    const user = userEvent.setup();
    const mock = api();
    const executable = 'C:\\synthetic\\mcp-server.exe';
    vi.mocked(mock.create)
      .mockResolvedValueOnce(
        connection({
          label: 'No arguments',
          transport: 'stdio',
          stdio: { executable, args: [] },
          endpoint: '',
        }),
      )
      .mockResolvedValueOnce(
        connection({
          id: 'server-empty',
          label: 'One empty argument',
          transport: 'stdio',
          stdio: { executable, args: [''] },
          endpoint: '',
        }),
      );
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Connection type' }),
      'stdio',
    );
    const label = screen.getByRole('textbox', { name: 'Connection label' });
    const path = screen.getByRole('textbox', {
      name: 'Absolute executable path',
    });
    const args = screen.getByRole('textbox', {
      name: 'Arguments — one per line',
    });
    const trust = screen.getByRole('checkbox', {
      name: 'I trust this local program and its arguments',
    });
    await user.type(label, 'No arguments');
    await user.type(path, executable);
    expect(args).toHaveValue('');
    await user.click(trust);
    await user.click(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    );
    expect(mock.create).toHaveBeenNthCalledWith(1, 'main', {
      label: 'No arguments',
      stdio: { executable, args: [] },
    });
    await screen.findByRole('article', {
      name: 'External server No arguments',
    });
    await waitFor(() => expect(trust).toBeEnabled());

    await user.type(label, 'One empty argument');
    await user.type(path, executable);
    await user.click(trust);
    const oneEmpty = screen.getByRole('checkbox', {
      name: 'Pass one empty argument',
    });
    expect(oneEmpty).not.toBeChecked();
    await user.click(oneEmpty);
    expect(oneEmpty).toBeChecked();
    expect(trust).not.toBeChecked();
    expect(args).toHaveValue('');
    await user.click(trust);
    await user.click(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    );
    expect(mock.create).toHaveBeenNthCalledWith(2, 'main', {
      label: 'One empty argument',
      stdio: { executable, args: [''] },
    });
    await screen.findByRole('article', {
      name: 'External server One empty argument',
    });
    expect(mock.create).toHaveBeenCalledTimes(2);
    expect(mock.connect).not.toHaveBeenCalled();
    expect(mock.discover).not.toHaveBeenCalled();
  });

  it('rejects relative paths and oversized argv locally and invalidates trust whenever launch details change', async () => {
    const user = userEvent.setup();
    const mock = api();
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Connection type' }),
      'stdio',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Connection label' }),
      'Bounded local',
    );
    const executable = screen.getByRole('textbox', {
      name: 'Absolute executable path',
    });
    const args = screen.getByRole('textbox', {
      name: 'Arguments — one per line',
    });
    const trust = screen.getByRole('checkbox', {
      name: 'I trust this local program and its arguments',
    });
    await user.type(executable, 'relative.exe');
    await user.click(trust);
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeDisabled();
    expect(
      screen.getByText('Enter an absolute executable path.'),
    ).toBeVisible();
    await user.clear(executable);
    await user.type(executable, 'C:\\synthetic\\mcp.exe');
    expect(trust).not.toBeChecked();
    await user.click(trust);
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeEnabled();
    fireEvent.change(args, {
      target: {
        value: Array.from({ length: 33 }, (_, index) => `arg-${index}`).join(
          '\n',
        ),
      },
    });
    expect(trust).not.toBeChecked();
    await user.click(trust);
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeDisabled();
    expect(screen.getByText(/Use at most 32 arguments/)).toBeVisible();
    fireEvent.change(args, { target: { value: 'é'.repeat(1025) } });
    await user.click(trust);
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeDisabled();
    expect(screen.getByText(/Use at most 32 arguments/)).toBeVisible();
    fireEvent.change(args, { target: { value: 'argument\twith-control' } });
    await user.click(trust);
    expect(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    ).toBeDisabled();
    expect(mock.create).not.toHaveBeenCalled();
    expect(mock.connect).not.toHaveBeenCalled();
  });

  it('shows restored local launch arguments as plain owner data and never starts on load, refresh or agent switch', async () => {
    const mock = api([
      connection({
        transport: 'stdio',
        stdio: {
          executable: '/synthetic/mcp-server',
          args: ['<img src="https://remote.test/track">', '$SECRET'],
        },
        endpoint: '',
        enabled: true,
      }),
    ]);
    const view = render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    expect(screen.getByText('/synthetic/mcp-server')).toBeVisible();
    expect(
      screen.getByText(JSON.stringify('<img src="https://remote.test/track">')),
    ).toBeVisible();
    expect(view.container.querySelector('img, a, iframe')).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Start local server' }),
    ).toBeEnabled();
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    expect(mock.connect).not.toHaveBeenCalled();
    expect(mock.discover).not.toHaveBeenCalled();
    expect(mock.setGrant).not.toHaveBeenCalled();
    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Connection type' }),
      'stdio',
    );
    await userEvent.type(
      screen.getByRole('textbox', { name: 'Absolute executable path' }),
      '/synthetic/other-server',
    );
    await userEvent.click(
      screen.getByRole('checkbox', {
        name: 'I trust this local program and its arguments',
      }),
    );
    vi.mocked(mock.list).mockResolvedValue([]);
    view.rerender(<McpConnectionsPanel agentId="beta" api={mock} />);
    await loaded();
    expect(
      screen.getByRole('combobox', { name: 'Connection type' }),
    ).toHaveValue('streamableHttp');
    expect(
      screen.queryByRole('textbox', { name: 'Absolute executable path' }),
    ).not.toBeInTheDocument();
    expect(mock.connect).not.toHaveBeenCalled();
    expect(mock.create).not.toHaveBeenCalled();
  });

  it('saves disabled, enables, connects, reviews then grants exactly one displayed binding', async () => {
    const user = userEvent.setup();
    const mock = api();
    const changed = vi.fn();
    vi.mocked(mock.create).mockResolvedValue(connection());
    vi.mocked(mock.update).mockResolvedValue(
      connection({ enabled: true, version: 2 }),
    );
    vi.mocked(mock.connect).mockResolvedValue(
      connection({
        enabled: true,
        version: 2,
        state: 'needs_review',
        discoveryId: 'discovery-1',
        candidates: [tool],
      }),
    );
    vi.mocked(mock.review).mockResolvedValue(ready({ version: 3 }));
    vi.mocked(mock.setGrant).mockResolvedValue({
      connection: ready({ version: 4 }),
      granted: true,
      toolNames: ['read_file', 'unknown_custom', alias],
    });
    render(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    await loaded();
    for (const method of [
      mock.create,
      mock.connect,
      mock.discover,
      mock.review,
      mock.setGrant,
    ])
      expect(method).not.toHaveBeenCalled();
    await user.type(
      screen.getByRole('textbox', { name: 'Connection label' }),
      'Local server',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Server URL' }),
      'http://127.0.0.1:8089/mcp',
    );
    await user.click(
      screen.getByRole('button', { name: 'Save disabled connection' }),
    );
    expect(mock.create).toHaveBeenCalledWith('main', {
      label: 'Local server',
      endpoint: 'http://127.0.0.1:8089/mcp',
    });
    expect(await screen.findByText('Saved — disabled')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Enable connection' }));
    expect(mock.update).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 1,
      enabled: true,
    });
    expect(await screen.findByText('Enabled — disconnected')).toBeVisible();
    expect(mock.connect).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Connect server' }));
    expect(mock.connect).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 2,
    });
    await user.click(
      await screen.findByRole('button', { name: 'Review lookup' }),
    );
    expect(mock.review).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 2,
      discoveryId: 'discovery-1',
      descriptorDigest: digest,
    });
    expect(mock.setGrant).not.toHaveBeenCalled();
    await user.click(
      await screen.findByRole('button', { name: 'Grant lookup' }),
    );
    expect(mock.setGrant).toHaveBeenCalledWith('main', 'server-1', alias, {
      expectedVersion: 3,
      descriptorDigest: digest,
      granted: true,
    });
    await waitFor(() =>
      expect(changed).toHaveBeenCalledWith([
        'read_file',
        'unknown_custom',
        alias,
      ]),
    );
    expect(screen.getByRole('button', { name: 'Revoke lookup' })).toBeVisible();
    expect(mock.discover).not.toHaveBeenCalled();
  });

  it('uses actual grants independently from review, retains dormant permissions, and refreshes without reconnecting', async () => {
    const mock = api(
      [ready({ state: 'disconnected', candidates: [], discoveryId: null })],
      ['read_file', alias],
    );
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    expect(
      screen.getByText(/Permission granted · Currently unavailable/),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Revoke lookup' })).toBeEnabled();
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await waitFor(() => expect(mock.list).toHaveBeenCalledTimes(2));
    expect(mock.toolNames).toHaveBeenCalledTimes(2);
    expect(mock.connect).not.toHaveBeenCalled();
    expect(mock.discover).not.toHaveBeenCalled();
    expect(mock.review).not.toHaveBeenCalled();
    expect(mock.setGrant).not.toHaveBeenCalled();
  });

  it('renders remote descriptions and schemas only as plain text and distinguishes retained old matches', async () => {
    const hostile = {
      ...tool,
      description:
        '<img src="https://remote.example/track" onerror="alert(1)">',
      inputSchema: {
        properties: { url: { default: 'https://remote.example/track' } },
      },
      descriptorDigest: 'b'.repeat(64),
    };
    const { container } = render(
      <McpConnectionsPanel
        agentId="main"
        api={api(
          [ready({ state: 'needs_review', candidates: [tool, hostile] })],
          [alias],
        )}
      />,
    );
    await loaded();
    expect(screen.getByText(hostile.description)).toBeVisible();
    expect(container.querySelector('img, a, iframe')).toBeNull();
    expect(
      screen.getByText(/Permission granted · Available on this connection/),
    ).toBeVisible();
    expect(
      screen.getByText('Not reviewed — not available to the agent.'),
    ).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Grant lookup' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Revoke lookup' })).toBeVisible();
  });

  it('keeps edited fields after conflict and requires explicit refresh and reconfirmation, with one shared mutation lock', async () => {
    const mock = api([connection()]);
    const pending = deferred<McpConnection>();
    vi.mocked(mock.update)
      .mockReturnValueOnce(pending.promise)
      .mockRejectedValueOnce({ status: 409 });
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    await userEvent.click(
      screen.getByRole('button', { name: 'Enable connection' }),
    );
    expect(
      screen.getByRole('button', { name: 'Refresh status' }),
    ).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Remove saved connection' }),
    ).toBeDisabled();
    await act(async () => {
      pending.resolve(connection({ enabled: true, version: 2 }));
      await pending.promise;
    });
    await userEvent.click(screen.getByText('Edit saved connection'));
    const label = screen.getByRole('textbox', { name: 'Saved label' });
    await userEvent.clear(label);
    await userEvent.type(label, 'Edited label');
    await userEvent.click(
      screen.getByRole('button', { name: 'Save connection changes' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This connection changed',
    );
    expect(label).toHaveValue('Edited label');
    expect(
      screen.getByRole('button', { name: 'Save connection changes' }),
    ).toBeDisabled();
    expect(mock.update).toHaveBeenCalledTimes(2);
    vi.mocked(mock.list).mockResolvedValue([
      connection({ enabled: true, version: 5 }),
    ]);
    vi.mocked(mock.update).mockResolvedValue(
      connection({ label: 'Edited label', version: 6 }),
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    expect(label).toHaveValue('Edited label');
    await userEvent.click(
      screen.getByRole('button', { name: 'Save connection changes' }),
    );
    expect(mock.update).toHaveBeenLastCalledWith('main', 'server-1', {
      expectedVersion: 5,
      label: 'Edited label',
      endpoint: 'http://127.0.0.1:8089/mcp',
    });
  });

  it('preserves a dirty label but refreshes an untouched URL before saving, then clears acknowledged dirty state', async () => {
    const mock = api([connection()]);
    render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    await userEvent.click(screen.getByText('Edit saved connection'));
    const label = screen.getByRole('textbox', { name: 'Saved label' });
    const endpoint = screen.getByRole('textbox', { name: 'Saved server URL' });
    await userEvent.clear(label);
    await userEvent.type(label, 'Edited label');
    const endpointB = 'http://127.0.0.1:8090/mcp';
    vi.mocked(mock.list).mockResolvedValue([
      connection({ version: 5, endpoint: endpointB }),
    ]);
    vi.mocked(mock.update).mockResolvedValue(
      connection({ version: 6, label: 'Edited label', endpoint: endpointB }),
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    expect(label).toHaveValue('Edited label');
    expect(endpoint).toHaveValue(endpointB);
    await userEvent.click(
      screen.getByRole('button', { name: 'Save connection changes' }),
    );
    expect(mock.update).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 5,
      label: 'Edited label',
      endpoint: endpointB,
    });
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Refresh status' }),
      ).toBeEnabled(),
    );
    vi.mocked(mock.list).mockResolvedValue([
      connection({
        version: 7,
        label: 'Latest canonical label',
        endpoint: endpointB,
      }),
    ]);
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    expect(label).toHaveValue('Latest canonical label');
    expect(endpoint).toHaveValue(endpointB);
  });

  it('ignores old-agent reads and mutation completions, without notifying the new owner', async () => {
    const oldRead = deferred<McpConnection[]>();
    const oldGrant = deferred<Awaited<ReturnType<McpOwnerApi['setGrant']>>>();
    const mock = api([ready()]);
    vi.mocked(mock.list)
      .mockReturnValueOnce(oldRead.promise)
      .mockResolvedValueOnce([ready()])
      .mockResolvedValueOnce([
        connection({ id: 'beta', agentId: 'beta', label: 'Beta server' }),
      ]);
    vi.mocked(mock.setGrant).mockReturnValue(oldGrant.promise);
    const changed = vi.fn();
    const view = render(
      <McpConnectionsPanel
        agentId="old"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    view.rerender(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    await loaded();
    await act(async () => {
      oldRead.resolve([connection({ label: 'Old server' })]);
      await oldRead.promise;
    });
    expect(screen.queryByText('Old server')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Grant lookup' }));
    view.rerender(
      <McpConnectionsPanel
        agentId="beta"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    await screen.findByRole('heading', { name: 'Beta server' });
    await act(async () => {
      oldGrant.resolve({
        connection: ready({ version: 4 }),
        granted: true,
        toolNames: [alias],
      });
      await oldGrant.promise;
    });
    expect(changed).not.toHaveBeenCalled();
    expect(screen.queryByText('Permission granted')).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Beta server' })).toBeVisible();
  });

  it('reports unknown permissions after a failed owner read, never optimistically grants, and gates restricted writes', async () => {
    const mock = api([ready()]);
    vi.mocked(mock.toolNames).mockRejectedValueOnce({ status: 503 });
    const view = render(<McpConnectionsPanel agentId="main" api={mock} />);
    await loaded();
    expect(screen.getByRole('alert')).toHaveTextContent('Status unavailable');
    expect(
      screen.queryByRole('button', { name: 'Grant lookup' }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    vi.mocked(mock.setGrant).mockRejectedValue({ status: 503 });
    await userEvent.click(screen.getByRole('button', { name: 'Grant lookup' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not be confirmed',
    );
    expect(screen.getByRole('button', { name: 'Grant lookup' })).toBeDisabled();
    expect(
      screen.queryByRole('button', { name: 'Revoke lookup' }),
    ).not.toBeInTheDocument();
    expect(mock.setGrant).toHaveBeenCalledTimes(1);
    view.rerender(<McpConnectionsPanel agentId="main" api={mock} disabled />);
    expect(
      screen.getByRole('textbox', { name: 'Connection label' }),
    ).toBeDisabled();
  });

  it('disconnects and removes only after explicit actions, using each acknowledged version and exact card', async () => {
    const mock = api(
      [
        ready({ version: 7 }),
        connection({ id: 'neighbor', label: 'Neighbor' }),
      ],
      [alias],
    );
    vi.mocked(mock.disconnect).mockResolvedValue(
      ready({ version: 8, state: 'stopping', physicalCallsActive: true }),
    );
    vi.mocked(mock.remove).mockResolvedValue(undefined);
    const changed = vi.fn();
    render(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
      />,
    );
    await loaded();
    const card = within(
      screen.getByRole('article', { name: 'External server Local server' }),
    );
    await userEvent.click(
      card.getByRole('button', { name: 'Disconnect server' }),
    );
    expect(mock.disconnect).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 7,
    });
    expect(
      await card.findByText('Stopping — waiting for cleanup'),
    ).toBeVisible();
    expect(card.getByRole('button', { name: 'Connect server' })).toBeDisabled();
    expect(
      card.getByRole('button', { name: 'Remove saved connection' }),
    ).toBeDisabled();
    vi.mocked(mock.list).mockResolvedValue([
      ready({ version: 8, state: 'disconnected', candidates: [] }),
      connection({ id: 'neighbor', label: 'Neighbor' }),
    ]);
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    await userEvent.click(
      card.getByRole('button', { name: 'Remove saved connection' }),
    );
    expect(mock.remove).not.toHaveBeenCalled();
    await userEvent.click(
      card.getByRole('button', { name: 'Keep connection' }),
    );
    expect(mock.remove).not.toHaveBeenCalled();
    await userEvent.click(
      card.getByRole('button', { name: 'Remove saved connection' }),
    );
    vi.mocked(mock.toolNames).mockResolvedValue([
      'read_file',
      'unknown_custom',
    ]);
    await userEvent.click(
      card.getByRole('button', { name: 'Confirm removal' }),
    );
    expect(mock.remove).toHaveBeenCalledWith('main', 'server-1', {
      expectedVersion: 8,
    });
    await waitFor(() =>
      expect(
        screen.queryByRole('article', { name: 'External server Local server' }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole('article', { name: 'External server Neighbor' }),
    ).toBeVisible();
    await waitFor(() =>
      expect(changed).toHaveBeenCalledWith(['read_file', 'unknown_custom']),
    );
  });

  it('retains an acknowledged grant after parent-read failure and explicitly refreshes only reads', async () => {
    const mock = api([ready()]);
    const changed = vi
      .fn()
      .mockRejectedValueOnce(new Error('Read unavailable'))
      .mockResolvedValue(undefined);
    const uncertain = vi.fn();
    vi.mocked(mock.setGrant).mockResolvedValue({
      connection: ready({ version: 4 }),
      granted: true,
      toolNames: ['read_file', 'unknown_custom', alias],
    });
    render(
      <McpConnectionsPanel
        agentId="main"
        api={mock}
        onGrantsChanged={changed}
        onPermissionsUncertain={uncertain}
      />,
    );
    await loaded();
    await userEvent.click(screen.getByRole('button', { name: 'Grant lookup' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not be confirmed',
    );
    expect(
      screen.getByRole('button', { name: 'Revoke lookup' }),
    ).toBeDisabled();
    expect(uncertain).toHaveBeenCalledTimes(1);
    vi.mocked(mock.list).mockResolvedValue([ready({ version: 4 })]);
    vi.mocked(mock.toolNames).mockResolvedValue([
      'read_file',
      'unknown_custom',
      alias,
    ]);
    await userEvent.click(
      screen.getByRole('button', { name: 'Refresh status' }),
    );
    await loaded();
    expect(mock.setGrant).toHaveBeenCalledTimes(1);
    expect(changed).toHaveBeenCalledTimes(2);
    expect(screen.getByRole('button', { name: 'Revoke lookup' })).toBeEnabled();
    expect(mock.connect).not.toHaveBeenCalled();
  });
});
