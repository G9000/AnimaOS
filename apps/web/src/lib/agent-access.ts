import type { AgentDetail } from './types';

export type AccessProfile = 'observe' | 'collaborate' | 'operate';
export type DerivedAccessProfile = AccessProfile | 'custom';

/** Kept separate from presets because arbitrary Python needs an explicit grant. */
export const PROGRAMMATIC_TOOL_ACCESS = {
  name: 'execute_code',
  label: 'Allow programmatic tool chains',
  requires: 'bash',
} as const;

/** Workflow creation and inspection are one explicit chat permission. */
export const WORKFLOW_TOOL_ACCESS = {
  tools: ['create_workflow', 'list_workflows'],
  label: 'Allow workflows in chat',
} as const;

export const BACKGROUND_HELPER_TOOL_ACCESS = {
  tools: [
    'spawn_helper_async',
    'spawn_helpers',
    'wait_helper_runs',
    'list_helper_runs',
    'steer_helper',
    'stop_helper',
  ],
  label: 'Allow background helpers',
} as const;

export const WORKFLOW_TEMPLATE_TOOL_ACCESS = {
  tools: [
    'save_workflow_template',
    'list_workflow_templates',
    'run_workflow_template',
  ],
  label: 'Allow workflow templates in chat',
} as const;

export const TOOL_RESULT_ACCESS = {
  name: 'read_tool_result',
  label: 'Allow saved tool result reads',
} as const;

export const BROWSER_TOOL_ACCESS = [
  { name: 'browser_navigate', label: 'Allow browser navigation' },
  { name: 'browser_observe', label: 'Allow browser observation' },
  { name: 'browser_act', label: 'Allow browser interaction' },
  { name: 'browser_transfer', label: 'Allow browser file transfers' },
] as const;

export function isBrowserTool(name: string): boolean {
  return BROWSER_TOOL_ACCESS.some((tool) => tool.name === name);
}

const COMMON_TOOLS = [
  'memory_search',
  'memory_add',
  'recent_memories',
  'get_current_time',
  'calculate',
  'search_conversations',
  'read_conversation_window',
  'load_skill',
  'skill_history',
  'list_automations',
  'check_automation_status',
  'clarify',
] as const;

const OBSERVE_TOOLS = [
  ...COMMON_TOOLS,
  'read_file',
  'read_document_source',
  'list_dir',
  'glob',
  'grep',
  'todo_read',
] as const;

const COLLABORATE_TOOLS = [
  ...OBSERVE_TOOLS,
  'memory_update',
  'write_file',
  'write_document',
  'edit_file',
  'multi_edit',
  'todo_write',
  'propose_skill',
  'remove_skill',
  'undo_skill',
  'propose_plan',
  'create_automation',
  'pause_automation',
  'complete_reminder',
  'snooze_reminder',
] as const;

const OPERATE_TOOLS = [
  ...COLLABORATE_TOOLS,
  'bash',
  'bg_start',
  'bg_output',
  'bg_stop',
  'bg_list',
  'bg_wait',
  'app_start',
  'app_send',
  'app_read',
  'app_stop',
  'app_list',
  'acp_start',
  'acp_prompt',
  'acp_read',
  'acp_cancel',
  'acp_stop',
  'acp_list',
  'web_search',
  'search_tools',
  'describe_tool',
] as const;

export const ACCESS_PROFILES = {
  observe: {
    label: 'Observe',
    summary: 'Inspect workspace files and todos.',
    risk: 'Read-only workspace access; cannot modify files or execute processes.',
    tools: OBSERVE_TOOLS,
  },
  collaborate: {
    label: 'Collaborate',
    summary: 'Inspect and update workspace files and todos.',
    risk: 'Can modify workspace files; cannot execute processes.',
    tools: COLLABORATE_TOOLS,
  },
  operate: {
    label: 'Operate',
    summary: 'Edits files, runs commands, uses apps and searches the web.',
    risk: 'Can run shell commands, background work and apps such as coding agents. Rules still apply.',
    tools: OPERATE_TOOLS,
  },
} as const;

const PROFILE_NAMES: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];

export function toolNamesForProfile(profile: AccessProfile): string[] {
  return [...ACCESS_PROFILES[profile].tools];
}

export function deriveAccessProfile(
  toolNames: readonly string[],
): DerivedAccessProfile {
  const uniqueTools = new Set(toolNames);
  if (uniqueTools.size !== toolNames.length) {
    return 'custom';
  }

  for (const profile of PROFILE_NAMES) {
    const profileTools = ACCESS_PROFILES[profile].tools;
    if (
      profileTools.length === uniqueTools.size &&
      profileTools.every((tool) => uniqueTools.has(tool))
    ) {
      return profile;
    }
  }

  return 'custom';
}

export function selectMainAgent(
  agents: readonly AgentDetail[],
): AgentDetail | null {
  const companions = agents.filter((agent) => agent.workspaceRole !== 'helper');
  if (companions.length === 0) {
    return null;
  }

  return [...companions].sort((left, right) => {
    const leadOrder =
      Number(right.workspaceRole === 'lead') -
      Number(left.workspaceRole === 'lead');
    if (leadOrder !== 0) return leadOrder;
    const creationOrder = left.created_at_ms - right.created_at_ms;
    if (creationOrder !== 0) {
      return creationOrder;
    }

    if (left.id < right.id) {
      return -1;
    }
    if (left.id > right.id) {
      return 1;
    }
    return 0;
  })[0];
}
