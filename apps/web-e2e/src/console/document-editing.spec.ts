import { expect, test, type Page } from '@playwright/test';
import type { Approval, OfficeInventory, OfficeInventoryPage, Run } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { about, send } from './fixtures/journeys';
import { AGENT, createState, queuedRun } from './fixtures/state';

// All documents and credentials below are synthetic. No daemon or account is used.
const sourceHash = 'a'.repeat(64);
const reviewHash = 'b'.repeat(64);
const candidateHash = 'c'.repeat(64);
const proposalId = 'proposal_fixture';
const path = 'reports/edited.xlsx';
const destination = { state: 'absent' as const };
const inventory: OfficeInventory = {
  version: 1,
  format: 'xlsx',
  targets: [{
    id: 'sheet:s1:B2', label: 'Budget total', editable: true,
    value: { kind: 'cell', sheet_id: 'sheet:s1', address: 'B2', value: { type: 'number', value: 42 }, formula: 'SUM(A1:A2)', calculation: 'needs_external_recalculation' },
  }],
  retained_hyperlinks: [{ relationship_id: 'r1', target: 'https://example.invalid/reference' }],
  limitations: ['Content preview only; layout is not rendered.'],
};
const review = {
  version: 1, proposal_id: proposalId, review_sha256: reviewHash,
  source: { kind: 'attachment', display_name: 'budget.xlsx' }, source_sha256: sourceHash,
  file_path: path, expected_destination: destination, expires_at_ms: 4_000_000_000_000,
  preview_quality: 'structural_content', before: inventory, after: inventory,
  changes: [
    { operation_index: 0, label: 'Budget total', before: '40', after: '42', derived: false },
    { operation_index: null, label: 'Formula cache', before: 'stored', after: 'recalculate', derived: true },
  ],
  preservation: { unchanged_parts: 4, edited_parts: 1, unchanged_bytes_verified: true, container_normalized: true },
};

function documentApproval(run: Run): Approval {
  return {
    id: 'approval_document', agentId: run.agentId, sessionId: run.sessionId, runId: run.id,
    toolCallId: 'call_document', tool: 'apply_document_edit', class: 'write',
    arguments: JSON.stringify({ _anima_exact_document_edit: true, proposal_id: proposalId, review_sha256: reviewHash, candidate_sha256: candidateHash, source_sha256: sourceHash, file_path: path, expected_destination: destination }),
    argumentsTruncated: false, suggestedMatcher: { kind: 'any', value: '' }, matcherKinds: ['any'],
    createdAtMs: Date.now(), expiresAtMs: Date.now() + 60_000,
    status: 'pending', revision: 1, resolution: null,
  };
}

async function noHorizontalOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

async function openDocumentConsole(page: Page) {
  const fixture = await installConsoleFixture(page, createState(), { chatSuite: {} });
  // The shared fixture predates the composer's timeZone field. Keep this
  // override local and admit only the exact ordinary, unassigned chat request.
  await page.route('**/api/agents/companion/sessions/*/runs', async (route) => {
    const request = route.request();
    if (request.method() !== 'POST') return route.fallback();
    const pathname = decodeURIComponent(new URL(request.url()).pathname);
    const session = fixture.state.sessions.find((item) => item.mainChat);
    expect(session).toBeDefined();
    expect(session?.agentId).toBe(AGENT);
    expect(session?.projectId ?? null).toBeNull();
    expect(session?.projectBindingRevision ?? 0).toBe(0);
    expect(session?.clearGeneration ?? 0).toBe(0);
    expect(pathname).toBe(`/api/agents/${AGENT}/sessions/${session!.id}/runs`);
    const body = request.postDataJSON() as Record<string, unknown>;
    expect(Object.keys(body).sort()).toEqual(['mode', 'text', 'timeZone']);
    expect(typeof body.text).toBe('string');
    expect(body.mode).toBe('queue');
    expect(typeof body.timeZone).toBe('string');
    expect(body.timeZone).toBeTruthy();
    const key = request.headers()['idempotency-key'];
    expect(key).toBeTruthy();
    const run = queuedRun(session!.id, { text: body.text as string, attachmentIds: [], skill: null }, key);
    fixture.calls.push({ method: 'POST', path: pathname, body, key, headers: request.headers() });
    fixture.state.runs.push(run);
    fixture.state.snapshotRuns = [{ run, stepId: null, text: '', textOffset: 0, tools: [] }];
    session!.activeRuns = 1;
    if (run.input.text) session!.title = run.input.text;
    await route.fulfill({ status: 202, json: { run } });
  });
  await page.goto('/');
  await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
  await fixture.waitForStream();
  return fixture;
}

