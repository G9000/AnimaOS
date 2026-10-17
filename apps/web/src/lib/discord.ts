import type {
  DiscordConnector,
  DiscordConnectorStatus,
} from '@animaOS-SWARM/sdk';

const STATUS: Record<DiscordConnectorStatus, string> = {
  disabled: 'Saved · disabled',
  ready: 'Online · listening',
  connecting: 'Connecting…',
  reconnecting: 'Reconnecting…',
  credentialRequired: 'Token required',
  error: 'Setup needs attention',
  degraded: 'Paused · needs attention',
  reconciling: 'Reconciling',
};
export function discordStatusLabel(status: DiscordConnectorStatus): string {
  return STATUS[status] ?? 'Unavailable';
}
export function isDiscordSnowflake(value: string): boolean {
  return (
    /^[1-9]\d{0,19}$/.test(value) && BigInt(value) <= 18446744073709551615n
  );
}
export function discordApprovedUsers(value: string): string[] | null {
  const ids = value
    .trim()
    .split(/[\s,]+/)
    .filter(Boolean);
  return ids.length >= 1 &&
    ids.length <= 16 &&
    ids.every(isDiscordSnowflake) &&
    new Set(ids).size === ids.length
    ? ids
    : null;
}
export function discordCanReply(connector: DiscordConnector | null): boolean {
  return (
    connector !== null &&
    connector.enabled &&
    connector.status === 'ready' &&
    connector.unknownOutbound === 0
  );
}

/** Only static, locally written messages reach setup UI, never provider bodies. */
export function safeDiscordError(error: unknown): string {
  const body =
    typeof error === 'object' && error !== null && 'body' in error
      ? error.body
      : null;
  const code =
    typeof body === 'object' && body !== null && 'code' in body
      ? body.code
      : null;
  // Never render backend/provider text, even for recognized error codes.
  switch (code) {
    case 'connector_token_invalid':
      return 'Discord rejected the bot token. Enter a valid token from the Bot page in the Discord Developer Portal.';
    case 'discord_permission_denied':
      return 'Discord denied access. Install the bot in the selected server and grant View Channel, Read Message History, and Send Messages for this channel.';
    case 'discord_channel_not_found':
      return 'Discord could not find the selected channel. Check the channel ID and the bot’s access to that channel.';
    case 'discord_rate_limited':
      return 'Discord is limiting requests. Wait before retrying setup.';
    case 'discord_invalid_response':
      return 'Discord returned an unexpected response. Select a regular server text channel; threads, forums, and voice channels are unsupported.';
    case 'discord_network_error':
      return 'The daemon could not reach Discord. Check the daemon computer’s network, proxy, firewall, and TLS connection.';
    case 'discord_provider_unavailable':
      return 'Discord could not complete the request. Wait and try again; check the daemon log for the failed setup step and HTTP status.';
  }
  const status =
    typeof error === 'object' && error !== null && 'status' in error
      ? error.status
      : null;
  if (status === 400)
    return 'Check the server, channel, approved user IDs, and private-channel confirmation.';
  if (status === 409)
    return 'This change is blocked. Check uncertain delivery or an existing connection. Replacement tokens must belong to the same bot.';
  if (status === 422)
    return 'Discord rejected the setup. Check the bot token and selected channel permissions.';
  if (status === 401 || status === 403)
    return 'Owner access is required to change this connection.';
  return 'Discord request failed. Refresh status and check the connection setup.';
}
export function discordWarning(warning: string | null): string | null {
  if (!warning) return null;
  const key = warning.replace(/[_\s-]/g, '').toLowerCase();
  if (key.includes('disallowedintents'))
    return 'Enable Message Content intent in Discord Developer Portal → Bot → Privileged Gateway Intents, then restart this connection.';
  if (key.includes('gatewayidentity') || key.includes('gatewayauthentication'))
    return 'Discord rejected the Gateway bot identity or token. Replace the token with one for this connection’s bot.';
  if (key.includes('gatewaydisconnected'))
    return 'The live Discord connection was interrupted. Anima is reconnecting and reconciling missed messages.';
  if (key.includes('gateway'))
    return 'The live Discord connection needs attention. Check the bot configuration and restart this connection.';
  if (key.includes('backlog') || key.includes('capacity'))
    return 'The catch-up limit was reached. Intake is paused to avoid skipping messages; inspect the channel before reconnecting.';
  if (key.includes('permission'))
    return 'Check View Channel, Read Message History, and Send Messages permissions for the selected channel.';
  if (key.includes('content') || key.includes('unsupported'))
    return 'Some messages have no readable text. Check Message Content intent; attachments-only and unsupported messages are not dispatched.';
  if (key.includes('authentication') || key.includes('credential'))
    return 'The bot token needs attention. Replace it with a token for the same bot.';
  if (key.includes('ratelimit'))
    return 'Discord is limiting requests. The daemon waits for the provider retry time.';
  if (key.includes('uncertain') || key.includes('unknown'))
    return 'Delivery is uncertain. Check Discord before suppressing the unsent remainder.';
  return 'This connection needs attention. Refresh status and check the bot setup and channel.';
}
