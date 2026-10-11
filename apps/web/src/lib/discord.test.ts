import { describe, expect, it } from 'vitest';
import {
  discordApprovedUsers,
  discordCanReply,
  discordWarning,
  isDiscordSnowflake,
} from './discord';
import { discordFixture } from '../test/discord';

describe('Discord selected authority and delivery state', () => {
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
