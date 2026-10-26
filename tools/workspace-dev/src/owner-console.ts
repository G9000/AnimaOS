import { execFile } from 'node:child_process';
import { createHmac, randomBytes } from 'node:crypto';
import {
  closeSync,
  fstatSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { dirname, win32 } from 'node:path';
import { platform } from 'node:os';
import { defaultControlPlaneFile } from './hosts.js';

export interface OwnerCredentials {
  token: string;
  daemonEnv: Record<string, string>;
}

function validToken(value: string): string {
  const token = value.trim();
  if (!token || token.length > 4096)
    throw new Error('A valid local owner token is required.');
  return token;
}

function readToken(file: string): string {
  try {
    const entry = lstatSync(file);
    if (
      !entry.isFile() ||
      entry.isSymbolicLink() ||
      entry.nlink !== 1 ||
      entry.size > 4096
    )
      throw new Error('invalid token file');
    return validToken(readFileSync(file, 'utf8'));
  } catch {
    throw new Error(
      'The local owner token file is unavailable or invalid. Check ANIMA_LOCAL_ADMIN_TOKEN_FILE and the token file contents and permissions.',
    );
  }
}

/** Native launcher only. Owner credentials are never VITE variables or served files. */
export function prepareOwnerCredentials(
  env: NodeJS.ProcessEnv,
  homeDir: string,
  create: boolean,
): OwnerCredentials {
  if (env.ANIMA_LOCAL_ADMIN_TOKEN_FILE !== undefined) {
    const file = env.ANIMA_LOCAL_ADMIN_TOKEN_FILE;
    return {
      token: readToken(file),
      daemonEnv: { ANIMA_LOCAL_ADMIN_TOKEN_FILE: file },
    };
  }
  if (env.ANIMA_LOCAL_ADMIN_TOKEN !== undefined) {
    return {
      token: validToken(env.ANIMA_LOCAL_ADMIN_TOKEN),
      daemonEnv: { ANIMA_LOCAL_ADMIN_TOKEN: env.ANIMA_LOCAL_ADMIN_TOKEN },
    };
  }
  const file =
    (env.ANIMAOS_RS_CONTROL_PLANE_FILE ?? defaultControlPlaneFile(homeDir)) +
    '.admin-token';
  if (create) {
    mkdirSync(dirname(file), { recursive: true, mode: 0o700 });
    let descriptor: number | undefined;
    try {
      descriptor = openSync(file, 'wx', 0o600);
      if (!fstatSync(descriptor).isFile())
        throw new Error('invalid owner token file');
      writeFileSync(descriptor, randomBytes(32).toString('hex'));
    } catch (error) {
      if (!(
        error instanceof Error &&
        'code' in error &&
        error.code === 'EEXIST'
      ))
        throw error;
    } finally {
      if (descriptor !== undefined) closeSync(descriptor);
    }
  }
  return {
    token: readToken(file),
    daemonEnv: { ANIMA_LOCAL_ADMIN_TOKEN_FILE: file },
  };
}

/** Same HMAC domain as anima_daemon::console_session::launch_code. */
export function consoleLaunchUrl(
  consoleOrigin: string,
  token: string,
  issued = Math.floor(Date.now() / 1000),
  id = randomBytes(16).toString('hex'),
): string {
  const url = new URL(consoleOrigin);
  if (
    url.protocol !== 'http:' ||
    !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) ||
    url.username ||
    url.password
  )
    throw new Error('The owner console must use a loopback HTTP origin.');
  const mac = createHmac('sha256', validToken(token))
    .update('anima-console-v1|launch|' + issued + '|' + id)
    .digest('hex');
  url.searchParams.set('anima-login', issued + '.' + id + '.' + mac);
  return url.toString();
}

/** Native browser dispatch; launch secrets are never copied into an error message. */
export async function openConsoleBrowser(address: string): Promise<void> {
  const url = new URL(address);
  if (
    url.protocol !== 'http:' ||
    !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) ||
    url.username ||
    url.password
  )
    throw new Error('The owner console must use a loopback HTTP origin.');

  let command: string;
  let args: string[];
  const currentPlatform = platform();
  if (currentPlatform === 'win32') {
    command = win32.join(
      process.env.SystemRoot || 'C:/Windows',
      'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe',
    );
    // Start-Process uses the registered URL handler. Encode the script to keep
    // PowerShell parsing separate from native argument quoting; quote the URL as data.
    const script = "Start-Process -FilePath '" + url.toString().replaceAll("'", "''") + "' -ErrorAction Stop";
    args = [
      '-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-EncodedCommand',
      Buffer.from(script, 'utf16le').toString('base64'),
    ];
  } else {
    command = currentPlatform === 'darwin' ? '/usr/bin/open' : 'xdg-open';
    args = [url.toString()];
  }
  await new Promise<void>((resolve, reject) => {
    execFile(command, args, { shell: false, windowsHide: true, timeout: 10_000 }, error => {
      if (error) reject(new Error('Could not open the local console browser.'));
      else resolve();
    });
  });
}
