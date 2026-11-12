import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { load } from 'js-yaml';
import type {
  AgencyConfig,
  AgentDefinition,
  LaunchStrategy,
  WorkspaceFile,
} from './types.js';

const STRATEGIES: readonly LaunchStrategy[] = ['supervisor', 'dynamic', 'round-robin'];

/**
 * Check whether an `anima.yaml` workspace file exists in the given directory.
 */
export function workspaceExists(dir: string): boolean {
  return existsSync(join(dir, 'anima.yaml'));
}

function loadAgent(a: Record<string, unknown>): AgentDefinition {
  return {
    name: (a.name as string) ?? 'unnamed',
    bio: (a.bio as string) ?? '',
    lore: a.lore as string | undefined,
    adjectives: Array.isArray(a.adjectives)
      ? (a.adjectives as string[])
      : undefined,
    topics: Array.isArray(a.topics) ? (a.topics as string[]) : undefined,
    knowledge: Array.isArray(a.knowledge)
      ? (a.knowledge as string[])
      : undefined,
    style: a.style as string | undefined,
    system: (a.system as string) ?? '',
    model: a.model as string | undefined,
    provider: typeof a.provider === 'string' ? a.provider.trim() || undefined : undefined,
    tools: a.tools as string[] | undefined,
  };
}

/**
 * Load and validate an `anima.yaml` workspace file from the given directory.
 *
 * Requires a name and at least one agent with a name and system prompt.
 * `description` defaults to empty, `model` to `"gpt-4o"`, `provider` to
 * `"openai"`.
 */
export function loadWorkspace(dir: string): WorkspaceFile {
  const filePath = join(dir, 'anima.yaml');

  if (!existsSync(filePath)) {
    throw new Error(`Workspace file not found: ${filePath}`);
  }

  const raw = readFileSync(filePath, 'utf-8');
  const parsed = load(raw) as Record<string, unknown>;

  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`Invalid workspace file: ${filePath} is not a valid YAML object`);
  }

  if (!parsed.name || typeof parsed.name !== 'string') {
    throw new Error('Workspace file missing required field: name');
  }

  const agents = Array.isArray(parsed.agents)
    ? (parsed.agents as Record<string, unknown>[]).map(loadAgent)
    : [];
  if (agents.length === 0) {
    throw new Error('Workspace file must list at least one agent under agents');
  }
  for (const agent of agents) {
    if (!agent.name.trim() || !agent.system.trim()) {
      throw new Error('Every agent in the workspace file needs a name and a system prompt');
    }
  }

  return {
    name: parsed.name as string,
    description: (parsed.description as string) ?? '',
    mission: typeof parsed.mission === 'string' ? parsed.mission : undefined,
    values: Array.isArray(parsed.values) ? (parsed.values as string[]) : undefined,
    model: (parsed.model as string) ?? 'gpt-4o',
    provider: (parsed.provider as string) ?? 'openai',
    agents,
  };
}

/**
 * Forms a temporary agency for one launch. `lead` names the agent that
 * coordinates this launch only (default: the first agent); everyone else
 * joins as a member.
 */
export function formAgency(
  workspace: WorkspaceFile,
  options: { lead?: string; strategy?: string; maxParallelDelegations?: number } = {}
): AgencyConfig {
  const strategy = (options.strategy ?? 'supervisor') as LaunchStrategy;
  if (!STRATEGIES.includes(strategy)) {
    throw new Error(`Unknown strategy "${options.strategy}". Use one of: ${STRATEGIES.join(', ')}`);
  }
  const wanted = options.lead?.trim().toLowerCase();
  const lead = wanted
    ? workspace.agents.find((agent) => agent.name.toLowerCase() === wanted)
    : workspace.agents[0];
  if (!lead) {
    throw new Error(
      `No agent named "${options.lead}". Agents: ${workspace.agents.map((a) => a.name).join(', ')}`
    );
  }
  return {
    name: workspace.name,
    description: workspace.description,
    mission: workspace.mission,
    values: workspace.values,
    model: workspace.model,
    provider: workspace.provider,
    strategy,
    ...(options.maxParallelDelegations
      ? { maxParallelDelegations: options.maxParallelDelegations }
      : {}),
    lead,
    members: workspace.agents.filter((agent) => agent !== lead),
    workspace,
  };
}
