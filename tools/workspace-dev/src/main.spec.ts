import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { buildWorkspaceDevPlan, parseHostArg } from './main.js';

describe('workspace-dev orchestration', () => {
  it('parses --host from argv', () => {
    expect(parseHostArg(['--host', 'rust'])).toBe('rust');
  });

  it('plans the rust host and UI processes with injected backend origin', () => {
    expect(
      buildWorkspaceDevPlan('rust', { env: {}, homeDir: '/home/dev' }),
    ).toEqual({
      host: {
        key: 'rust',
        projectName: 'rust-daemon',
        baseUrl: 'http://127.0.0.1:8080',
        env: {
          ANIMAOS_RS_HOST: '127.0.0.1',
          ANIMAOS_RS_PORT: '8080',
        },
      },
      processes: [
        {
          name: 'rust-daemon',
          command: 'bun',
          args: ['run', '--filter', 'rust-daemon', 'dev'],
          env: {
            ANIMAOS_RS_HOST: '127.0.0.1',
            ANIMAOS_RS_PORT: '8080',
            ANIMAOS_RS_CONTROL_PLANE_FILE: join(
              '/home/dev',
              '.animaos-swarm',
              'data',
              'control-plane.json',
            ),
          },
        },
        {
          name: '@animaos/web',
          command: 'bun',
          args: ['run', '--filter', '@animaos/web', 'dev'],
          env: {
            UI_BACKEND_ORIGIN: 'http://127.0.0.1:8080',
            VITE_HOST_KEY: 'rust',
          },
        },
        {
          name: '@animaos/playground',
          command: 'bun',
          args: ['run', '--filter', '@animaos/playground', 'dev'],
          env: {
            UI_BACKEND_ORIGIN: 'http://127.0.0.1:8080',
            VITE_HOST_KEY: 'rust',
          },
        },
        {
          name: '@animaos/docs',
          command: 'bun',
          args: ['run', '--filter', '@animaos/docs', 'dev'],
        },
        {
          name: '@animaos/contributor-docs',
          command: 'bun',
          args: ['run', '--filter', '@animaos/contributor-docs', 'dev'],
        },
      ],
    });
  });

  it('reuses an already-running host and only starts the UI', () => {
    expect(
      buildWorkspaceDevPlan('rust', {
        reuseExistingHost: true,
      }),
    ).toEqual({
      host: {
        key: 'rust',
        projectName: 'rust-daemon',
        baseUrl: 'http://127.0.0.1:8080',
        env: {
          ANIMAOS_RS_HOST: '127.0.0.1',
          ANIMAOS_RS_PORT: '8080',
        },
      },
      processes: [
        {
          name: '@animaos/web',
          command: 'bun',
          args: ['run', '--filter', '@animaos/web', 'dev'],
          env: {
            UI_BACKEND_ORIGIN: 'http://127.0.0.1:8080',
            VITE_HOST_KEY: 'rust',
          },
        },
        {
          name: '@animaos/playground',
          command: 'bun',
          args: ['run', '--filter', '@animaos/playground', 'dev'],
          env: {
            UI_BACKEND_ORIGIN: 'http://127.0.0.1:8080',
            VITE_HOST_KEY: 'rust',
          },
        },
        {
          name: '@animaos/docs',
          command: 'bun',
          args: ['run', '--filter', '@animaos/docs', 'dev'],
        },
        {
          name: '@animaos/contributor-docs',
          command: 'bun',
          args: ['run', '--filter', '@animaos/contributor-docs', 'dev'],
        },
      ],
    });
  });
});
