import type { DiscordConnector } from '@animaos/sdk';

export function discordFixture(
  overrides: Partial<DiscordConnector> = {},
): DiscordConnector {
  return {
    id: 'dc-1',
    agentId: 'agent-a',
    roomId: 'discord:dc-1',
    type: 'discord',
    bot: { id: '1', username: 'anima_bot', displayName: 'Anima' },
    channel: { id: '2', guildId: '3', name: 'private-chat', kind: 'guildText' },
    approvedUserIds: ['4'],
    enabled: false,
    status: 'disabled',
    createdAtMs: 1,
    updatedAtMs: 1,
    pendingInbound: 0,
    pendingOutbound: 0,
    unknownOutbound: 0,
    deliveredMessageIds: [],
    warning: null,
    ...overrides,
  };
}
