import { describe, expect, it } from 'vitest';

import {
  codesignArgs,
  DAEMON_SIGNING_IDENTIFIER,
  findDaemonExecutable,
  parseDaemonDevArgs,
  resolveSigningIdentity,
  SIGNING_IDENTITY_ENV,
} from './rust-daemon-dev.js';

describe('rust daemon dev signing', () => {
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
