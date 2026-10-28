import { execFile, spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { lstat, mkdir, readdir, readFile, readlink, realpath, rename, rm, writeFile } from 'node:fs/promises';
import { basename, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { promisify } from 'node:util';

import { measureRustCacheBytes, resolveRustCacheDir, RUST_CACHE_WARNING_BYTES } from './rust-cache.js';

const execFileAsync = promisify(execFile);
const GIB = 1024 ** 3;
export const DEFAULT_RUST_CACHE_BUDGET_BYTES = RUST_CACHE_WARNING_BYTES;
export const RUST_CACHE_BUDGET_ENV = 'ANIMA_RUST_CACHE_BUDGET_GIB';
export const DEFAULT_RUST_CACHE_CLEAN_TIMEOUT_MS = 120_000;
export const DEFAULT_RUST_CACHE_MONITOR_INTERVAL_MS = 15_000;
export const RUST_CACHE_WATCHER_PID_ENV = 'ANIMA_RUST_CACHE_WATCHER_PID';
export const RUST_CACHE_WATCHER_LEASE_ENV = 'ANIMA_RUST_CACHE_WATCHER_LEASE_ID';
const MAX_BUDGET_BYTES = DEFAULT_RUST_CACHE_BUDGET_BYTES;
const CHILD_TARGETS = new Set([
  'validation', 'validation-chat-controls', 'validation-chat-suite',
  'validation-document-renderer', 'validation-documents', 'validation-rust-daemon',
  'workspace-dev-fixture', 'sdk-daemon-integration', 'flycheck0',
]);

export function resolveRustCacheBudgetBytes(value: string | undefined): number {
  if (value === undefined) return DEFAULT_RUST_CACHE_BUDGET_BYTES;
  const gib = value.trim() === '' ? Number.NaN : Number(value);
  if (!Number.isFinite(gib) || gib <= 0) {
    throw new Error(`${RUST_CACHE_BUDGET_ENV} cache budget must be a finite positive GiB value.`);
  }
  return Math.min(gib, MAX_BUDGET_BYTES / GIB) * GIB;
}

export interface RustCacheScope {
  targetDir: string;
  outputDir: string;
  profile?: string;
}

export interface RustCacheDependencies {
  measure: (directory: string) => Promise<number>;
  clean: (scope: RustCacheScope, options: { signal: AbortSignal; timeoutMs: number }) => Promise<void>;
  isAlive: (pid: number) => boolean;
  isTargetBusy: (scope: RustCacheScope) => Promise<boolean>;
}

export interface RustCacheLeaseOptions {
  workspaceRoot: string;
  targetDir?: string;
  profile?: string;
  budgetBytes?: number;
  cleanTimeoutMs?: number;
  monitor?: boolean;
  monitorIntervalMs?: number;
  dependencies?: Partial<RustCacheDependencies>;
}

export interface RustCacheLease {
  targetDir: string;
  env: Record<string, string> & { CARGO_TARGET_DIR: string };
  signal: AbortSignal;
}

interface LeaseRecord {
  version: 1;
  id: string;
  pid: number;
  targetDir: string;
  profile: string;
  watching?: boolean;
  budgetBytes?: number;
}

interface LockRecord {
  version: 1;
  id: string;
  pid: number;
  ticket: number;
}

interface MaintenanceContext {
  workspaceRoot: string;
  cacheDir: string;
  stateDir: string;
  budgetBytes: number;
  cleanTimeoutMs: number;
  profile: string;
  dependencies: RustCacheDependencies;
}

function profileDirectory(profile: string): string {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(profile)) {
    throw new Error(`Invalid Rust cache profile: ${profile}`);
  }
  if (profile === 'dev' || profile === 'test') return 'debug';
  if (profile === 'release' || profile === 'bench') return 'release';
  return profile;
}

function within(parent: string, child: string): boolean {
  const difference = relative(parent, child);
  return difference === '' || (!isAbsolute(difference) && difference !== '..' && !difference.startsWith(`..${sep}`));
}

function absent(error: unknown): boolean {
  return (error as NodeJS.ErrnoException).code === 'ENOENT';
}

/** Check each existing component; realpath alone would silently follow a junction. */
async function safeDirectory(root: string, directory: string, create = false): Promise<boolean> {
  if (!within(root, directory)) throw new Error(`Rust cache resolved outside its workspace: ${directory}`);
  let current = root;
  for (const component of relative(root, directory).split(sep).filter(Boolean)) {
    current = join(current, component);
    let metadata;
    try {
      metadata = await lstat(current);
    } catch (error) {
      if (!absent(error)) throw error;
      if (!create) return false;
      await mkdir(current).catch((failure: NodeJS.ErrnoException) => {
        if (failure.code !== 'EEXIST') throw failure;
      });
      metadata = await lstat(current);
    }
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw new Error(`Rust cache preserves symbolic links, junctions, and non-directory paths: ${current}`);
    }
    if (!within(root, await realpath(current))) {
      throw new Error(`Rust cache resolved outside its workspace: ${current}`);
    }
  }
  return true;
}

