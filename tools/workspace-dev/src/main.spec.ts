import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { buildWorkspaceDevPlan, parseHostArg } from './main.js';

describe('workspace-dev orchestration', () => {
  it('parses --host from argv', () => {
    expect(parseHostArg(['--host', 'rust'])).toBe('rust');
  });

  it('starts only the daemon/API and web by default', () => {
    const plan = buildWorkspaceDevPlan('rust', { env: {}, homeDir: '/home/dev' });
    expect(plan.processes).toEqual([
      {
        name: 'rust-daemon', command: 'bun',
        args: ['run', '--filter', 'rust-daemon', 'dev'],
        env: {
          ANIMAOS_RS_HOST: '127.0.0.1', ANIMAOS_RS_PORT: '8080',
          ANIMAOS_RS_CONTROL_PLANE_FILE: join('/home/dev', '.animaos-swarm', 'data', 'control-plane.json'),
        },
      },
      {
        name: '@animaos/web', command: 'bun',
        args: ['run', '--filter', '@animaos/web', 'dev'],
        env: { UI_BACKEND_ORIGIN: 'http://127.0.0.1:8080', VITE_HOST_KEY: 'rust' },
      },
    ]);
  });

  it('starts only web when the daemon is already running', () => {
    const plan = buildWorkspaceDevPlan('rust', { reuseExistingHost: true });
    expect(plan.processes.map(p => p.name)).toEqual(['@animaos/web']);
  });

  it.each([
    ['docs', ['@animaos/docs', '@animaos/contributor-docs']],
    ['game', ['Village game']],
    ['playground', ['rust-daemon', '@animaos/playground']],
    ['all', ['rust-daemon', '@animaos/web', '@animaos/playground', '@animaos/docs', '@animaos/contributor-docs', 'Village game']],
  ] as const)('starts exactly the requested %s services', (surface, names) => {
    const plan = buildWorkspaceDevPlan('rust', { surface, env: {}, homeDir: '/home/dev' });
    expect(plan.processes.map(p => p.name)).toEqual(names);
  });

  it('runs the game through its own existing launcher and working directory', () => {
    const plan = buildWorkspaceDevPlan('rust', { surface: 'game' });
    expect(plan.processes).toEqual([{
      name: 'Village game', command: 'bun', args: ['run', 'dev'],
      cwd: fileURLToPath(new URL('../../../poc/village/', import.meta.url)),
    }]);
  });

  it('reuses one daemon for all assistant surfaces', () => {
    const plan = buildWorkspaceDevPlan('rust', { surface: 'all', reuseExistingHost: true });
    expect(plan.processes.map(p => p.name)).toEqual([
      '@animaos/web', '@animaos/playground', '@animaos/docs', '@animaos/contributor-docs', 'Village game',
    ]);
  });

  it('passes generated owner credentials only to the daemon process', () => {
    const plan = buildWorkspaceDevPlan('rust', {
      surface: 'all', env: {}, homeDir: '/home/dev',
      ownerEnv: { ANIMA_LOCAL_ADMIN_TOKEN_FILE: '/private/token' },
    });
    expect(plan.processes[0]?.env?.ANIMA_LOCAL_ADMIN_TOKEN_FILE).toBe('/private/token');
    for (const process of plan.processes.slice(1)) {
      expect(process.env?.ANIMA_LOCAL_ADMIN_TOKEN_FILE).toBeUndefined();
    }
  });
});
