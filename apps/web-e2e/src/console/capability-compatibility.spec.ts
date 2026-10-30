import { expect, test, type Page } from '@playwright/test';
import { installConsoleFixture } from './fixtures/api';
import { createState, session } from './fixtures/state';

async function compatibilityFixture(page: Page, supported: boolean, knownWork: boolean) {
  const state = createState();
  state.agents = state.agents.slice(0, 1);
  const chat = { ...session('chat:compatibility', 'Compatibility conversation'), mainChat: true, clearGeneration: 0, messageCount: knownWork ? 2 : 0 };
  state.sessions = [chat];
  state.messages[chat.id] = knownWork ? [
    { id: 'native-call', role: 'assistant', text: '', attachments: [], metadata: { toolCalls: [{ id: 'image-call', name: 'generate_image', args: { prompt: 'Synthetic fixture' } }] }, createdAtMs: 1 },
    { id: 'native-result', role: 'tool', text: 'Outcome is unknown. Do not replay.', attachments: [], metadata: { toolCallId: 'image-call' }, createdAtMs: 2 },
  ] : [];
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  let inventoryReads = 0;
  let ledgerReads = 0;
  await page.route('**/api/capabilities', (route) => {
    inventoryReads++;
    return route.fulfill({ json: {
      schemaVersion: 1, tools: [], extensions: [], limitations: [], persistence: { controlPlane: 'durable', memory: 'durable', executionJournal: false },
      ...(supported ? { runtime: { instanceId: 'synthetic-current-instance', observedAtMs: 10, daemonVersion: 'fixture', images: { ledgerVersion: 1, observationVersion: 1, configuration: { state: 'configured', reason: 'Synthetic local prerequisite fixture.' } }, pdfPages: { version: 1 }, limits: 'API support is not an observed provider outcome.' } } : {}),
    } });
  });
  await page.route('**/api/agents/companion/sessions/*/images', (route) => {
    ledgerReads++;
    if (knownWork) {
      fixture.expectHttpRefusal(route.request().url());
      return route.fulfill({ status: 404, json: { error: 'Unsupported route on older backend' } });
    }
    return route.fulfill({ json: { clearGeneration: 0, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: [] } });
  });
  await page.goto('/');
  await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
  await fixture.waitForStream();
  await expect.poll(() => inventoryReads).toBeGreaterThan(0);
  return { fixture, ledgerReads: () => ledgerReads };
}

test('healthy legacy backend keeps a regular chat quiet without image ledger traffic', async ({ page }) => {
  const context = await compatibilityFixture(page, false, false);
  await expect(page.getByRole('region', { name: 'Chat images' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Refresh images' })).toHaveCount(0);
  expect(context.ledgerReads()).toBe(0);
  context.fixture.assertClean();
});

test('current backend empty ledger keeps a regular chat quiet', async ({ page }) => {
  const context = await compatibilityFixture(page, true, false);
  await expect.poll(context.ledgerReads).toBeGreaterThan(0);
  await expect(page.getByRole('region', { name: 'Chat images' })).toHaveCount(0);
  context.fixture.assertClean();
});

test('legacy backend explains unsupported history for known native image work', async ({ page }) => {
  const context = await compatibilityFixture(page, false, true);
  await expect(page.getByText(/running backend does not advertise image history support/)).toBeVisible();
  await expect(page.getByRole('region', { name: 'Chat images' })).toHaveCount(0);
  expect(context.ledgerReads()).toBe(0);
  context.fixture.assertClean();
});

test('advertised backend preserves a visible failure for known image work without replay', async ({ page }) => {
  const context = await compatibilityFixture(page, true, true);
  await expect(page.getByText(/Image versions could not be loaded/)).toContainText('will not be resent');
  await page.getByRole('button', { name: 'Refresh images' }).click();
  await expect.poll(context.ledgerReads).toBeGreaterThan(1);
  expect(context.fixture.calls.filter((call) => call.method !== 'GET' && call.path.endsWith('/runs'))).toHaveLength(0);
  context.fixture.assertClean();
});
