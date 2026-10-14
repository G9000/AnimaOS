import type { DaemonClient } from './client.js';
import type { McpStdioSpec } from './mcp.js';

export interface CommunityManifest {
  schemaVersion: 1;
  id: string;
  version: string;
  name: string;
  description: string;
  compatibility: {
    host: string;
    minVersion: string;
    maxVersionExclusive: string;
    requiredCapabilities: string[];
  };
  permissions: {
    nativeToolNames: string[];
    externalTools: { component: string; remoteName: string }[];
  };
  attribution: {
    component: string;
    source: string;
    revision: string;
    license: string;
    notice: string;
  }[];
  components: CommunityManifestComponent[];
}

export type CommunityManifestComponent =
  | { key: string; kind: 'mcpHttp'; label: string; endpoint: string }
  | {
      key: string;
      kind: 'mcpStdio';
      label: string;
      programRef: string;
      args: string[];
    }
  | { key: string; kind: 'skill'; skillFile: string };

export interface BundleSummary {
  id: string;
  agentId: string;
  manifestId: string;
  manifestVersion: string;
  name: string;
  description: string;
  manifestDigest: string;
  reviewDigest: string;
  version: number;
  enabled: boolean;
  state: 'installed' | 'disabled' | 'unavailable' | 'removing';
  reviewedComponentStateDigest: string;
  mcpCount: number;
  skillCount: number;
  settlement: {
    id: string;
    kind: 'install' | 'update' | 'enable' | 'disable' | 'remove';
    phase: 'cleanup_pending' | 'reconciliation_required';
  } | null;
}

export interface McpComponentView {
  key: string;
  kind: 'mcpHttp' | 'mcpStdio';
  connectionId: string;
  connectionVersion: number;
  configurationDigest: string;
  enabled: boolean;
  runtimeState: string;
  reviewedAliasNames: string[];
  grantedAliasNames: string[];
  physicalCallsActive: boolean;
}

export interface SkillComponentView {
  key: string;
  kind: 'skill';
  slug: string;
  sourceHash: string;
  canonicalHash: string;
  approvedHash: string | null;
  actualHash: string | null;
  enabled: boolean;
  status:
    | 'pending'
    | 'active'
    | 'disabled'
    | 'changed'
    | 'missing'
    | 'unavailable';
  draftId: string | null;
  draftStatus: 'pending' | 'approved' | 'rejected' | null;
  baseHash: string | null;
  workspaceGlobal: true;
}

export type BundleComponentView = McpComponentView | SkillComponentView;

/** Installation is metadata/content state, never a tool grant or process start. */
export interface BundleView extends BundleSummary {
  manifest: CommunityManifest;
  components: BundleComponentView[];
  predecessors: McpComponentView[];
  /** Current canonical owner grants, independent of manifest requests. */
  toolNames: string[];
}

interface McpComponentReviewBase {
  key: string;
  action: 'create' | 'keep' | 'replace';
  connectionId: string;
  previousConnectionId: string | null;
  expectedVersion: number | null;
  label: string;
  configurationDigest: string;
}

export type McpComponentReview = McpComponentReviewBase &
  (
    | { kind: 'mcpHttp'; endpoint: string; stdio: null }
    | { kind: 'mcpStdio'; endpoint: null; stdio: McpStdioSpec }
  );

export interface SkillComponentReview {
  key: string;
  kind: 'skill';
  action: 'create' | 'keep' | 'replace';
  slug: string;
  draftId: string | null;
  sourceHash: string;
  canonicalHash: string;
  canonicalSkillFile: string;
  baseHash: string | null;
  actualHash: string | null;
  workspaceGlobal: true;
}

export type ComponentReview = McpComponentReview | SkillComponentReview;

export interface BundleReview {
  id: string;
  reviewDigest: string;
  manifestDigest: string;
  mode: 'install' | 'update';
  bundleId: string;
  expectedBundleVersion: number | null;
  agentId: string;
  expiresAtMs: number;
  manifest: CommunityManifest;
  compatibility: {
    host: string;
    version: string;
    capabilities: string[];
    supported: true;
  };
  permissions: CommunityManifest['permissions'];
  components: ComponentReview[];
  reviewedComponentStateDigest: string;
}

type ReviewContent = {
  manifestText: string;
  programBindings: Record<string, string>;
};

