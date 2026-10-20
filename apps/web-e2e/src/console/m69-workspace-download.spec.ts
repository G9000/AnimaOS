import {
  expect,
  test,
  type Browser,
  type BrowserContext,
} from '@playwright/test';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, readdir, realpath, stat, writeFile } from 'node:fs/promises';
import { isAbsolute, join, resolve } from 'node:path';
import { installConsoleFixture } from './fixtures/api';

const ROOT = resolve(__dirname, '../../../..');
const PRIVATE = join(
  ROOT,
  '.superpowers/sdd/2026-10-16-companion-report-browser-m69',
);
const PREVIEW = 'http://127.0.0.1:18753';
const EXPECTED_SHA =
  '8feefe05a78114ac5648616031900111d679834277bc8272ef757b6215be6fee';
const EXPECTED_BYTES = 84;
const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SHA = /^[0-9a-f]{64}$/;

interface Release {
  version: 1;
  milestone: 'M69';
  authorized: true;
  caseId: string;
  nonce: string;
  expiresAt: string;
  hashes: Record<string, string>;
  images: Record<
    'node' | 'daemon' | 'browser',
    { path: string; sha256: string }
  >;
}

function object(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid M69 object');
  return value as Record<string, unknown>;
}
function samePath(left: string, right: string) {
  const canonical = (path: string) =>
    process.platform === 'win32' ? resolve(path).toLowerCase() : resolve(path);
  return canonical(left) === canonical(right);
}
async function regularFile(path: string, max: number, allowEmpty = false) {
  expect(
    samePath(await realpath(path), path),
    'File must not be redirected',
  ).toBe(true);
  const metadata = await stat(path);
  expect(metadata.isFile()).toBe(true);
  expect(metadata.size).toBeGreaterThanOrEqual(allowEmpty ? 0 : 1);
  expect(metadata.size).toBeLessThanOrEqual(max);
}
async function boundedJson(path: string, max: number): Promise<unknown> {
  await regularFile(path, max);
  return JSON.parse((await readFile(path)).toString('utf8'));
}
async function digest(path: string) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}
async function bytesProof(path: string, expected: Buffer) {
  await regularFile(path, EXPECTED_BYTES);
  const actual = await readFile(path);
  expect(actual.length).toBe(EXPECTED_BYTES);
  expect(actual.equals(expected), 'Raw BOM, UTF-8, CRLF and CSV bytes').toBe(
    true,
  );
  expect(createHash('sha256').update(actual).digest('hex')).toBe(EXPECTED_SHA);
}

