import { createHash } from 'node:crypto';
import { expect, test, type Locator, type Page, type Route } from '@playwright/test';
import type { Approval, ImageVersion, Run, SessionImages } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { about } from './fixtures/journeys';
import { AGENT, ORIGIN, createState, queuedRun, session } from './fixtures/state';

// Tiny red/blue PNGs, credentials, image ledger and streams are synthetic.
// The shared fixture serves ViewHarness's built assets; no daemon or provider is used.
const OWNER_KEY = 'fixture-image-owner-key-only';
const MODEL = 'gpt-image-2.5-sunburst';
const originalBytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==', 'base64');
const editedBytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==', 'base64');

function imageVersion(id: string, name: string, bytes: Buffer, parentAttachmentId: string | null = null): ImageVersion {
  return {
    id, attachment: { id: `att_${id}`, name, mime: 'image/png', size: bytes.length, kind: 'image', path: `attachments/${id}.png`, createdAtMs: 2 },
    parentAttachmentId, width: 1024, height: 1024, model: MODEL, createdAtMs: 2,
  };
}

async function openImagesConsole(page: Page) {
  const state = createState();
  state.agents = state.agents.slice(0, 1);
  const chat = { ...session('chat:images', 'Image conversation'), mainChat: true, clearGeneration: 0, messageCount: 1 };
  const other = { ...session('chat:other', 'Another conversation'), clearGeneration: 0, messageCount: 1 };
  state.sessions = [chat, other];
  state.messages[chat.id] = [{ id: 'image-reply', role: 'assistant', text: 'Your saved image and edited version are ready.', attachments: [], metadata: {}, createdAtMs: 2 }];
  state.messages[other.id] = [{ id: 'other-reply', role: 'assistant', text: 'This is another conversation.', attachments: [], metadata: {}, createdAtMs: 2 }];
  const original = imageVersion('original', 'Original.png', originalBytes);
  const edited = imageVersion('edited', 'Edited.png', editedBytes, original.attachment.id);
  const images: SessionImages = {
    clearGeneration: 0, provider: 'openai', model: MODEL,
    operations: [original, edited].map((version) => ({
      id: `op_${version.id}`, prompt: version.parentAttachmentId ? 'Make the bird blue' : 'Draw a red bird',
      sourceAttachmentId: version.parentAttachmentId, createdAtMs: 1, status: 'succeeded', version,
    })),
  };
  state.attachments = [original.attachment, edited.attachment];
  const fixture = await installConsoleFixture(page, state, { chatSuite: { ownerApiKey: OWNER_KEY } });
  const record = (route: Route) => {
    const request = route.request();
    const url = new URL(request.url());
    expect(url.origin).toBe(ORIGIN);
    expect(request.headers()['x-api-key']).toBe(OWNER_KEY);
    const body = request.postData() ? request.postDataJSON() as Record<string, unknown> : {};
    const call = { method: request.method(), path: decodeURIComponent(url.pathname), body, key: request.headers()['idempotency-key'] ?? null, headers: request.headers() };
    fixture.calls.push(call);
    return call;
  };
  await page.route('**/api/agents/companion/sessions/*/images', async (route) => {
    const call = record(route);
    expect(call.method).toBe('GET');
    expect([`/api/agents/${AGENT}/sessions/${chat.id}/images`, `/api/agents/${AGENT}/sessions/${other.id}/images`]).toContain(call.path);
    await route.fulfill({ json: call.path.includes(`/${chat.id}/`) ? images : { ...images, operations: [] } });
  });
  await page.route('**/api/agents/companion/attachments/*', async (route) => {
    const call = record(route);
    expect(call.method).toBe('GET');
    expect([original.attachment.id, edited.attachment.id]).toContain(call.path.split('/').at(-1));
    await route.fulfill({ contentType: 'image/png', body: call.path.endsWith(`/${original.attachment.id}`) ? originalBytes : editedBytes });
  });
  // Admit the current composer's timeZone locally; the shared fixture predates it.
  await page.route('**/api/agents/companion/sessions/*/runs', async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const call = record(route);
    expect(Object.keys(call.body).sort()).toEqual(call.body.attachmentIds ? ['attachmentIds', 'mode', 'text', 'timeZone'] : ['mode', 'text', 'timeZone']);
    expect(typeof call.body.text).toBe('string');
    expect(typeof call.body.timeZone).toBe('string');
    expect(call.body.timeZone).toBeTruthy();
    expect(call.body.mode).toBe('queue');
    expect(call.key).toBeTruthy();
    const target = state.sessions.find((item) => call.path === `/api/agents/${AGENT}/sessions/${item.id}/runs`);
    expect(target).toBeDefined();
    const attachmentIds = (call.body.attachmentIds ?? []) as string[];
    expect(attachmentIds.every((id) => state.attachments.some((item) => item.id === id))).toBe(true);
    const run = { ...queuedRun(target!.id, { text: call.body.text as string, attachmentIds, skill: null }, call.key!), id: `run_${state.runs.length + 1}` };
    state.runs.push(run);
    state.snapshotRuns = [{ run, stepId: null, text: '', textOffset: 0, tools: [] }];
    target!.activeRuns = 1;
    await route.fulfill({ status: 202, json: { run } });
  });
  await page.goto('/');
  await page.getByLabel('Owner API key').fill(OWNER_KEY);
  await page.getByRole('button', { name: 'Unlock owner console' }).click();
  await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
  await fixture.waitForStream();
  await expect(page.getByRole('region', { name: 'Chat images' })).toBeVisible();
  return { fixture, chat, other, images, original, edited, record };
}

