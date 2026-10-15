import { link, readFile, stat, unlink, writeFile } from 'node:fs/promises';
import { basename, isAbsolute, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { expect, test, type Browser, type Page } from '@playwright/test';

import {
  installConsoleFixture,
  installMemoryRedirectGuard,
  type ConsoleFixtureController,
} from './fixtures/api';
import { AGENT, ORIGIN, createState } from './fixtures/state';

const PREVIEW = 'http://127.0.0.1:18753';
const DAEMON = 'http://127.0.0.1:18752';
const OPT_IN = process.env['M61_MEMORY_HTTP'] === '1';

interface Release {
  version: number;
  milestone: string;
  caseId: string;
  nonce: string;
  authorized: boolean;
  expiresAt: string;
  hashes: Record<string, string>;
}

interface SavedRow {
  id: string;
  agentId: string;
  agentName: string;
  type: string;
  content: string;
  importance: number;
  tags: string[] | null;
}

async function boundedJson(path: string, maximum: number): Promise<unknown> {
  const info = await stat(path);
  expect(info.isFile()).toBe(true);
  expect(info.size).toBeGreaterThan(0);
  expect(info.size).toBeLessThanOrEqual(maximum);
  return JSON.parse(await readFile(path, 'utf8')) as unknown;
}

async function readStore(caseDir: string): Promise<SavedRow[]> {
  const store = (await boundedJson(
    join(caseDir, 'stores', 'memory.json'),
    1_000_000,
  )) as {
    memories?: SavedRow[];
  };
  expect(Array.isArray(store.memories)).toBe(true);
  return store.memories!;
}

function assertRow(
  row: SavedRow | undefined,
  id: string,
  content: string,
  importance: number,
  tags: string[],
) {
  expect(row).toMatchObject({
    id,
    agentId: AGENT,
    agentName: 'Anima',
    type: 'fact',
    content,
    importance,
    tags,
  });
}

async function control(
  ipcDir: string,
  nonce: string,
  number: 1 | 2,
  command: 'restart' | 'stop',
) {
  const requestPath = join(ipcDir, `request-${number}.json`);
  await publishJsonExclusive(requestPath, { version: 1, nonce, command });
  const responsePath = join(ipcDir, `response-${number}.json`);
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    try {
      const response = (await boundedJson(responsePath, 1_024)) as Record<
        string,
        unknown
      >;
      expect(response).toEqual({ version: 1, nonce, command, ok: true });
      return;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    await delay(100);
  }
  throw new Error(`M61 ${command} acknowledgement did not arrive`);
}

async function publishJsonExclusive(path: string, value: unknown) {
  const temporary = `${path}.tmp`;
  await writeFile(temporary, JSON.stringify(value, null, 2), { flag: 'wx' });
  try {
    // A same-directory hard link publishes complete bytes without replacing
    // another request or result on this fresh NTFS case directory.
    await link(temporary, path);
  } finally {
    await unlink(temporary);
  }
}

async function openMemory(browser: Browser, id?: string) {
  const context = await browser.newContext({
    serviceWorkers: 'block',
    viewport: { width: 1280, height: 900 },
  });
  context.setDefaultTimeout(5_000);
  context.setDefaultNavigationTimeout(10_000);
  const page = await context.newPage();
  const fixture = await installConsoleFixture(page, createState(), {
    origin: PREVIEW,
    realStatic: true,
    realMemory: true,
  });
  if (id) fixture.setRealMemoryId(id);
  await page.goto(`${PREVIEW}/`);
  await expect(page.getByPlaceholder('Message Anima…')).toBeVisible();
  await fixture.waitForStream();
  return { context, page, fixture };
}

async function browserGet(page: Page, path: string) {
  return page.evaluate(async (path) => {
    const response = await fetch(path, { redirect: 'manual' });
    return {
      status: response.status,
      body: (await response.json()) as unknown,
    };
  }, path);
}

async function responseFor(
  fixture: ConsoleFixtureController,
  method: string,
  path: string,
) {
  await expect
    .poll(
      () =>
        fixture.realResponses.filter(
          (response) => response.method === method && response.path === path,
        ).length,
      { timeout: 5_000 },
    )
    .toBe(1);
  const matches = fixture.realResponses.filter(
    (response) => response.method === method && response.path === path,
  );
  expect(matches).toHaveLength(1);
  expect(matches[0].status).toBeGreaterThanOrEqual(200);
  expect(matches[0].status).toBeLessThan(300);
  return matches[0].body as Record<string, unknown>;
}

function countAll(
  first: ConsoleFixtureController,
  second: ConsoleFixtureController,
  key: string,
) {
  return (first.realCounts.get(key) ?? 0) + (second.realCounts.get(key) ?? 0);
}

async function editSavedMemory(
  page: Page,
  previousContent: string,
  content: string,
  importance: number,
  tags: string[],
) {
  const row = page.locator('.memory-item').filter({ hasText: previousContent });
  await expect(row).toHaveCount(1);
  await row.getByRole('button', { name: 'Edit' }).click();
  // The old text stops identifying the row as soon as the textarea changes.
  const editor = page.getByRole('form', { name: 'Edit memory', exact: true });
  await expect(editor).toHaveCount(1);
  await editor.getByRole('textbox', { name: 'Memory text' }).fill(content);
  const slider = editor.getByRole('slider', { name: 'Importance' });
  await slider.focus();
  await slider.press('Home');
  for (let step = 0; step < Math.round(importance / 0.05); step++)
    await slider.press('ArrowRight');
  await expect(editor.locator('output')).toHaveText(importance.toFixed(2));
  await editor.getByRole('textbox', { name: 'Tags' }).fill(tags.join(', '));
  await editor.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await expect(page.getByText(content, { exact: true })).toBeVisible();
}

test('memory editor retains importance and tags controls after changing text', async ({
  page,
}) => {
  test.skip(
    OPT_IN,
    'The isolated runner selects only the real acceptance case',
  );
  page.setDefaultTimeout(5_000);
  const fixture = await installConsoleFixture(page);
  const memory = {
    id: 'm61-offline-editor',
    agentId: AGENT,
    agentName: 'Anima',
    type: 'fact',
    content: 'Offline Cedar editor fixture',
    importance: 0.35,
    tags: ['m61-seed'],
    createdAt: 1,
    scope: 'private',
    roomId: null,
    worldId: null,
    sessionId: null,
  };
  const updatedContent = 'Offline Hibiscus editor fixture';
  let patches = 0;
  let releasePatch!: () => void;
  const patchMayFinish = new Promise<void>((resolve) => {
    releasePatch = resolve;
  });
  const unexpected: string[] = [];
  await page.route('**/api/memories**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    let body: unknown;
    if (request.method() === 'GET' && path === '/api/memories/recent')
      body = { memories: [memory] };
    else if (request.method() === 'GET' && path === '/api/memories/facts')
      body = { facts: [] };
    else if (request.method() === 'GET' && path === '/api/memories/entities')
      body = { entities: [] };
    else if (
      request.method() === 'GET' &&
      path === '/api/memories/relationships'
    )
      body = { relationships: [] };
    else if (
      request.method() === 'PATCH' &&
      path === `/api/memories/${memory.id}`
    ) {
      const patch = request.postDataJSON() as Record<string, unknown>;
      expect(patch).toEqual({
        content: updatedContent,
        importance: 0.65,
        tags: ['m61-updated'],
      });
      patches++;
      await patchMayFinish;
      Object.assign(memory, patch);
      body = memory;
    } else {
      unexpected.push(`${request.method()} ${path}`);
      return route.abort('blockedbyclient');
    }
    return route.fulfill({ json: body });
  });
  await page.goto(ORIGIN);
  await fixture.waitForStream();
  await page.getByRole('button', { name: 'Memory', exact: true }).click();
  await Promise.all([
    editSavedMemory(page, memory.content, updatedContent, 0.65, [
      'm61-updated',
    ]),
    (async () => {
      try {
        await expect.poll(() => patches, { timeout: 5_000 }).toBe(1);
        await expect(
          page
            .getByRole('form', { name: 'Edit memory' })
            .getByRole('button', { name: 'Save', exact: true }),
        ).toBeDisabled();
      } finally {
        releasePatch();
      }
    })(),
  ]);
  expect(patches).toBe(1);
  expect(unexpected).toEqual([]);
  fixture.assertClean();
});

