import { Command } from 'commander';
import * as clack from '@clack/prompts';
import pc from 'picocolors';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import yaml from 'js-yaml';
import type { AgencyConfig, AgentDefinition } from '../agency/types.js';
import {
  agentSlug,
  renderOrgChart,
  renderAgencyBrief,
  renderAgentProfile,
} from '../agency/diagram.js';
import {
  DETERMINISTIC_PROVIDER,
  PROVIDER_HELP_TEXT,
  REMOTE_PROVIDER_IDS,
  hasProviderApiKey,
  normalizeProvider,
  providerKeyEnvNames,
  providerRequiresApiKey,
} from '../provider-config.js';

export type CreateKind = 'agent';

export interface CreateOptions {
  kind?: string;
  provider?: string;
  model: string;
  description?: string;
  apiKey?: string;
  system?: string;
}

interface CreateTarget {
  kind?: CreateKind;
  nameArg?: string;
  error?: string;
}

interface WrittenWorkspace {
  dirName: string;
  agentCount: number;
}

const DEFAULT_CREATE_MODEL = 'gpt-4o-mini';

function cancelAndExit(): never {
  clack.cancel('Cancelled.');
  process.exit(0);
}

export function resolveCreateTarget(
  kindOrName: string | undefined,
  nameArg: string | undefined,
  kindOption: string | undefined
): CreateTarget {
  const kind = (kindOption ?? kindOrName)?.trim().toLowerCase();

  if (kind === 'agency') {
    return {
      error:
        'Agency generation was removed. Create a single agent with "animaos create agent <name>", or write anima.yaml by hand.',
    };
  }

  if (kindOption && kind !== 'agent') {
    return { error: `Unknown create kind "${kindOption}". Use "agent".` };
  }

  if (kind === 'agent') {
    // `--kind agent my-name` puts the name in the first positional slot.
    return { kind: 'agent', nameArg: kindOption ? nameArg ?? kindOrName : nameArg };
  }

  if (kindOrName) {
    return {
      error: `Unknown create kind "${kindOrName}". Use "animaos create agent <name>".`,
    };
  }

  return { kind: 'agent' };
}

export function resolveCreateProvider(
  provider: string | undefined,
  apiKey?: string
): string {
  const explicitProvider = normalizeProvider(provider);
  if (explicitProvider) return explicitProvider;

  const configuredProvider = REMOTE_PROVIDER_IDS.find(
    (candidate) => providerRequiresApiKey(candidate) && hasProviderApiKey(candidate, apiKey)
  );

  return configuredProvider ?? DETERMINISTIC_PROVIDER;
}

export function getCreateProviderIssue(provider: string | undefined): string | undefined {
  const normalizedProvider = normalizeProvider(provider);
  if (
    !normalizedProvider ||
    normalizedProvider === DETERMINISTIC_PROVIDER ||
    REMOTE_PROVIDER_IDS.includes(normalizedProvider)
  ) {
    return undefined;
  }

  return `Unknown provider "${provider}". ${PROVIDER_HELP_TEXT}`;
}

function workspaceSlug(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+|-+$/g, '');

  return slug || 'anima-workspace';
}

function inferAgentTopics(description: string): string[] {
  const topics = description
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, ' ')
    .split(/\s+/)
    .filter((word) => word.length > 3)
    .filter(
      (word) => !['agent', 'build', 'create', 'that', 'this', 'with'].includes(word)
    );

  return [...new Set(topics)].slice(0, 5);
}

export function buildSingleAgentConfig(input: {
  name: string;
  description: string;
  provider: string;
  model: string;
  system?: string;
}): AgencyConfig {
  const description = input.description.trim();
  const system =
    input.system?.trim() ||
    `You are ${input.name}. ${description}. Be direct, preserve useful context, and challenge assumptions when a better path is visible.`;

  return {
    name: input.name,
    description,
    mission: description,
    values: [
      'Keep context useful',
      'Prefer concrete next steps',
      'Challenge weak assumptions',
    ],
    model: input.model,
    provider: input.provider,
    strategy: 'supervisor',
    orchestrator: {
      name: input.name,
      position: 'Agent',
      role: 'orchestrator',
      bio: description,
      lore: 'Created as a single-agent workspace with animaos create agent.',
      adjectives: ['focused', 'curious', 'direct'],
      topics: inferAgentTopics(description),
      knowledge: [`Primary mandate: ${description}`],
      style: 'Clear, practical, and concise. Calls out uncertainty instead of hiding it.',
      system,
      model: input.model,
      tools: [],
      collaboratesWith: [],
    },
    agents: [],
  };
}

