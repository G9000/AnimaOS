import { describe, expect, it } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './index.js';

describe('onboarding HTTP contracts', () => {
  it('uses workspace endpoints, forces validation, encodes paths and preserves explicit tools and providers', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({ url: String(url), init });
        return Response.json({
          configured: false,
          workspace: null,
          rootPath: null,
        });
      },
    });
    const workspace = {
      rootPath: 'C:\\Lab & #1',
      companyName: 'Lab',
      mission: 'Research',
      values: [],
    };
    const agent = {
      name: 'Ada',
      presetId: 'custom',
      system: 'Help out',
      provider: 'openai',
      model: 'model',
      tools: [],
    };
    const bootstrap = { workspace, agent };
    expect((await client.workspace.get()).workspace).toBeNull();
    await client.workspace.put(workspace);
    await client.workspace.validate({ ...workspace, validateOnly: false });
    await client.workspace.bootstrap(bootstrap);
    await client.workspace.inspect(workspace.rootPath);
    expect((await client.workspace.pickFolder()).rootPath).toBeNull();
    expect((await client.workspace.pickFolder({ purpose: 'project_knowledge' })).rootPath).toBeNull();
    await client.workspace.resume(workspace.rootPath);
    expect(
      calls.map(({ url, init }) => [
        url,
        init?.method ?? 'GET',
        init?.body ? JSON.parse(String(init.body)) : undefined,
      ]),
    ).toEqual([
      ['/api/workspace', 'GET', undefined],
      ['/api/workspace', 'PUT', workspace],
      ['/api/workspace', 'PUT', { ...workspace, validateOnly: true }],
      ['/api/workspace/bootstrap', 'POST', bootstrap],
      [
        `/api/workspace/inspect?rootPath=${encodeURIComponent(workspace.rootPath)}`,
        'GET',
        undefined,
      ],
      ['/api/workspace/pick-folder', 'POST', undefined],
      ['/api/workspace/pick-folder?purpose=project_knowledge', 'POST', undefined],
      ['/api/workspace/resume', 'POST', { rootPath: workspace.rootPath }],
    ]);
  });

  it('preserves daemon failures for caller recovery', async () => {
    const client = createDaemonClient({
      fetch: async () =>
        Response.json(
          { error: 'workspace already configured' },
          { status: 409 },
        ),
    });
    await expect(client.workspace.resume('C:\\Lab')).rejects.toBeInstanceOf(
      DaemonHttpError,
    );
  });
});
