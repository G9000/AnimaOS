import { execFile } from 'node:child_process';
import { lstat, mkdir, mkdtemp, realpath, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

export interface PrivateTestHome {
  readonly home: string;
  readonly workspace: string;
  readonly profile: string;
  cleanup(): Promise<void>;
}

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const target = resolve(
  repository,
  process.env.CARGO_TARGET_DIR ?? 'target/cargo/validation',
);
const windowsPeer = join(
  target,
  'debug',
  'examples',
  'private_test_home_peer.exe',
);
let windowsPeerPreparation: Promise<void> | undefined;

/** Prepare under the SDK test script's managed validation cache, before runtime deadlines. */
export function preparePrivateTestHome(): Promise<void> {
  if (process.platform !== 'win32') return Promise.resolve();
  windowsPeerPreparation ??= (async () => {
    await promisify(execFile)(
      'cargo',
      [
        'build',
        '--manifest-path',
        join(repository, 'Cargo.toml'),
        '-p',
        'anima-daemon',
        '--example',
        'private_test_home_peer',
        '--target-dir',
        target,
      ],
      {
        cwd: repository,
        windowsHide: true,
        timeout: 300_000,
        maxBuffer: 8 * 1024 * 1024,
      },
    );
    if (!(await stat(windowsPeer)).isFile())
      throw new Error('Private home peer binary is unavailable');
  })();
  return windowsPeerPreparation;
}

async function windowsHome(
  operation: 'create' | 'inspect',
  root: string,
  home: string,
): Promise<void> {
  await preparePrivateTestHome();
  const systemEnvironment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) =>
      /^(PATH|SYSTEMROOT|WINDIR|TEMP|TMP)$/i.test(key),
    ),
  );
  await promisify(execFile)(windowsPeer, [operation, root, home], {
    shell: false,
    windowsHide: true,
    timeout: 15000,
    maxBuffer: 1024 * 1024,
    env: systemEnvironment,
  });
}

export async function createPrivateTestHome(
  prefix = 'anima-private-test-',
): Promise<PrivateTestHome> {
  if (!/^[a-z0-9][a-z0-9-]*-$/i.test(prefix))
    throw new Error('Invalid private test home prefix');
  // Existing callers also prepare lazily, independent of test file order.
  await preparePrivateTestHome();
  const parent = await realpath(tmpdir());
  const root = await mkdtemp(join(parent, prefix));
  const identity = await lstat(root, { bigint: true });
  const home = join(root, 'home');
  const workspace = join(home, 'workspace');
  const profile = join(home, 'profile');
  let removed = false;

  async function verifyOwnedRoot() {
    const current = await lstat(root, { bigint: true });
    if (
      dirname(root) !== parent ||
      !basename(root).startsWith(prefix) ||
      !current.isDirectory() ||
      current.isSymbolicLink() ||
      (await realpath(root)) !== root ||
      current.dev !== identity.dev ||
      current.ino !== identity.ino
    )
      throw new Error('Private test home root was replaced or redirected');
  }

  async function cleanup() {
    if (removed) return;
    await verifyOwnedRoot();
    if (process.platform === 'win32') await windowsHome('inspect', root, home);
    await verifyOwnedRoot();
    await rm(root, { recursive: true, maxRetries: 3, retryDelay: 100 });
    removed = true;
  }

  try {
    await verifyOwnedRoot();
    if (process.platform === 'win32') await windowsHome('create', root, home);
    else await mkdir(home, { mode: 0o700 });
    await mkdir(workspace, { mode: 0o700 });
    await mkdir(profile, { mode: 0o700 });
    return { home, workspace, profile, cleanup };
  } catch (error) {
    await cleanup();
    throw error;
  }
}