async function hasLinks(directory: string): Promise<boolean> {
  const pending = [directory];
  while (pending.length) {
    const current = pending.pop()!;
    for (const entry of await readdir(current, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) return true;
      if (entry.isDirectory()) pending.push(join(current, entry.name));
    }
  }
  return false;
}

function pidIsAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code !== 'ESRCH';
  }
}

async function writeRecord(path: string, record: LockRecord | LeaseRecord): Promise<void> {
  const temporary = `${path}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporary, JSON.stringify(record), { flag: 'wx' });
    for (let attempt = 0; ; attempt++) {
      try {
        await rename(temporary, path);
        break;
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code;
        // Windows can briefly deny replacement while another participant has
        // this ticket open. Its prior atomic record remains valid meanwhile.
        if (process.platform !== 'win32' || !['EPERM', 'EACCES', 'EBUSY'].includes(code ?? '') || attempt >= 100) throw error;
        await new Promise((done) => setTimeout(done, 10));
      }
    }
  } finally {
    await rm(temporary, { force: true });
  }
}

async function readRecords<T extends LockRecord | LeaseRecord>(
  context: MaintenanceContext,
  kind: 'locks' | 'leases',
): Promise<T[]> {
  const directory = join(context.stateDir, kind);
  await safeDirectory(context.workspaceRoot, directory, true);
  const records: T[] = [];
  for (const name of await readdir(directory)) {
    if (!name.endsWith('.json')) continue;
    const path = join(directory, name);
    let record: T;
    try {
      const metadata = await lstat(path);
      if (!metadata.isFile() || metadata.isSymbolicLink()) throw new Error(`Unsafe Rust cache ${kind} record: ${path}`);
      record = JSON.parse(await readFile(path, 'utf8')) as T;
    } catch (error) {
      if (absent(error)) continue;
      throw new Error(`Cannot safely read Rust cache ${kind} record: ${path}`, { cause: error });
    }
    if (record.version !== 1 || typeof record.id !== 'string' || !Number.isSafeInteger(record.pid) || record.pid <= 0) {
      throw new Error(`Invalid Rust cache ${kind} record: ${path}`);
    }
    if (!context.dependencies.isAlive(record.pid)) {
      await rm(path, { force: true });
      continue;
    }
    if (kind === 'locks') {
      const ticket = (record as LockRecord).ticket;
      if (!Number.isSafeInteger(ticket) || ticket < 0) throw new Error(`Invalid Rust cache lock ticket: ${path}`);
    } else {
      const lease = record as LeaseRecord;
      if (typeof lease.targetDir !== 'string' || !isAbsolute(lease.targetDir) || typeof lease.profile !== 'string') {
        throw new Error(`Invalid Rust cache lease: ${path}`);
      }
      profileDirectory(lease.profile);
    }
    records.push(record);
  }
  return records;
}

/**
 * Bakery tickets serialize maintenance across processes without an unsafe stale
 * lock-file replacement race. Every ticket has a unique name and atomic contents.
 */
async function withMaintenanceLock<T>(context: MaintenanceContext, run: () => Promise<T>): Promise<T> {
  const id = randomUUID();
  const path = join(context.stateDir, 'locks', `${id}.json`);
  await safeDirectory(context.workspaceRoot, join(context.stateDir, 'locks'), true);
  const own: LockRecord = { version: 1, id, pid: process.pid, ticket: 0 };
  await writeRecord(path, own);
  try {
    const records = await readRecords<LockRecord>(context, 'locks');
    own.ticket = Math.max(0, ...records.map((record) => record.ticket)) + 1;
    await writeRecord(path, own);
    const started = Date.now();
    while (true) {
      const waiting = (await readRecords<LockRecord>(context, 'locks')).some((other) => other.id !== own.id && (
        other.ticket === 0 || other.ticket < own.ticket || (other.ticket === own.ticket && other.id < own.id)
      ));
      if (!waiting) break;
      if (Date.now() - started > 120_000) throw new Error('Timed out waiting for active Rust cache maintenance. Retry after the other command finishes.');
      await new Promise((done) => setTimeout(done, 50));
    }
    return await run();
  } finally {
    await rm(path, { force: true });
  }
}

async function cargoOwnedProfile(directory: string): Promise<boolean> {
  for (const marker of ['.cargo-lock', '.fingerprint']) {
    try {
      const metadata = await lstat(join(directory, marker));
      if (!metadata.isSymbolicLink() && (metadata.isFile() || metadata.isDirectory())) return true;
    } catch (error) {
      if (!absent(error)) throw error;
    }
  }
  return false;
}

async function cleanupCandidates(context: MaintenanceContext): Promise<RustCacheScope[]> {
  if (!await safeDirectory(context.workspaceRoot, context.cacheDir)) return [];
  const candidates: RustCacheScope[] = [];
  const profiles = new Set(['dev', 'release', context.profile === 'test' ? 'dev' : context.profile === 'bench' ? 'release' : context.profile]);
  for (const profile of profiles) {
    const outputDir = join(context.cacheDir, profileDirectory(profile));
    if (await safeDirectory(context.workspaceRoot, outputDir) && await cargoOwnedProfile(outputDir)) {
      candidates.push({ targetDir: context.cacheDir, outputDir, profile });
    }
  }
  for (const name of CHILD_TARGETS) {
    const targetDir = join(context.cacheDir, name);
    if (!await safeDirectory(context.workspaceRoot, targetDir)) continue;
    const entries = await readdir(targetDir, { withFileTypes: true });
    const known = entries.every((entry) => ['.rustc_info.json', '.future-incompat-report.json', 'CACHEDIR.TAG', 'debug', 'release', 'doc', 'tmp'].includes(entry.name));
    if (known && (await cargoOwnedProfile(join(targetDir, 'debug')) || await cargoOwnedProfile(join(targetDir, 'release')))) {
      candidates.push({ targetDir, outputDir: targetDir });
    }
  }
  return candidates;
}

function protectedByLease(scope: RustCacheScope, leases: LeaseRecord[]): boolean {
  return leases.some((lease) => {
    const output = join(lease.targetDir, profileDirectory(lease.profile));
    return within(scope.outputDir, output) || within(output, scope.outputDir);
  });
}

function measuredBytes(bytes: number): number {
  if (!Number.isFinite(bytes) || bytes < 0) throw new Error('Cannot safely enforce Rust cache budget: invalid cache measurement.');
  return bytes;
}

async function maintain(context: MaintenanceContext): Promise<void> {
  await safeDirectory(context.workspaceRoot, context.cacheDir);
  const leases = await readRecords<LeaseRecord>(context, 'leases');
  let total = measuredBytes(await context.dependencies.measure(context.cacheDir));
  if (total <= context.budgetBytes) return;
  const candidates = await Promise.all((await cleanupCandidates(context)).map(async (scope) => ({
    scope, bytes: measuredBytes(await context.dependencies.measure(scope.outputDir)),
  })));
  candidates.sort((a, b) => b.bytes - a.bytes);
  const blocked: string[] = [];
  for (const { scope, bytes } of candidates) {
    if (bytes === 0) continue;
    if (protectedByLease(scope, leases)) {
      blocked.push(`${scope.outputDir} (protected by a live command)`);
      continue;
    }
    await safeDirectory(context.workspaceRoot, scope.outputDir);
    if (await hasLinks(scope.outputDir)) {
      blocked.push(`${scope.outputDir} (contains a symbolic link or junction)`);
      continue;
    }
    if (await context.dependencies.isTargetBusy(scope)) {
      blocked.push(`${scope.outputDir} (active process or build)`);
      continue;
    }
    // Revalidate immediately before handing an exact Cargo-owned scope to clean.
    await safeDirectory(context.workspaceRoot, scope.outputDir);
    try {
      const controller = new AbortController();
      let timer: ReturnType<typeof setTimeout> | undefined;
      const cleaning = context.dependencies.clean(scope, { signal: controller.signal, timeoutMs: context.cleanTimeoutMs });
      try {
        await Promise.race([
          cleaning,
          new Promise<never>((_done, reject) => {
            timer = setTimeout(() => {
              controller.abort();
              reject(new Error(`Cargo cleaner timed out after ${context.cleanTimeoutMs} ms; an external build may hold its lock`));
            }, context.cleanTimeoutMs);
          }),
        ]);
      } finally {
        clearTimeout(timer);
        // Do not release the maintenance lock while an aborted cleaner might
        // still acquire Cargo's lock and delete a later command's output.
        if (controller.signal.aborted) await cleaning.catch(() => undefined);
      }
    } catch (error) {
      throw new Error(`Rust cache cleanup failed for ${scope.outputDir}: ${error instanceof Error ? error.message : String(error)}. The cache budget could not be enforced; stop the process using this output and retry.`, { cause: error });
    }
    total = measuredBytes(await context.dependencies.measure(context.cacheDir));
    if (total <= context.budgetBytes) return;
  }
  throw new Error(`Rust cache budget exceeded: ${total} bytes (${(total / GIB).toFixed(1)} GiB), budget ${context.budgetBytes} bytes (${(context.budgetBytes / GIB).toFixed(1)} GiB). Protected, active, linked, or unknown paths cannot be removed safely.${blocked.length ? ` ${blocked.join('; ')}.` : ''} Stop commands holding protected outputs and retry; the repository cache ceiling is 32 GiB.`);
}

export interface RustCacheProcess {
  pid: number;
  parentPid: number;
  name: string;
  executable: string;
  command: string;
}

async function runningProcesses(stateDir: string): Promise<RustCacheProcess[]> {
  if (process.platform === 'win32') {
    const scriptPath = join(stateDir, `processes-${randomUUID()}.vbs`);
    const source = [
      'Function Text(value)',
      '  If IsNull(value) Then Text = "" Else Text = Replace(Replace(CStr(value), vbCr, " "), vbLf, " ")',
      'End Function',
      'For Each p In GetObject("winmgmts:{impersonationLevel=impersonate}!\\\\.\\root\\cimv2").ExecQuery("SELECT ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine FROM Win32_Process")',
      '  WScript.Echo p.ProcessId & vbTab & p.ParentProcessId & vbTab & Text(p.Name) & vbTab & Text(p.ExecutablePath) & vbTab & Text(p.CommandLine)',
      'Next',
    ].join('\r\n');
    try {
      await writeFile(scriptPath, source, { flag: 'wx' });
      const { stdout } = await execFileAsync('cscript.exe', ['//nologo', scriptPath], { windowsHide: true, timeout: 15_000, maxBuffer: 8 * 1024 ** 2 });
      return stdout.trim().split(/\r?\n/).filter(Boolean).map((line) => {
        const [pid, parentPid, name = '', executable = '', ...command] = line.split('\t');
        return { pid: Number(pid), parentPid: Number(parentPid), name, executable, command: command.join('\t') };
      });
    } finally {
      await rm(scriptPath, { force: true });
    }
  }
  const { stdout } = await execFileAsync('ps', ['-ax', '-o', 'pid=,ppid=,comm=,args='], { timeout: 15_000, maxBuffer: 8 * 1024 ** 2 });
  return Promise.all(stdout.trim().split('\n').filter(Boolean).map(async (line) => {
    const match = /^\s*(\d+)\s+(\d+)\s+(\S+)\s*(.*)$/.exec(line);
    if (!match) throw new Error('Cannot safely inspect running Rust processes.');
    let executable = match[3]!;
    if (process.platform === 'linux') {
      try { executable = await readlink(`/proc/${match[1]}/exe`); } catch { /* A process may have just exited. */ }
    }
    return { pid: Number(match[1]), parentPid: Number(match[2]), name: basename(match[3]!), executable, command: match[4]! };
  }));
}

export function isRustCacheScopeBusy(scope: RustCacheScope, cacheDir: string, processes: RustCacheProcess[]): boolean {
  const insideManagedCache = (entry: RustCacheProcess) => isAbsolute(entry.executable) && within(cacheDir, entry.executable);
  const descendants = (pid: number): RustCacheProcess[] => {
    const result: RustCacheProcess[] = [];
    const pending = [pid];
    const seen = new Set<number>();
    while (pending.length) {
      const parent = pending.pop()!;
      if (seen.has(parent)) continue;
      seen.add(parent);
      for (const child of processes.filter((entry) => entry.parentPid === parent)) {
        result.push(child);
        pending.push(child.pid);
      }
    }
    return result;
  };
  for (const entry of processes) {
    if (isAbsolute(entry.executable) && within(scope.outputDir, entry.executable)) return true;
    const name = entry.name.toLowerCase().replace(/\.exe$/, '');
    if (['rustc', 'rustdoc', 'clippy-driver'].includes(name)) return true;
    if (name === 'cargo') {
      // `cargo run` remains alive with a running daemon/game. Its live child
      // narrows protection to that output rather than blocking all validation.
      const children = descendants(entry.pid);
      const isRunningChild = /\brun\b/.test(entry.command) && children.some(insideManagedCache);
      if (!isRunningChild) return true;
    }
  }
  return false;
}

export function cargoCleanScopeArgs(workspaceRoot: string, scope: RustCacheScope): string[] {
  const args = ['clean', '--manifest-path', join(workspaceRoot, 'Cargo.toml'), '--target-dir', scope.targetDir];
  if (scope.profile) args.push('--profile', scope.profile);
  return args;
}

async function cargoClean(scope: RustCacheScope, workspaceRoot: string, options: { signal: AbortSignal; timeoutMs: number }): Promise<void> {
  options.signal.throwIfAborted();
  const child = spawn('cargo', cargoCleanScopeArgs(workspaceRoot, scope), {
    cwd: workspaceRoot,
    windowsHide: true,
    detached: process.platform !== 'win32',
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let diagnostic = '';
  child.stderr?.on('data', (chunk: Buffer) => { diagnostic = `${diagnostic}${chunk.toString()}`.slice(-8192); });
  const completed = new Promise<void>((done, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => {
      if (code === 0) done();
      else reject(new Error(`Cargo clean exited with ${signal ?? code}: ${diagnostic.trim()}`));
    });
  });
  let stopping: Promise<void> | undefined;
  const stop = () => {
    if (stopping || child.pid === undefined) return;
    const pid = child.pid;
    stopping = (async () => {
      if (process.platform === 'win32') {
        try {
          await execFileAsync('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true, timeout: 15_000 });
        } catch (error) {
          if (child.exitCode === null) throw error;
        }
      } else {
        try { process.kill(-pid, 'SIGKILL'); } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== 'ESRCH') throw error;
        }
      }
    })();
    void stopping.catch(() => undefined);
  };
  options.signal.addEventListener('abort', stop, { once: true });
  try {
    if (options.signal.aborted) stop();
    await completed;
  } finally {
    options.signal.removeEventListener('abort', stop);
    if (stopping) await stopping;
  }
}

async function liveWatcher(context: MaintenanceContext, pid: number, id: string): Promise<LeaseRecord | undefined> {
  return (await readRecords<LeaseRecord>(context, 'leases')).find((record) => (
    record.pid === pid && record.id === id && record.watching === true &&
    typeof record.budgetBytes === 'number' && record.budgetBytes <= context.budgetBytes &&
    within(context.cacheDir, record.targetDir)
  ));
}

async function inheritedWatcher(context: MaintenanceContext): Promise<LeaseRecord | undefined> {
  const pid = Number(process.env[RUST_CACHE_WATCHER_PID_ENV]);
  const id = process.env[RUST_CACHE_WATCHER_LEASE_ENV];
  if (!Number.isSafeInteger(pid) || pid <= 0 || !id) return undefined;
  return liveWatcher(context, pid, id);
}

function startMonitor(
  context: MaintenanceContext,
  controller: AbortController,
  intervalMs: number,
  parent: LeaseRecord | undefined,
  promote: () => Promise<void>,
): {
  stop: () => Promise<void>;
  failure: () => Error | undefined;
} {
  let stopped = false;
  let failure: Error | undefined;
  let current: Promise<void> | undefined;
  let timer: ReturnType<typeof setTimeout>;
  let inherited = parent;
  const poll = async () => {
    try {
      if (inherited) {
        if (await liveWatcher(context, inherited.pid, inherited.id)) {
          if (!stopped) schedule();
          return;
        }
        // A crashed parent must not leave a surviving overnight child without
        // enforcement. Healthy children only inspect their parent's lease.
        await promote();
        inherited = undefined;
      }
      await safeDirectory(context.workspaceRoot, context.cacheDir);
      const total = measuredBytes(await context.dependencies.measure(context.cacheDir));
      if (total > context.budgetBytes) {
        failure = new Error(`Rust cache budget exceeded while command was running: ${total} bytes (${(total / GIB).toFixed(1)} GiB), budget ${context.budgetBytes} bytes (${(context.budgetBytes / GIB).toFixed(1)} GiB). Stopping this command before pruning its inactive output.`);
      }
    } catch (error) {
      failure = new Error(`Rust cache monitor could not enforce its budget: ${error instanceof Error ? error.message : String(error)}`, { cause: error });
    }
    if (failure) controller.abort(failure);
    else if (!stopped) schedule();
  };
  const schedule = () => {
    timer = setTimeout(() => { current = poll(); }, intervalMs);
    timer.unref?.();
  };
  schedule();
  return {
    stop: async () => {
      stopped = true;
      clearTimeout(timer);
      await current;
    },
    failure: () => failure,
  };
}

export async function withRustCacheLease<T>(
  options: RustCacheLeaseOptions,
  run: (lease: RustCacheLease) => Promise<T>,
): Promise<T> {
  const workspaceRoot = await realpath(resolve(options.workspaceRoot));
  const cacheDir = resolveRustCacheDir(workspaceRoot);
  const targetDir = resolve(workspaceRoot, options.targetDir ?? cacheDir);
  const profile = options.profile ?? 'dev';
  profileDirectory(profile);
  if (within(cacheDir, targetDir)) {
    const child = relative(cacheDir, targetDir);
    if (child && !CHILD_TARGETS.has(child)) throw new Error(`Unknown managed Rust cache target; only recognized Cargo targets are allowed: ${targetDir}`);
    await safeDirectory(workspaceRoot, join(targetDir, profileDirectory(profile)));
  }
  const requestedBudget = options.budgetBytes ?? resolveRustCacheBudgetBytes(process.env[RUST_CACHE_BUDGET_ENV]);
  if (!Number.isFinite(requestedBudget) || requestedBudget <= 0) throw new Error('Rust cache budget must be finite and positive.');
  const budgetBytes = Math.min(requestedBudget, MAX_BUDGET_BYTES);
  const cleanTimeoutMs = options.cleanTimeoutMs ?? DEFAULT_RUST_CACHE_CLEAN_TIMEOUT_MS;
  if (!Number.isSafeInteger(cleanTimeoutMs) || cleanTimeoutMs <= 0 || cleanTimeoutMs > 600_000) throw new Error('Rust cache cleaner timeout must be a positive integer at most 600000 ms.');
  const monitorIntervalMs = options.monitorIntervalMs ?? DEFAULT_RUST_CACHE_MONITOR_INTERVAL_MS;
  if (!Number.isSafeInteger(monitorIntervalMs) || monitorIntervalMs <= 0 || monitorIntervalMs > 600_000) throw new Error('Rust cache monitor interval must be a positive integer at most 600000 ms.');
  const stateDir = join(workspaceRoot, 'target', '.rust-cache-maintenance');
  await safeDirectory(workspaceRoot, stateDir, true);
  const context: MaintenanceContext = {
    workspaceRoot, cacheDir, stateDir, budgetBytes, cleanTimeoutMs, profile,
    dependencies: {
      measure: measureRustCacheBytes,
      clean: (scope, cleanOptions) => cargoClean(scope, workspaceRoot, cleanOptions),
      isAlive: pidIsAlive,
      isTargetBusy: async (scope) => isRustCacheScopeBusy(scope, cacheDir, await runningProcesses(context.stateDir)),
      ...options.dependencies,
    },
  };
  const record: LeaseRecord = { version: 1, id: randomUUID(), pid: process.pid, targetDir, profile };
  const leasePath = join(stateDir, 'leases', `${record.id}.json`);
  let watcher: LeaseRecord | undefined;
  await withMaintenanceLock(context, async () => {
    await maintain(context);
    watcher = await inheritedWatcher(context);
    record.watching = options.monitor !== false && within(cacheDir, targetDir) && watcher === undefined;
    record.budgetBytes = budgetBytes;
    if (record.watching) watcher = record;
    await writeRecord(leasePath, record);
  });
  const controller = new AbortController();
  const env = {
    CARGO_TARGET_DIR: targetDir,
    [RUST_CACHE_BUDGET_ENV]: String(budgetBytes / GIB),
    [RUST_CACHE_WATCHER_PID_ENV]: watcher ? String(watcher.pid) : '',
    [RUST_CACHE_WATCHER_LEASE_ENV]: watcher?.id ?? '',
  };
  const monitor = options.monitor !== false && within(cacheDir, targetDir)
    ? startMonitor(context, controller, monitorIntervalMs, record.watching ? undefined : watcher, async () => {
      await withMaintenanceLock(context, async () => {
        record.watching = true;
        await writeRecord(leasePath, record);
        env[RUST_CACHE_WATCHER_PID_ENV] = String(record.pid);
        env[RUST_CACHE_WATCHER_LEASE_ENV] = record.id;
      });
    })
    : undefined;
  let result: T;
  let commandError: unknown;
  let commandFailed = false;
  try {
    result = await run({ targetDir, env, signal: controller.signal });
  } catch (error) {
    commandFailed = true;
    commandError = error;
  }
  await monitor?.stop();
  const monitorError = monitor?.failure();
  let maintenanceError: unknown;
  try {
    await withMaintenanceLock(context, async () => {
      try {
        // Cancellation has fully settled. This command's failed output can now
        // be pruned, while every other live command retains its own lease.
        if (controller.signal.aborted) await rm(leasePath, { force: true });
        // Keep this lease until maintenance finishes: callers may use the build
        // result after return, and an enclosing lease may extend that lifetime.
        await maintain(context);
      } finally {
        await rm(leasePath, { force: true });
      }
    });
  } catch (error) {
    maintenanceError = error;
    await rm(leasePath, { force: true });
  }
  if ((commandFailed || monitorError) && maintenanceError !== undefined) throw new AggregateError([monitorError ?? commandError, maintenanceError], `${monitorError?.message ?? 'Rust command failed.'} Cache budget maintenance also failed: ${maintenanceError instanceof Error ? maintenanceError.message : String(maintenanceError)}`);
  if (monitorError) throw monitorError;
  if (commandFailed) throw commandError;
  if (maintenanceError !== undefined) throw maintenanceError;
  return result!;
}
