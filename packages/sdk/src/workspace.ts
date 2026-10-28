import type { AgentSnapshot } from './agents.js';
import type { DaemonClient } from './client.js';

export interface WorkspaceConfigRequest {
  rootPath: string;
  companyName: string;
  mission: string;
  values?: string[];
  validateOnly?: boolean;
}

export interface WorkspaceConfigResponse {
  rootPath: string;
  companyName: string;
  mission: string;
  values: string[];
  hasAvatar: boolean;
}

export interface WorkspaceResponse {
  configured: boolean;
  workspace: WorkspaceConfigResponse | null;
  defaultRoot: string;
  rootPathExists?: boolean;
}

export interface WorkspaceValidationResponse extends WorkspaceResponse {
  rootPathExists: boolean;
}

export interface BootstrapAgentRequest {
  name: string;
  presetId: string;
  bio?: string | null;
  adjectives?: string[] | null;
  style?: string | null;
  system: string;
  provider?: string | null;
  model: string;
  tools?: string[];
}

export interface WorkspaceBootstrapRequest {
  workspace: WorkspaceConfigRequest;
  agent: BootstrapAgentRequest;
  workers?: BootstrapAgentRequest[];
}

export interface WorkspaceBootstrapResponse {
  workspace: WorkspaceConfigResponse;
  agent: AgentSnapshot;
  workers: AgentSnapshot[];
}

export interface WorkspaceInspectAgentPreview {
  name: string;
  bio?: string;
  provider: string;
  model: string;
}

export interface WorkspaceInspectResponse {
  found: boolean;
  companyName?: string;
  mission?: string;
  values?: string[];
  orchestrator?: WorkspaceInspectAgentPreview;
  workers?: WorkspaceInspectAgentPreview[];
  providerAvailable?: boolean;
}

export interface WorkspaceResumeRequest {
  rootPath: string;
}

export interface WorkspaceResumeResponse {
  workspace: WorkspaceConfigResponse;
  orchestrator: AgentSnapshot;
  workers: AgentSnapshot[];
  skipped: string[];
}

export interface WorkspacePickFolderResponse {
  rootPath: string | null;
}

export class WorkspaceClient {
  constructor(private readonly client: DaemonClient) {}

  listFiles(): Promise<WorkspaceFilesResponse> {
    return this.client.requestJson('/api/workspace/files');
  }

  readFile(path: string, options: { cursor?: string; signal?: AbortSignal } = {}): Promise<WorkspaceFileResponse> {
    const query = `path=${encodeURIComponent(path)}${options.cursor ? `&cursor=${encodeURIComponent(options.cursor)}` : ''}`;
    return this.client.requestJson(
      `/api/workspace/file?${query}`, { signal: options.signal },
    );
  }

  downloadFile(
    path: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<Blob> {
    const generatedDocument = /\.(?:docx|xlsx|pptx|pdf)$/i.test(path);
    return this.client.requestBoundedBlob(
      `/api/workspace/file/download?path=${encodeURIComponent(path)}`,
      { method: 'GET', signal: options.signal },
      {
        maxBytes: (generatedDocument ? 4 : 1) * 1024 * 1024,
        contentType: 'application/octet-stream',
        allowEmpty: !generatedDocument,
      },
    );
  }

  get(): Promise<WorkspaceResponse> {
    return this.client.requestJson('/api/workspace');
  }

  put(input: WorkspaceConfigRequest): Promise<WorkspaceResponse> {
    return this.client.requestJson('/api/workspace', {
      method: 'PUT',
      body: input,
    });
  }

  validate(
    input: WorkspaceConfigRequest,
  ): Promise<WorkspaceValidationResponse> {
    return this.client.requestJson('/api/workspace', {
      method: 'PUT',
      body: { ...input, validateOnly: true },
    });
  }

  bootstrap(
    input: WorkspaceBootstrapRequest,
  ): Promise<WorkspaceBootstrapResponse> {
    return this.client.requestJson('/api/workspace/bootstrap', {
      method: 'POST',
      body: input,
    });
  }

  inspect(rootPath: string): Promise<WorkspaceInspectResponse> {
    return this.client.requestJson(
      `/api/workspace/inspect?rootPath=${encodeURIComponent(rootPath)}`,
    );
  }

  resume(rootPath: string): Promise<WorkspaceResumeResponse> {
    return this.client.requestJson('/api/workspace/resume', {
      method: 'POST',
      body: { rootPath } satisfies WorkspaceResumeRequest,
    });
  }

  pickFolder(): Promise<WorkspacePickFolderResponse> {
    return this.client.requestJson('/api/workspace/pick-folder', {
      method: 'POST',
    });
  }
}

export interface WorkspaceFileEntry {
  path: string;
  name: string;
  sizeBytes: number;
  modifiedAtMs: number | null;
}

export interface WorkspaceFilesResponse {
  files: WorkspaceFileEntry[];
  truncated: boolean;
}

export interface WorkspaceFileResponse {
  path: string;
  content: string;
  truncated: boolean;
  /** Verified Anima-generated document content, rendered as safe Markdown. */
  previewKind?: 'document' | 'office_content';
  /** Bounded structural content from an exact server publication receipt. */
  officeContent?: import('./document-edits.js').OfficeInventoryPage;
  /** Hash of the exact file bytes captured for the document preview. */
  sha256?: string;
}
