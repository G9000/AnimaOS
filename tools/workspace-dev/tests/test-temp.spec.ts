import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

const require = createRequire(import.meta.url);
const { prepareTask, cleanupTask } = require('../task-temp.cjs');
function startTask(_options: unknown, ctx: ReturnType<typeof context>) {
  Object.assign(process.env, prepareTask(ctx));
}
function finishTask(
  _options: unknown,
  ctx: ReturnType<typeof context> & { taskResults?: unknown },
) {
  cleanupTask(ctx);
}
const originalEnv = {
  TMP: process.env.TMP,
  TEMP: process.env.TEMP,
  TMPDIR: process.env.TMPDIR,
  ANIMAOS_TASK_TEMP_ROOT: process.env.ANIMAOS_TASK_TEMP_ROOT,
};
const roots: string[] = [];
const scratchParents: string[] = [];

afterEach(() => {
  for (const [key, value] of Object.entries(originalEnv)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  vi.restoreAllMocks();
  for (const parent of scratchParents.splice(0))
    rmSync(parent, { recursive: true, force: true });
  for (const root of roots.splice(0))
    rmSync(root, { recursive: true, force: true });
});

function workspace() {
  const root = mkdtempSync(join(tmpdir(), 'anima-test-temp-spec-'));
  roots.push(root);
  return root;
}

function context(workspaceRoot: string, id = 'invocation-1') {
  return {
    workspaceRoot,
    id,
    argv: ['run', 'workspace-dev:test'],
    nxJsonConfiguration: {},
  };
}

function start(ctx: ReturnType<typeof context>) {
  startTask(undefined, ctx);
  const scratch = process.env.TMPDIR!;
  scratchParents.push(dirname(scratch));
  return scratch;
}

describe('task temporary-file lifecycle', () => {
  it('returns child-only temp settings without changing the caller environment', () => {
    const root = workspace();
    const ctx = context(root);
    const before = { ...process.env };
    const env = prepareTask(ctx);
    scratchParents.push(dirname(env.TMPDIR));
    expect(process.env).toEqual(before);
    expect(env.TMP).toBe(env.TMPDIR);
    expect(env.TEMP).toBe(env.TMPDIR);
    expect(env.ANIMAOS_TASK_TEMP_ROOT).toBeTruthy();
    cleanupTask(ctx);
    expect(existsSync(env.TMPDIR)).toBe(false);
  });

  it('isolates child-process temporary files and removes them after a failed test', () => {
    const root = workspace();
    const ctx = context(root);
    const scratch = start(ctx);
    const fromWorkspace = relative(root, scratch);
    expect(fromWorkspace.startsWith('..') || isAbsolute(fromWorkspace)).toBe(
      true,
    );
    expect(process.env.TMP).toBe(scratch);
    expect(process.env.TEMP).toBe(scratch);
    expect(existsSync(scratch)).toBe(true);
    writeFileSync(join(scratch, 'test-data'), 'disposable');
    const taskResults = {
      'workspace-dev:test': { status: 'failure', code: 17 },
    };
    finishTask(undefined, { ...ctx, taskResults });
    expect(existsSync(scratch)).toBe(false);
    expect(taskResults['workspace-dev:test'].code).toBe(17);
  });

  it('cleans only the matching invocation and preserves reports, cache, and unrelated scratch', () => {
    const root = workspace();
    for (const dir of ['target/cargo', 'test-output', '.tmp']) {
      mkdirSync(join(root, dir), { recursive: true });
      writeFileSync(join(root, dir, 'keep'), 'keep');
    }
    const first = context(root, 'first');
    const firstDir = start(first);
    const second = context(root, 'second');
    const secondDir = start(second);
    expect(firstDir).not.toBe(secondDir);
    finishTask(undefined, { ...first, taskResults: {} });
    expect(existsSync(firstDir)).toBe(false);
    expect(existsSync(secondDir)).toBe(true);
    for (const dir of ['target/cargo', 'test-output', '.tmp']) {
      expect(readFileSync(join(root, dir, 'keep'), 'utf8')).toBe('keep');
    }
    finishTask(undefined, { ...second, taskResults: {} });
    expect(existsSync(secondDir)).toBe(false);
  });

  it('does not treat an arbitrary TMPDIR value as cleanup authority', () => {
    const root = workspace();
    process.env.TMPDIR = root;
    writeFileSync(join(root, 'keep'), 'keep');
    finishTask(undefined, { ...context(root), taskResults: {} });
    expect(readFileSync(join(root, 'keep'), 'utf8')).toBe('keep');
  });

  it('rejects a redirected scratch parent', () => {
    const root = workspace();
    const outside = workspace();
    const ctx = context(root);
    const scratch = start(ctx);
    finishTask(undefined, { ...ctx, taskResults: {} });
    const parent = dirname(scratch);
    rmSync(parent, { recursive: true });
    symlinkSync(
      outside,
      parent,
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    expect(() => startTask(undefined, context(root))).toThrow(
      /symlink|redirect/i,
    );
    expect(existsSync(outside)).toBe(true);
  });

  it('leaves a substituted run directory untouched and reports the cleanup failure', () => {
    const root = workspace();
    const outside = workspace();
    writeFileSync(join(outside, 'keep'), 'keep');
    const ctx = context(root);
    const scratch = start(ctx);
    rmSync(scratch, { recursive: true });
    symlinkSync(
      outside,
      scratch,
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(() =>
      finishTask(undefined, { ...ctx, taskResults: {} }),
    ).not.toThrow();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('temporary'));
    expect(readFileSync(join(outside, 'keep'), 'utf8')).toBe('keep');
    expect(existsSync(scratch)).toBe(true);
  });

  it('unlinks nested symlinks without removing their target', () => {
    const root = workspace();
    const outside = workspace();
    writeFileSync(join(outside, 'keep'), 'keep');
    const ctx = context(root, '../../not-a-path');
    const scratch = start(ctx);
    symlinkSync(
      outside,
      join(scratch, 'linked'),
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    finishTask(undefined, { ...ctx, taskResults: {} });
    expect(existsSync(scratch)).toBe(false);
    expect(readFileSync(join(outside, 'keep'), 'utf8')).toBe('keep');
  });

  it('keeps temporary fixtures outside the checkout so Git discovery cannot reach it', () => {
    const root = workspace();
    execFileSync('git', ['init', root], { stdio: 'ignore' });
    const ctx = context(root);
    const scratch = start(ctx);
    expect(() =>
      execFileSync('git', ['-C', scratch, 'rev-parse', '--show-toplevel'], {
        stdio: 'ignore',
      }),
    ).toThrow();
    finishTask(undefined, { ...ctx, taskResults: {} });
  });

  it('accepts another invocation winning creation of the shared workspace parent', () => {
    const root = workspace();
    const nativeFs = require('node:fs');
    const mkdir = nativeFs.mkdirSync;
    vi.spyOn(nativeFs, 'mkdirSync').mockImplementation((directory: unknown) => {
      mkdir(directory);
      if (
        typeof directory === 'string' &&
        basename(directory).startsWith('anima-task-')
      ) {
        throw Object.assign(
          new Error('another invocation created the parent'),
          { code: 'EEXIST' },
        );
      }
    });
    const ctx = context(root);
    expect(() => start(ctx)).not.toThrow();
    finishTask(undefined, { ...ctx, taskResults: {} });
  });
});
