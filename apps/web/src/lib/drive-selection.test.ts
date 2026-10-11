import { describe, expect, it } from 'vitest';
import { parseDriveSelection, googleDriveConsentUrl } from './drive-selection';

describe('selected Drive input', () => {
  it('extracts canonical IDs from exact Google links or plain IDs without fetching', () => {
    expect(
      parseDriveSelection(
        'Plain_1\nhttps://docs.google.com/document/d/Doc-2/edit?usp=sharing\nhttps://drive.google.com/file/d/Text_3/view\nhttps://drive.google.com/open?id=Other-4',
      ),
    ).toEqual({ ids: ['Plain_1', 'Doc-2', 'Text_3', 'Other-4'], error: null });
  });
  it.each([
    '',
    'file/other',
    'https://docs.google.com.attacker.test/document/d/a/edit',
    'http://docs.google.com/document/d/a/edit',
    'https://evil.test/?id=a',
    'https://drive.google.com/drive/folders/a',
    'https://drive.google.com/open?id=a&id=b',
    'https://owner@docs.google.com/document/d/a/edit',
    'https://docs.google.com/document/d/a/evil/path',
    'javascript:alert(1)',
    'a\na',
    'a\nhttps://drive.google.com/open?id=a',
    'a'.repeat(257),
    Array.from({ length: 17 }, (_, i) => `file${i}`).join('\n'),
  ])(
    'rejects invalid, ambiguous, duplicate or excessive selections (%s)',
    (value) => {
      expect(parseDriveSelection(value).ids).toBeNull();
      expect(parseDriveSelection(value).error).toBeTruthy();
    },
  );
  it('allows only Google HTTPS OAuth authorization links, excluding redirects and credentials', () => {
    const valid = 'https://accounts.google.com/o/oauth2/v2/auth?state=opaque';
    expect(googleDriveConsentUrl(valid)).toBe(valid);
    for (const invalid of [
      'javascript:alert(1)',
      'https://accounts.google.com.attacker.test/o/oauth2/v2/auth',
      'http://accounts.google.com/o/oauth2/v2/auth',
      'https://accounts.google.com/Logout?continue=https://evil.test',
      'https://user@accounts.google.com/o/oauth2/v2/auth',
    ])
      expect(googleDriveConsentUrl(invalid)).toBeNull();
  });
});
