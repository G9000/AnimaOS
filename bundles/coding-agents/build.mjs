#!/usr/bin/env node
// Regenerates every bundles/coding-agents/<dir>/manifest.json from its SKILL.md
// plus the metadata below. No dependencies: `node bundles/coding-agents/build.mjs`.
// SKILL.md is the source of truth; the manifest inlines its exact text.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(fileURLToPath(import.meta.url));
const VERSION = '1.1.0';

const PROCESS_TOOLS = ['bash', 'bg_start', 'bg_wait', 'bg_output', 'bg_stop', 'bg_list'];
const APP_TOOLS = ['app_start', 'app_send', 'app_read', 'app_stop'];
const ACP_TOOLS = ['acp_start', 'acp_prompt', 'acp_read', 'acp_cancel', 'acp_stop'];
const CLI_TOOLS = [...PROCESS_TOOLS, ...APP_TOOLS, ...ACP_TOOLS, 'write_file'];

const BUNDLES = [
  {
    dir: 'claude-code',
    id: 'anima.coding.claude-code',
    name: 'Coding agent: Claude Code',
    description:
      'Know-how for driving the Claude Code CLI as a coding worker: one-shot JSON runs, resume, background runs, the TUI in an app session, ACP, freedom per task, isolation and verification.',
    tools: CLI_TOOLS,
  },
  {
    dir: 'codex',
    id: 'anima.coding.codex',
    name: 'Coding agent: Codex',
    description:
      'Know-how for driving the OpenAI Codex CLI as a coding worker: codex exec JSONL runs, exec resume, codex review, sandbox modes, background runs, the TUI in an app session, ACP via codex-acp and verification.',
    tools: CLI_TOOLS,
  },
  {
    dir: 'opencode',
    id: 'anima.coding.opencode',
    name: 'Coding agent: OpenCode',
    description:
      'Know-how for driving the OpenCode CLI as a coding worker: opencode run JSON events, session resume, permission config, background runs, the TUI in an app session, ACP via opencode acp and verification.',
    tools: CLI_TOOLS,
  },
  {
    dir: 'gemini-cli',
    id: 'anima.coding.gemini-cli',
    name: 'Coding agent: Gemini CLI',
    description:
      "Know-how for driving Google's Gemini CLI as a coding worker: headless JSON runs, resume, approval modes and sandbox, folder trust, background runs, the TUI in an app session, ACP via gemini --acp and verification.",
    tools: CLI_TOOLS,
  },
  {
    dir: 'delegate',
    id: 'anima.coding.delegate',
    name: 'Coding agent handoff',
    description:
      'Generic know-how for delegating code work to a coding CLI: picking the CLI, writing the brief, parallel coders in separate git worktrees, reviewing the diff, running checks and reporting. Never commits or pushes unless the owner asks.',
    tools: ['load_skill', 'read_file', 'write_file', 'edit_file', ...PROCESS_TOOLS, ...APP_TOOLS, ...ACP_TOOLS],
  },
];

function attribution(bundle) {
  const local = {
    component: 'skill',
    source: `bundles/coding-agents/${bundle.dir}/SKILL.md`,
    revision: `animaos coding-agents bundle ${VERSION}`,
    license: 'MIT (repository package metadata; no root LICENSE file)',
    notice:
      'Written for Anima from the CLI help, its source and official docs; see bundles/coding-agents/ATTRIBUTION.md',
  };
  return [local];
}

function checkSkill(dir, text) {
  const match = /^---\n([\s\S]*?)\n---\n\n?([\s\S]*)$/.exec(text);
  if (!match) throw new Error(`${dir}/SKILL.md: missing front matter`);
  const [, front, body] = match;
  if (Buffer.byteLength(front) > 4 * 1024) throw new Error(`${dir}: front matter over 4 KiB`);
  if (Buffer.byteLength(body) > 32 * 1024) throw new Error(`${dir}: body over 32 KiB`);
  const name = /^name: (.+)$/m.exec(front)?.[1];
  const description = /^description: "(.+)"$/m.exec(front)?.[1];
  if (!name || name.length > 64) throw new Error(`${dir}: name missing or over 64 chars`);
  if (!description || [...description].length > 300) {
    throw new Error(`${dir}: description missing or over 300 chars`);
  }
  if (/[\u0000-\u0008\u000b-\u001f\u007f​-‏‪-‮⁠-⁯﻿]/u.test(text)) {
    throw new Error(`${dir}: control or hidden characters`);
  }
  return { bytes: Buffer.byteLength(text), name };
}

for (const bundle of BUNDLES) {
  const skillFile = readFileSync(join(ROOT, bundle.dir, 'SKILL.md'), 'utf8');
  const { bytes, name } = checkSkill(bundle.dir, skillFile);
  const manifest = {
    schemaVersion: 1,
    id: bundle.id,
    version: VERSION,
    name: bundle.name,
    description: bundle.description,
    compatibility: {
      host: 'rust-daemon',
      minVersion: '0.1.0',
      maxVersionExclusive: '0.2.0',
      requiredCapabilities: ['skills.import-draft.v1'],
    },
    permissions: { nativeToolNames: bundle.tools, externalTools: [] },
    attribution: attribution(bundle),
    components: [{ key: 'skill', kind: 'skill', skillFile }],
  };
  writeFileSync(join(ROOT, bundle.dir, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`${bundle.id}: skill ${name}, ${bytes} bytes`);
}
