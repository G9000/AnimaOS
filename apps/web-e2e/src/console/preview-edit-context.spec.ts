import { createHash } from 'node:crypto';
import { expect, test, type Page } from '@playwright/test';
import type { Approval, OfficeInventoryPage, PreviewEditContext } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, createState, queuedRun, session } from './fixtures/state';
import { about } from './fixtures/journeys';

// Browser acceptance uses synthetic files, run admission and approvals. No account or provider is used.
const path = 'artifacts/report.html';
const text = 'Original paragraph';
const source = `<!doctype html><html><body><p>${text}</p><script>window.artifactOnly=true</script></body></html>`;
const sha256 = createHash('sha256').update(source).digest('hex');
const start = Buffer.byteLength(source.slice(0, source.indexOf(text)));
const target = { id: `html:${start}:${start + text.length}`, label: 'Text 1', editable: true, value: { kind: 'html_text' as const, start_utf8: start, end_utf8: start + text.length, text } };
const inventory: OfficeInventoryPage = { version: 1, source_scope: 'd'.repeat(64), format: 'html', source_sha256: sha256, source: { kind: 'workspace', display_name: path }, targets: [target], coverage: { total_targets: 1, returned_targets: 1, complete: true }, retained_hyperlink_count: 0, limitations: ['Only original text is selectable.'], next_cursor: null };

async function openPreview(page: Page) {
  const state = createState();
  const chat = { ...session('chat:preview', 'Preview edit'), mainChat: true, clearGeneration: 0 };
  state.sessions.push(chat);
  state.messages[chat.id] = [{ id: 'report', role: 'assistant', text: `Created \`${path}\`.`, attachments: [], metadata: {}, createdAtMs: 1 }];
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  await page.route('**/api/workspace/file/download?*', async (route) => {
    expect(new URL(route.request().url()).searchParams.get('path')).toBe(path);
    await route.fulfill({ contentType: 'application/octet-stream', body: source });
  });
  await page.route('**/api/document-edits/source?*', async (route) => {
    const url = new URL(route.request().url());
    expect(url.searchParams.get('agent_id')).toBe(AGENT);
    expect(url.searchParams.get('session_id')).toBe(chat.id);
    expect(url.searchParams.get('file_path')).toBe(path);
    await route.fulfill({ json: inventory });
  });
  await page.route('**/api/agents/companion/sessions/*/runs', async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const request = route.request();
    const body = request.postDataJSON();
    const key = request.headers()['idempotency-key'];
    const run = queuedRun(chat.id, { text: body.text, attachmentIds: [], skill: null }, key);
    fixture.calls.push({ method: 'POST', path: new URL(request.url()).pathname, body, key, headers: request.headers() });
    state.runs.push(run); state.snapshotRuns = [{ run, stepId: null, text: '', textOffset: 0, tools: [] }]; chat.activeRuns = 1;
    await route.fulfill({ status: 202, json: { run } });
  });
  await page.goto('/');
  await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
  await fixture.waitForStream();
  await page.getByPlaceholder('Message Anima…').fill('Rewrite this clearly');
  await expect(page.getByPlaceholder('Message Anima…')).toHaveValue('Rewrite this clearly');
  await page.getByRole('button', { name: `Open ${path}` }).click();
  await page.getByRole('button', { name: 'Select content to edit' }).click();
  await expect(page.getByRole('button', { name: 'Select Text 1' })).toBeVisible();
  return { fixture, chat };
}

for (const [name, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]] as const) {
  test(`selected HTML text becomes a scoped draft and exact review on ${name}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    const { fixture, chat } = await openPreview(page);
    const input = page.getByPlaceholder('Message Anima…');
    // The conversation and its pre-existing draft stay mounted beneath the phone preview.
    const content = page.getByTestId(`preview-target-text-${target.id}`);
    await content.evaluate((element) => {
      const range = document.createRange(); range.setStart(element.firstChild!, 0); range.setEnd(element.firstChild!, 8);
      const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range);
      element.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    });
    expect(fixture.calls.filter((call) => call.path.endsWith('/runs') && call.method === 'POST')).toHaveLength(0);
    await page.getByRole('button', { name: 'Add selection to draft' }).click();
    await expect(page.getByRole('region', { name: 'Selected content for edit' })).toContainText('Original');
    await expect(input).toHaveValue('Rewrite this clearly');
    await expect(input).toBeFocused();
    if (name === 'mobile') await expect(page.getByRole('complementary', { name: 'Artifact preview' })).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`preview-selection-${name}.png`) });
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => fixture.state.runs.length).toBe(1);
    const admission = fixture.calls.find((call) => call.path.endsWith('/runs') && call.method === 'POST')!.body as { text: string; documentEditContext: PreviewEditContext };
    expect(admission.text).toBe('Rewrite this clearly');
    expect(admission.documentEditContext).toEqual({ version: 1, source: { kind: 'workspace', path, sha256, scope: 'd'.repeat(64) }, selection: { kind: 'html_text', start_utf8: start, end_utf8: start + 8, expected_text: 'Original' } });
    const run = fixture.state.runs[0];
    const reviewHash = 'b'.repeat(64);
    const expectedDestination = { state: 'present', sha256 };
    const before = { version: 1, format: 'html', targets: [target], retained_hyperlinks: [], limitations: ['Text content only'] };
    const after = { ...before, targets: [{ ...target, id: `html:${start}:${start + text.length - 1}`,
      value: { ...target.value, end_utf8: start + text.length - 1, text: text.replace('Original', 'Clearer') } }] };
    await page.route('**/api/document-edits/proposal-preview?*', async (route) => route.fulfill({ json: { version: 1, proposal_id: 'proposal-preview', review_sha256: reviewHash, source: { kind: 'workspace', display_name: path }, source_sha256: sha256, file_path: path, expected_destination: expectedDestination, expires_at_ms: Date.now() + 60_000, preview_quality: 'structural_content', before, after, changes: [{ operation_index: 0, label: 'Text 1', before: 'Original', after: 'Clearer', derived: false }], preservation: { unchanged_parts: 1, edited_parts: 1, unchanged_bytes_verified: true, container_normalized: false } } }));
    const approval: Approval = { id: 'approval-preview', agentId: AGENT, sessionId: chat.id, runId: run.id, toolCallId: 'apply-preview', tool: 'apply_document_edit', class: 'write', arguments: JSON.stringify({ _anima_exact_document_edit: true, proposal_id: 'proposal-preview', review_sha256: reviewHash, candidate_sha256: 'c'.repeat(64), source_sha256: sha256, file_path: path, expected_destination: expectedDestination }), argumentsTruncated: false, suggestedMatcher: { kind: 'any', value: '' }, matcherKinds: ['any'], createdAtMs: Date.now(), expiresAtMs: Date.now() + 60_000, status: 'pending', revision: 1, resolution: null };
    fixture.requestApproval(approval);
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval });
    const review = page.getByRole('region', { name: 'Document changes' });
    await expect(review).toContainText('Before: Original');
    await expect(review).toContainText('After: Clearer');
    await expect(review).toContainText('Clearer paragraph');
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toHaveLength(0);
    await review.getByRole('button', { name: 'Apply these changes' }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: testInfo.outputPath(`preview-review-${name}.png`) });
    await review.getByRole('button', { name: 'Apply these changes' }).click();
    await expect.poll(() => fixture.calls.filter((call) => call.path.endsWith('/decision')).length).toBe(1);
    expect(fixture.calls.find((call) => call.path.endsWith('/decision'))?.body).toEqual({ decision: 'allow_once', revision: 1 });
    fixture.assertClean();
  });
}
