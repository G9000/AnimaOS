import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import {
  cargoCleanArgs,
  measureRustCacheBytes,
  resolveRustCacheDir,
  rustCacheSizeWarning,
} from '../src/rust-cache.js';

const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const cacheDir = resolveRustCacheDir(workspaceRoot);
const command = process.argv[2] ?? 'status';

if (command === 'status') {
  const bytes = existsSync(cacheDir)
    ? await measureRustCacheBytes(cacheDir)
    : 0;
  console.log(`Rust build cache: ${(bytes / 1024 ** 3).toFixed(1)} GiB`);
  const warning = rustCacheSizeWarning(bytes);
  if (warning) console.warn(warning);
} else if (command === 'clean') {
  if (!existsSync(cacheDir)) {
    console.log('Rust build cache is already empty.');
  } else {
    const result = spawnSync('cargo', cargoCleanArgs(workspaceRoot), {
      cwd: workspaceRoot,
      stdio: 'inherit',
      windowsHide: true,
    });
    if (result.error) throw result.error;
    process.exitCode = result.status ?? 1;
  }
} else {
  console.error('Usage: bun tools/workspace-dev/scripts/rust-cache.ts <status|clean>');
  process.exitCode = 2;
}
