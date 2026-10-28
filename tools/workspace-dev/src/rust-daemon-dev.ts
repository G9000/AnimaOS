import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Debug builds are ad-hoc signed, so macOS binds keychain "Always Allow" to one
// binary hash and forgets it on the next build. Re-signing every build with an
// owner-created certificate keeps the designated requirement stable.
export const SIGNING_IDENTITY_ENV = 'ANIMAOS_DEV_SIGNING_IDENTITY';
export const DAEMON_PACKAGE = 'anima-daemon';
export const DAEMON_SIGNING_IDENTIFIER = 'dev.animaos.anima-daemon';

export type DaemonDevMode = 'run' | 'build';

export function resolveSigningIdentity(
  env: NodeJS.ProcessEnv,
  platform: NodeJS.Platform
): string | null {
  if (platform !== 'darwin') {
    return null;
  }

  const identity = env[SIGNING_IDENTITY_ENV]?.trim();
  return identity ? identity : null;
}

export function codesignArgs(identity: string, executable: string): string[] {
  return [
    '--force',
    '--sign',
    identity,
    '--identifier',
    DAEMON_SIGNING_IDENTIFIER,
    executable,
  ];
}

/** Finds the daemon binary in `cargo build --message-format=json` output. */
export function findDaemonExecutable(cargoJsonOutput: string): string | null {
  let executable: string | null = null;

  for (const line of cargoJsonOutput.split('\n')) {
    if (!line.startsWith('{')) {
      continue;
    }

    try {
      const message = JSON.parse(line) as {
        reason?: string;
        target?: { name?: string; kind?: string[] };
        executable?: string | null;
      };

      if (
        message.reason === 'compiler-artifact' &&
        message.target?.name === DAEMON_PACKAGE &&
        message.target.kind?.includes('bin') &&
        message.executable
      ) {
        executable = message.executable;
      }
    } catch {
      // Cargo only emits JSON lines on stdout; ignore anything else.
    }
  }

  return executable;
}

export function parseDaemonDevArgs(argv: string[]): {
  mode: DaemonDevMode;
  daemonArgs: string[];
} {
  const separator = argv.indexOf('--');
  const own = separator >= 0 ? argv.slice(0, separator) : argv;
  const daemonArgs = separator >= 0 ? argv.slice(separator + 1) : [];

  return {
    mode: own.includes('--build-only') ? 'build' : 'run',
    daemonArgs,
  };
}

function buildDaemon(): string {
  const result = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL('../scripts/run-task.ts', import.meta.url)),
      '--cwd', process.cwd(), '--rust-cache', 'dev', '--', 'cargo',
      'build',
      '-p',
      DAEMON_PACKAGE,
      '--bin',
      DAEMON_PACKAGE,
      '--message-format=json-render-diagnostics',
    ],
    {
      cwd: process.cwd(),
      env: process.env,
      stdio: ['inherit', 'pipe', 'inherit'],
      maxBuffer: 256 * 1024 * 1024,
      encoding: 'utf8',
    }
  );

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    throw new Error(`cargo build -p ${DAEMON_PACKAGE} failed`);
  }

  const executable = findDaemonExecutable(result.stdout);

  if (!executable) {
    throw new Error(`cargo build did not report the ${DAEMON_PACKAGE} binary`);
  }

  return executable;
}

function signDaemon(executable: string): void {
  const identity = resolveSigningIdentity(process.env, process.platform);

  if (!identity) {
    return;
  }

  const result = spawnSync('codesign', codesignArgs(identity, executable), {
    stdio: ['ignore', 'inherit', 'pipe'],
    encoding: 'utf8',
  });

  if (result.status === 0) {
    console.log(`Signed ${DAEMON_PACKAGE} with "${identity}".`);
    return;
  }

  // An unsigned daemon still works; it only brings keychain prompts back.
  const detail = (result.stderr || result.error?.message || '').trim();
  console.warn(
    `Warning: could not sign ${DAEMON_PACKAGE} with ${SIGNING_IDENTITY_ENV}="${identity}"; ` +
      `keychain prompts will return after each rebuild.${detail ? `\n${detail}` : ''}`
  );
}

function runDaemon(executable: string, args: string[]): void {
  const child = spawn(executable, args, {
    cwd: process.cwd(),
    env: process.env,
    stdio: 'inherit',
  });

  const forward = (signal: NodeJS.Signals) => {
    child.kill(signal);
  };
  process.on('SIGINT', forward);
  process.on('SIGTERM', forward);

  child.once('exit', (code, signal) => {
    process.removeListener('SIGINT', forward);
    process.removeListener('SIGTERM', forward);

    if (signal) {
      process.kill(process.pid, signal);
      return;
    }

    process.exit(code ?? 1);
  });

  child.once('error', (error) => {
    console.error(error);
    process.exit(1);
  });
}

export function main(argv: string[]): void {
  const { mode, daemonArgs } = parseDaemonDevArgs(argv);
  const executable = buildDaemon();
  signDaemon(executable);

  if (mode === 'run') {
    runDaemon(executable, daemonArgs);
  }
}

const isEntrypoint = (
  import.meta as ImportMeta & {
    main?: boolean;
  }
).main === true;

if (isEntrypoint) {
  try {
    main(process.argv.slice(2));
  } catch (error: unknown) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
