import { describe, expect, it } from 'vitest';
import {
  createDaemonClient,
  type CreateDiscordConnectorInput,
} from './index.js';

describe('Discord owner transport', () => {
  it('checks a bot and saves bot-wide access and response rules without a selected channel', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const checked = {
      applicationId: '1',
      bot: { id: '2', username: 'helper', displayName: null },
      ownerUserIds: ['3'],
      messageContentEnabled: true,
      inviteUrl: 'https://discord.com/oauth2/authorize?client_id=1',
    };
    const connector = {
      id: 'c',
      channel: null,
      botWide: true,
      conversations: [],
    };
    const api = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json(
          String(url).endsWith('/check') ? checked : { connector },
        );
      },
    }).connectors;
    expect(await api.checkDiscordBot('a /?', 'secret')).toEqual(checked);
    const settings = {
      allowedChannelIds: [],
      freeResponseChannelIds: ['4'],
      approvedRoleIds: ['5'],
      homeChannelId: null,
      requireMention: true,
      autoThread: true,
      threadRequireMention: false,
    };
    expect(
      await api.createDiscordConnector('a /?', {
        botToken: 'secret',
        settings,
      }),
    ).toEqual(connector);
    expect(
      await api.configureDiscordConnector('a /?', 'c /?', settings, ['3']),
    ).toEqual(connector);
    expect(
      requests.map((r) => [
        r.url,
        r.init?.method,
        JSON.parse(r.init!.body as string),
      ]),
    ).toEqual([
      [
        '/api/agents/a%20%2F%3F/connectors/discord/check',
        'POST',
        { botToken: 'secret' },
      ],
      [
        '/api/agents/a%20%2F%3F/connectors/discord',
        'POST',
        { botToken: 'secret', settings },
      ],
      [
        '/api/agents/a%20%2F%3F/connectors/discord/c%20%2F%3F/settings',
        'PUT',
        { settings, approvedUserIds: ['3'] },
      ],
    ]);
  });
  it('encodes every scope and keeps credentials and acknowledgement in bodies', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const connector = { id: 'c /?', type: 'discord' };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json({
          connector,
          connectors: [connector],
          deleted: true,
        });
      },
    });
    const input: CreateDiscordConnectorInput = {
      botToken: 'secret-token',
      guildId: '1',
      channelId: '2',
      approvedUserIds: ['3'],
      privateChannelConfirmed: true,
    };
    const api = client.connectors;
    expect(await api.discordConnectors('a /?')).toEqual([connector]);
    expect(await api.createDiscordConnector('a /?', input)).toEqual(connector);
    expect(await api.activateDiscordConnector('a /?', 'c /?')).toEqual(
      connector,
    );
    expect(
      await api.replaceDiscordCredential('a /?', 'c /?', 'replacement'),
    ).toEqual(connector);
    expect(await api.setDiscordEnabled('a /?', 'c /?', false)).toEqual(
      connector,
    );
    expect(await api.restartDiscordConnector('a /?', 'c /?')).toEqual(
      connector,
    );
    expect(await api.retryDiscordDelivery('a /?', 'c /?')).toEqual(connector);
    expect(await api.suppressDiscordUnknownDelivery('a /?', 'c /?')).toEqual(
      connector,
    );
    await expect(
      api.deleteDiscordConnector('a /?', 'c /?'),
    ).resolves.toBeUndefined();
    const base = '/api/agents/a%20%2F%3F/connectors/discord';
    const scoped = `${base}/c%20%2F%3F`;
    expect(requests.map((r) => r.url)).toEqual([
      base,
      base,
      `${scoped}/activate`,
      `${scoped}/credential`,
      scoped,
      `${scoped}/restart`,
      `${scoped}/retry`,
      `${scoped}/outbound/suppress`,
      scoped,
    ]);
    expect(requests.map((r) => r.init?.method)).toEqual([
      undefined,
      'POST',
      'POST',
      'PUT',
      'PATCH',
      'POST',
      'POST',
      'POST',
      'DELETE',
    ]);
    expect(JSON.parse(requests[1].init!.body as string)).toEqual(input);
    expect(JSON.parse(requests[3].init!.body as string)).toEqual({
      botToken: 'replacement',
    });
    expect(JSON.parse(requests[4].init!.body as string)).toEqual({
      enabled: false,
    });
    expect(JSON.parse(requests[7].init!.body as string)).toEqual({
      acknowledgeUncertainty: true,
    });
    expect(requests.every((r) => !r.url.includes('secret-token'))).toBe(true);
  });
  it('does not retry unknown delivery or credential failures', async () => {
    let calls = 0;
    const api = createDaemonClient({
      baseUrl: '',
      fetch: async () => {
        calls++;
        return Response.json({ error: 'unknown_delivery' }, { status: 409 });
      },
    }).connectors;
    await expect(api.retryDiscordDelivery('a', 'c')).rejects.toMatchObject({
      status: 409,
    });
    await expect(
      api.replaceDiscordCredential('a', 'c', 'secret'),
    ).rejects.toMatchObject({ status: 409 });
    expect(calls).toBe(2);
  });
});
