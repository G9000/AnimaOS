import { describe, expect, it } from 'vitest';
import { createDaemonClient } from './index.js';
import type {
  BundleReview,
  BundleReviewInput,
  BundleView,
  CommunityManifest,
} from './community-bundles.js';

const manifest: CommunityManifest = {
  schemaVersion: 1,
  id: 'example.violet',
  version: '1.0.0',
  name: 'Violet',
  description: 'Local example',
  compatibility: {
    host: 'rust-daemon',
    minVersion: '0.1.0',
    maxVersionExclusive: '0.2.0',
    requiredCapabilities: ['mcp.stdio.windows.v1'],
  },
  permissions: { nativeToolNames: [], externalTools: [] },
  attribution: [
    {
      component: 'example',
      source: 'Synthetic local fixture',
      revision: '1.0.0',
      license: 'MIT',
      notice: 'Synthetic fixture only',
    },
  ],
  components: [
    {
      key: 'peer',
      kind: 'mcpStdio',
      label: 'Peer',
      programRef: 'peer',
      args: ['', 'one argument', '"literal quotes"'],
    },
  ],
};
const view: BundleView = {
  id: 'bundle/1',
  agentId: 'agent/a',
  manifestId: manifest.id,
  manifestVersion: manifest.version,
  name: manifest.name,
  description: manifest.description,
  manifestDigest: 'manifest-digest',
  reviewDigest: 'review-digest',
  version: 3,
  enabled: true,
  state: 'installed',
  reviewedComponentStateDigest: 'state-digest',
  mcpCount: 1,
  skillCount: 0,
  settlement: null,
  manifest,
  predecessors: [],
  toolNames: ['read_file', 'unrelated_custom'],
  components: [
    {
      key: 'peer',
      kind: 'mcpStdio',
      connectionId: 'connection/1',
      connectionVersion: 1,
      configurationDigest: 'launch-digest',
      enabled: false,
      runtimeState: 'disconnected',
      reviewedAliasNames: [],
      grantedAliasNames: [],
      physicalCallsActive: false,
    },
  ],
};
const reviewed: BundleReview = {
  id: 'review/1',
  reviewDigest: 'review-digest',
  manifestDigest: 'manifest-digest',
  mode: 'install',
  bundleId: view.id,
  expectedBundleVersion: null,
  agentId: view.agentId,
  expiresAtMs: 123456,
  manifest,
  compatibility: {
    host: 'rust-daemon',
    version: '0.1.0',
    capabilities: ['mcp.stdio.windows.v1'],
    supported: true,
  },
  permissions: manifest.permissions,
  reviewedComponentStateDigest: 'state-digest',
  components: [
    {
      key: 'peer',
      kind: 'mcpStdio',
      action: 'create',
      connectionId: 'connection/1',
      previousConnectionId: null,
      expectedVersion: null,
      label: 'Peer',
      endpoint: null,
      stdio: {
        executable: 'C:\\synthetic\\peer.exe',
        args: ['', 'one argument', '"literal quotes"'],
      },
      configurationDigest: 'launch-digest',
    },
  ],
};

function harness(respond: (url: string, init?: RequestInit) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url), init);
    },
  });
  return { bundles: client.communityBundles, requests };
}

