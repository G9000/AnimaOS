export interface AgentDefinition {
  name: string;
  bio: string;
  lore?: string;
  knowledge?: string[];
  topics?: string[];
  adjectives?: string[];
  style?: string;
  system: string;
  model?: string;
  /** Overrides the workspace provider; omitted values inherit the workspace default. */
  provider?: string;
  tools?: string[];
}

/** `anima.yaml`: the workspace and its agents. Every agent is an equal. */
export interface WorkspaceFile {
  name: string;
  description: string;
  /** One-sentence north star for the workspace. */
  mission?: string;
  /** 3-5 principles the agents work by. */
  values?: string[];
  model: string;
  provider: string;
  agents: AgentDefinition[];
}

export type LaunchStrategy = 'supervisor' | 'dynamic' | 'round-robin';

/**
 * A temporary agency formed from the workspace's agents for one launch. The
 * lead coordinates only for that launch; the team is gone when it ends and
 * the agents in `anima.yaml` stay equals.
 */
export interface AgencyConfig {
  name: string;
  description: string;
  mission?: string;
  values?: string[];
  model: string;
  provider: string;
  strategy: LaunchStrategy;
  maxParallelDelegations?: number;
  lead: AgentDefinition;
  members: AgentDefinition[];
  /** The workspace file the agency was formed from. */
  workspace: WorkspaceFile;
}
