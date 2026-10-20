import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  HOST_KEYS,
  getHostDefinition,
  hostProcessEnv,
  isHostKey,
} from './hosts.js';

const DEV_CONTROL_PLANE = join(
  '/home/dev',
  '.animaos-swarm',
  'data',
  'control-plane.json'
);

describe('workspace-dev host registry', () => {
  it('recognizes the supported host keys', () => {
    expect(HOST_KEYS).toEqual(['rust', 'elixir', 'python']);
    expect(isHostKey('rust')).toBe(true);
    expect(isHostKey('elixir')).toBe(true);
    expect(isHostKey('python')).toBe(true);
  });

  it('marks rust as the ready host project', () => {
    expect(getHostDefinition('rust')).toEqual({
      key: 'rust',
      projectName: 'rust-daemon',
      baseUrl: 'http://127.0.0.1:8080',
      status: 'ready',
      env: {
        ANIMAOS_RS_HOST: '127.0.0.1',
        ANIMAOS_RS_PORT: '8080',
      },
    });
  });

  it('marks elixir and python as placeholders', () => {
    expect(getHostDefinition('elixir')).toMatchObject({
      key: 'elixir',
      projectName: 'elixir-phoenix',
      status: 'placeholder',
    });
    expect(getHostDefinition('python')).toMatchObject({
      key: 'python',
      projectName: 'python-service',
      status: 'placeholder',
    });
  });

  it('rejects unknown hosts with a clear error', () => {
    expect(() => getHostDefinition('go')).toThrowError(
      "Unknown host 'go'. Expected one of: rust, elixir, python."
    );
  });

  describe('rust daemon persistence', () => {
    const rust = getHostDefinition('rust');

    it('persists to ~/.animaos-swarm/data by default', () => {
      expect(hostProcessEnv(rust, {}, '/home/dev')).toEqual({
        ANIMAOS_RS_HOST: '127.0.0.1',
        ANIMAOS_RS_PORT: '8080',
        ANIMAOS_RS_CONTROL_PLANE_FILE: DEV_CONTROL_PLANE,
      });
    });

    it('leaves a control-plane file from the shell in charge', () => {
      expect(
        hostProcessEnv(
          rust,
          { ANIMAOS_RS_CONTROL_PLANE_FILE: '/elsewhere/cp.json' },
          '/home/dev'
        )
      ).toEqual(rust.env);
    });

    it('lets Postgres mode keep the control plane in Postgres', () => {
      expect(
        hostProcessEnv(
          rust,
          { ANIMAOS_RS_PERSISTENCE_MODE: 'Postgres' },
          '/home/dev'
        )
      ).toEqual(rust.env);
    });

    it('stays ephemeral when ANIMAOS_DEV_EPHEMERAL=1', () => {
      expect(
        hostProcessEnv(rust, { ANIMAOS_DEV_EPHEMERAL: '1' }, '/home/dev')
      ).toEqual(rust.env);
    });

    it('adds nothing for other hosts', () => {
      const elixir = getHostDefinition('elixir');
      expect(hostProcessEnv(elixir, {}, '/home/dev')).toEqual(elixir.env);
    });
  });
});
