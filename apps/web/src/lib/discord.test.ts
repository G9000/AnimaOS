import { describe, expect, it } from 'vitest';
import {
  discordApprovedUsers,
  discordCanReply,
  discordStatusLabel,
  discordWarning,
  isDiscordSnowflake,
  safeDiscordError,
} from './discord';
import { discordFixture } from '../test/discord';

describe('Discord selected authority and delivery state', () => {
  it('distinguishes gateway connection progress and missing intent from readiness', () => {
    expect(
      discordStatusLabel(
        'connecting' as Parameters<typeof discordStatusLabel>[0],
      ),
    ).toBe('Connecting…');
    expect(
      discordStatusLabel(
        'reconnecting' as Parameters<typeof discordStatusLabel>[0],
      ),
    ).toBe('Reconnecting…');
    expect(discordStatusLabel('ready')).toBe('Online · listening');
    expect(discordWarning('gatewayDisallowedIntents')).toContain(
      'Message Content',
    );
  });
  it('shows actionable allowlisted setup errors without displaying provider bodies', () => {
    for (const [code, status, expected] of [
      ['discord_permission_denied', 422, 'View Channel'],
      ['discord_channel_not_found', 422, 'could not find'],
      ['discord_rate_limited', 503, 'limiting requests'],
      ['discord_invalid_response', 502, 'unexpected response'],
      ['discord_network_error', 503, 'could not reach Discord'],
      ['connector_token_invalid', 422, 'bot token'],
    ] as const) {
      const result = safeDiscordError({
        status,
        body: { code, error: 'provider-secret' },
      });
      expect(result).toContain(expected);
      expect(result).not.toContain('provider-secret');
    }
    expect(
      safeDiscordError({
        status: 503,
        body: { code: 'provider-secret', error: 'provider-secret' },
      }),
    ).toBe(
      'Discord request failed. Refresh status and check the connection setup.',
    );
  });
  it('distinguishes duplicate channel setup from uncertain delivery without exposing backend text', () => {
    for (const [code, expected] of [
      ['discord_channel_already_connected', 'already connected to this agent'],
      ['discord_delivery_uncertain', 'Review uncertain delivery'],
      ['agent_busy', 'run in progress'],
    ]) {
      const message = safeDiscordError({
        status: 409,
        body: { code, error: 'provider-secret' },
      });
      expect(message).toContain(expected);
      expect(message).not.toContain('provider-secret');
      expect(message).not.toContain('Replacement tokens');
    }
  });
  it('keeps canonical u64 IDs as strings and rejects empty, duplicate or out-of-range selection', () => {
    expect(isDiscordSnowflake('18446744073709551615')).toBe(true);
    for (const invalid of [
      '0',
      '01',
      '-1',
      '1e3',
      '18446744073709551616',
      '9999999999999999999999',
    ])
      expect(isDiscordSnowflake(invalid)).toBe(false);
    expect(discordApprovedUsers('1, 18446744073709551615')).toEqual([
      '1',
      '18446744073709551615',
    ]);
    for (const invalid of [
      '',
      '1,1',
      '01',
      Array.from({ length: 17 }, (_, i) => String(i + 1)).join(','),
    ])
      expect(discordApprovedUsers(invalid)).toBeNull();
  });
  it('allows text replies only through a ready selected connector without uncertain sends', () => {
    const c = discordFixture({ enabled: true, status: 'ready' });
    expect(discordCanReply(c)).toBe(true);
    expect(discordCanReply({ ...c, unknownOutbound: 1 })).toBe(false);
    expect(discordCanReply({ ...c, status: 'degraded' })).toBe(false);
    expect(discordCanReply({ ...c, enabled: false })).toBe(false);
    expect(discordCanReply(null)).toBe(false);
    expect(discordWarning('provider raw secret')).not.toContain('secret');
  });
});
