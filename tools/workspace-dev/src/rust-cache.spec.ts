import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import * as filesystem from 'node:fs/promises';
import type { PathLike } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('node:fs/promises', async (importOriginal) => {
  const original = await importOriginal<typeof import('node:fs/promises')>();
  return { ...original, stat: vi.fn(original.stat) };
});

import {
  cargoCleanArgs,
  measureRustCacheBytes,
  resolveRustCacheDir,
  rustCacheSizeWarning,
} from './rust-cache.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { recursive: true, force: true }),
    ),
  );
});

describe('Rust build cache maintenance', () => {
  it('keeps Cargo output in the dedicated cache subdirectory', () => {
    const workspaceRoot = resolve('C:/workspace/animaos-kit');

    expect(resolveRustCacheDir(workspaceRoot)).toBe(
      resolve(workspaceRoot, 'target', 'cargo'),
    );
    expect(cargoCleanArgs(workspaceRoot)).toEqual([
      'clean',
      '--manifest-path',
      resolve(workspaceRoot, 'Cargo.toml'),
      '--target-dir',
      resolve(workspaceRoot, 'target', 'cargo'),
    ]);
  });

  it('measures files under the cache without counting its parent directory', async () => {
    const root = await mkdtemp(join(tmpdir(), 'anima-rust-cache-'));
    temporaryDirectories.push(root);
    const cache = resolveRustCacheDir(root);
    await mkdir(join(cache, 'debug', 'deps'), { recursive: true });
    await mkdir(join(root, 'target', 'notes'), { recursive: true });
    await writeFile(join(cache, 'debug', 'deps', 'library.rlib'), 'cargo');
    await writeFile(join(root, 'target', 'notes', 'keep.log'), 'user data');

    expect(await measureRustCacheBytes(cache)).toBe(5);
  });

  it('warns at the cache budget and stays quiet below it', () => {
    expect(rustCacheSizeWarning(32 * 1024 ** 3)).toContain('32 GiB');
    expect(rustCacheSizeWarning(32 * 1024 ** 3 - 1)).toBeNull();
  });

  it('tolerates output disappearing while a concurrent build or prune changes it', async () => {
    const root = await mkdtemp(join(tmpdir(), 'anima-rust-cache-race-'));
    temporaryDirectories.push(root);
    await mkdir(join(root, 'debug'));
    const gone = join(root, 'debug', 'gone.rlib');
    await writeFile(gone, 'gone');
    const originalStat = (await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises')).stat;
    vi.mocked(filesystem.stat).mockImplementation((async (path: PathLike) => {
      if (path === gone) {
        await rm(gone);
        throw Object.assign(new Error('file disappeared'), { code: 'ENOENT' });
      }
      return originalStat(path);
    }) as typeof filesystem.stat);
    expect(await measureRustCacheBytes(root)).toBe(0);
  });
});