// No browser fixture: an ordinary offline suite skips before starting a browser.
test('m69_actual_owner_files_preview_and_browser_report_save', async ({
  playwright,
}, testInfo) => {
  test.skip(
    process.env['M69_REPORT_BROWSER'] !== '1',
    'Requires a fresh root-released M69 case',
  );
  test.setTimeout(120_000);
  expect(testInfo.retry, 'A consumed case cannot be retried').toBe(0);
  expect(testInfo.project.retries).toBe(0);
  expect(testInfo.config.workers).toBe(1);
  let browser: Browser | undefined;
  let context: BrowserContext | undefined;
  let release: Release | undefined;
  let resultPath: string | undefined;
  let result: unknown;
  let failure: unknown;
  let verifyClosed: (() => void) | undefined;
  try {
    const releasePath = process.env['M69_RELEASE_FILE'];
    const caseDir = process.env['M69_CASE_DIR'];
    expect(typeof releasePath).toBe('string');
    expect(typeof caseDir).toBe('string');
    if (!releasePath || !caseDir) throw new Error('Missing M69 release paths');
    expect(isAbsolute(releasePath) && isAbsolute(caseDir)).toBe(true);
    expect(samePath(releasePath, join(PRIVATE, 'release.json'))).toBe(true);
    const value = object(await boundedJson(releasePath, 8 * 1024 * 1024));
    expect(Object.keys(value).sort()).toEqual([
      'authorized',
      'caseId',
      'expiresAt',
      'hashes',
      'images',
      'milestone',
      'nonce',
      'version',
    ]);
    expect(value.version).toBe(1);
    expect(value.milestone).toBe('M69');
    expect(value.authorized).toBe(true);
    expect(value.caseId).toEqual(expect.stringMatching(UUID_V4));
    expect(value.nonce).toEqual(expect.stringMatching(SHA));
    expect(value.nonce).toBe(process.env['M69_NONCE']);
    expect(value.expiresAt).toEqual(expect.stringMatching(/(?:Z|\+00:00)$/));
    const expires = Date.parse(value.expiresAt as string);
    expect(expires - Date.now()).toBeGreaterThan(0);
    expect(expires - Date.now()).toBeLessThanOrEqual(300_000);
    expect(
      samePath(caseDir, join(PRIVATE, 'live', value.caseId as string)),
    ).toBe(true);
    expect(samePath(await realpath(caseDir), caseDir)).toBe(true);
    const consumed = await boundedJson(
      join(PRIVATE, 'consumed', `${value.caseId}.json`),
      1024,
    );
    expect(consumed).toEqual({
      version: 1,
      caseId: value.caseId,
      nonce: value.nonce,
      releaseSha256: await digest(releasePath),
    });
    release = value as unknown as Release;
    expect(
      await readdir(caseDir),
      'A browser result cannot be replayed',
    ).not.toContain('browser-result.json');
    resultPath = join(caseDir, 'browser-result.json');

    const hashes = Object.entries(object(value.hashes));
    expect(hashes.length).toBeGreaterThan(0);
    expect(hashes.length).toBeLessThanOrEqual(60_000);
    const required = [
      'apps/web-e2e/src/console/m69-workspace-download.spec.ts',
      'apps/web-e2e/src/console/fixtures/api.ts',
      'apps/web-e2e/src/console/fixtures/workspace-files-policy.ts',
      'apps/web-e2e/src/console/workspace-files-policy.spec.ts',
      'apps/web-e2e/playwright.console.config.ts',
      'apps/web/dist/index.html',
      '.superpowers/sdd/2026-10-16-companion-report-browser-m69/bound-plan.md',
      '.superpowers/sdd/2026-10-16-companion-report-browser-m69/supervisor.py',
      '.superpowers/sdd/2026-10-16-companion-report-browser-m69/fixture.csv',
      '.superpowers/sdd/2026-10-16-companion-report-browser-m69/fixture-oracle.json',
      '.superpowers/sdd/2026-10-16-companion-memory-http-m61/process_guard.py',
      '.superpowers/sdd/2026-10-16-companion-memory-http-m61/preview-entry.mjs',
    ];
    expect(Object.keys(object(value.hashes))).toEqual(
      expect.arrayContaining(required),
    );
    // Exact raw plan published in commit 9b25c0e038df5eaf72f9da7d174bf08369a74c56.
    expect(
      object(value.hashes)[
        '.superpowers/sdd/2026-10-16-companion-report-browser-m69/bound-plan.md'
      ],
    ).toBe('03046498665443c98e6946bb7df5fde9151feb240c012864e83fe046412f103d');
    // Bounded concurrency keeps verification inside the finite case deadline.
    for (let offset = 0; offset < hashes.length; offset += 16) {
      await Promise.all(
        hashes.slice(offset, offset + 16).map(async ([relative, wanted]) => {
          expect(relative.length).toBeLessThanOrEqual(2048);
          expect(isAbsolute(relative)).toBe(false);
          expect(relative.includes('\\') || relative.includes(':')).toBe(false);
          expect(
            relative.split('/').some((part) => ['', '.', '..'].includes(part)),
          ).toBe(false);
          expect(wanted).toEqual(expect.stringMatching(SHA));
          const path = resolve(ROOT, relative);
          await regularFile(path, 512 * 1024 * 1024, true);
          expect(await digest(path), `Frozen input: ${relative}`).toBe(wanted);
        }),
      );
    }
    const images = object(value.images);
    expect(Object.keys(images).sort()).toEqual(['browser', 'daemon', 'node']);
    const fixedImages = {
      node: process.execPath,
      daemon: join(ROOT, 'target/cargo/debug/anima-daemon.exe'),
      browser: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    };
    for (const name of ['node', 'daemon', 'browser'] as const) {
      const image = object(images[name]);
      expect(Object.keys(image).sort()).toEqual(['path', 'sha256']);
      expect(typeof image.path).toBe('string');
      expect(samePath(image.path as string, fixedImages[name])).toBe(true);
      expect(image.sha256).toEqual(expect.stringMatching(SHA));
      await regularFile(image.path as string, 512 * 1024 * 1024);
      expect(await digest(image.path as string)).toBe(image.sha256);
    }
    expect(
      await boundedJson(join(PRIVATE, 'fixture-oracle.json'), 1024),
    ).toEqual({
      relativePath: 'reports/ledger.csv',
      basename: 'ledger.csv',
      bytes: EXPECTED_BYTES,
      sha256: EXPECTED_SHA.toUpperCase(),
    });
    const expected = await readFile(join(PRIVATE, 'fixture.csv'));
    expect(expected.length).toBe(EXPECTED_BYTES);
    expect(createHash('sha256').update(expected).digest('hex')).toBe(
      EXPECTED_SHA,
    );
    const source = join(caseDir, 'workspace/reports/ledger.csv');
    await bytesProof(source, expected);
    const output = join(caseDir, 'browser-output');
    expect(samePath(await realpath(output), output)).toBe(true);
    expect(await readdir(output)).toEqual([]);
    expect(
      expires - Date.now(),
      'Release must remain current before launch',
    ).toBeGreaterThan(0);

    browser = await playwright.chromium.launch({
      executablePath: release.images.browser.path,
      headless: true,
      timeout: 15_000,
    });
    context = await browser.newContext({
      serviceWorkers: 'block',
      acceptDownloads: true,
      viewport: { width: 1280, height: 900 },
    });
    context.setDefaultTimeout(5000);
    context.setDefaultNavigationTimeout(10_000);
    let otherReal = 0;
    // Page-level fixture routing takes precedence. This catch-all also blocks
    // popup/worker requests instead of allowing an uninstrumented real path.
    await context.route('**/*', async (route) => {
      otherReal++;
      await route.abort('blockedbyclient');
    });
    await context.routeWebSocket('**/*', async (socket) => {
      otherReal++;
      await socket.close();
    });
    const page = await context.newPage();
    let downloadEvents = 0;
    page.on('download', () => {
      downloadEvents++;
    });
    const fixture = await installConsoleFixture(page, undefined, {
      origin: PREVIEW,
      realStatic: true,
      realWorkspaceFiles: true,
    });
    await page.goto(`${PREVIEW}/#/files`);
    await fixture.waitForStream();
    await expect(
      page.getByRole('heading', { name: 'Files', exact: true }),
    ).toBeVisible();
    await expect
      .poll(() =>
        fixture.workspaceFileResponses.map((response) => response.kind),
      )
      .toEqual(['list']);
    expect(fixture.workspaceFileResponses[0]!.body).toMatchObject({
      files: [
        {
          name: 'ledger.csv',
          path: 'reports/ledger.csv',
          sizeBytes: EXPECTED_BYTES,
        },
      ],
      truncated: false,
    });
    await page
      .getByRole('button', { name: /^ledger\.csv reports\/ledger\.csv / })
      .click();
    await expect
      .poll(() =>
        fixture.workspaceFileResponses.map((response) => response.kind),
      )
      .toEqual(['list', 'preview']);
    expect(fixture.workspaceFileResponses[1]!.body).toEqual({
      path: 'reports/ledger.csv',
      content: expected.toString('utf8'),
      truncated: false,
    });
    await expect(
      page.getByRole('region', { name: 'File preview' }).locator('pre'),
    ).toContainText('137.42');
    expect(downloadEvents).toBe(0);
    expect(fixture.workspaceFilesPolicy.snapshot()).toEqual({
      admitted: ['list', 'preview'],
      refused: 0,
    });
    const downloadPromise = page.waitForEvent('download', { timeout: 10_000 });
    await page
      .getByRole('button', { name: 'Download current file', exact: true })
      .click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('ledger.csv');
    expect(download.url()).toMatch(/^blob:http:\/\/127\.0\.0\.1:18753\//);
    await download.saveAs(join(output, 'ledger.csv'));
    expect(await download.failure()).toBeNull();
    await fixture.waitForWorkspaceResponses();
    await expect(
      page.getByRole('button', { name: 'Download current file', exact: true }),
    ).toBeEnabled();
    await bytesProof(join(output, 'ledger.csv'), expected);
    await bytesProof(source, expected);
    expect(await readdir(output)).toEqual(['ledger.csv']);
    expect(fixture.workspaceFilesPolicy.snapshot()).toEqual({
      admitted: ['list', 'preview', 'download'],
      refused: 0,
    });
    expect(fixture.workspaceFileResponseCounts).toMatchObject({
      success: 3,
      errors: 0,
      redirects: 0,
    });
    expect(
      fixture.workspaceFileResponses.map((response) => response.kind),
    ).toEqual(['list', 'preview', 'download']);
    expect(fixture.workspaceFileResponses[2]!.bytes).toBe(EXPECTED_BYTES);
    expect(fixture.workspaceFileResponses[2]!.sha256).toBe(EXPECTED_SHA);
    expect(fixture.calls.every((call) => call.method === 'GET')).toBe(true);
    expect(otherReal).toBe(0);
    expect(downloadEvents).toBe(1);
    expect(context.pages()).toEqual([page]);
    fixture.assertClean();
    verifyClosed = () => {
      expect(fixture.workspaceFilesPolicy.snapshot()).toEqual({
        admitted: ['list', 'preview', 'download'],
        refused: 0,
      });
      expect(fixture.workspaceFileResponseCounts).toMatchObject({
        success: 3,
        errors: 0,
        redirects: 0,
      });
      expect(otherReal).toBe(0);
      expect(downloadEvents).toBe(1);
      fixture.assertClean();
    };
    result = {
      version: 1,
      milestone: 'M69',
      caseId: release.caseId,
      nonce: release.nonce,
      passed: true,
      counts: {
        list: 1,
        preview: 1,
        download: 1,
        otherReal,
        errors: fixture.workspaceFileResponseCounts.errors,
        downloadEvents,
      },
      order: fixture.workspaceFileResponses.map((response) => response.kind),
      responses: fixture.workspaceFileResponses.map(
        ({ kind, status, bytes, redirects }) => ({
          kind,
          status,
          bytes,
          redirects,
        }),
      ),
      saved: {
        relativePath: 'browser-output/ledger.csv',
        basename: 'ledger.csv',
        bytes: EXPECTED_BYTES,
        sha256: EXPECTED_SHA,
      },
      sourceUnchanged: true,
    };
  } catch (error) {
    failure = error;
  } finally {
    try {
      await context?.close();
    } catch (error) {
      failure ??= error;
    }
    try {
      await browser?.close();
    } catch (error) {
      failure ??= error;
    }
    try {
      verifyClosed?.();
    } catch (error) {
      failure ??= error;
    }
  }
  // Only the supervisor may consume a release; publish complete evidence once,
  // after closing our browser context, without replacing any existing result.
  if (resultPath && release) {
    await writeFile(
      resultPath,
      JSON.stringify(
        failure
          ? {
              version: 1,
              milestone: 'M69',
              caseId: release.caseId,
              nonce: release.nonce,
              passed: false,
              errorKind: (failure instanceof Error
                ? failure.name
                : 'UnknownFailure'
              ).slice(0, 64),
            }
          : result,
      ),
      { flag: 'wx' },
    );
  }
  if (failure) throw failure;
});
