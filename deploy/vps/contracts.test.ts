import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dir, '../..');
const read = (path: string) => readFileSync(resolve(root, path), 'utf8');
const compose = read('deploy/vps/compose.yaml');
const gateway = compose.split('  daemon:')[0];
const daemon = compose.split('  daemon:')[1].split('\nvolumes:')[0];
const caddy = read('deploy/vps/Caddyfile');

describe('offline one-owner deployment contracts', () => {
  test('state, history, workspace and vault share durable fixed roots', () => {
    for (const [key, value] of [
      ['CONTROL_PLANE_FILE', '/state/control-plane.json'],
      ['MEMORY_SQLITE_FILE', '/state/memory.sqlite'],
      ['HISTORY_SQLITE_FILE', '/state/history.sqlite'],
    ])
      expect(daemon).toContain(`ANIMAOS_RS_${key}: ${value}`);
    for (const mount of [
      'daemon_state:/state',
      'workspace:/workspace',
      'daemon_home:/home/anima',
    ])
      expect(daemon).toContain(`- ${mount}`);
    expect(daemon).toContain('ANIMAOS_WORKSPACE_ROOT: /workspace');
    const dockerfile = read('deploy/vps/daemon.Dockerfile');
    expect(dockerfile).toContain('USER 10001:10001');
    expect(dockerfile).toContain(
      'RUN CARGO_INCREMENTAL=0 cargo build --release --locked -p anima-daemon',
    );
  });

  test('only gateway web ports are public; credentials stay server-side', () => {
    expect(
      [...gateway.matchAll(/- ['"](\d+):(\d+)['"]/g)].map(
        (match) => `${match[1]}:${match[2]}`,
      ),
    ).toEqual(['80:80', '443:443']);
    expect(daemon).not.toMatch(/^\s+ports:/m);
    expect(daemon).toContain('network_mode: service:gateway');
    expect(daemon).toContain('ANIMAOS_RS_HOST: 127.0.0.1');
    expect(daemon).toContain('stop_grace_period: 45s');
    expect(compose).not.toContain('docker.sock');
    expect(compose).not.toMatch(/^  (postgres|database|db):/m);
    expect(read('deploy/vps/web.Dockerfile')).not.toMatch(
      /(API_KEY|ADMIN_TOKEN|KEYRING_PASSWORD)/,
    );
    const fixture = read('deploy/vps/fixtures/compose.test.env');
    const secrets = [
      'ANIMA_INTERNAL_API_KEY',
      'ANIMA_LOCAL_ADMIN_TOKEN',
      'ANIMA_KEYRING_PASSWORD',
    ].map((key) => fixture.match(new RegExp(`^${key}=(.+)$`, 'm'))?.[1]);
    expect(new Set(secrets).size).toBe(3);
    for (const secret of secrets)
      expect(secret?.length).toBeGreaterThanOrEqual(32);
  });

  test('owner and origin checks precede credential injection and unbuffered SSE', () => {
    expect(caddy.indexOf('basic_auth {')).toBeLessThan(
      caddy.indexOf('reverse_proxy 127.0.0.1:8080'),
    );
    expect(
      caddy.indexOf('invoke @notCallback check_browser_origin'),
    ).toBeLessThan(caddy.indexOf('header_up Authorization'));
    expect(caddy).toContain('flush_interval -1');
    expect(caddy).toContain('not method GET HEAD OPTIONS');
    expect(caddy).toContain('not header Origin https://{$ANIMA_DOMAIN}');
    const callbackBlock = caddy.slice(
      caddy.indexOf('@notCallback {'),
      caddy.indexOf('invoke @notCallback'),
    );
    expect(callbackBlock).toContain('method GET');
    expect(
      callbackBlock
        .match(/path (.+)/)?.[1]
        ?.trim()
        .split(' '),
    ).toEqual([
      '/api/connectors/gcalendar/callback',
      '/api/connectors/mail/gmail/callback',
      '/api/connectors/mail/outlook/callback',
    ]);
    for (const header of ['-Origin', '-Forwarded', '-X-Forwarded-*'])
      expect(caddy).toContain(`header_up ${header}`);
  });

  test('web build order and contract target cannot launch services', () => {
    const dockerfile = read('deploy/vps/web.Dockerfile');
    expect(dockerfile.indexOf('@animaOS-SWARM/sdk:build')).toBeLessThan(
      dockerfile.indexOf('@animaOS-SWARM/web:build'),
    );
    const target = JSON.parse(read('apps/web-e2e/package.json')).nx.targets[
      'deployment-contracts'
    ];
    expect(target.executor).toBe('nx:run-commands');
    expect(target.options.cwd).toBe('.');
    expect(target.options.command).toBe(
      'bun test deploy/vps/contracts.test.ts',
    );
    expect(target.dependsOn ?? []).toEqual([]);
  });
});
