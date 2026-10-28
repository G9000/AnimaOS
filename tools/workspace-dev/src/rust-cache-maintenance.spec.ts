import { mkdir, mkdtemp, readdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_RUST_CACHE_BUDGET_BYTES,
  RUST_CACHE_BUDGET_ENV,
  cargoCleanScopeArgs,
  isRustCacheScopeBusy,
  resolveRustCacheBudgetBytes,
  withRustCacheLease,
  type RustCacheDependencies,
  type RustCacheScope,
} from './rust-cache-maintenance.js';
import { measureRustCacheBytes } from './rust-cache.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  vi.unstubAllEnvs();
  await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })));
});

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'anima-cache-maintenance-'));
  temporaryDirectories.push(root);
  await writeFile(join(root, 'Cargo.toml'), '[workspace]\n');
  return root;
}

async function output(root: string, target: string, bytes: number, profile = 'debug') {
  const targetDir = resolve(root, 'target', 'cargo', target);
  await mkdir(join(targetDir, profile, 'deps'), { recursive: true });
  await writeFile(join(targetDir, '.rustc_info.json'), '');
  await writeFile(join(targetDir, profile, '.cargo-lock'), '');
  await writeFile(join(targetDir, profile, 'deps', 'fixture.rlib'), Buffer.alloc(bytes));
  return targetDir;
}

function dependencies(cleaned: RustCacheScope[] = []): Partial<RustCacheDependencies> {
  return {
    isTargetBusy: async () => false,
    clean: async (scope) => {
      cleaned.push(scope);
      await rm(scope.outputDir, { recursive: true, force: true });
    },
  };
}

