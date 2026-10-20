import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  cargoCleanArgs,
  measureRustCacheBytes,
  resolveRustCacheDir,
  rustCacheSizeWarning,
} from './rust-cache.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
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
    expect(rustCacheSizeWarning(64 * 1024 ** 3)).toContain('64 GiB');
    expect(rustCacheSizeWarning(64 * 1024 ** 3 - 1)).toBeNull();
  });
});
