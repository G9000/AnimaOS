import { expect, test, type Page, type Route } from '@playwright/test';
import type {
  Project,
  ProjectKnowledgeCitation,
  ProjectKnowledgeHit,
} from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, createState, session } from './fixtures/state';

const OWNER_KEY = 'synthetic-scanned-page-owner-key';
const ATTACHMENT = 'att_scanned_fixture';
const PAGE_WIDTH = 1600;

async function syntheticPagePng(page: Page, number: number) {
  const encoded = await page.evaluate(
    ({ pageNumber, width }) => {
      const canvas = document.createElement('canvas');
      canvas.width = width;
      canvas.height = 1066;
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Synthetic page canvas is unavailable');
      context.fillStyle = '#fff';
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.fillStyle = '#172033';
      context.font = '64px sans-serif';
      context.fillText(`Synthetic original page ${pageNumber}`, 80, 100);
      context.font = '40px sans-serif';
      context.fillText(
        'Offline fixture. No document or account was read.',
        80,
        180,
      );
      for (let row = 0; row < 4; row++) {
        context.fillText(`Table row ${row + 1}`, 100, 350 + row * 100);
        context.fillText(
          String(100 + row * 25 + pageNumber),
          750,
          350 + row * 100,
        );
        context.fillStyle = '#3b6d99';
        context.fillRect(
          1100 + row * 220,
          1000 - row * 100,
          140,
          200 + row * 100,
        );
        context.fillStyle = '#172033';
      }
      return canvas.toDataURL('image/png').split(',')[1];
    },
    { pageNumber: number, width: PAGE_WIDTH },
  );
  return Buffer.from(encoded, 'base64');
}

async function fulfillPage(route: Route, png: Buffer) {
  expect(route.request().method()).toBe('GET');
  expect(route.request().headers()['x-api-key']).toBe(OWNER_KEY);
  expect(route.request().headers().accept).toBe('image/png');
  expect(route.request().isNavigationRequest()).toBe(false);
  await route.fulfill({
    status: 200,
    headers: {
      'content-type': 'image/png',
      'content-length': String(png.length),
      'cache-control': 'no-store',
      'x-content-type-options': 'nosniff',
    },
    body: png,
  });
}

async function unlockOwner(page: Page) {
  await expect(
    page.getByRole('heading', { name: 'Open your Anima console' }),
  ).toBeVisible();
  await page.getByLabel('Owner API key').fill(OWNER_KEY);
  await page.getByRole('button', { name: 'Unlock owner console' }).click();
}

async function noHorizontalOverflow(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => ({
        document: document.documentElement.scrollWidth <= window.innerWidth,
        previews: Array.from(
          document.querySelectorAll<HTMLElement>(
            '.pdf-page-preview, .project-citation-dialog, .studio-message-attachment',
          ),
        ).every((element) => element.scrollWidth <= element.clientWidth),
      })),
    )
    .toEqual({ document: true, previews: true });
}

async function attachmentConsole(page: Page) {
  const state = createState();
  state.agents = state.agents.slice(0, 1);
  const chat = {
    ...session('chat:scanned-pages', 'Synthetic scanned pages'),
    mainChat: true,
    clearGeneration: 0,
    messageCount: 2,
  };
  state.sessions = [chat];
  const attachment = {
    id: ATTACHMENT,
    name: 'Synthetic scanned financial statement.pdf',
    mime: 'application/pdf',
    kind: 'document' as const,
    size: 512,
    path: 'attachments/synthetic-scanned.pdf',
    createdAtMs: 1,
  };
  state.attachments = [attachment];
  state.messages[chat.id] = [
    {
      id: 'scanned-prompt',
      role: 'user',
      text: 'Inspect these synthetic original pages.',
      createdAtMs: 1,
      metadata: {},
      attachments: [
        {
          ...attachment,
          type: 'file',
          extraction: {
            version: 2,
            format: 'pdf',
            status: 'no_text',
            pageCount: 3,
            textPageCount: 0,
            includedPageCount: 0,
            omissions: ['text_layer_only'],
          },
        },
      ],
    },
    {
      id: 'scanned-answer',
      role: 'assistant',
      createdAtMs: 2,
      metadata: {},
      attachments: [],
      text: `[Original chart page 3](/api/agents/${AGENT}/attachments/${ATTACHMENT}/pages/3)\n\n[Foreign page](https://elsewhere.invalid/api/agents/${AGENT}/attachments/${ATTACHMENT}/pages/3)`,
    },
  ];
  const fixture = await installConsoleFixture(page, state, {
    chatSuite: { ownerApiKey: OWNER_KEY },
  });
  const images = new Map<number, Buffer>();
  for (const number of [2, 3])
    images.set(number, await syntheticPagePng(page, number));
  const reads: number[] = [];
  await page.route('**/api/agents/*/attachments/*/pages/*', async (route) => {
    const url = new URL(route.request().url());
    const match = new RegExp(
      `^/api/agents/${AGENT}/attachments/${ATTACHMENT}/pages/([23])$`,
    ).exec(url.pathname);
    expect(match).not.toBeNull();
    expect(url.search).toBe('');
    const number = Number(match![1]);
    reads.push(number);
    await fulfillPage(route, images.get(number)!);
  });
  await page.goto('/');
  await unlockOwner(page);
  await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
  await fixture.waitForStream();
  return { fixture, reads };
}