describe('bounded Rust cache lifecycle', () => {
  it('limits Cargo cleanup to a main profile or an exact child target', () => {
    const root = resolve('workspace');
    const main = join(root, 'target', 'cargo');
    expect(cargoCleanScopeArgs(root, { targetDir: main, outputDir: join(main, 'debug'), profile: 'dev' })).toEqual([
      'clean', '--manifest-path', join(root, 'Cargo.toml'), '--target-dir', main, '--profile', 'dev',
    ]);
    expect(cargoCleanScopeArgs(root, { targetDir: join(main, 'validation'), outputDir: join(main, 'validation') })).toEqual([
      'clean', '--manifest-path', join(root, 'Cargo.toml'), '--target-dir', join(main, 'validation'),
    ]);
  });

  it('uses a 32 GiB default and rejects invalid caller budgets', () => {
    expect(DEFAULT_RUST_CACHE_BUDGET_BYTES).toBe(32 * 1024 ** 3);
    expect(resolveRustCacheBudgetBytes(undefined)).toBe(DEFAULT_RUST_CACHE_BUDGET_BYTES);
    expect(resolveRustCacheBudgetBytes('0.5')).toBe(0.5 * 1024 ** 3);
    for (const value of ['', '0', '-1', 'NaN', 'Infinity', '2GB']) {
      expect(() => resolveRustCacheBudgetBytes(value)).toThrow(/budget/i);
    }
    for (const value of ['32.1', '128', '1025']) {
      expect(resolveRustCacheBudgetBytes(value)).toBe(DEFAULT_RUST_CACHE_BUDGET_BYTES);
    }
  });

  it('caps inherited and programmatic budgets and passes the effective limit to children', async () => {
    const root = await fixture();
    vi.stubEnv(RUST_CACHE_BUDGET_ENV, '128');
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 128 * 1024 ** 3, dependencies: dependencies() }, async (lease) => {
      expect(lease.env[RUST_CACHE_BUDGET_ENV]).toBe('32');
      const directory = join(root, 'target', '.rust-cache-maintenance', 'leases');
      const [name] = await readdir(directory);
      const record = JSON.parse(await readFile(join(directory, name!), 'utf8'));
      expect(record.budgetBytes).toBe(DEFAULT_RUST_CACHE_BUDGET_BYTES);
    });
  });

  it('cannot bypass the 32 GiB ceiling with a larger requested budget', async () => {
    const root = await fixture();
    const run = vi.fn(async () => {});
    await expect(withRustCacheLease({
      workspaceRoot: root,
      budgetBytes: 128 * 1024 ** 3,
      dependencies: { ...dependencies(), measure: async () => 33 * 1024 ** 3 },
    }, run)).rejects.toThrow(/budget exceeded/i);
    expect(run).not.toHaveBeenCalled();
  });

  it('passes an absolute target and holds its lease for the whole callback', async () => {
    const root = await fixture();
    const targetDir = resolve(root, 'target', 'cargo', 'validation');
    await withRustCacheLease({ workspaceRoot: root, targetDir, dependencies: dependencies() }, async (lease) => {
      expect(lease).toMatchObject({ targetDir, env: { CARGO_TARGET_DIR: targetDir } });
      expect(lease.signal).toBeInstanceOf(AbortSignal);
      const records = await readdir(join(root, 'target', '.rust-cache-maintenance', 'leases'));
      expect(records).toHaveLength(1);
      expect(JSON.parse(await readFile(join(root, 'target', '.rust-cache-maintenance', 'leases', records[0]!), 'utf8'))).toMatchObject({ pid: process.pid, targetDir, profile: 'dev' });
    });
    expect(await readdir(join(root, 'target', '.rust-cache-maintenance', 'leases'))).toEqual([]);
  });

  it('cleans inactive child targets before a command and keeps source and unknown paths', async () => {
    const root = await fixture();
    await output(root, 'validation-rust-daemon', 20);
    const unknown = join(root, 'target', 'cargo', 'my-notes');
    await mkdir(unknown, { recursive: true });
    await writeFile(join(unknown, 'keep.txt'), 'keep');
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {
      expect(await measureRustCacheBytes(join(root, 'target', 'cargo'))).toBe(4);
    });
    expect(cleaned.map((scope) => scope.targetDir)).toEqual([join(root, 'target', 'cargo', 'validation-rust-daemon')]);
    expect(await readFile(join(unknown, 'keep.txt'), 'utf8')).toBe('keep');
    expect(await readFile(join(root, 'Cargo.toml'), 'utf8')).toBe('[workspace]\n');
  });

  it('recognizes Cargo compatibility reports when pruning an inactive validation target', async () => {
    const root = await fixture();
    const validation = await output(root, 'validation', 20);
    await writeFile(join(validation, '.future-incompat-report.json'), '{}');
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {});
    expect(cleaned.map((scope) => scope.targetDir)).toEqual([validation]);
  });

  it('preserves unrecognized files inside a recognized validation target', async () => {
    const root = await fixture();
    const validation = await output(root, 'validation', 20);
    await writeFile(join(validation, 'keep.txt'), 'keep');
    const cleaned: RustCacheScope[] = [];
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {})).rejects.toThrow(/budget/i);
    expect(cleaned).toEqual([]);
    expect(await readFile(join(validation, 'keep.txt'), 'utf8')).toBe('keep');
  });

  it('uses profile-scoped cleaning for the main target so nested validation survives', async () => {
    const root = await fixture();
    const main = await output(root, '', 20);
    const validation = await output(root, 'validation', 1);
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, targetDir: validation, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {});
    expect(cleaned).toEqual([expect.objectContaining({ targetDir: main, profile: 'dev', outputDir: join(main, 'debug') })]);
    expect(await measureRustCacheBytes(validation)).toBe(1);
  });

  it('keeps an outer lease active while a nested command builds and returns output', async () => {
    const root = await fixture();
    const main = await output(root, '', 4);
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {
      await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {
        await writeFile(join(main, 'debug', 'deps', 'fresh.rlib'), Buffer.alloc(4));
      });
      expect(await readFile(join(main, 'debug', 'deps', 'fresh.rlib'))).toHaveLength(4);
    });
    expect(cleaned).toEqual([]);
  });

  it('prunes other inactive output after a command while retaining its fresh result', async () => {
    const root = await fixture();
    const main = await output(root, '', 3);
    await output(root, 'validation', 3);
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {
      await writeFile(join(main, 'debug', 'deps', 'fresh.rlib'), Buffer.alloc(5));
      return 'built';
    });
    expect(cleaned.map((scope) => scope.targetDir)).toEqual([join(root, 'target', 'cargo', 'validation')]);
    expect(await readFile(join(main, 'debug', 'deps', 'fresh.rlib'))).toHaveLength(5);
  });

  it('fails clearly when fresh protected output alone exceeds budget and releases the lease', async () => {
    const root = await fixture();
    const main = await output(root, '', 2);
    const cleaned: RustCacheScope[] = [];
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 5, dependencies: dependencies(cleaned) }, async () => {
      await writeFile(join(main, 'debug', 'deps', 'fresh.rlib'), Buffer.alloc(8));
    })).rejects.toThrow(/budget.*protected|protected.*budget/i);
    expect(cleaned).toEqual([]);
    expect(await readFile(join(main, 'debug', 'deps', 'fresh.rlib'))).toHaveLength(8);
    expect(await readdir(join(root, 'target', '.rust-cache-maintenance', 'leases'))).toEqual([]);
  });

  it('ignores stale PID leases while preserving live leases', async () => {
    const root = await fixture();
    const main = await output(root, '', 3);
    const stale = await output(root, 'validation-rust-daemon', 10);
    const leases = join(root, 'target', '.rust-cache-maintenance', 'leases');
    await mkdir(leases, { recursive: true });
    await writeFile(join(leases, 'stale.json'), JSON.stringify({ version: 1, id: 'stale', pid: 1234567, targetDir: stale, profile: 'dev' }));
    await writeFile(join(leases, 'live.json'), JSON.stringify({ version: 1, id: 'live', pid: process.pid, targetDir: main, profile: 'dev' }));
    const cleaned: RustCacheScope[] = [];
    await withRustCacheLease({ workspaceRoot: root, targetDir: join(root, 'target', 'cargo', 'validation'), budgetBytes: 5, dependencies: { ...dependencies(cleaned), isAlive: (pid) => pid === process.pid } }, async () => {});
    expect(cleaned.map((scope) => scope.targetDir)).toEqual([stale]);
    expect(await readdir(leases)).toEqual(['live.json']);
    expect(await measureRustCacheBytes(main)).toBe(3);
  });

  it('never cleans a target held by a concurrent whole-command lease', async () => {
    const root = await fixture();
    const main = await output(root, '', 4);
    const cleaned: RustCacheScope[] = [];
    let release!: () => void;
    let entered!: () => void;
    const started = new Promise<void>((done) => { entered = done; });
    const running = withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {
      entered();
      await new Promise<void>((done) => { release = done; });
    });
    await started;
    await writeFile(join(main, 'debug', 'deps', 'grown.rlib'), Buffer.alloc(10));
    await expect(withRustCacheLease({ workspaceRoot: root, targetDir: join(root, 'target', 'cargo', 'validation'), budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {})).rejects.toThrow(/budget/i);
    expect(cleaned).toEqual([]);
    release();
    await expect(running).rejects.toThrow(/budget/i);
  });

  it('preserves externally active target output even when no lease was registered', async () => {
    const root = await fixture();
    const main = await output(root, '', 20);
    const cleaned: RustCacheScope[] = [];
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: { ...dependencies(cleaned), isTargetBusy: async (scope) => scope.targetDir === main } }, async () => {})).rejects.toThrow(/active|busy/i);
    expect(cleaned).toEqual([]);
    expect(await measureRustCacheBytes(main)).toBe(20);
  });

  it('preserves symlinks and junctions inside a cleanup candidate', async () => {
    const root = await fixture();
    const external = join(root, 'keep');
    await mkdir(external);
    await writeFile(join(external, 'valuable.txt'), 'valuable');
    const main = await output(root, '', 20);
    await symlink(external, join(main, 'debug', 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
    const cleaned: RustCacheScope[] = [];
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {})).rejects.toThrow(/budget|symbolic|junction/i);
    expect(cleaned).toEqual([]);
    expect(await readFile(join(external, 'valuable.txt'), 'utf8')).toBe('valuable');
  });

  it('passes through external targets without cleaning or creating them', async () => {
    const root = await fixture();
    const external = join(root, 'external-target');
    await withRustCacheLease({ workspaceRoot: root, targetDir: external, dependencies: dependencies() }, async ({ targetDir }) => {
      expect(targetDir).toBe(external);
      await expect(readdir(external)).rejects.toMatchObject({ code: 'ENOENT' });
    });
  });

  it('refuses linked managed roots before invoking a command', async () => {
    const root = await fixture();
    const external = join(root, 'external');
    await mkdir(external);
    await mkdir(join(root, 'target'));
    await symlink(external, join(root, 'target', 'cargo'), process.platform === 'win32' ? 'junction' : 'dir');
    let ran = false;
    await expect(withRustCacheLease({ workspaceRoot: root, dependencies: dependencies() }, async () => { ran = true; })).rejects.toThrow(/symbolic|junction|resolved/i);
    expect(ran).toBe(false);
  });

  it('releases its lease when the command fails', async () => {
    const root = await fixture();
    await expect(withRustCacheLease({ workspaceRoot: root, dependencies: dependencies() }, async () => { throw new Error('command failed'); })).rejects.toThrow('command failed');
    expect(await readdir(join(root, 'target', '.rust-cache-maintenance', 'leases'))).toEqual([]);
  });

  it('reports failed Cargo cleanup instead of silently allowing growth', async () => {
    const root = await fixture();
    await output(root, '', 20);
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, dependencies: { ...dependencies(), clean: async () => { throw new Error('binary is locked'); } } }, async () => {})).rejects.toThrow(/binary is locked/);
  });

  it('refuses an unrecognized target below the managed root', async () => {
    const root = await fixture();
    await expect(withRustCacheLease({ workspaceRoot: root, targetDir: join(root, 'target', 'cargo', 'personal-files'), dependencies: dependencies() }, async () => {})).rejects.toThrow(/unknown|recognized/i);
  });

  it('serializes measurements and lease registration across simultaneous callers', async () => {
    const root = await fixture();
    let measuring = 0;
    let maximum = 0;
    const deps = {
      ...dependencies(),
      measure: async () => {
        maximum = Math.max(maximum, ++measuring);
        await new Promise((done) => setTimeout(done, 2));
        measuring--;
        return 0;
      },
    };
    const results = await Promise.all(Array.from({ length: 8 }, (_, index) => withRustCacheLease({ workspaceRoot: root, dependencies: deps }, async () => index)));
    expect(results).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
    expect(maximum).toBe(1);
  });

  it('protects a lease held by another process', async () => {
    const root = await fixture();
    const main = await output(root, '', 20);
    const entered = join(root, 'entered');
    const release = join(root, 'release');
    const script = join(root, 'child.ts');
    await writeFile(script, `
      import { readFile, writeFile } from 'node:fs/promises';
      import { withRustCacheLease } from ${JSON.stringify(new URL('./rust-cache-maintenance.ts', import.meta.url).href)};
      await withRustCacheLease({ workspaceRoot: ${JSON.stringify(root)}, budgetBytes: 100 }, async () => {
        await writeFile(${JSON.stringify(entered)}, 'ready');
        for (;;) {
          try { await readFile(${JSON.stringify(release)}); break; } catch {}
          await new Promise((done) => setTimeout(done, 10));
        }
      });
    `);
    const child = promisify(execFile)('bun', [script], { windowsHide: true });
    // Surface a child launch failure without leaving an unhandled rejection.
    const started = async () => {
      for (let attempt = 0; attempt < 100; attempt++) {
        try { await readFile(entered); return; } catch {}
        await new Promise((done) => setTimeout(done, 10));
      }
      throw new Error('Child did not acquire its fixture lease');
    };
    try {
      await Promise.race([started(), child.then(() => { throw new Error('Child exited before acquiring its fixture lease'); })]);
      const cleaned: RustCacheScope[] = [];
      await expect(withRustCacheLease({ workspaceRoot: root, targetDir: join(main, 'validation'), budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {})).rejects.toThrow(/protected/i);
      expect(cleaned).toEqual([]);
    } finally {
      await writeFile(release, 'done');
      await child;
    }
  });

  it('does not retry the same output through dev/test profile aliases', async () => {
    const root = await fixture();
    await output(root, '', 4);
    await mkdir(join(root, 'target', 'cargo', 'unknown'));
    await writeFile(join(root, 'target', 'cargo', 'unknown', 'keep'), Buffer.alloc(20));
    const cleaned: RustCacheScope[] = [];
    await expect(withRustCacheLease({ workspaceRoot: root, profile: 'test', budgetBytes: 10, dependencies: dependencies(cleaned) }, async () => {})).rejects.toThrow(/budget/i);
    expect(cleaned).toHaveLength(1);
  });

  it('narrows cargo-run protection through Cargo shim ancestors', () => {
    const cache = resolve('workspace', 'target', 'cargo');
    const processes = [
      { pid: 1, parentPid: 0, name: 'cargo.exe', executable: resolve('tools', 'cargo.exe'), command: 'cargo run --manifest-path poc/village/Cargo.toml' },
      { pid: 2, parentPid: 1, name: 'cargo.exe', executable: resolve('toolchain', 'cargo.exe'), command: 'cargo run --manifest-path poc/village/Cargo.toml' },
      { pid: 3, parentPid: 2, name: 'anima-village-game.exe', executable: join(cache, 'debug', 'anima-village-game.exe'), command: 'anima-village-game.exe' },
    ];
    expect(isRustCacheScopeBusy({ targetDir: cache, outputDir: join(cache, 'debug'), profile: 'dev' }, cache, processes)).toBe(true);
    expect(isRustCacheScopeBusy({ targetDir: join(cache, 'validation'), outputDir: join(cache, 'validation') }, cache, processes)).toBe(false);
    expect(isRustCacheScopeBusy({ targetDir: join(cache, 'validation'), outputDir: join(cache, 'validation') }, cache, [...processes, { pid: 4, parentPid: 0, name: 'rustc.exe', executable: resolve('tools', 'rustc.exe'), command: 'rustc --crate-name fixture' }])).toBe(true);
  });

  it('aborts a timed-out cleaner and reports the cache budget failure', async () => {
    const root = await fixture();
    await output(root, '', 20);
    let aborted = false;
    let stopped = false;
    await expect(withRustCacheLease({
      workspaceRoot: root,
      budgetBytes: 10,
      cleanTimeoutMs: 10,
      dependencies: {
        ...dependencies(),
        clean: async (_scope, { signal, timeoutMs }) => {
          expect(timeoutMs).toBe(10);
          await new Promise<void>((_done, reject) => {
            signal.addEventListener('abort', () => {
              aborted = true;
              setTimeout(() => { stopped = true; reject(new Error('Cleaner stopped')); }, 20);
            }, { once: true });
          });
        },
      },
    }, async () => {})).rejects.toThrow(/cleanup failed.*timed out/i);
    expect(aborted).toBe(true);
    expect(stopped).toBe(true);
  });

  it('aborts an over-budget running command and prunes only after cancellation settles', async () => {
    const root = await fixture();
    const main = await output(root, '', 2);
    const cleaned: RustCacheScope[] = [];
    let stopped = false;
    await expect(withRustCacheLease({
      workspaceRoot: root,
      budgetBytes: 5,
      monitorIntervalMs: 5,
      dependencies: {
        ...dependencies(cleaned),
        clean: async (scope) => {
          expect(stopped).toBe(true);
          cleaned.push(scope);
          await rm(scope.outputDir, { recursive: true, force: true });
        },
      },
    }, async ({ signal }) => {
      expect(signal).toBeInstanceOf(AbortSignal);
      await writeFile(join(main, 'debug', 'deps', 'growing.rlib'), Buffer.alloc(20));
      await new Promise<void>((done) => {
        if (signal.aborted) done();
        else signal.addEventListener('abort', () => done(), { once: true });
      });
      expect(signal.reason).toBeInstanceOf(Error);
      await new Promise((done) => setTimeout(done, 10));
      stopped = true;
    })).rejects.toThrow(/budget.*running|running.*budget/i);
    expect(stopped).toBe(true);
    expect(cleaned).toEqual([expect.objectContaining({ targetDir: main, profile: 'dev' })]);
    expect(await measureRustCacheBytes(main)).toBe(0);
    expect(await readdir(join(root, 'target', '.rust-cache-maintenance', 'leases'))).toEqual([]);
  });

  it('defers nested monitoring only to a matching live parent watcher lease', async () => {
    const root = await fixture();
    await output(root, '', 2);
    let measurements = 0;
    const deps = { ...dependencies(), measure: async (directory: string) => { measurements++; return measureRustCacheBytes(directory); } };
    await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, monitorIntervalMs: 1000, dependencies: deps }, async (outer) => {
      expect(outer.env.ANIMA_RUST_CACHE_WATCHER_PID).toBe(String(process.pid));
      expect(outer.env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).toBeTruthy();
      vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_PID', outer.env.ANIMA_RUST_CACHE_WATCHER_PID);
      vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_LEASE_ID', outer.env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID);
      await withRustCacheLease({ workspaceRoot: root, budgetBytes: 10, monitorIntervalMs: 5, dependencies: deps }, async (inner) => {
        expect(inner.env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).toBe(outer.env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID);
        await new Promise((done) => setTimeout(done, 30));
        expect(measurements).toBe(2);
      });
    });
    expect(measurements).toBe(4);
  });

  it('starts a watcher when an inherited live PID has no matching watcher lease', async () => {
    const root = await fixture();
    const main = await output(root, '', 2);
    vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_PID', String(process.pid));
    vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_LEASE_ID', 'missing');
    await expect(withRustCacheLease({ workspaceRoot: root, budgetBytes: 5, monitorIntervalMs: 5, dependencies: dependencies() }, async ({ env, signal }) => {
      expect(signal).toBeInstanceOf(AbortSignal);
      expect(env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).not.toBe('missing');
      await writeFile(join(main, 'debug', 'deps', 'growing.rlib'), Buffer.alloc(20));
      await new Promise<void>((done) => {
        if (signal.aborted) done();
        else signal.addEventListener('abort', () => done(), { once: true });
      });
    })).rejects.toThrow(/budget/i);
  });

  it('fails closed when its running-command cache monitor cannot measure output', async () => {
    const root = await fixture();
    let running = false;
    await expect(withRustCacheLease({
      workspaceRoot: root,
      monitorIntervalMs: 5,
      dependencies: {
        ...dependencies(),
        measure: async () => {
          if (running) throw new Error('fixture disk read failed');
          return 0;
        },
      },
    }, async ({ signal }) => {
      expect(signal).toBeInstanceOf(AbortSignal);
      running = true;
      await new Promise<void>((done) => {
        if (signal.aborted) done();
        else signal.addEventListener('abort', () => done(), { once: true });
      });
      running = false;
    })).rejects.toThrow(/monitor.*fixture disk read failed/i);
  });

  it('keeps an idle runtime lease alive without advertising a watcher', async () => {
    const root = await fixture();
    const main = await output(root, '', 2);
    await expect(withRustCacheLease({ workspaceRoot: root, monitor: false, monitorIntervalMs: 5, budgetBytes: 5, dependencies: dependencies() }, async ({ signal, env }) => {
      expect(env.ANIMA_RUST_CACHE_WATCHER_PID).toBe('');
      expect(env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).toBe('');
      await writeFile(join(main, 'debug', 'deps', 'other-build.rlib'), Buffer.alloc(20));
      await new Promise((done) => setTimeout(done, 30));
      expect(signal.aborted).toBe(false);
    })).rejects.toThrow(/budget.*protected|protected.*budget/i);
    expect(await readFile(join(main, 'debug', 'deps', 'other-build.rlib'))).toHaveLength(20);
  });

  it('promotes a nested lease to watcher when its inherited owner dies', async () => {
    const root = await fixture();
    const main = await output(root, '', 2);
    const leases = join(root, 'target', '.rust-cache-maintenance', 'leases');
    await mkdir(leases, { recursive: true });
    await writeFile(join(leases, 'parent.json'), JSON.stringify({ version: 1, id: 'parent', pid: 1234567, targetDir: main, profile: 'dev', watching: true, budgetBytes: 5 }));
    vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_PID', '1234567');
    vi.stubEnv('ANIMA_RUST_CACHE_WATCHER_LEASE_ID', 'parent');
    let parentAlive = true;
    await expect(withRustCacheLease({
      workspaceRoot: root,
      budgetBytes: 5,
      monitorIntervalMs: 5,
      dependencies: { ...dependencies(), isAlive: (pid) => pid === process.pid || (pid === 1234567 && parentAlive) },
    }, async ({ env, signal }) => {
      expect(env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).toBe('parent');
      parentAlive = false;
      await writeFile(join(main, 'debug', 'deps', 'growing.rlib'), Buffer.alloc(20));
      await new Promise<void>((done) => {
        const timeout = setTimeout(done, 100);
        if (signal.aborted) { clearTimeout(timeout); done(); }
        else signal.addEventListener('abort', () => { clearTimeout(timeout); done(); }, { once: true });
      });
      expect(signal.aborted).toBe(true);
      expect(env.ANIMA_RUST_CACHE_WATCHER_LEASE_ID).not.toBe('parent');
      const records = await readdir(leases);
      expect(records).toHaveLength(1);
      expect(JSON.parse(await readFile(join(leases, records[0]!), 'utf8'))).toMatchObject({ watching: true, pid: process.pid });
    })).rejects.toThrow(/budget.*running|running.*budget/i);
    expect(await measureRustCacheBytes(main)).toBe(0);
  });
});