describe('community bundle owner client', () => {
  it('serializes exact install and update reviews without altering raw manifest, paths or literal argv', async () => {
    const { bundles, requests } = harness(() =>
      Response.json({ review: reviewed }),
    );
    const manifestText = JSON.stringify(manifest, null, 2);
    const programBindings = { peer: 'C:\\synthetic\\peer.exe' };
    expect(
      await bundles.review('agent/a', { manifestText, programBindings }),
    ).toEqual(reviewed);
    await bundles.review('agent/a', {
      manifestText,
      programBindings,
      bundleId: 'bundle/1',
      expectedBundleVersion: 3,
    });
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/community-bundles/reviews',
      '/api/agents/agent%2Fa/community-bundles/reviews',
    ]);
    expect(JSON.parse(requests[0].init!.body as string)).toEqual({
      manifestText,
      programBindings,
    });
    expect(JSON.parse(requests[1].init!.body as string)).toEqual({
      manifestText,
      programBindings,
      bundleId: 'bundle/1',
      expectedBundleVersion: 3,
    });
    expect(
      JSON.parse(JSON.parse(requests[0].init!.body as string).manifestText)
        .components[0].args,
    ).toEqual(['', 'one argument', '"literal quotes"']);
    expect(requests).toHaveLength(2);
  });

  it('uses encoded read/lifecycle paths, exact CAS and component witnesses, and canonical grants including deletion', async () => {
    const removal = {
      bundleId: view.id,
      removed: true,
      trashPaths: ['.trash/owned-skill'],
      toolNames: ['read_file'],
    };
    const { bundles, requests } = harness((url, init) => {
      if (init?.method === 'DELETE') return Response.json(removal);
      if (url.endsWith('/community-bundles') && !init?.method)
        return Response.json({ bundles: [view] });
      return Response.json({ bundle: view });
    });
    expect(await bundles.list('agent/a')).toEqual([view]);
    expect(await bundles.get('agent/a', 'bundle/1')).toEqual(view);
    expect(
      await bundles.install('agent/a', {
        reviewId: reviewed.id,
        reviewDigest: reviewed.reviewDigest,
      }),
    ).toEqual(view);
    expect(
      await bundles.update('agent/a', view.id, {
        expectedVersion: 3,
        reviewId: 'review/2',
        reviewDigest: 'next-digest',
      }),
    ).toEqual(view);
    expect(
      await bundles.setEnabled('agent/a', view.id, {
        expectedVersion: 3,
        reviewedComponentStateDigest: 'state-digest',
        enabled: false,
      }),
    ).toEqual(view);
    expect(
      await bundles.remove('agent/a', view.id, {
        expectedVersion: 3,
        reviewedComponentStateDigest: 'state-digest',
      }),
    ).toEqual(removal);
    expect(
      requests.map(({ url, init }) => `${init?.method ?? 'GET'} ${url}`),
    ).toEqual([
      'GET /api/agents/agent%2Fa/community-bundles',
      'GET /api/agents/agent%2Fa/community-bundles/bundle%2F1',
      'POST /api/agents/agent%2Fa/community-bundles',
      'POST /api/agents/agent%2Fa/community-bundles/bundle%2F1/update',
      'PATCH /api/agents/agent%2Fa/community-bundles/bundle%2F1',
      'DELETE /api/agents/agent%2Fa/community-bundles/bundle%2F1',
    ]);
    expect(
      requests.map(({ init }) =>
        init?.body ? JSON.parse(init.body as string) : undefined,
      ),
    ).toEqual([
      undefined,
      undefined,
      { reviewId: 'review/1', reviewDigest: 'review-digest' },
      { expectedVersion: 3, reviewId: 'review/2', reviewDigest: 'next-digest' },
      {
        expectedVersion: 3,
        reviewedComponentStateDigest: 'state-digest',
        enabled: false,
      },
      { expectedVersion: 3, reviewedComponentStateDigest: 'state-digest' },
    ]);
    expect(view.toolNames).toEqual(['read_file', 'unrelated_custom']);
    expect(view.components[0].enabled).toBe(false);
  });

  it('forwards cancellation only for collection and detail reads', async () => {
    const { bundles, requests } = harness(() =>
      Response.json({ bundles: [], bundle: view }),
    );
    const controller = new AbortController();
    await bundles.list('a', { signal: controller.signal });
    await bundles.get('a', 'b', { signal: controller.signal });
    await bundles.install('a', { reviewId: 'r', reviewDigest: 'd' });
    expect(requests.map(({ init }) => init?.signal)).toEqual([
      controller.signal,
      controller.signal,
      undefined,
    ]);
  });

  it('keeps extra caller authority fields outside the strict review and install envelopes', async () => {
    const { bundles, requests } = harness((url) =>
      Response.json(
        url.endsWith('/reviews') ? { review: reviewed } : { bundle: view },
      ),
    );
    const reviewInput = {
      manifestText: JSON.stringify(manifest),
      programBindings: { peer: 'C:\\synthetic\\peer.exe' },
      enabled: true,
      toolNames: ['arbitrary_grant'],
    };
    const installInput = {
      reviewId: reviewed.id,
      reviewDigest: reviewed.reviewDigest,
      enabled: true,
      approveSkills: true,
      toolNames: ['arbitrary_grant'],
    };
    await bundles.review('agent/a', reviewInput);
    await bundles.install('agent/a', installInput);
    expect(JSON.parse(requests[0].init!.body as string)).toEqual({
      manifestText: reviewInput.manifestText,
      programBindings: reviewInput.programBindings,
    });
    expect(JSON.parse(requests[1].init!.body as string)).toEqual({
      reviewId: reviewed.id,
      reviewDigest: reviewed.reviewDigest,
    });
  });

  it('refuses an incomplete update pair instead of silently reviewing a new installation', async () => {
    const { bundles, requests } = harness(() =>
      Response.json({ review: reviewed }),
    );
    const content = {
      manifestText: JSON.stringify(manifest),
      programBindings: {},
    };
    for (const incomplete of [
      { ...content, bundleId: 'bundle/1' },
      { ...content, expectedBundleVersion: 3 },
    ])
      await expect(
        bundles.review('agent/a', incomplete as BundleReviewInput),
      ).rejects.toThrow('requires both bundleId and expectedBundleVersion');
    expect(requests).toHaveLength(0);
  });

  it.each([409, 503])(
    'preserves the existing static error envelope and never retries an uncertain %s mutation',
    async (status) => {
      let attempts = 0;
      const body = { error: 'Community bundle state unavailable' };
      const { bundles } = harness(() => {
        attempts += 1;
        return Response.json(body, { status });
      });
      await expect(
        bundles.remove('a', 'b', {
          expectedVersion: 2,
          reviewedComponentStateDigest: 's',
        }),
      ).rejects.toMatchObject({ status, body, message: body.error });
      expect(attempts).toBe(1);
    },
  );
});
