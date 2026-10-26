import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { consoleLaunchUrl, prepareOwnerCredentials } from './owner-console.js';
import {
  getHostDefinition,
  hostProcessEnv,
  type HostDefinition,
  type HostKey,
} from './hosts.js';
import {
  runManagedProcesses,
  type ManagedProcessDefinition,
} from './process.js';

export interface WorkspaceDevPlan {
  host: HostDefinition;
  processes: ManagedProcessDefinition[];
}

export type DevSurface = 'app' | 'docs' | 'game' | 'playground' | 'all';

export interface WorkspaceDevPlanOptions {
  surface?: DevSurface;
  reuseExistingHost?: boolean;
  env?: NodeJS.ProcessEnv;
  homeDir?: string;
  ownerEnv?: Record<string, string>;
}

export function parseHostArg(argv: string[]): HostKey {
  const hostFlagIndex = argv.findIndex((value) => value === '--host');
  const nextValue = hostFlagIndex >= 0 ? argv[hostFlagIndex + 1] : undefined;

  if (!nextValue) {
    return 'rust';
  }

  return nextValue as HostKey;
}

export function buildWorkspaceDevPlan(
  hostName: string,
  options: WorkspaceDevPlanOptions = {},
): WorkspaceDevPlan {
  const host = getHostDefinition(hostName);

  const surface = options.surface ?? 'app';
  const assistant =
    surface === 'app' || surface === 'playground' || surface === 'all';
  const processes: ManagedProcessDefinition[] = [];

  if (assistant && !options.reuseExistingHost) {
    processes.push({
      name: host.projectName,
      command: 'bun',
      args: ['run', '--filter', host.projectName, 'dev'],
      env: {
        ...hostProcessEnv(host, options.env, options.homeDir),
        ...options.ownerEnv,
      },
    });
  }

  const browserEnv = { UI_BACKEND_ORIGIN: host.baseUrl, VITE_HOST_KEY: host.key };
  if (surface === 'app' || surface === 'all') {
    processes.push({
      name: '@animaos/web',
      command: 'bun',
      args: ['run', '--filter', '@animaos/web', 'dev'],
      env: browserEnv,
    });
  }
  if (surface === 'playground' || surface === 'all') {
    processes.push({
      name: '@animaos/playground',
      command: 'bun',
      args: ['run', '--filter', '@animaos/playground', 'dev'],
      env: browserEnv,
    });
  }
  if (surface === 'docs' || surface === 'all') {
    for (const name of ['@animaos/docs', '@animaos/contributor-docs']) {
      processes.push({
        name,
        command: 'bun',
        args: ['run', '--filter', name, 'dev'],
      });
    }
  }
  if (surface === 'game' || surface === 'all') {
    processes.push({
      name: 'Village game',
      command: 'bun',
      args: ['run', 'dev'],
      cwd: fileURLToPath(new URL('../../../poc/village/', import.meta.url)),
    });
  }

  return {
    host,
    processes,
  };
}

function parseSurface(argv: string[]): DevSurface {
  const flags = [
    ['--docs', 'docs'],
    ['--game', 'game'],
    ['--playground', 'playground'],
    ['--all', 'all'],
  ] as const;
  const selected = flags.filter(([flag]) => argv.includes(flag));
  if (selected.length > 1) {
    throw new Error(
      'Choose one development service: --docs, --game, --playground or --all.',
    );
  }
  return selected[0]?.[1] ?? 'app';
}