async function projectConsole(page: Page) {
  const fixture = await installConsoleFixture(page, createState(), {
    chatSuite: { ownerApiKey: OWNER_KEY },
  });
  const project: Project = {
    id: 'project_scanned',
    title: 'Synthetic scan evidence',
    status: 'active',
    revision: 1,
    accessEpoch: 1,
    members: [{ agentId: AGENT, generation: 1, joinedAtMs: 1 }],
    instructions: '',
    createdAtMs: 1,
    updatedAtMs: 1,
    chatCount: 0,
    resourceCount: 1,
    resources: [
      {
        id: 'source_scanned',
        role: 'reference',
        label: 'Synthetic scans',
        state: 'ready',
        useInContext: true,
        createdAtMs: 1,
        target: {
          kind: 'knowledge_folder',
          rootPath: 'D:\\Synthetic scans',
          rootIdentity: 'synthetic-root-identity',
        },
      },
    ],
  };
  const binding = { projectRevision: 1, accessEpoch: 1, memberGeneration: 1 };
  const sources = [
    {
      resourceId: 'source_scanned',
      status: 'ready',
      documentCount: 2,
      indexedAtMs: 1_730_000_000_000,
      generation: 1,
      omittedCount: 0,
    },
  ];
  const hits: ProjectKnowledgeHit[] = [
    {
      citationId: 'cite_scanned_ocr',
      resourceId: 'source_scanned',
      documentId: 'doc_scanned',
      title: 'Synthetic scanned prose.pdf',
      locator: 'pdf#page=1;byte=0',
      excerpt: 'Recognized synthetic operating reserve is eighteen months.',
      recognition: {
        kind: 'ocr',
        language: 'en',
        pipeline: 'synthetic-ocr-v1',
      },
    },
    {
      citationId: 'cite_scanned_image',
      resourceId: 'source_scanned',
      documentId: 'doc_chart',
      title: 'Synthetic chart.pdf',
      locator: 'pdf#page=2',
      excerpt: 'Original chart page evidence',
      recognition: { kind: 'page_image', pipeline: 'synthetic-render-v1' },
    },
  ];
  const citations = new Map<string, ProjectKnowledgeCitation>(
    hits.map((hit) => [
      hit.citationId,
      {
        ...binding,
        ...hit,
        text: hit.recognition?.kind === 'ocr' ? hit.excerpt : '',
      },
    ]),
  );
  const image = await syntheticPagePng(page, 2);
  const pageReads: string[] = [];
  await page.route('**/api/projects**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    expect(request.method()).toBe('GET');
    expect(request.headers()['x-api-key']).toBe(OWNER_KEY);
    if (url.pathname === '/api/projects')
      return route.fulfill({ json: { projects: [project] } });
    if (url.pathname === `/api/projects/${project.id}`)
      return route.fulfill({ json: { project } });
    if (url.pathname.endsWith('/chats'))
      return route.fulfill({
        json: {
          ...binding,
          projectId: project.id,
          chats: [],
          nextCursor: null,
        },
      });
    if (url.pathname.endsWith('/context'))
      return route.fulfill({
        json: {
          ...binding,
          projectId: project.id,
          basis: 'current_preview',
          estimatedTokens: 0,
          resources: [],
        },
      });
    expect(url.searchParams.get('asAgentId')).toBe(AGENT);
    if (url.pathname.endsWith('/knowledge/status'))
      return route.fulfill({ json: { ...binding, sources } });
    if (url.pathname.endsWith('/knowledge/search')) {
      expect(url.searchParams.get('query')).toBe('synthetic scan');
      return route.fulfill({ json: { ...binding, sources, hits } });
    }
    const match = new RegExp(
      `^/api/projects/${project.id}/knowledge/citations/(cite_scanned_ocr|cite_scanned_image)(/page)?$`,
    ).exec(url.pathname);
    if (!match)
      throw new Error(
        `Unexpected scanned citation fixture request: ${url.pathname}`,
      );
    const citation = citations.get(match[1])!;
    if (match[2]) {
      expect(citation.citationId).toBe('cite_scanned_image');
      pageReads.push(citation.citationId);
      return fulfillPage(route, image);
    }
    return route.fulfill({ json: citation });
  });
  await page.goto(`/#/p/${project.id}`);
  await unlockOwner(page);
  await fixture.waitForStream();
  await page.getByRole('tab', { name: 'Sources', exact: true }).click();
  const knowledge = page.getByRole('region', {
    name: 'Project knowledge',
    exact: true,
  });
  await knowledge
    .getByRole('searchbox', { name: 'Search project knowledge', exact: true })
    .fill('synthetic scan');
  await knowledge
    .getByRole('button', { name: 'Search knowledge', exact: true })
    .click();
  return { fixture, knowledge, pageReads };
}

