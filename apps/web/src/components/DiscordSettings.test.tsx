import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { discordFixture } from '../test/discord';
import { DiscordSettings } from './DiscordSettings';

function props() {
  return {
    agentId: 'agent-a',
    connectors: [],
    busy: null,
    error: null,
    loading: false,
    create: vi.fn(async () => false),
    activate: vi.fn(async () => true),
    replace: vi.fn(async () => false),
    setEnabled: vi.fn(async () => true),
    restart: vi.fn(async () => true),
    retry: vi.fn(async () => true),
    suppress: vi.fn(async () => true),
    disconnect: vi.fn(async () => true),
    refresh: vi.fn(async () => {}),
    openConversation: vi.fn(),
  };
}

describe('DiscordSettings', () => {
  it('fp02 Discord clears unsubmitted secrets but retains selected IDs on reconnect', async () => {
    const user = userEvent.setup();
    const p = props();
    const view = render(<DiscordSettings {...p} online epoch={1} />);
    await user.type(
      screen.getByLabelText('Discord bot token'),
      'synthetic-secret',
    );
    await user.type(screen.getByLabelText('Server ID'), '123');
    view.rerender(<DiscordSettings {...p} online={false} epoch={1} />);
    expect(screen.getByLabelText('Discord bot token')).toHaveValue('');
    expect(screen.getByLabelText('Server ID')).toHaveValue('123');
    expect(
      screen.getByRole('button', { name: 'Refresh Discord status' }),
    ).toBeDisabled();
  });

  it('requires selected IDs and owner privacy confirmation, clears submitted token immediately', async () => {
    const user = userEvent.setup();
    const p = props();
    let finish!: (value: boolean) => void;
    p.create.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(<DiscordSettings {...p} />);
    expect(screen.getByText(/enable Message Content intent/)).toBeVisible();
    expect(
      screen.getByText(/View Channel, Read Message History, and Send Messages/),
    ).toBeVisible();
    const token = screen.getByLabelText('Discord bot token');
    expect(token).toHaveAttribute('type', 'password');
    await user.type(token, 'secret');
    await user.type(screen.getByLabelText('Server ID'), '3');
    await user.type(screen.getByLabelText('Channel ID'), '2');
    const allowed = screen.getByLabelText('Approved user IDs');
    await user.type(allowed, '04');
    expect(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    ).toBeDisabled();
    await user.clear(allowed);
    await user.type(allowed, '4, 5');
    expect(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    ).toBeDisabled();
    await user.click(screen.getByRole('checkbox'));
    await user.click(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    );
    expect(p.create).toHaveBeenCalledWith({
      botToken: 'secret',
      guildId: '3',
      channelId: '2',
      approvedUserIds: ['4', '5'],
      privateChannelConfirmed: true,
    });
    expect(token).toHaveValue('');
    finish(false);
  });
  it('clears credentials and confirmations when switching agents and submitting replacement', async () => {
    const user = userEvent.setup();
    const p = props();
    const view = render(<DiscordSettings {...p} />);
    await user.type(screen.getByLabelText('Discord bot token'), 'secret');
    await user.click(screen.getByRole('checkbox'));
    view.rerender(<DiscordSettings {...p} agentId="agent-b" />);
    expect(screen.getByLabelText('Discord bot token')).toHaveValue('');
    expect(screen.getByRole('checkbox')).not.toBeChecked();
    view.rerender(<DiscordSettings {...p} connectors={[discordFixture()]} />);
    await user.click(screen.getByRole('button', { name: 'Replace token' }));
    await user.type(
      screen.getByLabelText('Replacement Discord bot token'),
      'replacement',
    );
    await user.click(
      screen.getByRole('button', { name: 'Save replacement token' }),
    );
    expect(p.replace).toHaveBeenCalledWith('dc-1', 'replacement');
    expect(
      screen.queryByLabelText('Replacement Discord bot token'),
    ).not.toBeInTheDocument();
  });
  it('activates saved disabled setup and opens the native conversation', async () => {
    const user = userEvent.setup();
    const p = props();
    const c = discordFixture();
    const view = render(<DiscordSettings {...p} connectors={[c]} />);
    expect(screen.getByText(/new messages after activation/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Activate' }));
    expect(p.activate).toHaveBeenCalledWith(c.id);
    view.rerender(
      <DiscordSettings
        {...p}
        connectors={[{ ...c, enabled: true, status: 'ready' }]}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Open conversation' }));
    expect(p.openConversation).toHaveBeenCalledWith({
      ...c,
      enabled: true,
      status: 'ready',
    });
    await user.click(screen.getByRole('button', { name: 'Disable' }));
    expect(p.setEnabled).toHaveBeenCalledWith(c.id, false);
  });
  it('blocks retry for uncertain output and requires review confirmation to suppress without resending', async () => {
    const user = userEvent.setup();
    const p = props();
    render(
      <DiscordSettings
        {...p}
        connectors={[
          discordFixture({
            enabled: true,
            status: 'degraded',
            unknownOutbound: 1,
            pendingOutbound: 2,
            deliveredMessageIds: ['999'],
            warning: 'backlog_limit',
          }),
        ]}
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Retry safe delivery' }),
    ).toBeDisabled();
    expect(screen.getByText(/999/)).toBeVisible();
    expect(screen.getByText(/catch-up limit/)).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Review uncertain delivery' }),
    );
    expect(p.suppress).not.toHaveBeenCalled();
    expect(screen.getByText(/Check Discord/)).toBeVisible();
    await user.click(
      screen.getByRole('button', {
        name: 'I checked Discord — suppress unsent remainder',
      }),
    );
    expect(p.suppress).toHaveBeenCalledWith('dc-1');
    expect(p.retry).not.toHaveBeenCalled();
  });
});