export async function run(argv: string[]): Promise<void> {
  const surface = parseSurface(argv);
  const hostName = parseHostArg(argv);
  const host = getHostDefinition(hostName);
  if (surface === 'docs' || surface === 'game') {
    if (argv.includes('--console')) {
      throw new Error('--console requires the app or playground development service.');
    }
    await runManagedProcesses(
      buildWorkspaceDevPlan(hostName, { surface }).processes,
    );
    return;
  }
  const reuseExistingHost = await isHostReachable(host.baseUrl);
  const consoleOnly = argv.includes('--console');
  if (consoleOnly && !reuseExistingHost)
    throw new Error(
      'Start bun dev before requesting a fresh owner console link.',
    );
  let owner: ReturnType<typeof prepareOwnerCredentials>;
  try {
    owner = prepareOwnerCredentials(process.env, homedir(), !reuseExistingHost);
  } catch (error) {
    if (!reuseExistingHost) throw error;
    throw new Error(
      `A daemon is already running at ${host.baseUrl}, but bun dev cannot load its owner credentials. ` +
        'Stop the running daemon with its launcher, then run bun dev again; or set ANIMA_LOCAL_ADMIN_TOKEN_FILE to the token file it uses.',
    );
  }
  if (
    reuseExistingHost &&
    !(await acceptsOwnerCredentials(
      host.baseUrl,
      owner.token,
      process.env.ANIMAOS_RS_API_KEY,
    ))
  ) {
    throw new Error(
      `The daemon at ${host.baseUrl} does not accept the configured owner credentials. ` +
        'Set ANIMA_LOCAL_ADMIN_TOKEN_FILE to its matching token file, or stop the running daemon with its launcher before starting bun dev.',
    );
  }
  const consoleOrigin = surface === 'playground'
    ? 'http://localhost:4201'
    : 'http://localhost:4200';
  const showConsole = () => {
    console.log(
      'Owner console link (expires in 15 seconds; refresh with bun dev --console):',
    );
    console.log(consoleLaunchUrl(consoleOrigin, owner.token));
  };
  if (consoleOnly) {
    showConsole();
    return;
  }
  const plan = buildWorkspaceDevPlan(hostName, {
    surface,
    reuseExistingHost,
    ownerEnv: owner.daemonEnv,
  });
  const stopAnnouncement = new AbortController();
  const announcement = announceWhenReady(
    consoleOrigin,
    stopAnnouncement.signal,
    showConsole,
  );
  try {
    await runManagedProcesses(plan.processes);
  } finally {
    stopAnnouncement.abort();
    await announcement;
  }
}

async function announceWhenReady(
  origin: string,
  signal: AbortSignal,
  announce: () => void,
): Promise<void> {
  // The launch TTL starts only after Vite and its daemon proxy are ready.
  while (!signal.aborted) {
    if (await isHostReachable(origin)) {
      if (!signal.aborted) announce();
      return;
    }
    await new Promise<void>((resolve) => {
      const finish = () => {
        clearTimeout(timer);
        signal.removeEventListener('abort', finish);
        resolve();
      };
      const timer = setTimeout(finish, 250);
      signal.addEventListener('abort', finish, { once: true });
      if (signal.aborted) finish();
    });
  }
}

async function isHostReachable(baseUrl: string): Promise<boolean> {
  try {
    const response = await fetch(`${baseUrl}/api/health`, {
      signal: AbortSignal.timeout(2_000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

async function acceptsOwnerCredentials(
  baseUrl: string,
  token: string,
  apiKey?: string,
): Promise<boolean> {
  try {
    const headers = new Headers({ authorization: `Bearer ${token}` });
    if (apiKey) headers.set('x-api-key', apiKey);
    const response = await fetch(`${baseUrl}/api/system/drain`, {
      headers,
      redirect: 'error',
      signal: AbortSignal.timeout(2_000),
    });
    if (!response.ok) return false;
    const body: unknown = await response.json();
    return (
      typeof body === 'object' &&
      body !== null &&
      'draining' in body &&
      typeof body.draining === 'boolean' &&
      'inFlight' in body &&
      typeof body.inFlight === 'object' &&
      body.inFlight !== null &&
      !Array.isArray(body.inFlight)
    );
  } catch {
    return false;
  }
}

const isEntrypoint =
  (
    import.meta as ImportMeta & {
      main?: boolean;
    }
  ).main === true;

if (isEntrypoint) {
  void run(process.argv.slice(2)).catch((error: unknown) => {
    const message =
      error instanceof Error ? error.message : 'workspace-dev failed';
    console.error(message);
    process.exitCode = 1;
  });
}
