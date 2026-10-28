import { expect, test, type Page } from '@playwright/test';
import type { ConnectedSearchSettings, ConnectedSource } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, createState } from './fixtures/state';

async function accountSearchFixture(page: Page) {
  const state = createState();
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  const base = `/api/agents/${AGENT}`;
  const sourcePath = `${base}/connected-search/sources/mail-fixture`;
  const filesPath = `${base}/connectors/microsoft-files`;
  const settings: ConnectedSearchSettings = {
    historyDepth: 'recent', retainedItemLimit: 100, attachmentText: 'metadataOnly', historyPaused: false,
  };
  let source: ConnectedSource = {
    connectorId: 'mail-fixture', provider: 'gmail', kind: 'mail', accountLabel: 'owner@example.test',
    generation: 'mail-generation', available: true, enabled: false, status: 'disabled',
    scope: 'Recent inbox, up to 100 messages', itemLimit: 100, itemCount: 0,
    lastAttemptAtMs: null, lastSuccessAtMs: null, error: null, settingsRevision: 1, settings,
    capabilities: { history: true, attachmentFormats: ['text', 'pdf', 'docx'], workflowEventKinds: ['mailObserved'] },
    historyCoverage: { state: 'notRequested' },
    attachmentCoverage: { indexed: 0, partial: 0, omitted: 0, failed: 0, pending: 0, error: null },
  };
  let failSettings = true;
  const writes: unknown[] = [];
  const selectionWrites: unknown[] = [];
  const microsoft = { id: 'files-fixture', mode: 'oneDrive', status: 'active', generation: 'files-generation',
    selectionRevision: 1, accountLabel: 'owner@example.test', approvedSiteUrl: null, selectedFiles: [] as { key: string; name: string }[] };
  const files = Array.from({ length: 17 }, (_, index) => ({ key: `file-${index + 1}`, name: `Document ${String(index + 1).padStart(2, '0')}.txt`, isFolder: false, mime: 'text/plain' }));

  // Registered after the common fixture, so these task-specific responses win.
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const method = request.method();
    if (method === 'GET' && path === `${base}/connected-search/sources`)
      return route.fulfill({ json: { sources: [source], pollIntervalMs: 120_000 } });
    if (method === 'PUT' && path === sourcePath) {
      const body = request.postDataJSON() as { enabled: boolean; expectedGeneration: string; expectedSettingsRevision?: number; settings?: ConnectedSearchSettings };
      writes.push(body);
      expect(body.expectedGeneration).toBe('mail-generation');
      if (body.settings && failSettings) {
        failSettings = false;
        fixture.expectHttpRefusal(request.url());
        return route.fulfill({ status: 503, json: { error: 'Search settings could not be saved' } });
      }
      if (body.settings) {
        expect(body.expectedSettingsRevision).toBe(1);
        source = { ...source, settings: body.settings, settingsRevision: 2, status: 'syncing',
          historyCoverage: { state: 'pending' },
          attachmentCoverage: { indexed: 0, partial: 0, omitted: 0, failed: 0, pending: 2, error: null } };
      } else source = { ...source, enabled: body.enabled, status: body.enabled ? 'syncing' : 'disabled' };
      return route.fulfill({ json: { source } });
    }
    if (method === 'GET' && path === `${base}/account-workflow-rules`) return route.fulfill({ json: { rules: [] } });
    if (method === 'GET' && path === `${base}/workflow-templates`) return route.fulfill({ json: { templates: [] } });
    if (method === 'GET' && path === base) return route.fulfill({ json: { agent: state.agents[0] } });
    if (method === 'GET' && path.startsWith('/api/connectors/oauth-apps/')) {
      const provider = path.endsWith('/microsoft') ? 'microsoft' : 'google';
      return route.fulfill({ json: { provider, configured: false, source: null, clientIdHint: null, redirectUris: [], tenant: null } });
    }
    if (method === 'GET' && [`${base}/connectors/gdrive`, `${base}/connectors/gcalendar`, `${base}/connectors/mail/gmail`, `${base}/connectors/mail/outlook`, `${base}/connectors/mail/email`].includes(path))
      return route.fulfill({ json: { configured: false, connector: null } });
    if (method === 'GET' && path === filesPath) return route.fulfill({ json: { configured: true, connectors: [microsoft] } });
    if (method === 'GET' && path === `${filesPath}/${microsoft.id}/files`) return route.fulfill({ json: { files, nextCursor: null } });
    if (method === 'PUT' && path === `${filesPath}/${microsoft.id}/selection`) {
      const body = request.postDataJSON() as { expectedGeneration: string; expectedSelectionRevision: number; fileKeys: string[] };
      selectionWrites.push(body);
      expect(body.expectedGeneration).toBe(microsoft.generation);
      expect(body.expectedSelectionRevision).toBe(1);
      expect(body.fileKeys).toHaveLength(16);
      microsoft.selectedFiles = files.filter((file) => body.fileKeys.includes(file.key)).map(({ key, name }) => ({ key, name }));
      microsoft.selectionRevision = 2;
      return route.fulfill({ json: { connector: microsoft } });
    }
    return route.fallback();
  });
  return { fixture, writes, selectionWrites };
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`account search requires opt-in, reports incomplete work, and bounds file selection at ${viewport.width}px`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const { fixture, writes, selectionWrites } = await accountSearchFixture(page);
    await page.goto('/#/connectors');
    await fixture.waitForStream();
    const accountPanel = page.getByRole('region', { name: 'Connected account search and workflows' });
    await accountPanel.getByRole('button', { name: 'Search and account workflows' }).click();
    const sourceToggle = accountPanel.getByRole('checkbox', { name: 'Use Gmail owner@example.test for search and workflows' });
    await expect(sourceToggle).not.toBeChecked();
    await expect(accountPanel.getByLabel('Search back')).toHaveCount(0);
    await sourceToggle.click();
    await expect(sourceToggle).toBeChecked();
    expect(writes[0]).toEqual({ enabled: true, expectedGeneration: 'mail-generation' });

    const depth = accountPanel.getByLabel('Search back');
    const retained = accountPanel.getByLabel('Keep locally');
    const attachment = accountPanel.getByRole('checkbox', { name: 'Include attachment text' });
    await expect(depth).toHaveValue('recent');
    await expect(attachment).not.toBeChecked();
    await expect(retained.locator('option')).toHaveText(['Up to 100 messages', 'Up to 500 messages', 'Up to 2,000 messages']);
    await depth.selectOption('days365');
    await retained.selectOption('2000');
    await attachment.check();
    await accountPanel.getByRole('button', { name: 'Save search settings' }).click();
    await expect(accountPanel.getByRole('alert')).toContainText('Search settings could not be saved');
    await expect(accountPanel.getByText('Search settings saved.')).toHaveCount(0);
    await expect(accountPanel.getByText('Older content: imported')).toHaveCount(0);
    expect(writes[1]).toEqual({ enabled: true, expectedGeneration: 'mail-generation', expectedSettingsRevision: 1,
      settings: { historyDepth: 'days365', retainedItemLimit: 2000, attachmentText: 'supportedText', historyPaused: false } });
    await accountPanel.getByRole('button', { name: 'Save search settings' }).click();
    await expect(accountPanel.getByText('Search settings saved.')).toBeVisible();
    await expect(accountPanel.getByText('Older content: waiting to import')).toBeVisible();
    await expect(accountPanel.getByText('Attachment text: 0 indexed · 2 waiting')).toBeVisible();

    const microsoft = page.getByRole('region', { name: 'Microsoft files' });
    await microsoft.getByRole('button', { name: 'Choose OneDrive files' }).click();
    for (let number = 1; number <= 17; number++)
      await microsoft.getByRole('checkbox', { name: `Document ${String(number).padStart(2, '0')}.txt` }).check();
    const saveFiles = microsoft.getByRole('button', { name: 'Save selected files' });
    await expect(saveFiles).toBeDisabled();
    await microsoft.getByRole('checkbox', { name: 'Document 17.txt' }).uncheck();
    await expect(saveFiles).toBeEnabled();
    await saveFiles.click();
    await expect(microsoft.getByText(/Selected: Document 01\.txt/)).toBeVisible();
    expect(selectionWrites).toHaveLength(1);

    const widths = await page.evaluate(() => {
      const connections = document.querySelector<HTMLElement>('.connections-page');
      if (!connections) throw new Error('Connections page is missing');
      return { root: document.documentElement.scrollWidth, viewport: document.documentElement.clientWidth,
        content: connections.scrollWidth, contentViewport: connections.clientWidth };
    });
    expect(widths.root, `page should not overflow horizontally at ${viewport.width}px`).toBeLessThanOrEqual(widths.viewport + 1);
    expect(widths.content, `connections content should not overflow horizontally at ${viewport.width}px`).toBeLessThanOrEqual(widths.contentViewport + 1);
    fixture.assertClean();
  });
}
