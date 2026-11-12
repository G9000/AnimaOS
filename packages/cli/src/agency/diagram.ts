import type { AgentDefinition, WorkspaceFile } from './types.js';

/** Filesystem-safe kebab-case slug from an agent name. */
export function agentSlug(name: string): string {
  return name
    .trim()
    .replace(/[_\s]+/g, '-')
    .replace(/[^a-zA-Z0-9-]/g, '')
    .toLowerCase();
}

/**
 * Render the workspace as a markdown brief — mission, values, and its agents —
 * suitable to drop into a README or docs site.
 */
export function renderWorkspaceBrief(config: WorkspaceFile): string {
  const out: string[] = [];
  out.push(`# ${config.name}`);
  out.push('');
  out.push(`> ${config.description}`);
  out.push('');

  if (config.mission) {
    out.push('## Mission');
    out.push('');
    out.push(config.mission);
    out.push('');
  }

  if (config.values?.length) {
    out.push('## Values');
    out.push('');
    for (const v of config.values) out.push(`- ${v}`);
    out.push('');
  }

  out.push('## Agents');
  out.push('');
  out.push('Every agent works on its own. `animaos launch` can form a temporary team from them for one task.');
  out.push('');
  for (const a of config.agents) {
    out.push(`### ${a.name}`);
    out.push('');
    out.push(a.bio);
    if (a.tools?.length) {
      out.push('');
      out.push(`**Skills:** ${a.tools.join(', ')}`);
    }
    out.push('');
  }

  return out.join('\n');
}

/**
 * Personnel-file-style profile for a single agent.
 * Lives in `agents/<slug>/profile.md` — a human-readable record of who this agent is.
 */
export function renderAgentProfile(
  agent: AgentDefinition,
  workspaceName: string
): string {
  const out: string[] = [];
  out.push(`# ${agent.name}`);
  out.push('');
  out.push(`*${workspaceName}*`);
  out.push('');

  out.push('## Bio');
  out.push('');
  out.push(agent.bio);
  out.push('');

  if (agent.lore) {
    out.push('## Backstory');
    out.push('');
    out.push(agent.lore);
    out.push('');
  }

  if (agent.adjectives?.length) {
    out.push(`**Personality:** ${agent.adjectives.join(', ')}`);
    out.push('');
  }

  if (agent.style) {
    out.push(`**Communication style:** ${agent.style}`);
    out.push('');
  }

  if (agent.topics?.length) {
    out.push('## Expertise');
    out.push('');
    for (const t of agent.topics) out.push(`- ${t}`);
    out.push('');
  }

  if (agent.knowledge?.length) {
    out.push('## Knows deeply');
    out.push('');
    for (const k of agent.knowledge) out.push(`- ${k}`);
    out.push('');
  }

  if (agent.tools?.length) {
    out.push('## Skills');
    out.push('');
    for (const t of agent.tools) out.push(`- 🔧 \`${t}\``);
    out.push('');
  }

  out.push('## Mandate');
  out.push('');
  out.push(agent.system);
  out.push('');

  out.push('---');
  out.push('');
  out.push('## Workspace');
  out.push('');
  out.push('- `assets/` — avatars, images, brand kit (drop files here)');
  out.push('- `memory/` — agent-specific memory and notes');
  out.push('- `memory/seed.json` — optional pre-loaded memories, posted to the daemon on every launch');

  return out.join('\n');
}
