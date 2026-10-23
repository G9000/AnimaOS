import { expect, test, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { ChatResult } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, ORIGIN, createState, session } from './fixtures/state';

const OWNER_KEY = 'fixture-owner-key-only';
const viewer = {
  shareId: 'share-11111111-2222-4333-8444-555555555555',
  viewerId: 'viewer-fixture', secret: 'fixture-viewer-secret-only', revoked: false,
};
const sharedPath = `/#/shared/${viewer.shareId}`;

function ownerState() {
  const state = createState();
  state.agents = state.agents.slice(0, 1);
  const chat = { ...session('chat:suite', 'Planning conversation'), mainChat: true, clearGeneration: 0, messageCount: 2 };
  state.sessions = [chat];
  state.messages[chat.id] = [
    { id: 'prompt-1', role: 'user', text: 'Help me prepare for the meeting.', attachments: [], metadata: {}, createdAtMs: 1 },
    { id: 'answer-1', role: 'assistant', text: 'Bring the agenda and a notebook.', attachments: [], metadata: {}, createdAtMs: 2 },
  ];
  const result: ChatResult = {
    id: 'result-planning', revision: 1, agentId: AGENT, sessionId: chat.id, clearGeneration: 0,
    title: 'Meeting preparation', pinned: false, createdAtMs: 3, updatedAtMs: 3,
    sourceRunId: 'run-answer', sourceToolCallId: 'tool-save', requestKey: 'save-planning',
    payload: { kind: 'comparison', columns: ['Item', 'Ready'], rows: [['Agenda', 'Yes'], ['Notebook', 'Yes']] },
  };
  return { state, result };
}

async function screenshot(page: Page, name: string) {
  const directory = resolve(__dirname, '../../../../.tmp');
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: resolve(directory, `chat-suite-${name}.png`), fullPage: true, animations: 'disabled' });
}

