import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const dependencies = vi.hoisted(() => ({ home: vi.fn(), processes: vi.fn() }));
vi.mock('node:os', async (importOriginal) => ({
  ...(await importOriginal<typeof import('node:os')>()),
  homedir: dependencies.home,
}));
vi.mock('./process.js', () => ({
  runManagedProcesses: dependencies.processes,
}));

import { run } from './main.js';
import { prepareOwnerCredentials } from './owner-console.js';

let home: string;
let log: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), 'anima-dev-startup-'));
  dependencies.home.mockReturnValue(home);
  dependencies.processes.mockReset();
  vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN_FILE', undefined);
  vi.stubEnv('ANIMA_LOCAL_ADMIN_TOKEN', undefined);
  vi.stubEnv('ANIMAOS_RS_CONTROL_PLANE_FILE', undefined);
  vi.stubEnv('ANIMAOS_RS_API_KEY', undefined);
  log = vi.spyOn(console, 'log').mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  rmSync(home, { recursive: true, force: true });
});

function reachableDaemon(ownerAccepted: boolean) {
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: string | URL, init?: RequestInit) => {
      const url = String(input);
      requests.push({ url, init });
      if (url.endsWith('/api/health')) return Response.json({ status: 'ok' });
      return ownerAccepted
        ? Response.json({ draining: false, inFlight: {} })
        : Response.json(
            { error: 'local owner authorization required' },
            { status: 403 },
          );
    }),
  );
  return requests;
}

describe('existing daemon owner bootstrap', () => {
  it('explains that the running daemon must stop when its owner file is missing', async () => {
    reachableDaemon(false);
    await expect(run(['--console'])).rejects.toThrow(/Stop the running daemon/);
    expect(log).not.toHaveBeenCalled();
    expect(dependencies.processes).not.toHaveBeenCalled();
  });

  it('refuses to print a signed link when the running daemon uses another key', async () => {
    prepareOwnerCredentials({}, home, true);
    reachableDaemon(false);
    await expect(run(['--console'])).rejects.toThrow(
      /does not accept.*owner credentials/,
    );
    expect(log).not.toHaveBeenCalled();
    expect(dependencies.processes).not.toHaveBeenCalled();
  });

  it('verifies native owner access without browser metadata or redirect forwarding', async () => {
    const owner = prepareOwnerCredentials({}, home, true);
    const requests = reachableDaemon(true);
    await run(['--console']);
    const probe = requests.find(({ url }) => url.endsWith('/api/system/drain'));
    expect(probe).toBeDefined();
    const headers = new Headers(probe?.init?.headers);
    expect(headers.get('authorization')).toBe(`Bearer ${owner.token}`);
    expect(headers.has('origin')).toBe(false);
    expect(probe?.init?.redirect).toBe('error');
    expect(log).toHaveBeenCalledWith(expect.stringContaining('anima-login='));
    expect(dependencies.processes).not.toHaveBeenCalled();
  });

  it('keeps the optional API key separate from the owner credential', async () => {
    const owner = prepareOwnerCredentials({}, home, true);
    vi.stubEnv('ANIMAOS_RS_API_KEY', 'synthetic-api-key');
    const requests = reachableDaemon(true);
    await run(['--console']);
    const headers = new Headers(
      requests.find(({ url }) => url.endsWith('/api/system/drain'))?.init
        ?.headers,
    );
    expect(headers.get('authorization')).toBe(`Bearer ${owner.token}`);
    expect(headers.get('x-api-key')).toBe('synthetic-api-key');
  });
});


describe('independent development services', () => {
  it.each(['--docs', '--game'])('%s skips daemon probes and owner bootstrap', async flag => {
    const fetch = vi.fn().mockRejectedValue(new Error('No daemon needed'));
    vi.stubGlobal('fetch', fetch);
    dependencies.processes.mockResolvedValue(undefined);
    await run([flag]);
    expect(fetch).not.toHaveBeenCalled();
    expect(dependencies.processes).toHaveBeenCalledOnce();
    expect(dependencies.processes.mock.calls[0]?.[0].map((p: { name: string }) => p.name)).toEqual(
      flag === '--docs' ? ['@animaos/docs', '@animaos/contributor-docs'] : ['Village game'],
    );
    expect(log).not.toHaveBeenCalled();
  });

  it('rejects conflicting service selections before startup', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('No network')));
    await expect(run(['--docs', '--game'])).rejects.toThrow(/Choose one/);
    expect(dependencies.processes).not.toHaveBeenCalled();
  });
});
