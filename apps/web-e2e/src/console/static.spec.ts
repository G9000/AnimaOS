import { test, expect } from '@playwright/test';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { staticAsset } from './fixtures/static';

test('static asset MIME, traversal, missing files and symlink containment', async () => {
  const tmp = await mkdtemp(join(tmpdir(), 'anima-static-'));
  const dist = join(tmp, 'dist'),
    outside = join(tmp, 'outside');
  try {
    await mkdir(dist);
    await mkdir(outside);
    await writeFile(join(dist, 'index.html'), '<html>fixture</html>');
    await writeFile(join(dist, 'app.js'), 'export {}');
    await writeFile(join(outside, 'secret.js'), 'outside');
    await symlink(
      outside,
      join(dist, 'escape'),
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    expect((await staticAsset('/', dist)).contentType).toBe('text/html');
    expect((await staticAsset('/app.js', dist)).contentType).toBe(
      'text/javascript',
    );
    for (const path of [
      '/../outside/secret.js',
      '/%2e%2e/outside/secret.js',
      '/C:/secret.js',
      '/..\\outside',
      '/escape/secret.js',
      '/missing.css',
    ])
      await expect(staticAsset(path, dist)).rejects.toThrow();
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
});
