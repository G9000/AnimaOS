// Explicit owner setup. Resolves only the repository's locked Playwright
// installation; never searches PATH for an arbitrary browser executable.
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const packagePath = require.resolve('playwright-core/package.json');
const metadata = require(packagePath);
const registry = require(path.join(path.dirname(packagePath), 'browsers.json'));
const expected = registry.browsers.find(
  (browser) => browser.name === 'chromium',
);

export async function setupBrowser({ install = true } = {}) {
  if (
    process.platform !== 'win32' ||
    ![22, 24].includes(Number(process.versions.node.split('.')[0])) ||
    metadata.version !== '1.64.1' ||
    !expected?.browserVersion
  )
    throw new Error(
      'Browser setup requires Windows, Node 22 or 24, and the locked Playwright-core 1.64.1 package.',
    );
  if (install) {
    const result = spawnSync(
      process.execPath,
      [path.join(path.dirname(packagePath), 'cli.js'), 'install', 'chromium'],
      { cwd: root, stdio: 'inherit', windowsHide: true, shell: false },
    );
    if (result.error || result.status !== 0)
      throw new Error('The maintained Chromium installation did not complete.');
  }
  const { chromium } = await import('playwright-core');
  const browserExecutable = chromium.executablePath();
  let probe;
  try {
    probe = await chromium.launch({
      executablePath: browserExecutable,
      headless: true,
      chromiumSandbox: true,
      timeout: 10000,
    });
    if (probe.version() !== expected.browserVersion)
      throw new Error(
        `Expected the maintained full Chromium ${expected.browserVersion}. Run setup again to repair the paired installation.`,
      );
  } finally {
    await probe?.close();
  }
  const hash = async (file) => {
    const stat = await fs.lstat(file);
    if (
      !stat.isFile() ||
      stat.isSymbolicLink() ||
      stat.size === 0 ||
      stat.size > 512 * 1024 * 1024
    )
      throw new Error('Browser setup refused an invalid program file.');
    return createHash('sha256')
      .update(await fs.readFile(file))
      .digest('hex');
  };
  const workerScript = path.join(
    root,
    'packages/core-rust/browser-worker/worker.mjs',
  );
  const installation = {
    id: 'maintained-chromium',
    nodeExecutable: process.execPath,
    nodeSha256: await hash(process.execPath),
    workerScript,
    workerSha256: await hash(workerScript),
    browserExecutable,
    browserSha256: await hash(browserExecutable),
  };
  if (!process.env.LOCALAPPDATA)
    throw new Error('The owner application data folder is unavailable.');
  const destination = path.join(process.env.LOCALAPPDATA, 'AnimaOS', 'browser');
  await fs.mkdir(destination, { recursive: true, mode: 0o700 });
  for (let directory = destination; ; directory = path.dirname(directory)) {
    if ((await fs.lstat(directory)).isSymbolicLink())
      throw new Error('Browser setup refuses linked application data folders.');
    if (path.dirname(directory) === directory) break;
  }
  const target = path.join(destination, 'setup.json');
  await fs.writeFile(
    target,
    JSON.stringify({ schemaVersion: 1, installation, policies: [] }, null, 2),
    { mode: 0o600 },
  );
  return { path: target, browserVersion: expected.browserVersion };
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]).toLowerCase() ===
    fileURLToPath(import.meta.url).toLowerCase()
) {
  try {
    const result = await setupBrowser({
      install: !process.argv.includes('--check'),
    });
    process.stdout.write(
      `Maintained Chromium ${result.browserVersion} is ready. Restart AnimaOS, then add a destination in Browser task.\n`,
    );
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
