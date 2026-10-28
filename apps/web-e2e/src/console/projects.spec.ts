import { test, expect, type Page } from '@playwright/test';
import type { Project } from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, agent, createState } from './fixtures/state';

async function projectsFixture(page: Page) {
  const state = createState();
  const bob = agent('member-b'); bob.state.name = 'Bob'; bob.state.config.name = 'Bob'; bob.state.createdAtMs = 3;
  state.agents.push(bob);
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  const project: Project = { id: 'project_shared', title: 'Shared research', status: 'active', revision: 1, accessEpoch: 1,
    members: [{ agentId: AGENT, generation: 1, joinedAtMs: 1 }, { agentId: 'member-b', generation: 1, joinedAtMs: 1 }],
    instructions: 'Use only selected evidence.', createdAtMs: 1, updatedAtMs: 1, chatCount: 0, resourceCount: 1,
    resources: [{ id: 'resource_selected', target: { kind: 'workspace_file', path: 'brief.md' }, role: 'reference', label: 'Selected brief',
      state: 'ready', useInContext: true, createdAtMs: 1 }] };
  let available = true;
  let indexed = false;
  const rootPath = 'D:\\Owner research\\Regional evidence and a long folder path for overflow checks';
  const grantCalls: unknown[] = [];
  const removeCalls: unknown[] = [];
  const workspaceWrites: string[] = [];
  const title = 'Research register and operational thresholds.pdf';
  const locator = 'Page 12 · paragraph 3';
  const passage = `The operating reserve threshold is 18 months. This is a bounded verified passage. ${'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.repeat(8)}`;
  const hasFolder = () => project.resources.some((row) => row.id === 'resource_knowledge');
  const addFolder = () => {
    project.resources.push({ id: 'resource_knowledge', role: 'reference', label: 'Owner research', state: 'ready',
      target: { kind: 'knowledge_folder', rootPath, rootIdentity: 'server-issued-root-identity' }, useInContext: true, createdAtMs: 2 });
    project.resourceCount = project.resources.length; project.revision += 1; project.accessEpoch += 1;
  };
  const sourceStatus = () => hasFolder() ? [{ resourceId: 'resource_knowledge', status: indexed ? 'partial' : 'indexing',
    documentCount: indexed ? 3 : 0, indexedAtMs: indexed ? 1_730_000_000_000 : null, generation: indexed ? 1 : 0,
    omittedCount: indexed ? 2 : 0, ...(indexed ? { error: 'Two files were omitted from this bounded index.' } : {}) }] : [];
  const hit = { citationId: 'citation_knowledge', resourceId: 'resource_knowledge', documentId: 'document_register',
    title, locator, excerpt: 'The operating reserve threshold is 18 months.' };
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/workspace') && request.method() !== 'GET') workspaceWrites.push(`${request.method()} ${url.pathname}${url.search}`);
  });
  await page.route('**/api/workspace/pick-folder*', async (route) => {
    const request = route.request(); const url = new URL(request.url());
    expect(request.method()).toBe('POST'); expect(url.searchParams.get('purpose')).toBe('project_knowledge');
    return route.fulfill({ json: { rootPath } });
  });
  await page.route('**/api/projects**', async (route) => {
    const request = route.request(); const url = new URL(request.url());
    const common = { projectId: project.id, projectRevision: project.revision, accessEpoch: project.accessEpoch, memberGeneration: 1 };
    if (url.pathname === '/api/projects') return route.fulfill({ json: { projects: available ? [project] : [] } });
    if (!available) {
      fixture.expectHttpRefusal(route.request().url());
      return route.fulfill({ status: 404, json: { error: 'Project unavailable to this member' } });
    }
    if (url.pathname.endsWith('/chats')) return route.fulfill({ json: { ...common, chats: [], nextCursor: null } });
    if (url.pathname.endsWith('/preview')) return route.fulfill({ json: { ...common, resourceId: 'resource_selected', state: 'ready', text: 'Pinned project evidence' } });
    if (url.pathname.endsWith('/context')) return route.fulfill({ json: { ...common, estimatedTokens: 20, basis: 'current_preview', resources: [] } });
    if (url.pathname.endsWith('/knowledge/sources') && request.method() === 'POST') {
      const body = request.postDataJSON() as Record<string, unknown>; grantCalls.push(body);
      expect(body).toEqual({ expectedRevision: project.revision, asAgentId: AGENT, rootPath, label: 'Owner research', requestKey: expect.any(String) });
      expect(hasFolder()).toBe(false); addFolder();
      return route.fulfill({ json: { project } });
    }
    if (url.pathname.endsWith('/knowledge/status')) {
      expect(url.searchParams.get('asAgentId')).toBe(AGENT);
      return route.fulfill({ json: { ...common, sources: sourceStatus() } });
    }
    if (url.pathname.endsWith('/knowledge/search')) {
      expect(url.searchParams.get('asAgentId')).toBe(AGENT); expect(url.searchParams.get('query')).toBe('operating reserve');
      expect(url.searchParams.get('limit')).toBe('8'); expect(hasFolder()).toBe(true);
      return route.fulfill({ json: { ...common, sources: sourceStatus(), hits: [hit] } });
    }
    if (url.pathname.endsWith('/knowledge/citations/citation_knowledge')) {
      expect(url.searchParams.get('asAgentId')).toBe(AGENT);
      if (!hasFolder()) {
        fixture.expectHttpRefusal(request.url());
        return route.fulfill({ status: 403, json: { error: 'This knowledge source was removed' } });
      }
      return route.fulfill({ json: { ...common, ...hit, text: passage } });
    }
    if (url.pathname.endsWith('/resources/resource_knowledge') && request.method() === 'DELETE') {
      const body = request.postDataJSON() as Record<string, unknown>; removeCalls.push(body);
      expect(body).toEqual({ expectedRevision: project.revision, asAgentId: AGENT });
      project.resources = project.resources.filter((row) => row.id !== 'resource_knowledge');
      project.resourceCount = project.resources.length; project.revision += 1; project.accessEpoch += 1;
      return route.fulfill({ json: { project } });
    }
    if (url.pathname === `/api/projects/${project.id}`) return route.fulfill({ json: { project } });
    throw new Error(`Unhandled project fixture request ${url.pathname}`);
  });
  return { fixture, rootPath, grantCalls, removeCalls, workspaceWrites, title, locator, passage,
    addFolder, finishIndex: () => { indexed = true; },
    revoke: () => { available = false; project.accessEpoch += 1; project.revision += 1; return { revision: project.revision, accessEpoch: project.accessEpoch }; } };
}