async function savedPreview(page: Page, version: ImageVersion, pixel: number[]) {
  const preview = page.getByRole('complementary', { name: 'Artifact preview' });
  const image = preview.getByRole('img', { name: version.attachment.name, exact: true });
  await expect(preview.getByRole('heading', { name: version.attachment.name })).toBeVisible();
  await expect(image).toHaveAttribute('src', /^blob:/);
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.complete && element.naturalWidth > 0)).toBe(true);
  // Check rendered pixels as well as IDs: returning to the original restores its bytes.
  expect(await image.evaluate((element: HTMLImageElement) => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d')!;
    context.drawImage(element, 0, 0);
    return Array.from(context.getImageData(0, 0, 1, 1).data);
  })).toEqual(pixel);
  await expect(preview.getByRole('link', { name: 'Download PNG' })).toHaveAttribute('href', /^blob:/);
  await expect(preview.getByRole('link', { name: 'Download PNG' })).toHaveAttribute('download', version.attachment.name);
  await expect(preview.locator('iframe')).toHaveCount(0);
  return preview;
}

function imageApproval(run: Run, tool: 'generate_image' | 'edit_image', original: ImageVersion): Approval {
  const edit = tool === 'edit_image';
  return {
    id: `approval_${tool}`, agentId: run.agentId, sessionId: run.sessionId, runId: run.id, toolCallId: `call_${tool}`,
    tool, class: 'write', arguments: JSON.stringify({
      _anima_exact_image: true, prompt: edit ? 'Make the selected bird blue' : 'Draw a red bird',
      sourceAttachmentId: edit ? original.attachment.id : null, sourceName: edit ? original.attachment.name : null,
      sourceSha256: edit ? createHash('sha256').update(originalBytes).digest('hex') : null,
      provider: 'openai', model: MODEL, size: '1024x1024', quality: 'medium', count: 1,
      disclosure: 'Remote OpenAI request. API charges apply; cost is unknown.',
    }),
    argumentsTruncated: false, suggestedMatcher: { kind: 'any', value: '' }, matcherKinds: ['any'],
    createdAtMs: Date.now(), expiresAtMs: Date.now() + 180_000, status: 'pending', revision: 1, resolution: null,
  };
}

async function exactActions(card: Locator, allow: string) {
  await expect(card).toContainText('will be sent to OpenAI. API charges apply.');
  await expect(card).toContainText('Cost is unknown');
  await expect(card).toContainText(MODEL);
  await expect(card.getByRole('button')).toHaveText([allow, 'Deny']);
  await expect(card.getByRole('combobox')).toHaveCount(0);
}

async function noHorizontalOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

