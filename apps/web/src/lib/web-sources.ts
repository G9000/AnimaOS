import type { WebSource } from '@animaOS-SWARM/sdk';

const utf8 = new TextEncoder();
const byteLength = (value: string) => utf8.encode(value).length;

function publicHost(hostname: string): boolean {
  const host = hostname.toLowerCase().replace(/\.$/, '');
  if (!host || host === 'localhost' || host.endsWith('.localhost'))
    return false;
  if (host.startsWith('[')) {
    const address = host.slice(1, -1);
    // Accept global unicast literals only; this also excludes mapped IPv4,
    // unspecified, loopback, private, link-local and multicast addresses.
    const first = Number.parseInt(address.split(':')[0], 16);
    return (
      first >= 0x2000 && first <= 0x3fff && !address.startsWith('2001:db8:')
    );
  }
  if (/^\d+\.\d+\.\d+\.\d+$/.test(host)) {
    const [a, b, c] = host.split('.').map(Number);
    return !(
      a === 0 ||
      a === 10 ||
      a === 127 ||
      a >= 224 ||
      (a === 100 && b >= 64 && b <= 127) ||
      (a === 169 && b === 254) ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 192 && (b === 168 || (b === 0 && (c === 0 || c === 2)))) ||
      (a === 198 && (b === 18 || b === 19 || (b === 51 && c === 100))) ||
      (a === 203 && b === 0 && c === 113)
    );
  }
  return true;
}

/** Syntax/literal checks only. No DNS lookup or background request occurs. */
function safeSourceUrl(value: string): boolean {
  if (
    byteLength(value) > 2048 ||
    !/^https?:\/\//i.test(value) ||
    /[\u0000-\u0020\u007f-\u009f]/u.test(value)
  )
    return false;
  try {
    const parsed = new URL(value);
    return (
      (parsed.protocol === 'http:' || parsed.protocol === 'https:') &&
      !parsed.username &&
      !parsed.password &&
      publicHost(parsed.hostname)
    );
  } catch {
    return false;
  }
}

/** Fail closed on an invalid projection; preserve complete URL bytes and rank. */
export function webSources(value: unknown): WebSource[] {
  if (!Array.isArray(value) || value.length > 5) return [];
  const sources: WebSource[] = [];
  const urls = new Set<string>();
  for (const row of value) {
    if (
      row === null ||
      typeof row !== 'object' ||
      Array.isArray(row) ||
      Object.keys(row).length !== 2 ||
      !Object.prototype.hasOwnProperty.call(row, 'title') ||
      !Object.prototype.hasOwnProperty.call(row, 'url') ||
      typeof row.title !== 'string' ||
      typeof row.url !== 'string' ||
      !row.title.trim() ||
      byteLength(row.title) > 256 ||
      !safeSourceUrl(row.url) ||
      urls.has(row.url)
    )
      return [];
    urls.add(row.url);
    sources.push({ title: row.title, url: row.url });
  }
  return byteLength(JSON.stringify(sources)) <= 8192 ? sources : [];
}

export function toolWebSources(
  name: string,
  status: string,
  value: unknown,
): WebSource[] {
  return name === 'web_search' && status === 'success' ? webSources(value) : [];
}