test('memory HTTP guard stops redirects inside the browser', async ({
  page,
}) => {
  test.skip(
    OPT_IN,
    'The isolated runner selects only the real acceptance case',
  );
  page.setDefaultTimeout(5_000);
  const fixture = await installConsoleFixture(page);
  await installMemoryRedirectGuard(page, ORIGIN);
  let requests = 0;
  await page.route('**/api/memories/recent', async (route) => {
    expect(route.request().method()).toBe('GET');
    requests++;
    await route.fulfill({
      status: 302,
      headers: { location: 'https://m61-refused.invalid/never-follow' },
    });
  });
  await page.goto(ORIGIN);
  await fixture.waitForStream();
  const redirected = await page.evaluate(async () => {
    const response = await fetch('/api/memories/recent');
    return {
      status: response.status,
      type: response.type,
      redirected: response.redirected,
    };
  });
  expect(redirected).toEqual({
    status: 0,
    type: 'opaqueredirect',
    redirected: false,
  });
  expect(requests).toBe(1);
  fixture.assertClean();
});

test('M61 built Memory page uses isolated real HTTP and survives one quiet forced restart', async ({
  browser,
}) => {
  test.skip(
    !OPT_IN,
    'M61 needs its fresh private startup release and owned loopback processes',
  );
  test.setTimeout(120_000);
  const caseDir = process.env['M61_CASE_DIR'];
  const ipcDir = process.env['M61_IPC_DIR'];
  const releaseFile = process.env['M61_RELEASE_FILE'];
  const nonce = process.env['M61_NONCE'];
  expect(caseDir && isAbsolute(caseDir)).toBe(true);
  expect(ipcDir && isAbsolute(ipcDir)).toBe(true);
  expect(releaseFile && isAbsolute(releaseFile)).toBe(true);
  expect(nonce).toMatch(/^[0-9a-f]{64}$/);
  const privateCaseDir = resolve(caseDir!);
  expect(resolve(ipcDir!)).toBe(join(privateCaseDir, 'ipc'));
  expect(basename(releaseFile!)).toBe('startup-release.json');
  const release = (await boundedJson(releaseFile!, 4 * 1024 * 1024)) as Release;
  expect(release).toMatchObject({
    version: 1,
    milestone: 'M61',
    authorized: true,
    nonce,
  });
  expect(release.caseId).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  expect(basename(privateCaseDir)).toBe(release.caseId);
  expect(Date.parse(release.expiresAt)).toBeGreaterThan(Date.now());
  expect(Object.keys(release.hashes).length).toBeGreaterThan(0);
  expect(new URL(PREVIEW).hostname).toBe('127.0.0.1');
  expect(new URL(DAEMON).hostname).toBe('127.0.0.1');

  try {
    const initialContent = `M61 synthetic Cedar memory ${release.caseId}`;
    const updatedContent = `M61 synthetic Hibiscus memory ${release.caseId}`;
    const initialImportance = 0.35;
    const updatedImportance = 0.65;
    const initialTags = ['m61-seed'];
    const updatedTags = ['m61-updated'];
    const first = await openMemory(browser);
    const seed = await first.page.evaluate(
      async (input) => {
        const response = await fetch('/api/memories', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(input),
          redirect: 'manual',
        });
        return {
          status: response.status,
          body: (await response.json()) as unknown,
        };
      },
      {
        agentId: AGENT,
        agentName: 'Anima',
        type: 'fact',
        content: initialContent,
        importance: initialImportance,
        tags: initialTags,
      },
    );
    expect(seed.status).toBeGreaterThanOrEqual(200);
    expect(seed.status).toBeLessThan(300);
    const memory = seed.body as {
      id: string;
      content: string;
      importance: number;
      tags: string[];
    };
    expect(memory.id).toMatch(/^[A-Za-z0-9_-]{1,100}$/);
    expect(memory).toMatchObject({
      content: initialContent,
      importance: initialImportance,
      tags: initialTags,
    });
    first.fixture.setRealMemoryId(memory.id);
    expect(
      await responseFor(first.fixture, 'POST', '/api/memories'),
    ).toMatchObject({
      id: memory.id,
      content: initialContent,
    });
    assertRow(
      (await readStore(privateCaseDir)).find((row) => row.id === memory.id),
      memory.id,
      initialContent,
      initialImportance,
      initialTags,
    );

    await first.page
      .getByRole('button', { name: 'Memory', exact: true })
      .click();
    const firstRow = first.page
      .locator('.memory-item')
      .filter({ hasText: initialContent });
    await expect(firstRow).toHaveCount(1);
    expect(
      first.fixture.realCounts.get('GET /api/memories/readiness') ?? 0,
    ).toBe(0);
    await first.page
      .getByRole('button', { name: 'Check memory search status' })
      .click();
    await expect(
      first.page.getByText(/Daemon-wide vector index: disabled/),
    ).toBeVisible();
    await expect(
      first.page.getByText(/Disk cache: not configured/),
    ).toBeVisible();
    await expect(
      first.page.getByText(/Local fixture checks: 14 of 14 passed/),
    ).toBeVisible();
    const firstStatus = await responseFor(
      first.fixture,
      'GET',
      '/api/memories/readiness',
    );
    expect(firstStatus.embeddings).toMatchObject({
      enabled: false,
      provider: 'disabled',
      vectorCount: 0,
      persisted: false,
    });
    const firstEvaluation = firstStatus.evaluation as {
      totalChecks: number;
      passedChecks: number;
    };
    expect(firstEvaluation).toMatchObject({
      totalChecks: 14,
      passedChecks: 14,
    });

    await editSavedMemory(
      first.page,
      initialContent,
      updatedContent,
      updatedImportance,
      updatedTags,
    );
    expect(
      await responseFor(first.fixture, 'PATCH', `/api/memories/${memory.id}`),
    ).toMatchObject({
      id: memory.id,
      content: updatedContent,
      importance: updatedImportance,
      tags: updatedTags,
    });
    const patch = first.fixture.calls.find(
      (call) =>
        call.method === 'PATCH' && call.path === `/api/memories/${memory.id}`,
    );
    expect(patch?.body).toMatchObject({
      content: updatedContent,
      importance: updatedImportance,
      tags: updatedTags,
    });
    assertRow(
      (await readStore(privateCaseDir)).find((row) => row.id === memory.id),
      memory.id,
      updatedContent,
      updatedImportance,
      updatedTags,
    );
    await first.page.screenshot({
      path: join(privateCaseDir, 'desktop.png'),
      fullPage: true,
    });
    expect(
      await first.page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await first.fixture.waitForRealResponses();
    first.fixture.assertClean();
    await first.context.close();

    await control(ipcDir!, nonce!, 1, 'restart');
    assertRow(
      (await readStore(privateCaseDir)).find((row) => row.id === memory.id),
      memory.id,
      updatedContent,
      updatedImportance,
      updatedTags,
    );
    const second = await openMemory(browser, memory.id);
    await second.page
      .getByRole('button', { name: 'Memory', exact: true })
      .click();
    const secondRow = second.page
      .locator('.memory-item')
      .filter({ hasText: updatedContent });
    await expect(secondRow).toHaveCount(1);
    const recent = await browserGet(
      second.page,
      `/api/memories/recent?agentId=${AGENT}&limit=200`,
    );
    expect(recent.status).toBe(200);
    expect(
      (
        recent.body as {
          memories: Array<{
            id: string;
            content: string;
            importance: number;
            tags: string[];
          }>;
        }
      ).memories,
    ).toContainEqual(
      expect.objectContaining({
        id: memory.id,
        content: updatedContent,
        importance: updatedImportance,
        tags: updatedTags,
      }),
    );
    expect(
      countAll(first.fixture, second.fixture, 'GET /api/memories/readiness'),
    ).toBe(1);
    await second.page
      .getByRole('button', { name: 'Check memory search status' })
      .click();
    await expect(
      second.page.getByText(/Daemon-wide vector index: disabled/),
    ).toBeVisible();
    await expect(
      second.page.getByText(/Local fixture checks: 14 of 14 passed/),
    ).toBeVisible();
    expect(
      (await responseFor(second.fixture, 'GET', '/api/memories/readiness'))
        .embeddings,
    ).toMatchObject({ enabled: false, vectorCount: 0, persisted: false });
    expect(
      (await responseFor(second.fixture, 'GET', '/api/memories/readiness'))
        .evaluation,
    ).toMatchObject({ totalChecks: 14, passedChecks: 14 });

    await second.page
      .getByRole('searchbox', { name: 'Search memories' })
      .fill('Hibiscus');
    await second.page
      .getByRole('button', { name: 'Search', exact: true })
      .click();
    const searched = await responseFor(
      second.fixture,
      'GET',
      '/api/memories/search',
    );
    expect(searched.results).toContainEqual(
      expect.objectContaining({
        id: memory.id,
        content: updatedContent,
      }),
    );
    await expect(
      second.page.locator('.memory-item').filter({ hasText: updatedContent }),
    ).toHaveCount(1);
    expect(second.fixture.realCounts.get('GET /api/memories/search') ?? 0).toBe(
      1,
    );
    await second.page.getByRole('button', { name: 'Clear' }).click();
    await second.page.setViewportSize({ width: 390, height: 844 });
    expect(
      await second.page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await second.page.screenshot({
      path: join(privateCaseDir, 'mobile.png'),
      fullPage: true,
    });
    await second.page.setViewportSize({ width: 1280, height: 900 });

    const finalRow = second.page
      .locator('.memory-item')
      .filter({ hasText: updatedContent });
    await finalRow.getByRole('button', { name: 'Delete', exact: true }).click();
    await finalRow.getByRole('button', { name: 'Delete memory' }).click();
    await expect(
      second.page.getByText(updatedContent, { exact: true }),
    ).toHaveCount(0);
    expect(
      await responseFor(second.fixture, 'DELETE', `/api/memories/${memory.id}`),
    ).toMatchObject({ id: memory.id });
    const deleted = second.fixture.calls.find(
      (call) =>
        call.method === 'DELETE' && call.path === `/api/memories/${memory.id}`,
    );
    expect(deleted?.body).toEqual({ expectedContent: updatedContent });
    const absent = await browserGet(
      second.page,
      `/api/memories/recent?agentId=${AGENT}&limit=200`,
    );
    expect(absent.status).toBe(200);
    expect(
      (absent.body as { memories: Array<{ id: string }> }).memories.some(
        (row) => row.id === memory.id,
      ),
    ).toBe(false);
    expect(
      (await readStore(privateCaseDir)).some((row) => row.id === memory.id),
    ).toBe(false);
    expect(countAll(first.fixture, second.fixture, 'POST /api/memories')).toBe(
      1,
    );
    expect(
      countAll(
        first.fixture,
        second.fixture,
        `PATCH /api/memories/${memory.id}`,
      ),
    ).toBe(1);
    expect(
      countAll(
        first.fixture,
        second.fixture,
        `DELETE /api/memories/${memory.id}`,
      ),
    ).toBe(1);
    expect(
      countAll(first.fixture, second.fixture, 'GET /api/memories/readiness'),
    ).toBe(2);
    await second.fixture.waitForRealResponses();
    second.fixture.assertClean();
    await second.context.close();

    await control(ipcDir!, nonce!, 2, 'stop');
    const methodCounts = Object.fromEntries(
      [...first.fixture.realCounts, ...second.fixture.realCounts].reduce(
        (counts, [key, value]) => {
          counts.set(key, (counts.get(key) ?? 0) + value);
          return counts;
        },
        new Map<string, number>(),
      ),
    );
    await publishJsonExclusive(join(privateCaseDir, 'case-result.json'), {
      version: 1,
      caseId: release.caseId,
      nonce,
      passed: true,
      boundary: 'built-preview-to-isolated-daemon-memory-only',
      restart: 'one quiet forced restart',
      memory: {
        id: memory.id,
        initialContent,
        updatedContent,
        initialImportance,
        updatedImportance,
        initialTags,
        updatedTags,
        absentAfterDelete: true,
      },
      localFixtureChecks: firstEvaluation,
      methodCounts,
      screenshots: ['desktop.png', 'mobile.png'],
    });
  } catch (error) {
    await publishJsonExclusive(join(privateCaseDir, 'case-result.json'), {
      version: 1,
      caseId: release.caseId,
      nonce,
      passed: false,
      failureKind: error instanceof Error ? error.name : 'UnknownError',
    }).catch(() => {
      // Preserve the first immutable result; supervisor records its own failure.
    });
    throw error;
  }
});
