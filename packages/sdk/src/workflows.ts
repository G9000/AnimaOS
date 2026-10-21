import type { DaemonClient } from './client.js';

export type WorkflowStatus =
  | 'running'
  | 'awaiting_approval'
  | 'awaiting_input'
  | 'needs_review'
  | 'completed'
  | 'cancelled';
export type WorkflowStepKind = 'task' | 'approval' | 'input';
export type WorkflowStepStatus = WorkflowStatus | 'pending';

export interface WorkflowStepInput {
  id: string;
  title: string;
  kind: WorkflowStepKind;
  prompt: string;
}

export interface WorkflowInput {
  title: string;
  requestKey: string;
  steps: WorkflowStepInput[];
}

export interface WorkflowResumeInput {
  revision: number;
  action: 'approve' | 'answer' | 'retry' | 'resolve';
  value?: string | null;
  acknowledgeUncertain?: boolean;
}

export interface WorkflowStep extends WorkflowStepInput {
  status: WorkflowStepStatus;
  jobId: string | null;
  attempt: number;
  attempts: string[];
  result: string | null;
}

export interface WorkflowDecision {
  stepIndex: number;
  request: WorkflowResumeInput;
  decidedAtMs: number;
}

export interface WorkflowRun {
  id: string;
  agentId: string;
  title: string;
  requestKey: string;
  status: WorkflowStatus;
  revision: number;
  currentStep: number;
  steps: WorkflowStep[];
  decisions: WorkflowDecision[];
  createdAtMs: number;
  updatedAtMs: number;
  error: string | null;
  sourceTemplate?: WorkflowTemplateSource;
}

export interface WorkflowTemplateParameter {
  name: string;
  default?: string | null;
}

export interface WorkflowTemplateInput {
  slug: string;
  revision: number;
  title: string;
  parameters: WorkflowTemplateParameter[];
  steps: WorkflowStepInput[];
}

export interface WorkflowTemplateRunInput {
  requestKey: string;
  revision: number;
  inputs: Record<string, string>;
}

export interface WorkflowTemplateSource {
  slug: string;
  revision: number;
  inputs: Record<string, string>;
  request: WorkflowTemplateRunInput;
}

export interface WorkflowTemplate extends WorkflowTemplateInput {
  agentId: string;
  createdAtMs: number;
  updatedAtMs: number;
  saveHistory: { request: WorkflowTemplateInput; savedAtMs: number }[];
  deleted: boolean;
}

export class WorkflowTemplatesClient {
  constructor(private readonly client: DaemonClient) {}

  async list(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<WorkflowTemplate[]> {
    const result = await this.client.requestJson<{
      templates: WorkflowTemplate[];
    }>(templatePath(agentId), { signal: options.signal });
    return result.templates;
  }

  get(
    agentId: string,
    slug: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<WorkflowTemplate> {
    return this.client.requestJson<WorkflowTemplate>(
      templatePath(agentId, slug),
      { signal: options.signal },
    );
  }

  save(
    agentId: string,
    input: WorkflowTemplateInput,
  ): Promise<WorkflowTemplate> {
    return this.client.requestJson<WorkflowTemplate>(templatePath(agentId), {
      method: 'POST',
      body: input,
    });
  }

  run(
    agentId: string,
    slug: string,
    input: WorkflowTemplateRunInput,
  ): Promise<WorkflowRun> {
    return this.client.requestJson<WorkflowRun>(
      `${templatePath(agentId, slug)}/run`,
      { method: 'POST', body: input },
    );
  }

  delete(
    agentId: string,
    slug: string,
    input: { revision: number },
  ): Promise<WorkflowTemplate> {
    return this.client.requestJson<WorkflowTemplate>(
      templatePath(agentId, slug),
      { method: 'DELETE', body: input },
    );
  }
}

function templatePath(agentId: string, slug?: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/workflow-templates${slug === undefined ? '' : `/${encodeURIComponent(slug)}`}`;
}

export class WorkflowsClient {
  constructor(private readonly client: DaemonClient) {}

  async list(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<WorkflowRun[]> {
    const response = await this.client.requestJson<{
      workflows: WorkflowRun[];
    }>(workflowPath(agentId), { signal: options.signal });
    return response.workflows;
  }

  get(
    agentId: string,
    id: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<WorkflowRun> {
    return this.client.requestJson<WorkflowRun>(workflowPath(agentId, id), {
      signal: options.signal,
    });
  }

  create(agentId: string, input: WorkflowInput): Promise<WorkflowRun> {
    return this.client.requestJson<WorkflowRun>(workflowPath(agentId), {
      method: 'POST',
      body: input,
    });
  }

  /** Replay an uncertain decision with its original revision and exact payload. */
  resume(
    agentId: string,
    id: string,
    input: WorkflowResumeInput,
  ): Promise<WorkflowRun> {
    return this.client.requestJson<WorkflowRun>(
      `${workflowPath(agentId, id)}/resume`,
      { method: 'POST', body: input },
    );
  }

  cancel(
    agentId: string,
    id: string,
    input: { revision: number },
  ): Promise<WorkflowRun> {
    return this.client.requestJson<WorkflowRun>(
      `${workflowPath(agentId, id)}/cancel`,
      { method: 'POST', body: input },
    );
  }
}

function workflowPath(agentId: string, id?: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/workflows${id === undefined ? '' : `/${encodeURIComponent(id)}`}`;
}
