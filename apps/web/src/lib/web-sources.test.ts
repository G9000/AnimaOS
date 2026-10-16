import { describe, expect, it } from 'vitest';
import { toolWebSources, webSources } from './web-sources';

describe('m83 source projection validation', () => {
  it('preserves complete URLs and literal titles at the UTF-8 field limits', () => {
    const prefix = 'https://example.com/?q=';
    const sources = [
      {
        title: '🙂'.repeat(64),
        url: prefix + 'a'.repeat(2048 - prefix.length),
      },
    ];
    expect(webSources(sources)).toEqual(sources);
    expect(
      webSources([{ ...sources[0], title: sources[0].title + 'a' }]),
    ).toEqual([]);
    expect(webSources([{ ...sources[0], url: sources[0].url + 'a' }])).toEqual(
      [],
    );
    expect(
      webSources([
        {
          title: '<b>literal title</b>',
          url: 'https://example.com/exact?x=%2F&y=2#part',
        },
      ]),
    ).toEqual([
      {
        title: '<b>literal title</b>',
        url: 'https://example.com/exact?x=%2F&y=2#part',
      },
    ]);
  });

  it('counts JSON escaping and UTF-8 against the complete array budget', () => {
    const within = Array.from({ length: 5 }, (_, i) => ({
      title: '界'.repeat(80),
      url: `https://example.com/${i}?q=${'a'.repeat(1000)}`,
    }));
    expect(webSources(within)).toEqual(within);
    const escaped = within.map((source) => ({
      ...source,
      title: '"'.repeat(256),
      url: source.url + 'a'.repeat(500),
    }));
    expect(
      new TextEncoder().encode(JSON.stringify(escaped)).length,
    ).toBeGreaterThan(8192);
    expect(webSources(escaped)).toEqual([]);
  });

  it('rejects duplicate records, unexpected fields and noncanonical projection shapes', () => {
    const source = { title: 'Returned', url: 'https://example.com/source' };
    expect(webSources([source, source])).toEqual([]);
    expect(
      webSources([{ ...source, excerpt: 'not a public source field' }]),
    ).toEqual([]);
    expect(webSources('[]')).toEqual([]);
    expect(webSources(undefined)).toEqual([]);
  });

  it.each([
    'http://0.0.0.0/',
    'http://0x7f000001/',
    'http://100.64.1.1/',
    'http://224.0.0.1/',
    'http://[::]/',
    'http://[ff02::1]/',
    'https://example.com/a b',
    'https:example.com',
    'http://192.0.0.1/',
    'http://192.0.2.1/',
    'http://198.51.100.1/',
    'http://203.0.113.1/',
    'http://[2001:db8::1]/',
    'http://[fec0::1]/',
    'http://[::ffff:8.8.8.8]/',
  ])('rejects nonpublic literal %s after URL parsing', (url) => {
    expect(webSources([{ title: 'Nonpublic', url }])).toEqual([]);
  });

  it('requires both native search identity and success at every rendering boundary', () => {
    const sources = [{ title: 'Returned', url: 'https://example.com/source' }];
    expect(toolWebSources('web_search', 'success', sources)).toEqual(sources);
    expect(toolWebSources('web_fetch', 'success', sources)).toEqual([]);
    expect(toolWebSources('web_search', 'error', sources)).toEqual([]);
    expect(toolWebSources('web_search', 'running', sources)).toEqual([]);
  });
});
