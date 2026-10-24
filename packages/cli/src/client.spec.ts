import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createCliDaemonClient } from './client.js';

afterEach(() => vi.unstubAllEnvs());

describe('CLI native daemon credentials', () => {
  it('sends explicit owner and global API credentials as separate headers', async () => {
    vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN', 'owner-token');
    vi.stubEnv('ANIMAOS_RS_API_KEY', 'global-key');
    const fetch = vi.fn(async () => Response.json({ status: 'ok' }));
    const client = createCliDaemonClient({ fetch });

    await client.health();

    const [, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    const headers = new Headers(init.headers);
    expect(headers.get('authorization')).toBe('Bearer owner-token');
    expect(headers.get('x-api-key')).toBe('global-key');
    expect(headers.has('origin')).toBe(false);
  });

  it('uses the local admin token file for loopback and never discovers it for remote URLs', async () => {
    const profile = mkdtempSync(join(tmpdir(), 'anima-cli-auth-'));
    try {
      const controlPlane = join(profile, 'data', 'control-plane.json');
      mkdirSync(dirname(controlPlane), { recursive: true });
      writeFileSync(`${controlPlane}.admin-token`, 'local-file-secret\n');
      vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN', '');
      vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN_FILE', '');
      vi.stubEnv('ANIMAOS_RS_API_KEY', '');
      vi.stubEnv('ANIMAOS_RS_CONTROL_PLANE_FILE', controlPlane);
      const localFetch = vi.fn(async () => Response.json({ status: 'ok' }));
      await createCliDaemonClient({ fetch: localFetch }).health();
      const [, localInit] = localFetch.mock.calls[0] as unknown as [string, RequestInit];
      expect(new Headers(localInit.headers).get('authorization')).toBe(
        'Bearer local-file-secret',
      );

      const packagedHome = join(profile, 'packaged-home');
      const packagedToken = join(packagedHome, '.animaos-swarm', 'admin-token');
      mkdirSync(dirname(packagedToken), { recursive: true });
      writeFileSync(packagedToken, 'packaged-profile-secret\n');
      vi.stubEnv('ANIMA_HOME', packagedHome);
      vi.stubEnv('ANIMAOS_RS_CONTROL_PLANE_FILE', '');
      const packagedFetch = vi.fn(async () => Response.json({ status: 'ok' }));
      await createCliDaemonClient({ fetch: packagedFetch }).health();
      const [, packagedInit] = packagedFetch.mock.calls[0] as unknown as [string, RequestInit];
      expect(new Headers(packagedInit.headers).get('authorization')).toBe(
        'Bearer packaged-profile-secret',
      );

      vi.stubEnv('ANIMA_HOME', '');
      const remoteFetch = vi.fn(async () => Response.json({ status: 'ok' }));
      await createCliDaemonClient({ baseUrl: 'https://daemon.example', fetch: remoteFetch }).health();
      const [, remoteInit] = remoteFetch.mock.calls[0] as unknown as [string, RequestInit];
      expect(new Headers(remoteInit.headers).has('authorization')).toBe(false);
    } finally {
      rmSync(profile, { recursive: true, force: true });
    }
  });

  it('reports unreadable explicit token files without exposing file contents', () => {
    vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN_FILE', join(tmpdir(), 'missing-admin-token-secret'));
    let tokenFileError: unknown;
    try {
      createCliDaemonClient();
      throw new Error('expected token file loading to fail');
    } catch (error) {
      tokenFileError = error;
    }
    expect(tokenFileError).toMatchObject({
      message: 'Unable to read the local owner token file; check ANIMA_LOCAL_ADMIN_TOKEN_FILE.',
    });
    expect((tokenFileError as Error).message).not.toContain('secret');
  });
});