/** An update pins both host-issued target ID and current host CAS revision. */
export type BundleReviewInput = ReviewContent &
  (
    | { bundleId?: never; expectedBundleVersion?: never }
    | { bundleId: string; expectedBundleVersion: number }
  );

export interface BundleInstallInput {
  reviewId: string;
  reviewDigest: string;
}

export interface BundleUpdateInput extends BundleInstallInput {
  expectedVersion: number;
}

export interface BundleStateInput {
  expectedVersion: number;
  reviewedComponentStateDigest: string;
}

export interface BundleEnabledInput extends BundleStateInput {
  enabled: boolean;
}

export interface BundleRemoval {
  bundleId: string;
  removed: true;
  trashPaths: string[];
  toolNames: string[];
}

export interface BundleReadOptions {
  signal?: AbortSignal;
}

/** Uses the shared owner transport; never retries, grants, approves or starts. */
export class CommunityBundlesClient {
  constructor(private readonly client: DaemonClient) {}

  async list(
    agentId: string,
    options: BundleReadOptions = {},
  ): Promise<BundleSummary[]> {
    const response = await this.client.requestJson<{
      bundles: BundleSummary[];
    }>(agentPath(agentId), { signal: options.signal });
    return response.bundles;
  }

  async get(
    agentId: string,
    id: string,
    options: BundleReadOptions = {},
  ): Promise<BundleView> {
    const response = await this.client.requestJson<{ bundle: BundleView }>(
      bundlePath(agentId, id),
      { signal: options.signal },
    );
    return response.bundle;
  }

  async review(
    agentId: string,
    input: BundleReviewInput,
  ): Promise<BundleReview> {
    if (
      (input.bundleId === undefined) !==
      (input.expectedBundleVersion === undefined)
    )
      throw new TypeError(
        'An update review requires both bundleId and expectedBundleVersion.',
      );
    const body: BundleReviewInput =
      input.bundleId === undefined
        ? {
            manifestText: input.manifestText,
            programBindings: input.programBindings,
          }
        : {
            manifestText: input.manifestText,
            programBindings: input.programBindings,
            bundleId: input.bundleId,
            expectedBundleVersion: input.expectedBundleVersion,
          };
    const response = await this.client.requestJson<{ review: BundleReview }>(
      `${agentPath(agentId)}/reviews`,
      { method: 'POST', body },
    );
    return response.review;
  }

  async install(
    agentId: string,
    input: BundleInstallInput,
  ): Promise<BundleView> {
    const response = await this.client.requestJson<{ bundle: BundleView }>(
      agentPath(agentId),
      {
        method: 'POST',
        body: { reviewId: input.reviewId, reviewDigest: input.reviewDigest },
      },
    );
    return response.bundle;
  }

  async update(
    agentId: string,
    id: string,
    input: BundleUpdateInput,
  ): Promise<BundleView> {
    const response = await this.client.requestJson<{ bundle: BundleView }>(
      `${bundlePath(agentId, id)}/update`,
      {
        method: 'POST',
        body: {
          expectedVersion: input.expectedVersion,
          reviewId: input.reviewId,
          reviewDigest: input.reviewDigest,
        },
      },
    );
    return response.bundle;
  }

  async setEnabled(
    agentId: string,
    id: string,
    input: BundleEnabledInput,
  ): Promise<BundleView> {
    const response = await this.client.requestJson<{ bundle: BundleView }>(
      bundlePath(agentId, id),
      {
        method: 'PATCH',
        body: {
          expectedVersion: input.expectedVersion,
          reviewedComponentStateDigest: input.reviewedComponentStateDigest,
          enabled: input.enabled,
        },
      },
    );
    return response.bundle;
  }

  async remove(
    agentId: string,
    id: string,
    input: BundleStateInput,
  ): Promise<BundleRemoval> {
    return this.client.requestJson<BundleRemoval>(bundlePath(agentId, id), {
      method: 'DELETE',
      body: {
        expectedVersion: input.expectedVersion,
        reviewedComponentStateDigest: input.reviewedComponentStateDigest,
      },
    });
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/community-bundles`;
}

function bundlePath(agentId: string, id: string): string {
  return `${agentPath(agentId)}/${encodeURIComponent(id)}`;
}