for (const [name, viewport] of [
  ['desktop', { width: 1440, height: 1000 }],
  ['mobile', { width: 390, height: 844 }],
] as const) {
  test(`chat-suite owner controls and pinned report at ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const { state, result } = ownerState();
    const fixture = await installConsoleFixture(page, state, { chatSuite: { ownerApiKey: OWNER_KEY, results: [result], viewer: { ...viewer } } });
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Open your Anima console' })).toBeVisible();
    expect(fixture.calls.every(call => call.path === '/api/capabilities')).toBe(true);
    await page.getByLabel('Owner API key').fill(OWNER_KEY);
    await page.getByRole('button', { name: 'Unlock owner console' }).click();
    const controls = page.getByRole('navigation', { name: 'Chat controls' });
    await expect(controls).toBeVisible();
    await expect(page.getByText('Bring the agenda and a notebook.', { exact: true })).toBeVisible();

    await controls.getByRole('button', { name: 'Model & thinking' }).click();
    const settings = page.getByRole('region', { name: 'Chat model settings' });
    await expect(settings.getByLabel('Chat model', { exact: true })).toHaveValue('');
    await settings.getByLabel('Chat model', { exact: true }).selectOption('openai:alternate-model');
    await settings.getByLabel('Chat reasoning effort', { exact: true }).selectOption('high');
    await settings.getByRole('button', { name: 'Save model settings' }).click();
    await expect(settings.getByText('Current chat choice: openai / alternate-model · high')).toBeVisible();
    expect(fixture.calls.find(call => call.method === 'PATCH' && call.path.endsWith('/model-preferences'))?.body).toEqual({
      primary: { mode: 'override', provider: 'openai', model: 'alternate-model', reasoningEffort: 'high' }, clearGeneration: 0,
    });

    await controls.getByRole('button', { name: 'Tools', exact: true }).click();
    await expect(page.getByRole('region', { name: 'Current chat tools' }).getByText('read_file', { exact: true })).toBeVisible();
    await controls.getByRole('button', { name: 'Second opinion' }).click();
    await page.getByLabel('Review focus', { exact: true }).fill('Check the preparation advice.');
    await page.getByRole('button', { name: 'Review latest answer' }).click();
    await expect(page.getByRole('article', { name: 'Review completed' })).toContainText('Repository and live state were not inspected.');

    await controls.getByRole('button', { name: 'Share', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Share conversation' });
    await dialog.getByLabel('Viewer name').fill('Guest reviewer');
    await dialog.getByRole('button', { name: 'Create viewer access' }).click();
    await expect(dialog.getByLabel('Share link', { exact: true })).toHaveValue(`${ORIGIN}${sharedPath}`);
    expect(await dialog.getByLabel('Share link', { exact: true }).inputValue()).not.toContain(viewer.secret);
    await expect(dialog.getByLabel('Viewer secret', { exact: true })).toHaveValue(viewer.secret);
    await dialog.getByRole('button', { name: 'Close sharing' }).click();
    const inline = page.getByRole('region', { name: 'Conversation results' });
    await inline.getByRole('searchbox', { name: 'Filter comparison' }).fill('Agenda');
    await expect(inline.getByRole('cell', { name: 'Notebook', exact: true })).toHaveCount(0);
    await inline.getByRole('searchbox', { name: 'Filter comparison' }).fill('');
    await inline.getByRole('button', { name: 'Pin report', exact: true }).click();
    const shelf = page.getByRole('complementary', { name: 'Pinned reports' });
    await expect(shelf.getByRole('heading', { name: 'Meeting preparation' })).toBeVisible();
    await expect(shelf.getByText('Pinned report · revision 2')).toBeVisible();
    await expect(page.getByPlaceholder('Message Anima…')).toBeVisible();
    await screenshot(page, name);
    expect(fixture.calls.filter(call => call.path !== '/api/capabilities').every(call => call.headers['x-api-key'] === OWNER_KEY)).toBe(true);
    fixture.assertClean();
  });
}

test('chat-suite standalone viewer uses credentials without mounting owner controls', async ({ page, context }) => {
  const grant = { ...viewer };
  const fixture = await installConsoleFixture(page, createState(), { chatSuite: { ownerApiKey: OWNER_KEY, viewer: grant } });
  await context.addCookies([{ name: 'owner-cookie-fixture', value: 'must-not-be-sent', url: ORIGIN }]);
  await page.goto(sharedPath);
  await expect(page.getByRole('button', { name: 'Open conversation' })).toBeVisible();
  expect(fixture.calls).toEqual([]);
  await page.getByLabel('Viewer ID', { exact: true }).fill(viewer.viewerId);
  await page.getByLabel('Viewer secret', { exact: true }).fill('incorrect-fixture-secret');
  await page.getByRole('button', { name: 'Open conversation' }).click();
  await expect(page.getByRole('alert')).toContainText('Viewer access is unavailable.');
  await page.getByRole('button', { name: 'Sign out' }).click();
  await page.getByLabel('Viewer ID', { exact: true }).fill(viewer.viewerId);
  await page.getByLabel('Viewer secret', { exact: true }).fill(viewer.secret);
  await page.getByRole('button', { name: 'Open conversation' }).click();
  await expect(page.getByRole('region', { name: 'Shared transcript' })).toContainText('Bring the agenda and a notebook.');
  await expect(page.getByRole('navigation', { name: 'Chat controls' })).toHaveCount(0);
  await expect(page.getByPlaceholder('Message Anima…')).toHaveCount(0);
  expect(fixture.calls.every(call => call.method === 'GET' && call.path.startsWith('/api/shared/conversations/'))).toBe(true);
  expect(fixture.calls.every(call => !call.headers['x-api-key'] && !call.headers.cookie)).toBe(true);
  expect(fixture.calls.some(call => call.headers['x-anima-viewer-secret'] === viewer.secret)).toBe(true);
  await screenshot(page, 'viewer');
  grant.revoked = true;
  await page.getByRole('button', { name: 'Refresh conversation' }).click();
  await expect(page.getByRole('alert')).toContainText('Viewer access is unavailable.');
  await expect(page.getByRole('region', { name: 'Shared transcript' })).not.toContainText('Bring the agenda and a notebook.');
  fixture.assertClean();
});

test('chat-suite owner to viewer transition clears owner credentials and bootstrap', async ({ page }) => {
  const { state } = ownerState();
  const fixture = await installConsoleFixture(page, state, { chatSuite: { ownerApiKey: OWNER_KEY, viewer: { ...viewer } } });
  await page.goto('/');
  await page.getByLabel('Owner API key').fill(OWNER_KEY);
  await page.getByRole('button', { name: 'Unlock owner console' }).click();
  await expect(page.getByRole('navigation', { name: 'Chat controls' })).toBeVisible();
  await fixture.waitForStream();
  await page.evaluate(hash => { location.hash = hash; }, `/shared/${viewer.shareId}`);
  await expect(page.getByRole('button', { name: 'Open conversation' })).toBeVisible();
  await expect(page).toHaveURL(`${ORIGIN}${sharedPath}`);
  const boundary = fixture.calls.length;
  await page.getByLabel('Viewer ID', { exact: true }).fill(viewer.viewerId);
  await page.getByLabel('Viewer secret', { exact: true }).fill(viewer.secret);
  await page.getByRole('button', { name: 'Open conversation' }).click();
  await expect(page.getByRole('region', { name: 'Shared transcript' })).toBeVisible();
  const after = fixture.calls.slice(boundary);
  expect(after).toHaveLength(2);
  expect(after.every(call => call.path.startsWith('/api/shared/conversations/') && !call.headers['x-api-key'])).toBe(true);
  await page.evaluate(() => { location.hash = ''; });
  await expect(page.getByRole('heading', { name: 'Open your Anima console' })).toBeVisible();
  expect(fixture.calls.at(-1)?.headers['x-api-key']).toBeUndefined();
  fixture.assertClean();
});
