import { readdir, stat } from 'node:fs/promises';
import { resolve } from 'node:path';

export const RUST_CACHE_WARNING_BYTES = 32 * 1024 ** 3;

export function resolveRustCacheDir(workspaceRoot: string): string {
  return resolve(workspaceRoot, 'target', 'cargo');
}

export function cargoCleanArgs(workspaceRoot: string): string[] {
  return [
    'clean',
    '--manifest-path',
    resolve(workspaceRoot, 'Cargo.toml'),
    '--target-dir',
    resolveRustCacheDir(workspaceRoot),
  ];
}

export async function measureRustCacheBytes(cacheDir: string): Promise<number> {
  let total = 0;
  const pending = [cacheDir];

  while (pending.length > 0) {
    const directory = pending.pop();
    if (!directory) continue;

    let entries;
    try {
      entries = await readdir(directory, { withFileTypes: true });
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') continue;
      throw error;
    }

    for (const entry of entries) {
      const entryPath = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        pending.push(entryPath);
      } else if (entry.isFile()) {
        try {
          total += (await stat(entryPath)).size;
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        }
      }
    }
  }

  return total;
}

export function rustCacheSizeWarning(bytes: number): string | null {
  if (bytes < RUST_CACHE_WARNING_BYTES) return null;

  const gib = (bytes / 1024 ** 3).toFixed(1);
  return `Rust build cache is ${gib} GiB by summed file size (warning threshold: 32 GiB). Run bun run rust:cache:clean to remove compiler outputs; files outside target/cargo are preserved.`;
}
