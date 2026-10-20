import { act, render, screen, waitFor } from '@testing-library/react';
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
    check: vi.fn(async () => ({
      applicationId: '1',
      bot: { id: '9', username: 'helper', displayName: null },
      ownerUserIds: ['4'],
      messageContentEnabled: true,
      inviteUrl: 'https://discord.com/oauth2/authorize?client_id=1',
    })),
    configure: vi.fn(async () => true),
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
  it('opens the selected channel conversation when several channels share an agent', async () => {
    const user = userEvent.setup();
    const p = props();
    const connectors = ['22', '55', '66'].map((id) =>
      discordFixture({
        id: `discord-${id}`,
        roomId: `discord:discord-${id}`,
        channel: {
          id,
          guildId: '33',
          name: `channel-${id}`,
          kind: 'guildText',
        },
      }),
    );
    render(<DiscordSettings {...p} connectors={connectors} />);
    const open = screen.getAllByRole('button', { name: 'Open conversation' });
    for (const [index, button] of open.entries()) {
      await user.click(button);
      expect(p.openConversation).toHaveBeenNthCalledWith(
        index + 1,
        connectors[index],
      );
    }
    expect(
      screen.getByText(
        /Replies stay in the channel or thread where each message arrives/,
      ),
    ).toBeVisible();
    expect(screen.getByText('Connect a Discord bot')).toBeVisible();
  });
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

  it('validates a bot, offers an invite and saves without mandatory channel IDs or privacy confirmation', async () => {
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
    const token = screen.getByLabelText('Discord bot token');
    expect(token).toHaveAttribute('type', 'password');
    await user.type(token, 'secret');
    expect(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    ).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Check bot token' }));
    expect(
      await screen.findByRole('link', { name: 'Add to Discord' }),
    ).toHaveAttribute(
      'href',
      'https://discord.com/oauth2/authorize?client_id=1',
    );
    expect(screen.getByText(/Message Content intent is enabled/)).toBeVisible();
    expect(
      screen.queryByText(/I confirm this selected channel/),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    );
    expect(p.create).toHaveBeenCalledWith({
      botToken: 'secret',
      approvedUserIds: [],
      settings: {
        allowedChannelIds: [],
        freeResponseChannelIds: [],
        approvedRoleIds: [],
        homeChannelId: null,
        requireMention: true,
        autoThread: true,
        threadRequireMention: false,
      },
    });
    expect(token).toHaveValue('');
    finish(false);
  });
  it('clears credentials and verification when switching agents and submitting replacement', async () => {
    const user = userEvent.setup();
    const p = props();
    const view = render(<DiscordSettings {...p} />);
    await user.type(screen.getByLabelText('Discord bot token'), 'secret');
    await user.click(screen.getByRole('button', { name: 'Check bot token' }));
    view.rerender(<DiscordSettings {...p} agentId="agent-b" />);
    expect(screen.getByLabelText('Discord bot token')).toHaveValue('');
    expect(
      screen.queryByRole('link', { name: 'Add to Discord' }),
    ).not.toBeInTheDocument();
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
  it('discards token checks after token edits and agent changes', async () => {
    const user = userEvent.setup();
    const p = props();
    const checked = await p.check();
    let finish!: (value: typeof checked) => void;
    p.check.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const view = render(<DiscordSettings {...p} />);
    await user.type(screen.getByLabelText('Discord bot token'), 'old-secret');
    await user.click(screen.getByRole('button', { name: 'Check bot token' }));
    await user.clear(screen.getByLabelText('Discord bot token'));
    await user.type(screen.getByLabelText('Discord bot token'), 'new-secret');
    await act(async () => {
      finish(checked);
    });
    expect(
      screen.queryByRole('link', { name: 'Add to Discord' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Save Discord connection' }),
    ).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Check bot token' }));
    view.rerender(<DiscordSettings {...p} agentId="agent-b" />);
    await act(async () => {
      finish(checked);
    });
    expect(
      screen.queryByRole('link', { name: 'Add to Discord' }),
    ).not.toBeInTheDocument();
  });
  it('edits bot access and response rules and opens distinct channel histories', async () => {
    const user = userEvent.setup();
    const p = props();
    const c = discordFixture({
      channel: null,
      botWide: true,
      conversations: [
        {
          channelId: '22',
          guildId: '33',
          parentChannelId: '11',
          name: 'topic',
          roomId: 'discord:dc-1:22',
        },
      ],
    });
    render(<DiscordSettings {...p} connectors={[c]} />);
    await user.click(screen.getByRole('button', { name: 'Open #topic' }));
    expect(p.openConversation).toHaveBeenCalledWith(c, 'discord:dc-1:22');
    await user.click(screen.getByText('Edit Discord rules'));
    await user.type(screen.getByLabelText('Approved role IDs'), '55');
    await user.click(screen.getByLabelText('Create a thread for mentions'));
    await user.type(screen.getByLabelText('Home channel ID (optional)'), '66');
    await user.click(
      screen.getByRole('button', { name: 'Save Discord rules' }),
    );
    await waitFor(() =>
      expect(p.configure).toHaveBeenCalledWith(
        'dc-1',
        {
          allowedChannelIds: [],
          freeResponseChannelIds: [],
          approvedRoleIds: ['55'],
          homeChannelId: '66',
          requireMention: true,
          autoThread: false,
          threadRequireMention: false,
        },
        ['4'],
      ),
    );
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
