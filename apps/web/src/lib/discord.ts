import type {
  DiscordConnector,
  DiscordConnectorStatus,
} from '@animaOS-SWARM/sdk';

const STATUS: Record<DiscordConnectorStatus, string> = {
  disabled: 'Saved · disabled',
  ready: 'Listening',
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
