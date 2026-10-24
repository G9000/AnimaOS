import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { consoleLaunchUrl, prepareOwnerCredentials } from './owner-console.js';

const owned: string[] = [];
const home = () => { const dir = mkdtempSync(join(tmpdir(), 'anima-owner-console-')); owned.push(dir); return dir; };
afterEach(() => { for (const dir of owned.splice(0)) rmSync(dir, { recursive: true, force: true }); });

describe('owner console bootstrap', () => {
  it('persists an owner token outside browser configuration and reuses it', () => {
    const dir = home();
    const created = prepareOwnerCredentials({}, dir, true);
    expect(created.daemonEnv.ANIMA_LOCAL_ADMIN_TOKEN_FILE).toBe(join(dir, '.animaos-swarm', 'data', 'control-plane.json.admin-token'));
    expect(created.token).toMatch(/^[a-f0-9]{64}$/);
    expect(Object.keys(created.daemonEnv)).toEqual(['ANIMA_LOCAL_ADMIN_TOKEN_FILE']);
    expect(readFileSync(created.daemonEnv.ANIMA_LOCAL_ADMIN_TOKEN_FILE!, 'utf8')).toBe(created.token);
    expect(prepareOwnerCredentials({}, dir, false).token).toBe(created.token);
  });

  it('requires an existing token when reusing an existing daemon', () => {
    expect(() => prepareOwnerCredentials({}, home(), false)).toThrow(/owner token/);
  });

  it('an explicit token file wins and unreadability never falls back', () => {
    const dir = home(); const file = join(dir, 'owner-token');
    writeFileSync(file, 'explicit-file-token');
    const env = { ANIMA_LOCAL_ADMIN_TOKEN_FILE: file, ANIMA_LOCAL_ADMIN_TOKEN: 'inline' };
    expect(prepareOwnerCredentials(env, dir, false).token).toBe('explicit-file-token');
    expect(() => prepareOwnerCredentials({ ...env, ANIMA_LOCAL_ADMIN_TOKEN_FILE: join(dir, 'missing') }, dir, true)).toThrow(/owner token/);
  });

  it('rejects empty and oversized explicit tokens', () => {
    for (const token of ['', 'x'.repeat(4097)]) expect(() => prepareOwnerCredentials({ ANIMA_LOCAL_ADMIN_TOKEN: token }, home(), true)).toThrow(/owner token/);
  });

  it('mints a short-lived launch URL matching the Rust HMAC domain', () => {
    expect(consoleLaunchUrl('http://localhost:4200', 'owner', 1234, '0123456789abcdef0123456789abcdef')).toBe('http://localhost:4200/?anima-login=1234.0123456789abcdef0123456789abcdef.ccefed0c2cd871902935b0f9d725f08507dfb0a35263d5de8029bf9f432c53a6');
    expect(() => consoleLaunchUrl('https://remote.example', 'owner')).toThrow(/loopback/);
  });
});