async function expectNoProjectOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => {
    const project = document.querySelector<HTMLElement>('.project-home');
    const citation = document.querySelector<HTMLElement>('.project-citation-dialog');
    return {
      root: document.documentElement.scrollWidth <= window.innerWidth,
      project: project !== null && project.scrollWidth <= project.clientWidth,
      citation: citation === null || citation.scrollWidth <= citation.clientWidth,
    };
  })).toEqual({ root: true, project: true, citation: true });
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
test.describe(`project layout at ${viewport.width}px`, () => {
test.use({ viewport });

test('project navigation preserves the main-chat draft and one shared identity across reload', async ({ page }) => {
  const { fixture } = await projectsFixture(page);
  await page.goto('/');
  const composer = page.getByPlaceholder('Message Anima…');
  await expect(composer).toBeVisible(); await fixture.waitForStream();
  await composer.fill('Keep this private draft');
  await page.getByRole('button', { name: 'Projects', exact: true }).click();
  await expectNoProjectOverflow(page);
  await page.getByRole('button', { name: /Shared research/ }).click();
  await expect(page).toHaveURL(/#\/p\/project_shared$/);
  await expect(page.getByRole('heading', { name: 'Shared research', exact: true })).toBeVisible();
  await expect(page.getByText(/Shared with Anima, Bob/)).toBeVisible();
  await expectNoProjectOverflow(page);
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Shared research', exact: true })).toBeVisible();
  await expectNoProjectOverflow(page);
  // Return to the retained native chat. Its draft was never moved into project context.
  await page.evaluate(() => { location.hash = '#/'; });
  await expect(composer).toHaveValue('Keep this private draft');
  fixture.assertClean();
});

test('member invalidation removes a cached shared preview before a refetch can publish it', async ({ page }) => {
  const { fixture, revoke } = await projectsFixture(page);
  await page.goto('/#/p/project_shared'); await fixture.waitForStream();
  await page.getByRole('tab', { name: 'Sources', exact: true }).click();
  await page.getByRole('button', { name: 'Preview', exact: true }).click();
  await expect(page.getByText('Pinned project evidence', { exact: true })).toBeVisible();
  await expectNoProjectOverflow(page);
  revoke();
  await fixture.emit({ type: 'project.updated', agentId: AGENT, seq: 20, at: 20, projectId: 'project_shared', revision: 2,
    accessEpoch: 2, deleted: false, membershipChanged: true });
  await expect(page.getByText('Pinned project evidence', { exact: true })).toHaveCount(0);
  await expect(page.getByText('This project is unavailable to the selected agent.')).toBeVisible();
  await expectNoProjectOverflow(page);
  fixture.assertClean();
});

test('read-only folder grant, indexed search and citation remain usable before source removal', async ({ page }, testInfo) => {
  const { fixture, rootPath, grantCalls, removeCalls, workspaceWrites, title, locator, passage, finishIndex } = await projectsFixture(page);
  await page.goto('/#/p/project_shared'); await fixture.waitForStream();
  await page.getByRole('tab', { name: 'Sources', exact: true }).click();
  const knowledge = page.getByRole('region', { name: 'Project knowledge', exact: true });
  await knowledge.getByRole('button', { name: 'Add folder', exact: true }).click();
  const grant = page.getByRole('form', { name: 'Add knowledge folder' });
  await grant.getByRole('button', { name: 'Choose folder', exact: true }).click();
  await expect(grant.getByText(rootPath, { exact: true })).toBeVisible();
  const submit = grant.getByRole('button', { name: 'Grant read-only access and add folder', exact: true });
  await expect(submit).toBeDisabled(); expect(grantCalls).toHaveLength(0);
  await expect(grant.getByText(/This folder and its supported documents/)).toBeVisible();
  await grant.getByLabel('Folder label', { exact: true }).fill('Owner research');
  await grant.getByRole('checkbox', { name: 'I grant this project read-only access to this folder.', exact: true }).check();
  await expectNoProjectOverflow(page);
  await submit.click();
  await expect.poll(() => grantCalls.length).toBe(1);
  await expect(knowledge.getByText('indexing', { exact: true })).toBeVisible();
  finishIndex();
  await expect(knowledge.getByText('3 documents indexed · 2 omitted', { exact: true })).toBeVisible();
  await expect(knowledge.getByText('Two files were omitted from this bounded index.', { exact: true })).toBeVisible();
  await expect(knowledge.getByRole('button', { name: 'Retry indexing Owner research', exact: true })).toBeEnabled();
  await expect(knowledge.getByRole('checkbox', { name: 'Use in new project turns', exact: true })).toBeChecked();
  await knowledge.getByRole('searchbox', { name: 'Search project knowledge', exact: true }).fill('operating reserve');
  await knowledge.getByRole('button', { name: 'Search knowledge', exact: true }).click();
  const result = knowledge.getByRole('button', { name: `${title} · ${locator}`, exact: true });
  await result.click();
  const citation = page.getByRole('dialog', { name: title, exact: true });
  await expect(citation).toContainText(passage);
  await expect(citation.getByRole('button', { name: 'Close citation', exact: true })).toBeFocused();
  await expectNoProjectOverflow(page);
  const screenshot = testInfo.outputPath(`knowledge-citation-${viewport.width}.png`);
  await page.screenshot({ path: screenshot });
  testInfo.attachments.push({ name: `Synthetic project knowledge at ${viewport.width}px`, path: screenshot, contentType: 'image/png' });
  await page.keyboard.press('Escape');
  await expect(citation).toHaveCount(0); await expect(result).toBeFocused();
  await knowledge.getByRole('button', { name: 'Unlink Owner research', exact: true }).click();
  await expect.poll(() => removeCalls.length).toBe(1);
  await expect(knowledge.getByText('Add a folder to search its documents here.', { exact: true })).toBeVisible();
  await expect(page.getByText(passage, { exact: true })).toHaveCount(0);
  await expect(knowledge.getByRole('button', { name: `${title} · ${locator}`, exact: true })).toHaveCount(0);
  await expect(knowledge.getByRole('button', { name: 'Search knowledge', exact: true })).toBeDisabled();
  expect(workspaceWrites).toEqual(['POST /api/workspace/pick-folder?purpose=project_knowledge']);
  await expectNoProjectOverflow(page); fixture.assertClean();
});

test('member revocation clears indexed search results and an open knowledge citation', async ({ page }) => {
  const { fixture, addFolder, finishIndex, title, locator, passage, revoke } = await projectsFixture(page);
  addFolder(); finishIndex();
  await page.goto('/#/p/project_shared'); await fixture.waitForStream();
  await page.getByRole('tab', { name: 'Sources', exact: true }).click();
  const knowledge = page.getByRole('region', { name: 'Project knowledge', exact: true });
  await knowledge.getByRole('searchbox', { name: 'Search project knowledge', exact: true }).fill('operating reserve');
  await knowledge.getByRole('button', { name: 'Search knowledge', exact: true }).click();
  await knowledge.getByRole('button', { name: `${title} · ${locator}`, exact: true }).click();
  await expect(page.getByRole('dialog', { name: title, exact: true })).toContainText(passage);
  const binding = revoke();
  await fixture.emit({ type: 'project.updated', agentId: AGENT, seq: 20, at: 20, projectId: 'project_shared', ...binding,
    deleted: false, membershipChanged: true });
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByText(passage, { exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: `${title} · ${locator}`, exact: true })).toHaveCount(0);
  await expect(page.getByText('This project is unavailable to the selected agent.')).toBeVisible();
  await expectNoProjectOverflow(page); fixture.assertClean();
});
});
}