for (const [size, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
] as const) {
  test(`receipt-backed Office preview is paginated and inert on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const fixture = await installConsoleFixture(page, createState(), { chatSuite: {} });
    const requests: string[] = [];
    const first: OfficeInventoryPage = {
      version: 1, format: 'xlsx', source_sha256: sourceHash,
      source: { kind: 'workspace', display_name: path }, targets: inventory.targets,
      coverage: { total_targets: 2, returned_targets: 1, complete: false },
      retained_hyperlink_count: 1, limitations: inventory.limitations, next_cursor: 'next-page',
    };
    await page.route('**/api/workspace/file**', async (route) => {
      const request = route.request();
      const url = new URL(request.url());
      expect(request.method()).toBe('GET');
      requests.push(`${url.pathname}${url.search}`);
      if (url.pathname === '/api/workspace/files') {
        return route.fulfill({ json: { files: [{ path, name: 'edited.xlsx', sizeBytes: 500, modifiedAtMs: null }], truncated: false } });
      }
      expect(url.pathname).toBe('/api/workspace/file');
      expect(url.searchParams.get('path')).toBe(path);
      const cursor = url.searchParams.get('cursor');
      expect([null, 'next-page']).toContain(cursor);
      const pageData: OfficeInventoryPage = cursor === null ? first : {
        ...first, targets: [{ id: 'sheet:s1:B3', label: 'Note', editable: false, value: { kind: 'text', text: '<script>not markup</script>' } }],
        coverage: { total_targets: 2, returned_targets: 1, complete: true }, next_cursor: null,
      };
      return route.fulfill({ json: { path, content: '', truncated: cursor === null, previewKind: 'office_content', sha256: sourceHash, officeContent: pageData } });
    });
    await page.goto('/#/files');
    await fixture.waitForStream();
    await page.getByRole('button', { name: /edited\.xlsx/ }).click();
    const preview = page.getByRole('region', { name: 'File preview' });
    await expect(preview).toContainText('needs external recalculation');
    await expect(preview).toContainText('1 of 2 content items shown');
    await expect(preview).toContainText('layout is not rendered');
    await preview.getByRole('button', { name: 'Load more content' }).click();
    await expect(preview).toContainText('2 of 2 content items shown');
    await expect(preview).toContainText('<script>not markup</script>');
    await expect(preview.locator('script, img, a')).toHaveCount(0);
    await noHorizontalOverflow(page);
    expect(requests).toHaveLength(3);
    fixture.assertClean();
  });

  test(`exact owner review waits for the complete matching response on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const fixture = await openDocumentConsole(page);
    const run = await send(page, fixture, 'Review my spreadsheet');
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    let reads = 0;
    await page.route('**/api/document-edits/**', async (route) => {
      const url = new URL(route.request().url());
      expect(route.request().method()).toBe('GET');
      expect(url.pathname).toBe(`/api/document-edits/${proposalId}`);
      expect(url.searchParams.get('agent_id')).toBe(run.agentId);
      expect(url.searchParams.get('session_id')).toBe(run.sessionId);
      reads++;
      await gate;
      await route.fulfill({ json: review });
    });
    const approval = documentApproval(run);
    fixture.requestApproval(approval);
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval });
    const card = page.getByRole('region', { name: 'Document changes' });
    await expect(card).toBeVisible();
    await expect(card.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
    await expect.poll(() => reads).toBe(1);
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toHaveLength(0);
    release();
    await expect(card).toContainText('Formula cache');
    await expect(card).toContainText('automatic change');
    await expect(card).toContainText('needs external recalculation');
    await expect(card).toContainText('Content preview only');
    await expect(card.locator('a')).toHaveCount(0);
    await expect(card.getByRole('button', { name: 'Always allow' })).toHaveCount(0);
    await expect(card.getByRole('button', { name: 'Allow for this session' })).toHaveCount(0);
    await expect(card.getByRole('button', { name: 'Apply these changes' })).toBeEnabled();
    await noHorizontalOverflow(page);
    await card.getByRole('button', { name: 'Apply these changes' }).click();
    await expect.poll(() => fixture.calls.filter((call) => call.path.endsWith('/decision')).length).toBe(1);
    expect(fixture.calls.find((call) => call.path.endsWith('/decision'))?.body).toEqual({ decision: 'allow_once', revision: 1 });
    await expect(card).toContainText('Decision sent.');
    fixture.assertClean();
  });

  test(`mismatched review cannot be applied on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const fixture = await openDocumentConsole(page);
    const run = await send(page, fixture, 'Review a changed spreadsheet');
    await page.route('**/api/document-edits/**', (route) => route.fulfill({ json: { ...review, review_sha256: 'd'.repeat(64) } }));
    const approval = documentApproval(run);
    fixture.requestApproval(approval);
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval });
    const card = page.getByRole('region', { name: 'Document changes' });
    await expect(card.getByRole('alert')).toContainText('changed or is incomplete');
    await expect(card.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
    await noHorizontalOverflow(page);
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toHaveLength(0);
    await card.getByRole('button', { name: 'Keep original' }).click();
    await expect.poll(() => fixture.calls.filter((call) => call.path.endsWith('/decision')).length).toBe(1);
    expect(fixture.calls.find((call) => call.path.endsWith('/decision'))?.body).toEqual({ decision: 'deny', revision: 1 });
    fixture.assertClean();
  });
}

test('read-only shared conversation never fetches private document review', async ({ page }) => {
  const viewer = { shareId: 'share-11111111-2222-4333-8444-555555555555', viewerId: 'viewer-fixture', secret: 'fixture-secret-only', revoked: false };
  const fixture = await installConsoleFixture(page, createState(), { chatSuite: { viewer } });
  let privateReads = 0;
  await page.route('**/api/document-edits/**', async (route) => { privateReads++; await route.abort(); });
  await page.goto(`/#/shared/${viewer.shareId}`);
  await page.getByLabel('Viewer ID', { exact: true }).fill(viewer.viewerId);
  await page.getByLabel('Viewer secret', { exact: true }).fill(viewer.secret);
  await page.getByRole('button', { name: 'Open conversation' }).click();
  await expect(page.getByRole('region', { name: 'Shared transcript' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Apply these changes' })).toHaveCount(0);
  expect(privateReads).toBe(0);
  expect(fixture.calls.every((call) => call.method === 'GET' && call.path.startsWith('/api/shared/conversations/'))).toBe(true);
  fixture.assertClean();
});
