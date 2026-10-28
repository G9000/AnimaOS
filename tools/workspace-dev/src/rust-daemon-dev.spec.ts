import { describe, expect, it, vi } from 'vitest';

const native = vi.hoisted(() => ({ spawnSync: vi.fn() }));
vi.mock('node:child_process', () => ({ spawnSync: native.spawnSync, spawn: vi.fn() }));

import {
  codesignArgs,
  DAEMON_SIGNING_IDENTIFIER,
  findDaemonExecutable,
  main,
  parseDaemonDevArgs,
  resolveSigningIdentity,
  SIGNING_IDENTITY_ENV,
} from './rust-daemon-dev.js';

describe('rust daemon dev signing', () => {
  it('builds through the live cache guard before launching the runtime', () => {
    native.spawnSync.mockReturnValue({ status: 0, stdout: JSON.stringify({ reason: 'compiler-artifact', target: { name: 'anima-daemon', kind: ['bin'] }, executable: '/cache/debug/anima-daemon' }) });
    main(['--build-only']);
    expect(native.spawnSync.mock.calls[0][0]).toBe(process.execPath);
    expect(native.spawnSync.mock.calls[0][1]).toEqual(expect.arrayContaining(['--rust-cache', 'dev', '--', 'cargo', 'build', '--message-format=json-render-diagnostics']));
    native.spawnSync.mockReset();
  });
  it('signs only on macOS when the identity variable is set', () => {
    const env = { [SIGNING_IDENTITY_ENV]: '  AnimaOS Dev Signing  ' };

    expect(resolveSigningIdentity(env, 'darwin')).toBe('AnimaOS Dev Signing');
    expect(resolveSigningIdentity(env, 'linux')).toBeNull();
    expect(resolveSigningIdentity(env, 'win32')).toBeNull();
    expect(resolveSigningIdentity({}, 'darwin')).toBeNull();
    expect(
      resolveSigningIdentity({ [SIGNING_IDENTITY_ENV]: '   ' }, 'darwin')
    ).toBeNull();
  });

  it('re-signs with a fixed identifier so the designated requirement is stable', () => {
    expect(codesignArgs('AnimaOS Dev Signing', '/repo/target/cargo/debug/anima-daemon')).toEqual([
      '--force',
      '--sign',
      'AnimaOS Dev Signing',
      '--identifier',
      DAEMON_SIGNING_IDENTIFIER,
      '/repo/target/cargo/debug/anima-daemon',
    ]);
  });

  it('finds the daemon binary in cargo JSON output', () => {
    const output = [
      JSON.stringify({
        reason: 'compiler-artifact',
        target: { name: 'anima_daemon', kind: ['lib'] },
        executable: null,
      }),
      'not json',
      JSON.stringify({
        reason: 'compiler-artifact',
        target: { name: 'anima-daemon', kind: ['bin'] },
        executable: '/custom-target/debug/anima-daemon',
      }),
      JSON.stringify({ reason: 'build-finished', success: true }),
    ].join('\n');

    expect(findDaemonExecutable(output)).toBe('/custom-target/debug/anima-daemon');
    expect(findDaemonExecutable('')).toBeNull();
  });

  it('separates launcher flags from daemon arguments', () => {
    expect(parseDaemonDevArgs([])).toEqual({ mode: 'run', daemonArgs: [] });
    expect(parseDaemonDevArgs(['--build-only'])).toEqual({
      mode: 'build',
      daemonArgs: [],
    });
    expect(parseDaemonDevArgs(['--', '--build-only'])).toEqual({
      mode: 'run',
      daemonArgs: ['--build-only'],
    });
  });
});