for (const [size, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
] as const) {
  test(`scanned attachment pages and answer citations use explicit authenticated previews on ${size}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    const { fixture, reads } = await attachmentConsole(page);
    const attachment = page.getByRole('list', { name: 'Attached files' });
    await expect(attachment).toContainText('Only the PDF text layer was read');
    expect(reads).toEqual([]);
    await attachment
      .getByRole('combobox', { name: 'PDF page' })
      .selectOption('2');
    expect(reads).toEqual([]);
    await attachment
      .getByRole('button', { name: 'Preview PDF page 2', exact: true })
      .click();
    const image = attachment.getByRole('img', {
      name: 'Original PDF page 2',
      exact: true,
    });
    await expect(image).toHaveAttribute('src', /^blob:/);
    await expect(image).toHaveJSProperty('naturalWidth', PAGE_WIDTH);
    expect(reads).toEqual([2]);
    await noHorizontalOverflow(page);
    await attachment
      .getByRole('button', { name: 'Close PDF preview', exact: true })
      .click();
    await expect(image).toHaveCount(0);
    await expect(
      page.getByRole('link', { name: 'Original chart page 3', exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('link', { name: 'Foreign page', exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Foreign page', exact: true }),
    ).toHaveCount(0);
    const opener = page.getByRole('button', {
      name: 'Original chart page 3',
      exact: true,
    });
    await opener.click();
    const dialog = page.getByRole('dialog', {
      name: 'Original PDF page 3',
      exact: true,
    });
    const citedImage = dialog.getByRole('img', {
      name: 'Original PDF page 3',
      exact: true,
    });
    await expect(citedImage).toHaveAttribute('src', /^blob:/);
    await expect(citedImage).toHaveJSProperty('naturalWidth', PAGE_WIDTH);
    expect(reads).toEqual([2, 3]);
    await noHorizontalOverflow(page);
    await page.screenshot({
      path: testInfo.outputPath(`scanned-attachment-${size}.png`),
      animations: 'disabled',
    });
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(opener).toBeFocused();
    fixture.assertClean();
  });

  test(`OCR and page-only project citations preserve evidence labels on ${size}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    const { fixture, knowledge, pageReads } = await projectConsole(page);
    await knowledge
      .getByRole('button', {
        name: 'Synthetic scanned prose.pdf · pdf#page=1;byte=0',
        exact: true,
      })
      .click();
    const ocr = page.getByRole('dialog', {
      name: 'Synthetic scanned prose.pdf',
      exact: true,
    });
    await expect(
      ocr.getByText('Recognized text', { exact: true }),
    ).toBeVisible();
    await expect(ocr).toContainText(
      'Text recognition may contain errors. Check the original page.',
    );
    await expect(ocr.locator('pre')).toHaveText(
      'Recognized synthetic operating reserve is eighteen months.',
    );
    await expect(ocr).not.toContainText('Bounded passage verified');
    expect(pageReads).toEqual([]);
    await noHorizontalOverflow(page);
    await ocr
      .getByRole('button', { name: 'Close citation', exact: true })
      .click();
    await knowledge
      .getByRole('button', {
        name: 'Synthetic chart.pdf · pdf#page=2',
        exact: true,
      })
      .click();
    const pageOnly = page.getByRole('dialog', {
      name: 'Synthetic chart.pdf',
      exact: true,
    });
    await expect(
      pageOnly.getByText('Page evidence', { exact: true }),
    ).toBeVisible();
    await expect(pageOnly).toContainText(
      'No recognized passage is available. Open the original page to inspect it.',
    );
    await expect(pageOnly.locator('pre')).toHaveCount(0);
    await expect(pageOnly).not.toContainText('Bounded passage verified');
    expect(pageReads).toEqual([]);
    await pageOnly
      .getByRole('button', { name: 'Preview PDF page 2', exact: true })
      .click();
    await expect(
      pageOnly.getByRole('img', { name: 'Original PDF page 2', exact: true }),
    ).toHaveJSProperty('naturalWidth', PAGE_WIDTH);
    expect(pageReads).toEqual(['cite_scanned_image']);
    await noHorizontalOverflow(page);
    await page.screenshot({
      path: testInfo.outputPath(`scanned-project-${size}.png`),
      animations: 'disabled',
    });
    await pageOnly
      .getByRole('button', { name: 'Close citation', exact: true })
      .click();
    await expect(pageOnly).toHaveCount(0);
    fixture.assertClean();
  });
}