function writeWorkspace(
  config: AgencyConfig,
  allAgentDefs: AgentDefinition[]
): WrittenWorkspace {
  const dirName = workspaceSlug(config.name);
  const dirPath = join(process.cwd(), dirName);

  if (!existsSync(dirPath)) {
    mkdirSync(dirPath, { recursive: true });
  }

  writeFileSync(
    join(dirPath, 'anima.yaml'),
    yaml.dump(config, { lineWidth: 120, noRefs: true })
  );

  writeFileSync(join(dirPath, 'org-chart.mmd'), renderOrgChart(config));
  writeFileSync(join(dirPath, 'README.md'), renderAgencyBrief(config));

  const agentsRoot = join(dirPath, 'agents');
  mkdirSync(agentsRoot, { recursive: true });

  for (const agent of allAgentDefs) {
    const slug = agentSlug(agent.name);
    const agentDir = join(agentsRoot, slug);
    mkdirSync(join(agentDir, 'assets'), { recursive: true });
    mkdirSync(join(agentDir, 'memory'), { recursive: true });
    writeFileSync(join(agentDir, 'profile.md'), renderAgentProfile(agent, config.name));
    writeFileSync(join(agentDir, 'assets', '.gitkeep'), '');
    writeFileSync(join(agentDir, 'memory', '.gitkeep'), '');
  }

  return {
    dirName,
    agentCount: allAgentDefs.length,
  };
}

function logWorkspaceResult(workspace: WrittenWorkspace) {
  clack.log.success(`Agent saved to ${pc.green(workspace.dirName + '/')}`);
  clack.log.message(
    `  ${pc.dim('•')} ${pc.green('anima.yaml')}     ${pc.dim('- runtime config')}\n` +
      `  ${pc.dim('•')} ${pc.green('org-chart.mmd')}  ${pc.dim('- mermaid diagram')}\n` +
      `  ${pc.dim('•')} ${pc.green('README.md')}      ${pc.dim(
        '- mission, values, roster'
      )}\n` +
      `  ${pc.dim('•')} ${pc.green('agents/')}        ${pc.dim(
        '- personnel folders (' + workspace.agentCount + ')'
      )}`
  );
  clack.outro(
    `Launch with: ${pc.cyan(`cd ${workspace.dirName} && animaos launch "your task"`)}`
  );
}

async function promptRequiredText(input: {
  message: string;
  placeholder: string;
  initialValue?: string;
}): Promise<string> {
  const value = await clack.text({
    message: input.message,
    placeholder: input.placeholder,
    initialValue: input.initialValue,
    validate: (candidate) => {
      if (!candidate.trim()) return 'Required';
      return undefined;
    },
  });

  if (clack.isCancel(value)) cancelAndExit();
  return String(value).trim();
}

function logResolvedProvider(provider: string, explicitProvider: string | undefined) {
  if (explicitProvider) return;

  if (provider === DETERMINISTIC_PROVIDER) {
    clack.log.info(
      `No remote provider credentials were found; using ${DETERMINISTIC_PROVIDER} local scaffolding.`
    );
    return;
  }

  clack.log.info(`Using configured provider "${provider}".`);
}

async function runCreateAgent(nameArg: string | undefined, opts: CreateOptions) {
  const name =
    nameArg ??
    (await promptRequiredText({
      message: 'Agent name:',
      placeholder: 'research-agent',
    }));
  const description =
    opts.description && opts.description.trim()
      ? opts.description.trim()
      : await promptRequiredText({
          message: 'What should this agent do?',
          placeholder: 'Research medical literature and summarize risks clearly',
        });
  const provider = resolveCreateProvider(opts.provider, opts.apiKey);
  logResolvedProvider(provider, opts.provider);

  if (
    normalizeProvider(opts.provider) &&
    providerRequiresApiKey(provider) &&
    !hasProviderApiKey(provider, opts.apiKey)
  ) {
    clack.log.warn(
      `Provider "${provider}" is not configured yet. The workspace was still created; launch will need ${
        providerKeyEnvNames(provider).join(', ') || 'provider credentials'
      } or --api-key.`
    );
  }

  const config = buildSingleAgentConfig({
    name,
    description,
    provider,
    model: opts.model,
    system: opts.system,
  });
  const allAgentDefs: AgentDefinition[] = [
    { ...config.orchestrator, role: 'orchestrator' },
  ];
  const workspace = writeWorkspace(config, allAgentDefs);
  logWorkspaceResult(workspace);
}

export async function executeCreateCommand(
  kindOrName: string | undefined,
  nameArg: string | undefined,
  opts: CreateOptions
): Promise<void> {
  clack.intro(pc.bgCyan(pc.black(' animaOS-SWARM ')));

  const target = resolveCreateTarget(kindOrName, nameArg, opts.kind);
  if (target.error) {
    clack.log.error(target.error);
    process.exit(1);
  }

  const providerIssue = getCreateProviderIssue(opts.provider);
  if (providerIssue) {
    clack.log.error(providerIssue);
    process.exit(1);
  }

  await runCreateAgent(target.nameArg, opts);
}

export const createCommand = new Command('create')
  .description('Create a new single-agent workspace')
  .argument('[kind]', 'What to create (agent)')
  .argument('[name]', 'Agent name')
  .option('--kind <kind>', 'What to create: agent')
  .option('-p, --provider <provider>', PROVIDER_HELP_TEXT)
  .option('-m, --model <model>', 'Model to use', DEFAULT_CREATE_MODEL)
  .option('-d, --description <description>', 'Agent description')
  .option('--api-key <key>', 'API key')
  .option('--system <prompt>', 'System prompt')
  .action(
    (kindOrName: string | undefined, nameArg: string | undefined, opts: CreateOptions) =>
      executeCreateCommand(kindOrName, nameArg, opts)
  );