for (const [size, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
] as const) {
  test(`saved image versions open authenticated blobs and hand the original to chat on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const { fixture, original, edited, images } = await openImagesConsole(page);
    const ledger = JSON.stringify(images);
    const gallery = page.getByRole('region', { name: 'Chat images' });
    await expect(gallery.getByRole('button', { name: 'Open image version 1' })).toContainText('Original.png');
    await expect(gallery.getByRole('button', { name: 'Open image version 2' })).toContainText('Edited');
    const input = page.getByPlaceholder('Message Anima…');
    await input.fill('Keep my edit instruction');
    await gallery.getByRole('button', { name: 'Open image version 1' }).click();
    let preview = await savedPreview(page, original, [255, 0, 0, 255]);
    if (size === 'desktop') {
      await expect(input).toBeVisible();
      const composerBounds = await input.boundingBox();
      const previewBounds = await preview.boundingBox();
      expect(previewBounds!.x).toBeGreaterThanOrEqual(composerBounds!.x + composerBounds!.width);
    }
    await expect(preview.getByRole('button', { name: 'Previous image version' })).toBeDisabled();
    await preview.getByRole('button', { name: 'Next image version' }).click();
    preview = await savedPreview(page, edited, [0, 0, 255, 255]);
    await expect(preview).toContainText('Version 2 of 2');
    await expect(preview.getByRole('button', { name: 'Next image version' })).toBeDisabled();
    await preview.getByRole('button', { name: 'Previous image version' }).click();
    preview = await savedPreview(page, original, [255, 0, 0, 255]);
    await expect(preview).toContainText('Version 1 of 2');
    await preview.getByRole('button', { name: 'Edit in chat' }).click();
    await expect(page.getByRole('list', { name: 'Message attachments' })).toContainText('Original.png');
    await expect(input).toHaveValue('Keep my edit instruction');
    await expect(input).toBeFocused();
    if (size === 'mobile') await expect(preview).toHaveCount(0);
    else await expect(preview).toBeVisible();
    expect(fixture.calls.filter((call) => call.method === 'POST' && /\/(runs|attachments|decision)$/.test(call.path))).toEqual([]);
    expect(JSON.stringify(images)).toBe(ledger);
    await noHorizontalOverflow(page);
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => fixture.state.runs.length).toBe(1);
    expect(fixture.state.runs[0].input).toEqual({ text: 'Keep my edit instruction', attachmentIds: [original.attachment.id], skill: null });
    expect(fixture.calls.filter((call) => call.method === 'POST' && call.path.endsWith('/attachments'))).toEqual([]);
    expect(fixture.calls.filter((call) => call.path.endsWith(`/attachments/${original.attachment.id}`)).length).toBeGreaterThanOrEqual(2);
    expect(fixture.calls.some((call) => call.path.endsWith(`/attachments/${edited.attachment.id}`))).toBe(true);
    fixture.assertClean();
  });

  test(`image approval discloses the remote charge and permits only one decision on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const { fixture, original, record } = await openImagesConsole(page);
    await page.getByPlaceholder('Message Anima…').fill('Draw a bird, then edit my saved image');
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => fixture.state.runs.length).toBe(1);
    const run = fixture.state.runs[0];
    fixture.start(run);
    await fixture.emit({ type: 'run.started', ...about(run), run });
    const generate = imageApproval(run, 'generate_image', original);
    fixture.requestApproval(generate);
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval: generate });
    const generateCard = page.getByRole('region', { name: 'Approval needed: generate_image' });
    await exactActions(generateCard, 'Allow once and generate');
    await expect(generateCard.getByLabel('Image prompt')).toHaveText('Draw a red bird');
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toHaveLength(0);
    await generateCard.getByRole('button', { name: 'Deny', exact: true }).click();
    await expect.poll(() => fixture.calls.filter((call) => call.path.endsWith('/decision')).length).toBe(1);
    expect(fixture.calls.find((call) => call.path.endsWith('/decision'))?.body).toEqual({ decision: 'deny', revision: 1 });
    await fixture.emit({ type: 'approval.resolved', ...about(run), approval: generate });

    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    let sourceReads = 0;
    await page.route(`**/api/agents/${AGENT}/attachments/${original.attachment.id}`, async (route) => {
      expect(record(route).method).toBe('GET');
      sourceReads++;
      await gate;
      await route.fulfill({ contentType: 'image/png', body: originalBytes });
    });
    const edit = imageApproval(run, 'edit_image', original);
    fixture.requestApproval(edit);
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval: edit });
    const editCard = page.getByRole('region', { name: 'Approval needed: edit_image' });
    try {
      await exactActions(editCard, 'Allow once and edit');
      await expect(editCard).toContainText('The original is preserved.');
      await expect(editCard.getByLabel('Image prompt')).toHaveText('Make the selected bird blue');
      await expect(editCard.getByRole('button', { name: 'Allow once and edit' })).toBeDisabled();
      await expect.poll(() => sourceReads).toBe(1);
      expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toHaveLength(1);
    } finally { release(); }
    await expect(editCard.getByRole('img', { name: 'Original.png', exact: true })).toHaveAttribute('src', /^blob:/);
    await expect(editCard.getByRole('button', { name: 'Allow once and edit' })).toBeEnabled();
    await noHorizontalOverflow(page);
    await editCard.getByRole('button', { name: 'Allow once and edit' }).click();
    await expect.poll(() => fixture.calls.filter((call) => call.path.endsWith('/decision')).length).toBe(2);
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))[1]).toMatchObject({ path: `/api/approvals/${edit.id}/decision`, body: { decision: 'allow_once', revision: 1 } });
    await expect(editCard).toContainText('Decision sent.');
    await expect(editCard.getByRole('button')).toHaveCount(0);
    fixture.assertClean();
  });

  test(`switching conversations and clearing chat invalidate the active image scope on ${size}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const { fixture, original, chat, other, images } = await openImagesConsole(page);
    await page.getByRole('button', { name: 'Open image version 1' }).click();
    const preview = await savedPreview(page, original, [255, 0, 0, 255]);
    await preview.getByRole('button', { name: 'Edit in chat' }).click();
    await expect(page.getByRole('list', { name: 'Message attachments' })).toContainText('Original.png');
    if (size === 'desktop') await preview.getByRole('button', { name: 'Close artifact preview' }).click();
    await page.getByRole('button', { name: 'Anima, menu' }).click();
    await page.getByRole('menuitem', { name: 'Other conversations' }).click();
    await page.getByRole('dialog', { name: 'History', exact: true }).getByRole('button', { name: other.title, exact: true }).click();
    await expect(page.getByText('This is another conversation.', { exact: true })).toBeVisible();
    await expect(page.getByRole('complementary', { name: 'Artifact preview' })).toHaveCount(0);
    await expect(page.getByRole('list', { name: 'Message attachments' })).toHaveCount(0);
    await expect(page.getByRole('region', { name: 'Chat images' })).toHaveCount(0);
    await page.getByPlaceholder('Message Anima…').fill('A separate text request');
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => fixture.state.runs.length).toBe(1);
    expect(fixture.state.runs[0]).toMatchObject({ sessionId: other.id, input: { text: 'A separate text request', attachmentIds: [] } });

    await page.getByRole('button', { name: 'Main chat', exact: true }).click();
    await expect(page.getByRole('list', { name: 'Message attachments' })).toContainText('Original.png');
    await page.getByRole('button', { name: 'Open image version 1' }).click();
    await savedPreview(page, original, [255, 0, 0, 255]);
    // A clear from another owner window arrives through the ordinary session stream.
    const cleared = { ...chat, clearGeneration: 1, messageCount: 0, preview: null };
    fixture.state.sessions = fixture.state.sessions.map((item) => item.id === chat.id ? cleared : item);
    fixture.state.messages[chat.id] = [];
    images.clearGeneration = 1;
    images.operations = [];
    await fixture.emit({ type: 'session.cleared', agentId: AGENT, sessionId: chat.id, seq: 0, at: Date.now() });
    await expect(page.getByRole('complementary', { name: 'Artifact preview' })).toHaveCount(0);
    await expect(page.getByRole('list', { name: 'Message attachments' })).toHaveCount(0);
    await expect(page.getByRole('region', { name: 'Chat images' })).toHaveCount(0);
    await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
    await noHorizontalOverflow(page);
    await page.getByPlaceholder('Message Anima…').fill('Fresh text after clearing');
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => fixture.state.runs.length).toBe(2);
    expect(fixture.state.runs[1]).toMatchObject({ sessionId: chat.id, input: { text: 'Fresh text after clearing', attachmentIds: [] } });
    expect(fixture.calls.filter((call) => call.path.endsWith('/decision'))).toEqual([]);
    fixture.assertClean();
  });
}
