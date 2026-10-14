import type {
  BundleReview,
  BundleView,
  CommunityManifest,
  ComponentReview,
} from '../lib/daemon-api';

export const bundleSkillFile =
  '---\nname: Violet procedure\ndescription: Read synthetic evidence\n---\n\nUse only already granted tools and the existing approval rules.\n';

export function communityManifest(
  patch: Partial<CommunityManifest> = {},
): CommunityManifest {
  return {
    schemaVersion: 1,
    id: 'example.violet',
    version: '1.0.0',
    name: 'Violet bundle',
    description: 'A synthetic local bundle',
    compatibility: {
      host: 'rust-daemon',
      minVersion: '0.1.0',
      maxVersionExclusive: '0.2.0',
      requiredCapabilities: ['mcp.stdio.windows.v1', 'skills.import-draft.v1'],
    },
    permissions: {
      nativeToolNames: ['read_file'],
      externalTools: [
        { component: 'peer', remoteName: 'synthetic_stdio_record' },
      ],
    },
    attribution: [
      {
        component: 'example',
        source: 'Synthetic local fixture',
        revision: '1.0.0',
        license: 'MIT',
        notice: 'Synthetic evidence only',
      },
    ],
    components: [
      {
        key: 'peer',
        kind: 'mcpStdio',
        label: 'Violet peer',
        programRef: 'peer',
        args: ['', 'one argument with spaces', '"literal quotes"'],
      },
      { key: 'procedure', kind: 'skill', skillFile: bundleSkillFile },
    ],
    ...patch,
  };
}

export function communityBundle(patch: Partial<BundleView> = {}): BundleView {
  const manifest = communityManifest();
  return {
    id: 'bundle-1',
    agentId: 'main',
    manifestId: manifest.id,
    manifestVersion: manifest.version,
    name: manifest.name,
    description: manifest.description,
    manifestDigest: 'manifest-digest',
    reviewDigest: 'review-digest',
    version: 3,
    enabled: true,
    state: 'installed',
    reviewedComponentStateDigest: 'component-state-3',
    mcpCount: 1,
    skillCount: 1,
    settlement: null,
    manifest,
    predecessors: [],
    toolNames: ['read_file', 'unknown_custom'],
    components: [
      {
        key: 'peer',
        kind: 'mcpStdio',
        connectionId: 'connection-1',
        connectionVersion: 1,
        configurationDigest: 'launch-digest',
        enabled: false,
        runtimeState: 'disconnected',
        reviewedAliasNames: [],
        grantedAliasNames: [],
        physicalCallsActive: false,
      },
      {
        key: 'procedure',
        kind: 'skill',
        slug: 'cb-owned-procedure',
        sourceHash: 'source-hash',
        canonicalHash: 'canonical-hash',
        approvedHash: null,
        actualHash: null,
        enabled: false,
        status: 'pending',
        draftId: 'skd-owned',
        draftStatus: 'pending',
        baseHash: null,
        workspaceGlobal: true,
      },
    ],
    ...patch,
  };
}

export function communityReview(
  manifest = communityManifest(),
  patch: Partial<BundleReview> = {},
): BundleReview {
  return {
    id: 'review-1',
    reviewDigest: 'review-digest',
    manifestDigest: 'manifest-digest',
    mode: 'install',
    bundleId: 'bundle-1',
    expectedBundleVersion: null,
    agentId: 'main',
    expiresAtMs: Date.now() + 60_000,
    manifest,
    compatibility: {
      host: 'rust-daemon',
      version: '0.1.0',
      capabilities: manifest.compatibility.requiredCapabilities,
      supported: true,
    },
    permissions: manifest.permissions,
    reviewedComponentStateDigest: 'component-state-3',
    components: manifest.components.map<ComponentReview>((part) =>
      part.kind === 'skill'
        ? {
            key: part.key,
            kind: 'skill',
            action: 'create',
            slug: 'cb-owned-procedure',
            draftId: 'skd-owned',
            sourceHash: 'source-hash',
            canonicalHash: 'canonical-hash',
            canonicalSkillFile: part.skillFile,
            baseHash: null,
            actualHash: null,
            workspaceGlobal: true,
          }
        : {
            key: part.key,
            action: 'create',
            connectionId: 'connection-1',
            previousConnectionId: null,
            expectedVersion: null,
            label: part.label,
            ...(part.kind === 'mcpHttp'
              ? {
                  kind: 'mcpHttp' as const,
                  endpoint: part.endpoint,
                  stdio: null,
                }
              : {
                  kind: 'mcpStdio' as const,
                  endpoint: null,
                  stdio: {
                    executable: 'C:\\synthetic\\peer.exe',
                    args: part.args,
                  },
                }),
            configurationDigest: 'launch-digest',
          },
    ),
    ...patch,
  };
}
