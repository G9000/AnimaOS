import { EventEmitter } from 'node:events';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const dependencies = vi.hoisted(() => ({ lease: vi.fn(), spawn: vi.fn(), stop: vi.fn() }));
vi.mock('./rust-cache-maintenance.js', () => ({ withRustCacheLease: dependencies.lease }));
vi.mock('./process.js', () => ({ spawnManagedProcess: dependencies.spawn, stopManagedProcess: dependencies.stop }));
import { runTask } from './task.js';

const root = fileURLToPath(new URL('../../..', import.meta.url));
let held = false;

beforeEach(() => {
  held = false;
  vi.stubEnv('CARGO_TARGET_DIR', undefined);
  dependencies.lease.mockReset().mockImplementation(async (options, run) => {
    held = true;
    try { return await run({ targetDir: options.targetDir, env: { CARGO_TARGET_DIR: options.targetDir } }); }
    finally { held = false; }
  });
  dependencies.stop.mockReset().mockResolvedValue(undefined);
  dependencies.spawn.mockReset().mockImplementation(() => {
    const child = Object.assign(new EventEmitter(), { exitCode: null as number | null, pid: 12345 });
    queueMicrotask(() => { child.exitCode = 17; child.emit('exit', 17, null); });
    return child;
  });
});
afterEach(() => { vi.unstubAllEnvs(); vi.clearAllMocks(); });

describe('Rust cache task integration', () => {
  it('protects a complete validation task and passes an absolute shared target to its children', async () => {
    dependencies.spawn.mockImplementationOnce((definition) => {
      expect(held).toBe(true);
      expect(definition.env.CARGO_TARGET_DIR).toBe(resolve(root, 'target/cargo/validation'));
      const child = Object.assign(new EventEmitter(), { exitCode: null as number | null, pid: 12345 });
      queueMicrotask(() => { expect(held).toBe(true); child.exitCode = 17; child.emit('exit', 17, null); });
      return child;
    });
    expect(await runTask('bun', ['test'], { rustCache: 'validation' })).toBe(17);
    expect(dependencies.lease).toHaveBeenCalledOnce();
    expect(held).toBe(false);
  });

  it('normalizes an explicit relative target against the repository rather than the package cwd', async () => {
    vi.stubEnv('CARGO_TARGET_DIR', 'target/cargo/validation-rust-daemon');
    await runTask('bun', ['test'], { cwd: resolve(root, 'hosts/rust-daemon'), rustCache: 'validation' });
    expect(dependencies.lease.mock.calls[0][0].targetDir).toBe(resolve(root, 'target/cargo/validation-rust-daemon'));
  });

  it('preserves an explicit external target and protects release outputs through packaging', async () => {
    const external = resolve(root, '..', 'operator-cargo');
    vi.stubEnv('CARGO_TARGET_DIR', external);
    await runTask('bun', ['run', 'package:suite'], { rustCache: 'release' });
    expect(dependencies.lease.mock.calls[0][0]).toMatchObject({ targetDir: external, profile: 'release' });
  });

  it('automatically guards direct Cargo tests and leaves non-Rust commands alone', async () => {
    await runTask('cargo', ['test', '-p', 'anima-core']);
    expect(dependencies.lease.mock.calls[0][0].targetDir).toBe(resolve(root, 'target/cargo/validation'));
    dependencies.lease.mockClear();
    await runTask('bun', ['web-tests']);
    expect(dependencies.lease).not.toHaveBeenCalled();
  });

  it('keeps an explicit Cargo target and release profile consistent with the protected output', async () => {
    const cwd = resolve(root, 'hosts/rust-daemon');
    await runTask('cargo', ['test', '--release', '--target-dir', 'operator-target'], { cwd });
    const target = resolve(cwd, 'operator-target');
    expect(dependencies.lease.mock.calls[0][0]).toMatchObject({ targetDir: target, profile: 'release' });
    expect(dependencies.spawn.mock.calls[0][0].args).toContain(target);
  });

  it('stops only its owned child and waits for exit when the live cache budget aborts', async () => {
    const controller = new AbortController();
    dependencies.lease.mockImplementationOnce(async (options, run) => run({ targetDir: options.targetDir, env: { CARGO_TARGET_DIR: options.targetDir }, signal: controller.signal }));
    const child = Object.assign(new EventEmitter(), { exitCode: null as number | null, pid: 12345 });
    dependencies.spawn.mockReturnValueOnce(child);
    dependencies.stop.mockImplementationOnce(async stopped => {
      expect(stopped).toBe(child);
      child.exitCode = 1;
      child.emit('exit', 1, null);
    });
    const running = runTask('bun', ['long-build'], { rustCache: 'validation' });
    await vi.waitFor(() => expect(dependencies.spawn).toHaveBeenCalledOnce());
    controller.abort(new Error('Cache exceeded its budget'));
    try {
      await vi.waitFor(() => expect(dependencies.stop).toHaveBeenCalledWith(child), { timeout: 300 });
      expect(await running).toBe(1);
    } finally {
      if (child.exitCode === null) { child.exitCode = 17; child.emit('exit', 17, null); }
      await running;
    }
  });

  it('protects an idle app runtime without making it the victim of another build exceeding the budget', async () => {
    await runTask('anima-daemon.exe', [], { rustCache: 'dev', rustCacheMonitor: false });
    expect(dependencies.lease.mock.calls[0][0].monitor).toBe(false);
  });

  it.each([['bench'], ['build', '-r'], ['test', '-r']])('protects Cargo release output for %j', async (...args) => {
    await runTask('cargo', args);
    expect(dependencies.lease.mock.calls[0][0].profile).toBe('release');
  });

  it('preserves application flags after Cargo -- without mistaking them for build options', async () => {
    const args = ['run', '--', '--target-dir', 'exports', '--profile', 'ship', '--release', '-r'];
    await runTask('cargo', args);
    expect(dependencies.lease.mock.calls[0][0]).toMatchObject({ targetDir: resolve(root, 'target/cargo'), profile: 'dev' });
    expect(dependencies.spawn.mock.calls[0][0].args).toEqual(args);
  });
});
