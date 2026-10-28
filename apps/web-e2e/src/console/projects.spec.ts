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
  await page.route('**/api/projects**', async (route) => {
    const url = new URL(route.request().url());
    const common = { projectId: project.id, projectRevision: project.revision, accessEpoch: project.accessEpoch, memberGeneration: 1 };
    if (url.pathname === '/api/projects') return route.fulfill({ json: { projects: available ? [project] : [] } });
    if (!available) {
      fixture.expectHttpRefusal(route.request().url());
      return route.fulfill({ status: 404, json: { error: 'Project unavailable to this member' } });
    }
    if (url.pathname.endsWith('/chats')) return route.fulfill({ json: { ...common, chats: [], nextCursor: null } });
    if (url.pathname.endsWith('/preview')) return route.fulfill({ json: { ...common, resourceId: 'resource_selected', state: 'ready', text: 'Pinned project evidence' } });
    if (url.pathname.endsWith('/context')) return route.fulfill({ json: { ...common, estimatedTokens: 20, basis: 'current_preview', resources: [] } });
    if (url.pathname === `/api/projects/${project.id}`) return route.fulfill({ json: { project } });
    throw new Error(`Unhandled project fixture request ${url.pathname}`);
  });
  return { fixture, revoke: () => { available = false; project.accessEpoch += 1; project.revision += 1; } };
}

async function expectNoProjectOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => {
    const project = document.querySelector<HTMLElement>('.project-home');
    return {
      root: document.documentElement.scrollWidth <= window.innerWidth,
      project: project !== null && project.scrollWidth <= project.clientWidth,
    };
  })).toEqual({ root: true, project: true });
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
});
}
