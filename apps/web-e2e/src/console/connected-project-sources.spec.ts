import { expect, test, type Page } from '@playwright/test';
import type { Project, ProjectConnectedSourceGrant } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, createState } from './fixtures/state';

async function connectedProject(page: Page) {
  const fixture = await installConsoleFixture(page, createState(), { chatSuite: {} });
  const title = 'Launch brief.docx';
  const grant: ProjectConnectedSourceGrant = {
    ownerAgentId: AGENT, connectorId: 'drive-selected', provider: 'gdrive', remoteId: 'file-selected',
    citationId: 'account-citation', connectionGeneration: 'native-generation', searchGeneration: 'search-generation',
    grantEpoch: 'enrollment-epoch', selectionRevision: 1, streamEpoch: null, sourceKind: 'file',
  };
  const project: Project = {
    id: 'project_connected', title: 'Connected research', status: 'active', revision: 1, accessEpoch: 1,
    members: [{ agentId: AGENT, generation: 1, joinedAtMs: 1 }], instructions: '', resources: [],
    createdAtMs: 1, updatedAtMs: 1, chatCount: 0, resourceCount: 0,
  };
  const shares: unknown[] = [];
  let removedAccess = false;
  let generation = 1;
  const hit = { citationId: 'project-citation', resourceId: 'connected-resource', documentId: 'launch-document',
    title, locator: 'Paragraph 4', excerpt: 'Keep an operating reserve of eighteen months.' };
  await page.route('**/api/projects**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const common = { projectId: project.id, projectRevision: project.revision, accessEpoch: project.accessEpoch, memberGeneration: 1 };
    const status = () => ({ ...common, sources: project.resources.length ? [{ resourceId: 'connected-resource',
      status: removedAccess ? 'unavailable' : 'ready', documentCount: removedAccess ? 0 : 1,
      indexedAtMs: 1_730_000_000_000, generation, omittedCount: 0,
      ...(removedAccess ? { error: 'The original account access was removed.' } : {}) }] : [] });
    if (url.pathname === '/api/projects') return route.fulfill({ json: { projects: [project] } });
    if (url.pathname.endsWith('/chats')) return route.fulfill({ json: { ...common, chats: [], nextCursor: null } });
    if (url.pathname.endsWith('/context')) return route.fulfill({ json: { ...common, basis: 'current_preview', estimatedTokens: 0, resources: [] } });
    if (url.pathname.endsWith('/knowledge/connected-sources')) {
      if (request.method() === 'GET') return route.fulfill({ json: { choices: [{ ownerAgentId: AGENT,
        connectorId: grant.connectorId, citationId: grant.citationId, remoteId: grant.remoteId, provider: 'gdrive',
        title, accountLabel: 'Research account', sourceKind: 'file', coverage: 'One selected file; current account access is required.' }], omittedCount: 0 } });
      expect(request.method()).toBe('POST');
      const body = request.postDataJSON() as Record<string, unknown>;
      expect(body).toEqual({ expectedRevision: 1, requestKey: expect.any(String), asAgentId: AGENT,
        ownerAgentId: AGENT, connectorId: grant.connectorId, citationId: grant.citationId, shareWithProject: true });
      shares.push(body);
      project.resources.push({ id: 'connected-resource', target: { kind: 'connected_source', grant },
        label: title, state: 'ready', role: 'reference', useInContext: true, createdAtMs: 2 });
      project.resourceCount = 1; project.revision += 1; project.accessEpoch += 1;
      return route.fulfill({ json: { project } });
    }
    if (url.pathname.endsWith('/knowledge/status')) return route.fulfill({ json: status() });
    if (url.pathname.endsWith('/knowledge/search')) {
      expect(url.searchParams.get('query')).toBe('operating reserve');
      return route.fulfill({ json: { ...status(), hits: removedAccess ? [] : [hit] } });
    }
    if (url.pathname.endsWith('/knowledge/citations/project-citation')) {
      expect(removedAccess).toBe(false);
      return route.fulfill({ json: { ...common, ...hit, text: hit.excerpt, origin: { ownerAgentId: AGENT,
        ownerName: 'Anima', accountLabel: 'Research account', connectorId: grant.connectorId, provider: 'gdrive',
        remoteId: grant.remoteId, revisionSha256: 'a'.repeat(64), verifiedAtMs: 1_730_000_000_000,
        coverage: 'One selected file; current account access was checked.' } } });
    }
    if (url.pathname.endsWith('/knowledge/refresh')) {
      expect(request.method()).toBe('POST'); removedAccess = true; generation += 1;
      return route.fulfill({ json: status() });
    }
    if (url.pathname.endsWith('/resources/connected-resource') && request.method() === 'DELETE') {
      project.resources = []; project.resourceCount = 0; project.revision += 1; project.accessEpoch += 1;
      return route.fulfill({ json: { project } });
    }
    if (url.pathname === `/api/projects/${project.id}`) return route.fulfill({ json: { project } });
    throw new Error(`Unhandled connected project fixture: ${request.method()} ${url.pathname}`);
  });
  return { fixture, title, shares };
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`selected account content needs explicit sharing and loses stale access at ${viewport.width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    const { fixture, title, shares } = await connectedProject(page);
    await page.goto('/#/p/project_connected'); await fixture.waitForStream();
    await page.getByRole('tab', { name: 'Sources', exact: true }).click();
    const knowledge = page.getByRole('region', { name: 'Project knowledge', exact: true });
    await knowledge.getByRole('button', { name: 'Add connected source', exact: true }).click();
    const picker = page.getByRole('region', { name: 'Share a connected source', exact: true });
    await expect(picker.getByRole('button', { name: 'Share selected source', exact: true })).toBeDisabled();
    await picker.getByRole('radio').check();
    await expect(picker).toContainText('Research account');
    expect(shares).toHaveLength(0);
    await expect(picker.getByRole('button', { name: 'Share selected source', exact: true })).toBeDisabled();
    await picker.getByRole('checkbox').check();
    await picker.getByRole('button', { name: 'Share selected source', exact: true }).click();
    await expect.poll(() => shares.length).toBe(1);
    await expect(picker).toHaveCount(0);
    await expect(knowledge.getByText('1 document indexed · 0 omitted', { exact: true })).toBeVisible();
    await knowledge.getByRole('searchbox', { name: 'Search project knowledge', exact: true }).fill('operating reserve');
    await knowledge.getByRole('button', { name: 'Search knowledge', exact: true }).click();
    await knowledge.getByRole('button', { name: `${title} · Paragraph 4`, exact: true }).click();
    const citation = page.getByRole('dialog', { name: title, exact: true });
    await expect(citation).toContainText('Keep an operating reserve of eighteen months.');
    await expect(citation.getByLabel('Verified connected source')).toContainText('Google Drive');
    await expect(citation.getByLabel('Verified connected source')).toContainText('Research account');
    await page.screenshot({ path: testInfo.outputPath(`connected-source-${viewport.width}.png`) });
    await citation.getByRole('button', { name: 'Close citation', exact: true }).click();
    await knowledge.getByRole('button', { name: `Refresh index ${title}`, exact: true }).click();
    await expect(knowledge.getByText('The original account access was removed.', { exact: true })).toBeVisible();
    await expect(knowledge.getByRole('button', { name: `${title} · Paragraph 4`, exact: true })).toHaveCount(0);
    await knowledge.getByRole('button', { name: `Unlink ${title}`, exact: true }).click();
    await expect(knowledge.getByText(title, { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    expect(fixture.calls.filter((call) => call.method !== 'GET' && call.path.startsWith('/api/workspace'))).toEqual([]);
    fixture.assertClean();
  });
}
