import {
  act,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { render } from '../test/render-open';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { driveFixture } from '../test/drive';
import { GoogleDriveCard } from './GoogleDriveCard';

vi.mock('../lib/daemon-api', async () => {
  const actual =
    await vi.importActual<typeof import('../lib/daemon-api')>(
      '../lib/daemon-api',
    );
  return {
    ...actual,
    daemon: {
      ...actual.daemon,
      driveStatus: vi.fn(),
      connectDrive: vi.fn(),
      disconnectDrive: vi.fn(),
    },
  };
});
beforeEach(() => {
  vi.mocked(daemon.driveStatus)
    .mockReset()
    .mockResolvedValue({ configured: true, connector: null });
  vi.mocked(daemon.connectDrive).mockReset();
  vi.mocked(daemon.disconnectDrive).mockReset();
});
describe('GoogleDriveCard', () => {
  it('previews link-derived IDs and discloses broad consent before sending selected IDs', async () => {
    const user = userEvent.setup();
    const c = driveFixture({
      status: 'pairing',
      selectedFileIds: ['File_1', 'Doc_2'],
    });
    vi.mocked(daemon.connectDrive).mockResolvedValue({
      connector: c,
      consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=opaque',
    });
    render(<GoogleDriveCard agentId="agent-a" />);
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Connect selected files' }),
      ).toBeDisabled(),
    );
    expect(
      screen.getByText(/Google gives this app read access to your whole Drive/),
    ).toBeVisible();
    expect(screen.getByText(/other formats return details only/)).toBeVisible();
    const input = screen.getByLabelText('Selected Drive files');
    await user.type(
      input,
      'File_1\nhttps://docs.google.com/document/d/Doc_2/edit',
    );
    const preview = screen.getByRole('list', {
      name: 'Selected file IDs preview',
    });
    expect(within(preview).getByText('File_1')).toBeVisible();
    expect(within(preview).getByText('Doc_2')).toBeVisible();
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    await user.click(
      screen.getByRole('button', { name: 'Connect selected files' }),
    );
    expect(daemon.connectDrive).toHaveBeenCalledWith('agent-a', {
      selectedFileIds: ['File_1', 'Doc_2'],
    });
    const consent = await screen.findByRole('link', {
      name: 'Finish Google consent',
    });
    expect(consent).toHaveAttribute(
      'href',
      'https://accounts.google.com/o/oauth2/v2/auth?state=opaque',
    );
    expect(consent).toHaveAttribute('rel', 'noopener noreferrer');
    expect(
      screen.queryByLabelText('Selected Drive files'),
    ).not.toBeInTheDocument();
  });
  it('rejects foreign links and duplicate IDs before consent', async () => {
    const user = userEvent.setup();
    render(<GoogleDriveCard agentId="agent-a" />);
    const input = screen.getByLabelText('Selected Drive files');
    await user.type(input, 'https://evil.test/file/d/one/view');
    expect(
      screen.getByRole('button', { name: 'Connect selected files' }),
    ).toBeDisabled();
    expect(daemon.connectDrive).not.toHaveBeenCalled();
    await user.clear(input);
    await user.type(input, 'one\none');
    expect(
      screen.getByRole('button', { name: 'Connect selected files' }),
    ).toBeDisabled();
    expect(screen.getByText(/Duplicate selected files/)).toBeVisible();
  });
  it('keeps active selection immutable and makes local disconnect impact explicit', async () => {
    const user = userEvent.setup();
    const c = driveFixture();
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    vi.mocked(daemon.disconnectDrive).mockResolvedValue(undefined);
    render(<GoogleDriveCard agentId="agent-a" />);
    expect(await screen.findByText('owner@example.test')).toBeVisible();
    expect(
      screen.queryByLabelText('Selected Drive files'),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/Disconnect and reconnect to change selected files/),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Disconnect Drive' }));
    expect(daemon.disconnectDrive).not.toHaveBeenCalled();
    expect(
      screen.getByText(/Only this agent’s Drive access is removed locally/),
    ).toBeVisible();
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: { ...c, status: 'disconnected', cleanupPending: true },
    });
    await user.click(
      screen.getByRole('button', { name: 'Confirm local disconnect' }),
    );
    expect(daemon.disconnectDrive).toHaveBeenCalledWith('agent-a', c.id);
    expect(
      await screen.findByText(/Credential cleanup is pending/),
    ).toBeVisible();
    expect(
      screen.getByText(/may also disconnect Gmail and Calendar/),
    ).toBeVisible();
    expect(
      screen.getByRole('link', { name: 'Google account app access' }),
    ).toHaveAttribute('href', 'https://myaccount.google.com/connections');
  });
  it('clears agent input and old pending consent when switching agents', async () => {
    const user = userEvent.setup();
    const c = driveFixture({ status: 'pairing' });
    vi.mocked(daemon.connectDrive).mockResolvedValue({
      connector: c,
      consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=old',
    });
    const view = render(<GoogleDriveCard agentId="agent-a" />);
    await user.type(screen.getByLabelText('Selected Drive files'), 'File_1');
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    await user.click(
      screen.getByRole('button', { name: 'Connect selected files' }),
    );
    expect(
      await screen.findByRole('link', { name: 'Finish Google consent' }),
    ).toBeVisible();
    let finishSwitchedStatus!: () => void;
    vi.mocked(daemon.driveStatus).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSwitchedStatus = () =>
            resolve({ configured: true, connector: null });
        }),
    );
    view.rerender(<GoogleDriveCard agentId="agent-b" />);
    expect(
      screen.queryByRole('link', { name: 'Finish Google consent' }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText('Selected Drive files')).toHaveValue('');
    await act(async () => {
      finishSwitchedStatus();
    });
    expect(
      screen.getByRole('button', { name: 'Refresh Drive status' }),
    ).toBeEnabled();
  });

  it('reconnects the exact saved selection and does not confuse retired credential cleanup with revocation', async () => {
    const user = userEvent.setup();
    const c = driveFixture({
      status: 'reauthRequired',
      cleanupPending: true,
      selectedFileIds: ['File_1', 'Other_2'],
    });
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    vi.mocked(daemon.connectDrive).mockResolvedValue({
      connector: { ...c, status: 'pairing' },
      consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=new',
    });
    render(<GoogleDriveCard agentId="agent-a" />);
    const reconnect = await screen.findByRole('button', {
      name: 'Reconnect selected files',
    });
    await waitFor(() => expect(reconnect).toBeEnabled());
    expect(
      screen.getByText(/Previous credentials are queued for local cleanup/),
    ).toBeVisible();
    expect(
      screen.queryByText(/Drive authority is revoked locally/),
    ).not.toBeInTheDocument();
    await user.click(reconnect);
    expect(daemon.connectDrive).toHaveBeenCalledWith('agent-a', {
      selectedFileIds: ['File_1', 'Other_2'],
    });
  });

  it('restarts denied or expired pairing even while the old consent link exists, replacing it for the saved IDs', async () => {
    const user = userEvent.setup();
    const c = driveFixture({
      status: 'pairing',
      selectedFileIds: ['File_1', 'Other_2'],
    });
    const oldUrl = 'https://accounts.google.com/o/oauth2/v2/auth?state=expired';
    const newUrl = 'https://accounts.google.com/o/oauth2/v2/auth?state=fresh';
    vi.mocked(daemon.connectDrive)
      .mockResolvedValueOnce({ connector: c, consentUrl: oldUrl })
      .mockResolvedValueOnce({ connector: c, consentUrl: newUrl });
    render(<GoogleDriveCard agentId="agent-a" />);
    await user.type(
      screen.getByLabelText('Selected Drive files'),
      'File_1\nOther_2',
    );
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    await user.click(
      screen.getByRole('button', { name: 'Connect selected files' }),
    );
    expect(
      await screen.findByRole('link', { name: 'Finish Google consent' }),
    ).toHaveAttribute('href', oldUrl);
    // A denial/expiry leaves durable Pairing intact; refreshing cannot
    // establish that the retained provider URL is still usable.
    await user.click(
      screen.getByRole('button', { name: 'Refresh Drive status' }),
    );
    const restart = screen.getByRole('button', {
      name: 'Restart Google consent',
    });
    await waitFor(() => expect(restart).toBeEnabled());
    await user.click(restart);
    await waitFor(() =>
      expect(
        screen.getByRole('link', { name: 'Finish Google consent' }),
      ).toHaveAttribute('href', newUrl),
    );
    expect(daemon.connectDrive).toHaveBeenCalledTimes(2);
    expect(vi.mocked(daemon.connectDrive).mock.calls).toEqual(
      Array(2).fill(['agent-a', { selectedFileIds: ['File_1', 'Other_2'] }]),
    );
    expect(
      screen.queryByLabelText('Selected Drive files'),
    ).not.toBeInTheDocument();
  });
});
