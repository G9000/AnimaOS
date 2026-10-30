import { describe, expect, it } from 'vitest';

import {
  ACCESS_PROFILES,
  deriveAccessProfile,
  selectMainAgent,
  toolNamesForProfile,
} from './agent-access';
import type { AgentDetail } from './types';

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
];

const OBSERVE_TOOLS = [
  ...COMMON_TOOLS,
  'read_file',
  'read_document_source',
  'list_dir',
  'glob',
  'grep',
  'todo_read',
];

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
];

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
  'generate_image',
  'edit_image',
];

function agent(id: string, createdAtMs: number): AgentDetail {
  return {
    id,
    name: id,
    provider: 'deterministic',
    model: 'deterministic',
    created_at_ms: createdAtMs,
    status: 'Idle',
    token_usage: {
      prompt_tokens: 0,
      completion_tokens: 0,
      total_tokens: 0,
    },
    messages: [],
  };
}

describe('workspace access profiles', () => {
  it('offers paid image tools only in explicit Operate access without changing old grants', () => {
    for (const tool of ['generate_image', 'edit_image']) {
      expect(toolNamesForProfile('operate')).toContain(tool);
      expect(toolNamesForProfile('observe')).not.toContain(tool);
      expect(toolNamesForProfile('collaborate')).not.toContain(tool);
    }
    expect(deriveAccessProfile(OPERATE_TOOLS.filter((tool) => tool !== 'generate_image' && tool !== 'edit_image'))).toBe('custom');
  });
  it('offers generated document writes only with a write profile and preserves old grants as custom', () => {
    expect(toolNamesForProfile('observe')).toContain('read_document_source');
    expect(toolNamesForProfile('observe')).not.toContain('write_document');
    expect(toolNamesForProfile('collaborate')).toContain('write_document');
    expect(
      deriveAccessProfile(
        OBSERVE_TOOLS.filter((name) => name !== 'read_document_source'),
      ),
    ).toBe('custom');
  });
  it('grants correction only to write profiles and keeps existing tool sets custom', () => {
    expect(toolNamesForProfile('observe')).not.toContain('memory_update');
    expect(toolNamesForProfile('collaborate')).toContain('memory_update');
    expect(toolNamesForProfile('operate')).toContain('memory_update');
    expect(
      deriveAccessProfile(
        COLLABORATE_TOOLS.filter((tool) => tool !== 'memory_update'),
      ),
    ).toBe('custom');
  });
  it('defines presentation metadata for every profile', () => {
    expect(Object.keys(ACCESS_PROFILES)).toEqual([
      'observe',
      'collaborate',
      'operate',
    ]);

    for (const profile of Object.values(ACCESS_PROFILES)) {
      expect(profile.label).not.toHaveLength(0);
      expect(profile.summary).not.toHaveLength(0);
      expect(profile.risk).not.toHaveLength(0);
      expect(profile.tools.length).toBeGreaterThan(0);
    }
  });

  it('returns the exact Observe tools', () => {
    expect(toolNamesForProfile('observe')).toEqual(OBSERVE_TOOLS);
  });

  it('returns the exact Collaborate tools', () => {
    expect(toolNamesForProfile('collaborate')).toEqual(COLLABORATE_TOOLS);
  });

  it('includes web search and discovery in the highest access profile only', () => {
    for (const name of ['web_search', 'search_tools', 'describe_tool']) {
      expect(toolNamesForProfile('operate')).toContain(name);
      expect(toolNamesForProfile('observe')).not.toContain(name);
      expect(toolNamesForProfile('collaborate')).not.toContain(name);
    }
  });

  it('returns the exact Operate tools', () => {
    expect(toolNamesForProfile('operate')).toEqual(OPERATE_TOOLS);
  });

  it('lets only Operate start, drive and stop apps and coding agents', () => {
    const appTools = [
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
    ];
    expect(toolNamesForProfile('operate')).toEqual(
      expect.arrayContaining(appTools),
    );
    for (const tool of appTools) {
      expect(toolNamesForProfile('collaborate')).not.toContain(tool);
    }
    expect(ACCESS_PROFILES.operate.summary).toBe(
      'Edits files, runs commands, uses apps and searches the web.',
    );
  });

  it('returns defensive tool arrays', () => {
    const tools = toolNamesForProfile('observe');
    tools.push('bash');

    expect(toolNamesForProfile('observe')).toEqual(OBSERVE_TOOLS);
  });

  it.each([
    ['observe', OBSERVE_TOOLS],
    ['collaborate', COLLABORATE_TOOLS],
    ['operate', OPERATE_TOOLS],
  ] as const)(
    'derives %s from the exact set regardless of order',
    (profile, tools) => {
      expect(deriveAccessProfile([...tools].reverse())).toBe(profile);
    },
  );

  it('derives custom for an unmatched set', () => {
    expect(deriveAccessProfile(['read_file'])).toBe('custom');
  });

  it('derives custom when an otherwise exact profile contains a duplicate', () => {
    expect(deriveAccessProfile([...OBSERVE_TOOLS, 'read_file'])).toBe('custom');
  });
});

describe('selectMainAgent', () => {
  it('never promotes a generated helper into the companion identity', () => {
    const helper = { ...agent('helper', 1), workspaceRole: 'helper' as const };
    expect(selectMainAgent([helper])).toBeNull();
    const companion = agent('companion', 2);
    expect(selectMainAgent([helper, companion])).toBe(companion);
  });
  it('prefers the persisted agency lead over creation-time ties or older workers', () => {
    const worker = agent('a-worker', 10);
    const lead = { ...agent('z-lead', 20), workspaceRole: 'lead' as const };
    expect(selectMainAgent([worker, lead])).toBe(lead);
  });
  it('returns null when no agents exist', () => {
    expect(selectMainAgent([])).toBeNull();
  });

  it('returns the oldest persisted agent without mutating the input', () => {
    const oldest = agent('oldest', 10);
    const newer = agent('newer', 20);
    const agents = [newer, oldest];

    expect(selectMainAgent(agents)).toBe(oldest);
    expect(agents).toEqual([newer, oldest]);
  });

  it('breaks equal creation-time ties by id', () => {
    const second = agent('b-agent', 10);
    const first = agent('a-agent', 10);

    expect(selectMainAgent([second, first])).toBe(first);
  });
});
