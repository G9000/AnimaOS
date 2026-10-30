import { useResetOnChange } from '../hooks/useResetOnChange';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  BROWSER_TOOL_ACCESS,
  BACKGROUND_HELPER_TOOL_ACCESS,
  isBrowserTool,
  PROGRAMMATIC_TOOL_ACCESS,
  WORKFLOW_TOOL_ACCESS,
  WORKFLOW_TEMPLATE_TOOL_ACCESS,
  TOOL_RESULT_ACCESS,
  deriveAccessProfile,
  toolNamesForProfile,
  type AccessProfile,
} from '../lib/agent-access';
import type { AgentDetail } from '../lib/types';
import {
  MODEL_SUGGESTIONS,
  daemon,
  type AgentUpdateInput,
  type DaemonProvider,
} from '../lib/daemon-api';
import { AlertIcon } from './icons';
import { PenIcon } from './PenIcon';
import { ProfileVoice } from './ProfileVoice';
import { ChatGptConnection } from './ChatGptConnection';
import { ChatGptModelSelect } from './ChatGptModelSelect';
import { AgentProfileDetails } from './AgentProfileDetails';
import { AgentFilesPanel } from './AgentFilesPanel';
import { PersonalityEditor } from './personality/PersonalityEditor';
import { FallbackModels } from './models/FallbackModels';
import { DelegationSettings } from './models/DelegationSettings';
import { ProviderKeys } from './models/ProviderKeys';
import { SideModel } from './models/SideModel';
import { OrbStyleDialog } from './OrbStyleDialog';
import { ErrorBanner, SectionTitle, labelCls, primaryBtnCls } from './ui-bits';

/** The parts of the companion's own configuration. Each Settings page shows
 *  the ones it is about; one save covers whatever the page shows. */
export type SettingsSection = 'profile' | 'model' | 'access' | 'builtins';
type DangerAction = 'reset' | 'delete';
const ALL_SECTIONS: readonly SettingsSection[] = ['profile', 'model', 'access'];

const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];

const ACCESS_PROFILE_COPY: Record<
  AccessProfile,
  { label: string; summary: string; kicker?: string }
> = {
  observe: {
    label: 'Look only',
    summary: 'Reads files and todos. Changes nothing.',
  },
  collaborate: {
    label: 'Edit files',
    summary: 'Reads and edits files. Never runs commands.',
    kicker: 'RECOMMENDED',
  },
  operate: {
    label: 'Run commands',
    summary: 'Edits files, runs commands, uses apps and searches the web.',
    kicker: 'RULES STILL APPLY',
  },
};

const OWNER_NAME_LINE = /^The owner's name is (.+)\.$/m;

function ownerNameFromSystem(system: string | null | undefined): string {
  return system?.match(OWNER_NAME_LINE)?.[1]?.trim() ?? '';
}

function systemWithOwnerName(
  system: string | null | undefined,
  ownerName: string,
): string {
  const existing = system ?? '';
  const nextLine = `The owner's name is ${ownerName}.`;
  if (ownerName) {
    if (OWNER_NAME_LINE.test(existing)) {
      return existing.replace(OWNER_NAME_LINE, nextLine);
    }
    return `${nextLine}\n\n${existing || 'You are a helpful task agent.'}`;
  }

  const withoutOwnerName = existing
    .replace(/^The owner's name is .+\.(?:\r?\n){0,2}/m, '')
    .trim();
  return withoutOwnerName === 'You are a helpful task agent.'
    ? ''
    : withoutOwnerName;
}

function AbilityToggle({
  id,
  label,
  help,
  checked,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  help: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="settings-ability">
      <input
        id={`settings-${id}`}
        type="checkbox"
        aria-describedby={`settings-${id}-help`}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <div>
        <label htmlFor={`settings-${id}`}>{label}</label>
        <p id={`settings-${id}-help`}>{help}</p>
      </div>
    </div>
  );
}

const BUILT_IN_TOOL_GROUPS = [
  {
    id: 'files-documents',
    label: 'Files & documents',
    help: 'Read and update workspace files, tasks and supported documents.',
    tools: [
      'read_file', 'list_dir', 'glob', 'grep', 'write_file', 'edit_file',
      'multi_edit', 'todo_read', 'todo_write', 'read_document',
      'write_document', 'read_document_source',
    ],
  },
  {
    id: 'document-editing',
    label: 'Edit existing documents',
    help: 'Inspect and propose changes to supported documents; applying each change requires your review.',
    tools: ['inspect_office_document', 'propose_document_edit', 'apply_document_edit'],
  },
  {
    id: 'apps-terminals',
    label: 'Apps & terminals',
    help: 'Run commands, background work, apps and coding agents.',
    tools: [
      'bash', 'bg_start', 'bg_output', 'bg_stop', 'bg_list', 'bg_wait',
      'app_start', 'app_send', 'app_read', 'app_stop', 'app_list',
      'acp_start', 'acp_prompt', 'acp_read', 'acp_cancel', 'acp_stop',
      'acp_list', 'execute_code',
    ],
  },
  {
    id: 'research',
    label: 'Research',
    help: 'Search the web, your conversations and saved skills.',
    tools: [
      'web_search', 'web_fetch', 'search_conversations',
      'read_conversation_window', 'load_skill', 'skill_history',
    ],
  },
  {
    id: 'browser',
    label: 'Browser',
    help: 'Navigate, inspect pages, interact and transfer files.',
    tools: BROWSER_TOOL_ACCESS.map(({ name }) => name),
  },
  {
    id: 'memory',
    label: 'Memory',
    help: 'Find, save, update and manage remembered facts.',
    tools: [
      'memory_search', 'memory_add', 'recent_memories', 'memory_update',
      'memory_remove', 'memory_consolidate',
    ],
  },
  {
    id: 'team-coordination',
    label: 'Team coordination',
    help: 'Plan work and coordinate background helpers.',
    tools: [
      'propose_plan', 'spawn_helper_async', 'spawn_helpers',
      'wait_helper_runs', 'list_helper_runs', 'steer_helper', 'stop_helper',
    ],
  },
  {
    id: 'connected-work',
    label: 'Connected work',
    help: 'Use automations and saved workflows.',
    tools: [
      'list_automations', 'check_automation_status', 'create_automation',
      'pause_automation', 'complete_reminder', 'snooze_reminder',
      ...WORKFLOW_TOOL_ACCESS.tools, ...WORKFLOW_TEMPLATE_TOOL_ACCESS.tools,
    ],
  },
  {
    id: 'utilities',
    label: 'Utilities',
    help: 'Use time, calculations, clarifications and saved results.',
    tools: [
      'get_current_time', 'calculate', 'clarify', 'read_tool_result',
      'propose_skill', 'remove_skill', 'undo_skill',
    ],
  },
] as const;

type BuiltInToolGroupId = (typeof BUILT_IN_TOOL_GROUPS)[number]['id'];

/**
 * The companion's own settings, shown inside the Settings window. Saves
 * through PATCH /api/agents/:id: the conversation is kept and changes apply
 * on the next run.
 */
export function SettingsPanel({
  agent,
  providers,
  saving,
  resetting,
  saveError,
  resetError,
  saveSettings,
  resetAgent,
  deleteAgent = () => undefined,
  isMainAgent = agent.workspaceRole !== 'helper',
  refreshProviders,
  sections = ALL_SECTIONS,
  agents = [agent],
}: {
  agent: AgentDetail;
  agents?: readonly AgentDetail[];
  providers: DaemonProvider[] | null;
  saving: boolean;
  resetting: boolean;
  saveError: string | null;
  resetError: string | null;
  saveSettings: (patch: AgentUpdateInput) => Promise<boolean>;
  resetAgent: () => void;
  deleteAgent?: () => void;
  isMainAgent?: boolean;
  refreshProviders?: () => void | Promise<void>;
  sections?: readonly SettingsSection[];
}) {
  const seedModel = useMemo(() => {
    if (agent.provider === 'chatgpt') return agent.model;
    const options = MODEL_SUGGESTIONS[agent.provider] ?? [];
    return options.includes(agent.model) ? agent.model : '__custom__';
  }, [agent.provider, agent.model]);
  const agentToolsKey = JSON.stringify([...agent.toolNames].sort());
  const hasMemoryRemove = agent.toolNames.includes('memory_remove');
  const hasMemoryConsolidate = agent.toolNames.includes('memory_consolidate');
  const hasConversationRead = agent.toolNames.includes(
    'read_conversation_window',
  );
  const hasDocumentRead = agent.toolNames.includes('read_document');
  const hasWebSearch = agent.toolNames.includes('web_search');
  const hasWebFetch = agent.toolNames.includes('web_fetch');
  const hasToolChains = agent.toolNames.includes('execute_code');
  const hasWorkflows = WORKFLOW_TOOL_ACCESS.tools.every((tool) =>
    agent.toolNames.includes(tool),
  );
  const hasBackgroundHelpers = BACKGROUND_HELPER_TOOL_ACCESS.tools.every(
    (tool) => agent.toolNames.includes(tool),
  );
  const hasWorkflowTemplates = WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.every(
    (tool) => agent.toolNames.includes(tool),
  );
  const hasToolResultRead = agent.toolNames.includes(TOOL_RESULT_ACCESS.name);
  const derivedAccess = deriveAccessProfile([
    ...agent.toolNames.filter(
      (tool) =>
        tool !== 'memory_remove' &&
        tool !== 'memory_consolidate' &&
        !isBrowserTool(tool) &&
        tool !== 'read_conversation_window' &&
        tool !== 'read_document' &&
        (tool !== 'web_search' ||
          (agent.toolNames.includes('search_tools') &&
            agent.toolNames.includes('describe_tool'))) &&
        tool !== 'web_fetch' &&
        !WORKFLOW_TOOL_ACCESS.tools.some((name) => name === tool) &&
        !BACKGROUND_HELPER_TOOL_ACCESS.tools.some((name) => name === tool) &&
        !WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.some((name) => name === tool) &&
        tool !== TOOL_RESULT_ACCESS.name &&
        tool !== 'execute_code',
    ),
    'read_conversation_window',
  ]);

  const [name, setName] = useState(agent.name);
  const [bio, setBio] = useState(agent.bio ?? '');
  const [showInMainChat, setShowInMainChat] = useState(
    agent.showInMainChat ?? true,
  );
  const [ownerName, setOwnerName] = useState(() =>
    ownerNameFromSystem(agent.system),
  );
  const [ownerNameEditing, setOwnerNameEditing] = useState(false);
  const [personalityEditing, setPersonalityEditing] = useState(false);
  const [provider, setProvider] = useState(agent.provider);
  const [model, setModel] = useState(seedModel);
  const [customModel, setCustomModel] = useState(
    seedModel === '__custom__' ? agent.model : '',
  );
  const [modelScope, setModelScope] = useState({ revision: 0, pending: false });
  const [accessSelection, setAccessSelection] = useState<AccessProfile | null>(
    derivedAccess === 'custom' ? null : derivedAccess,
  );
  const [accessChanged, setAccessChanged] = useState(false);
  const [browserGrants, setBrowserGrants] = useState<string[]>(
    agent.toolNames.filter(isBrowserTool),
  );
  const [allowMemoryRemove, setAllowMemoryRemove] = useState(hasMemoryRemove);
  const [allowMemoryConsolidate, setAllowMemoryConsolidate] =
    useState(hasMemoryConsolidate);
  const [allowConversationRead, setAllowConversationRead] =
    useState(hasConversationRead);
  const [allowDocumentRead, setAllowDocumentRead] = useState(hasDocumentRead);
  const [allowWebSearch, setAllowWebSearch] = useState(hasWebSearch);
  const [allowWebFetch, setAllowWebFetch] = useState(hasWebFetch);
  const [allowToolChains, setAllowToolChains] = useState(hasToolChains);
  const [allowWorkflows, setAllowWorkflows] = useState(hasWorkflows);
  // A partial legacy grant stays unchanged until the owner uses this toggle.
  const [workflowsChanged, setWorkflowsChanged] = useState(false);
  const [allowBackgroundHelpers, setAllowBackgroundHelpers] =
    useState(hasBackgroundHelpers);
  const [backgroundHelpersChanged, setBackgroundHelpersChanged] =
    useState(false);
  const [allowWorkflowTemplates, setAllowWorkflowTemplates] =
    useState(hasWorkflowTemplates);
  const [templatesChanged, setTemplatesChanged] = useState(false);
  const [allowToolResultRead, setAllowToolResultRead] =
    useState(hasToolResultRead);
  const [toolGroupSelections, setToolGroupSelections] = useState<
    Partial<Record<BuiltInToolGroupId, boolean>>
  >({});
  const [toolChainStatus, setToolChainStatus] = useState('loading');
  const [savedFlash, setSavedFlash] = useState(false);
  const [orbStyleOpen, setOrbStyleOpen] = useState(false);
  // Which destructive action waits for confirmation, and which one ran last
  // (it shows Working… while the shell finishes it).
  const [confirmDanger, setConfirmDanger] = useState<DangerAction | null>(null);
  const [lastDanger, setLastDanger] = useState<DangerAction | null>(null);
  const saveErrorRef = useRef<HTMLDivElement>(null);
  const resetErrorRef = useRef<HTMLDivElement>(null);
  const controlsDisabled = saving || resetting;
  const mainAgent =
    agents.find((candidate) => candidate.workspaceRole !== 'helper') ?? agent;
  const helperMatchesMainModel =
    !isMainAgent &&
    agent.provider === mainAgent.provider &&
    agent.model === mainAgent.model;
  const shows = (section: SettingsSection) => sections.includes(section);
  useEffect(() => {
    if (!sections.includes('access') && !sections.includes('builtins')) return;
    let active = true;
    daemon
      .capabilities()
      .then((capabilities) => {
        if (active)
          setToolChainStatus(
            capabilities.programmaticTools?.status ?? 'unknown',
          );
      })
      .catch(() => {
        if (active) setToolChainStatus('unknown');
      });
    return () => {
      active = false;
    };
  }, [sections]);

  // Re-seed after an accepted agent update or a main-agent transition. Tool
  // order alone is not a config change and must not clobber an in-progress draft.
  useResetOnChange(
    [
      agent.id,
      agent.name,
      agent.bio,
      agent.showInMainChat,
      agent.system,
      agent.provider,
      agent.model,
      agentToolsKey,
      hasMemoryRemove,
      hasMemoryConsolidate,
      hasConversationRead,
      hasDocumentRead,
      hasWebSearch,
      hasWebFetch,
      hasToolChains,
      hasWorkflows,
      hasBackgroundHelpers,
      hasWorkflowTemplates,
      hasToolResultRead,
      derivedAccess,
    ],
    () => {
      const options = MODEL_SUGGESTIONS[agent.provider] ?? [];
      const seeded =
        agent.provider === 'chatgpt' || options.includes(agent.model)
          ? agent.model
          : '__custom__';
      setName(agent.name);
      setBio(agent.bio ?? '');
      setShowInMainChat(agent.showInMainChat ?? true);
      setOwnerName(ownerNameFromSystem(agent.system));
      setOwnerNameEditing(false);
      setProvider(agent.provider);
      setModel(seeded);
      setCustomModel(seeded === '__custom__' ? agent.model : '');
      setAccessSelection(derivedAccess === 'custom' ? null : derivedAccess);
      setAccessChanged(false);
      setBrowserGrants(agent.toolNames.filter(isBrowserTool));
      setAllowMemoryRemove(hasMemoryRemove);
      setAllowMemoryConsolidate(hasMemoryConsolidate);
      setAllowConversationRead(hasConversationRead);
      setAllowDocumentRead(hasDocumentRead);
      setAllowWebSearch(hasWebSearch);
      setAllowWebFetch(hasWebFetch);
      setAllowToolChains(hasToolChains);
      setAllowWorkflows(hasWorkflows);
      setWorkflowsChanged(false);
      setAllowBackgroundHelpers(hasBackgroundHelpers);
      setBackgroundHelpersChanged(false);
      setAllowWorkflowTemplates(hasWorkflowTemplates);
      setTemplatesChanged(false);
      setAllowToolResultRead(hasToolResultRead);
      setToolGroupSelections({});
    },
  );

  useEffect(() => {
    if (saveError) {
      saveErrorRef.current?.focus();
    }
  }, [saveError]);

  useEffect(() => {
    if (resetError) {
      resetErrorRef.current?.focus();
    }
  }, [resetError]);

  const resolvedModel = model === '__custom__' ? customModel.trim() : model;
  const accessDirty =
    accessChanged &&
    accessSelection !== null &&
    accessSelection !== derivedAccess;
  const toolsDirty =
    accessDirty ||
    BUILT_IN_TOOL_GROUPS.some((group) => {
      const selected = toolGroupSelections[group.id];
      if (selected === undefined) return false;
      return selected
        ? group.tools.some((tool) => !agent.toolNames.includes(tool))
        : group.tools.some((tool) => agent.toolNames.includes(tool));
    }) ||
    (workflowsChanged &&
      WORKFLOW_TOOL_ACCESS.tools.some(
        (tool) => agent.toolNames.includes(tool) !== allowWorkflows,
      )) ||
    (backgroundHelpersChanged &&
      BACKGROUND_HELPER_TOOL_ACCESS.tools.some(
        (tool) => agent.toolNames.includes(tool) !== allowBackgroundHelpers,
      )) ||
    (templatesChanged &&
      WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.some(
        (tool) => agent.toolNames.includes(tool) !== allowWorkflowTemplates,
      )) ||
    allowToolResultRead !== hasToolResultRead ||
    BROWSER_TOOL_ACCESS.some(
      ({ name }) =>
        browserGrants.includes(name) !== agent.toolNames.includes(name),
    ) ||
    allowMemoryRemove !== hasMemoryRemove ||
    allowMemoryConsolidate !== hasMemoryConsolidate ||
    allowConversationRead !== hasConversationRead ||
    allowDocumentRead !== hasDocumentRead ||
    allowWebSearch !== hasWebSearch ||
    allowWebFetch !== hasWebFetch ||
    allowToolChains !== hasToolChains;
  const dirty =
    name.trim() !== agent.name ||
    (!isMainAgent && bio.trim() !== (agent.bio ?? '')) ||
    (!isMainAgent && showInMainChat !== (agent.showInMainChat ?? true)) ||
    (isMainAgent && ownerName.trim() !== ownerNameFromSystem(agent.system)) ||
    provider !== agent.provider ||
    resolvedModel !== agent.model ||
    toolsDirty;

  const save = async () => {
    const patch: AgentUpdateInput = {};
    if (name.trim() && name.trim() !== agent.name) patch.name = name.trim();
    if (!isMainAgent && bio.trim() !== (agent.bio ?? ''))
      patch.bio = bio.trim();
    if (!isMainAgent && showInMainChat !== (agent.showInMainChat ?? true))
      patch.showInMainChat = showInMainChat;
    if (
      isMainAgent &&
      ownerName.trim() !== ownerNameFromSystem(agent.system)
    ) {
      patch.system = systemWithOwnerName(agent.system, ownerName.trim());
    }
    if (provider !== agent.provider) patch.provider = provider;
    if (resolvedModel && resolvedModel !== agent.model)
      patch.model = resolvedModel;
    if (toolsDirty) {
      let selectedTools =
        accessDirty && accessSelection
          ? toolNamesForProfile(accessSelection)
          : [...agent.toolNames];
      for (const group of BUILT_IN_TOOL_GROUPS) {
        const enabled = toolGroupSelections[group.id];
        if (enabled === undefined) {
          if (accessDirty && group.id === 'document-editing') {
            selectedTools = [...new Set([...selectedTools, ...group.tools.filter((tool) => agent.toolNames.includes(tool))])];
          }
          continue;
        }
        selectedTools = enabled
          ? [...new Set([...selectedTools, ...group.tools])]
          : selectedTools.filter(
              (tool) => !group.tools.some((name) => name === tool),
            );
      }
      patch.tools = selectedTools.filter(
        (tool) =>
          (tool !== 'memory_remove' || allowMemoryRemove) &&
          !isBrowserTool(tool) &&
          (tool !== 'memory_consolidate' || allowMemoryConsolidate) &&
          (tool !== 'read_conversation_window' || allowConversationRead) &&
          (tool !== 'read_document' || allowDocumentRead) &&
          (tool !== 'web_search' || allowWebSearch) &&
          (tool !== 'web_fetch' || allowWebFetch) &&
          (!workflowsChanged ||
            allowWorkflows ||
            !WORKFLOW_TOOL_ACCESS.tools.some((name) => name === tool)) &&
          (!backgroundHelpersChanged ||
            allowBackgroundHelpers ||
            !BACKGROUND_HELPER_TOOL_ACCESS.tools.some(
              (name) => name === tool,
            )) &&
          (!templatesChanged ||
            allowWorkflowTemplates ||
            !WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.some(
              (name) => name === tool,
            )) &&
          (tool !== TOOL_RESULT_ACCESS.name || allowToolResultRead) &&
          (tool !== 'execute_code' || allowToolChains),
      );
      if (allowMemoryRemove && !patch.tools.includes('memory_remove')) {
        patch.tools.push('memory_remove');
      }
      if (
        allowMemoryConsolidate &&
        !patch.tools.includes('memory_consolidate')
      ) {
        patch.tools.push('memory_consolidate');
      }
      if (
        allowConversationRead &&
        !patch.tools.includes('read_conversation_window')
      ) {
        patch.tools.push('read_conversation_window');
      }
      if (allowDocumentRead && !patch.tools.includes('read_document')) {
        patch.tools.push('read_document');
      }
      if (allowWebSearch && !patch.tools.includes('web_search')) {
        patch.tools.push('web_search');
      }
      if (allowWebFetch && !patch.tools.includes('web_fetch')) {
        patch.tools.push('web_fetch');
      }
      if (allowToolChains && !patch.tools.includes('execute_code')) {
        patch.tools.push('execute_code');
      }
      for (const tool of WORKFLOW_TOOL_ACCESS.tools) {
        if (
          (allowWorkflows ||
            (!workflowsChanged && agent.toolNames.includes(tool))) &&
          !patch.tools.includes(tool)
        ) {
          patch.tools.push(tool);
        }
      }
      for (const tool of BACKGROUND_HELPER_TOOL_ACCESS.tools) {
        if (
          (allowBackgroundHelpers ||
            (!backgroundHelpersChanged && agent.toolNames.includes(tool))) &&
          !patch.tools.includes(tool)
        )
          patch.tools.push(tool);
      }
      for (const tool of WORKFLOW_TEMPLATE_TOOL_ACCESS.tools) {
        if (
          (allowWorkflowTemplates ||
            (!templatesChanged && agent.toolNames.includes(tool))) &&
          !patch.tools.includes(tool)
        )
          patch.tools.push(tool);
      }
      if (allowToolResultRead && !patch.tools.includes(TOOL_RESULT_ACCESS.name))
        patch.tools.push(TOOL_RESULT_ACCESS.name);
      patch.tools.push(
        ...BROWSER_TOOL_ACCESS.filter(({ name }) =>
          browserGrants.includes(name),
        ).map(({ name }) => name),
      );
    }
    if (Object.keys(patch).length === 0) return;
    // The access profile only picks tools; Autonomy decides when it asks.
    if (await saveSettings(patch)) {
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 2000);
    }
  };

  // A main agent can be reset (it keeps its chats and files) or deleted; a
  // helper can only be deleted.
  const dangerActions: readonly {
    id: DangerAction;
    icon: 'arrow-counter-clockwise-fill' | 'x-circle-fill';
    title: string;
    help: string;
    open: string;
    confirmTitle: string;
    confirmCopy: string;
    confirm: string;
    run: () => void;
  }[] = [
    ...(isMainAgent
      ? [
          {
            id: 'reset' as const,
            icon: 'arrow-counter-clockwise-fill' as const,
            title: `Reset ${agent.name}`,
            help: 'Clears personality and memory. Files stay.',
            open: 'Reset…',
            confirmTitle: `Reset ${agent.name}?`,
            confirmCopy:
              'This clears its personality and agent memory, then restores the Balanced personality. Its files and chats stay.',
            confirm: 'Reset',
            run: resetAgent,
          },
        ]
      : []),
    {
      id: 'delete',
      icon: 'x-circle-fill',
      title: `Delete ${agent.name}`,
      help: `Removes ${agent.name}, its chats and memories. Other agents stay.`,
      open: 'Delete…',
      confirmTitle: `Delete ${agent.name}?`,
      confirmCopy:
        'This permanently removes the agent and its conversations. This cannot be undone.',
      confirm: 'Delete agent',
      run: deleteAgent,
    },
  ];
  const confirming =
    dangerActions.find((action) => action.id === confirmDanger) ?? null;

  const abilities = [
    {
      id: 'background-helpers',
      label: BACKGROUND_HELPER_TOOL_ACCESS.label,
      checked: allowBackgroundHelpers,
      setChecked: (checked: boolean) => {
        setAllowBackgroundHelpers(checked);
        setBackgroundHelpersChanged(true);
      },
      disabled: agent.workspaceRole === 'helper',
      help: 'Allows this agent to launch focused background helpers and collect, steer or stop their work. Helpers stay within existing tool permissions and approval rules.',
    },
    {
      id: 'workflows',
      label: WORKFLOW_TOOL_ACCESS.label,
      checked: allowWorkflows,
      setChecked: (checked: boolean) => {
        setAllowWorkflows(checked);
        setWorkflowsChanged(true);
      },
      disabled: agent.workspaceRole === 'helper',
      help: 'Allows this agent to create and list workflows from chat. Existing tool permissions and approval rules still apply when a workflow runs.',
    },
    {
      id: 'workflow-templates',
      label: WORKFLOW_TEMPLATE_TOOL_ACCESS.label,
      checked: allowWorkflowTemplates,
      setChecked: (checked: boolean) => {
        setAllowWorkflowTemplates(checked);
        setTemplatesChanged(true);
      },
      disabled: agent.workspaceRole === 'helper',
      help: 'Allows saving, listing, and starting reusable workflow templates from chat. Every start creates a separate run; owner checkpoints and tool permissions still apply.',
    },
    {
      id: 'tool-result-read',
      label: TOOL_RESULT_ACCESS.label,
      checked: allowToolResultRead,
      setChecked: setAllowToolResultRead,
      help: 'Allows bounded reads of this agent’s saved tool results in the current workspace. Results remain local and are treated as reference data.',
    },
    {
      id: 'tool-chains',
      label: PROGRAMMATIC_TOOL_ACCESS.label,
      checked: allowToolChains,
      setChecked: setAllowToolChains,
      disabled:
        toolChainStatus === 'unsupported' ||
        agent.workspaceRole === 'helper' ||
        !(
          accessDirty && accessSelection
            ? toolNamesForProfile(accessSelection)
            : agent.toolNames
        ).includes(PROGRAMMATIC_TOOL_ACCESS.requires),
      help: `${toolChainStatus === 'configured' ? 'One-shot Python is configured; each execution still verifies setup.' : toolChainStatus === 'unsupported' ? 'This host platform is not supported yet.' : toolChainStatus === 'unconfigured' ? 'Python is not configured.' : 'Interpreter configuration has not been confirmed.'} Set ANIMAOS_RS_TOOL_RPC_PYTHON_PATH to an absolute Python interpreter path and restart the daemon. Requires shell access. Python has this computer user’s operating-system access; native calls retain their approvals and receipts.`,
    },
    {
      id: 'web-search',
      label: 'Allow web search',
      checked: allowWebSearch,
      setChecked: setAllowWebSearch,
      help: "Sends the query to Exa's anonymous public service, which may be rate-limited. Existing Network approval rules still apply.",
    },
    {
      id: 'web-fetch',
      label: 'Allow web page reads',
      checked: allowWebFetch,
      setChecked: setAllowWebFetch,
      help: 'Sends the selected URL to the public site. Existing Network approval rules still apply.',
    },
    {
      id: 'document-read',
      label: 'Allow reading workspace documents',
      checked: allowDocumentRead,
      setChecked: setAllowDocumentRead,
      help: 'Allows this agent to read a selected DOCX, PDF, or XLSX as bounded untrusted content. The original file is not uploaded.',
    },
    {
      id: 'conversation-read',
      label: 'Allow reading conversation context',
      checked: allowConversationRead,
      setChecked: setAllowConversationRead,
      disabled: agent.workspaceRole === 'helper',
      help: 'Allows this agent to search your own past conversations and read a bounded window around a selected message. Past text is evidence, not current instructions.',
    },
    {
      id: 'memory-remove',
      label: 'Allow removing saved facts',
      checked: allowMemoryRemove,
      setChecked: setAllowMemoryRemove,
      help: 'Allows permanent removal of one active Fact only with its exact current text. Chats and backups remain; existing approval rules still apply.',
    },
    {
      id: 'memory-consolidate',
      label: 'Allow consolidating saved facts',
      checked: allowMemoryConsolidate,
      setChecked: setAllowMemoryConsolidate,
      help: 'Allows saving one summary from 2–8 exact current facts and keeping the originals in memory history. Original text and citations remain available; existing approval rules still apply.',
    },
  ];

  const setBuiltInToolGroup = (
    id: BuiltInToolGroupId,
    enabled: boolean,
  ) => {
    setToolGroupSelections((current) => ({ ...current, [id]: enabled }));
    switch (id) {
      case 'files-documents':
        setAllowDocumentRead(enabled);
        break;
      case 'apps-terminals':
        setAllowToolChains(enabled && toolChainStatus !== 'unsupported');
        break;
      case 'research':
        setAllowWebSearch(enabled);
        setAllowWebFetch(enabled);
        setAllowConversationRead(enabled);
        break;
      case 'browser':
        setBrowserGrants(enabled ? BROWSER_TOOL_ACCESS.map(({ name }) => name) : []);
        break;
      case 'memory':
        setAllowMemoryRemove(enabled);
        setAllowMemoryConsolidate(enabled);
        break;
      case 'team-coordination':
        setAllowBackgroundHelpers(enabled);
        setBackgroundHelpersChanged(true);
        break;
      case 'connected-work':
        setAllowWorkflows(enabled);
        setWorkflowsChanged(true);
        setAllowWorkflowTemplates(enabled);
        setTemplatesChanged(true);
        break;
      case 'utilities':
        setAllowToolResultRead(enabled);
        break;
    }
  };

  const isBuiltInToolEnabled = (tool: string) => {
    if (isBrowserTool(tool)) return browserGrants.includes(tool);
    if (tool === 'memory_remove') return allowMemoryRemove;
    if (tool === 'memory_consolidate') return allowMemoryConsolidate;
    if (tool === 'read_conversation_window') return allowConversationRead;
    if (tool === 'read_document') return allowDocumentRead;
    if (tool === 'web_search') return allowWebSearch;
    if (tool === 'web_fetch') return allowWebFetch;
    if (tool === 'execute_code') return allowToolChains;
    if (WORKFLOW_TOOL_ACCESS.tools.some((name) => name === tool))
      return workflowsChanged ? allowWorkflows : agent.toolNames.includes(tool);
    if (BACKGROUND_HELPER_TOOL_ACCESS.tools.some((name) => name === tool))
      return backgroundHelpersChanged
        ? allowBackgroundHelpers
        : agent.toolNames.includes(tool);
    if (WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.some((name) => name === tool))
      return templatesChanged
        ? allowWorkflowTemplates
        : agent.toolNames.includes(tool);
    if (tool === TOOL_RESULT_ACCESS.name) return allowToolResultRead;
    const selectedGroup = BUILT_IN_TOOL_GROUPS.find((group) =>
      group.tools.some((name) => name === tool),
    );
    const groupSelection = selectedGroup
      ? toolGroupSelections[selectedGroup.id]
      : undefined;
    if (groupSelection !== undefined) return groupSelection;
    return agent.toolNames.includes(tool);
  };

  const builtInToolGroups = BUILT_IN_TOOL_GROUPS.map((group) => {
    const enabledCount = group.tools.filter(isBuiltInToolEnabled).length;
    return {
      ...group,
      enabledCount,
      enabled: enabledCount === group.tools.length,
      mixed: enabledCount > 0 && enabledCount < group.tools.length,
    };
  });

  return (
    <div
      className={`settings-form${shows('profile') ? ' settings-profile' : ''}${shows('model') ? ' settings-model-page' : ''}`}
      aria-busy={controlsDisabled}
    >
      {shows('profile') && (
        personalityEditing ? (
          <div className="settings-profile-editor-view">
            <PersonalityEditor
              key={agent.id + '-soul'}
              agentId={agent.id}
              disabled={controlsDisabled}
              initialScreen="editor"
              onBackToProfile={() => setPersonalityEditing(false)}
            />
            <details className="settings-advanced settings-profile-files">
              <summary>Instruction files</summary>
              <AgentFilesPanel agentId={agent.id} />
            </details>
          </div>
        ) : (
        <>
          <section
            className="settings-group"
            aria-labelledby="settings-identity"
          >
            <h3 id="settings-identity" className="settings-group-title">
              Identity
            </h3>
            <div className="settings-list">
              <AgentProfileDetails
                key={agent.id}
                agent={agent}
                disabled={controlsDisabled}
                editing={ownerNameEditing}
              >
                <div className="profile-identity-row">
                  {ownerNameEditing ? (
                    <div className="profile-identity-edit-fields">
                      <div>
                        <label className={labelCls} htmlFor="settings-name">
                          Name
                        </label>
                        <input
                          id="settings-name"
                          value={name}
                          onChange={(event) => setName(event.target.value)}
                          disabled={controlsDisabled}
                          className="field"
                        />
                      </div>
                      {isMainAgent ? (
                        <div>
                          <label
                            className={labelCls}
                            htmlFor="settings-owner-name"
                          >
                            Calls you
                          </label>
                          <input
                            id="settings-owner-name"
                            value={ownerName}
                            maxLength={80}
                            autoComplete="given-name"
                            onChange={(event) =>
                              setOwnerName(event.target.value)
                            }
                            disabled={controlsDisabled}
                            className="field"
                          />
                        </div>
                      ) : (
                        <div>
                          <label
                            className={labelCls}
                            htmlFor="settings-agent-purpose"
                          >
                            Purpose
                          </label>
                          <textarea
                            id="settings-agent-purpose"
                            className="field"
                            rows={2}
                            maxLength={500}
                            value={bio}
                            placeholder="What this agent is for"
                            onChange={(event) => setBio(event.target.value)}
                            disabled={controlsDisabled}
                          />
                        </div>
                      )}
                      <button
                        type="button"
                        className="profile-orb-style-action"
                        onClick={() => setOrbStyleOpen(true)}
                        disabled={controlsDisabled}
                      >
                        Orb style
                      </button>
                    </div>
                  ) : (
                    <div className="profile-identity-summary">
                      <strong>{name || agent.name}</strong>
                      <small>
                        {isMainAgent
                          ? 'Calls you ' + (ownerName || 'Not set')
                          : bio || 'Helper agent'}
                      </small>
                    </div>
                  )}
                  <button
                    type="button"
                    className="settings-item-action"
                    aria-expanded={ownerNameEditing}
                    disabled={controlsDisabled}
                    onClick={() =>
                      setOwnerNameEditing((editing) => !editing)
                    }
                  >
                    {ownerNameEditing
                      ? 'Done'
                      : isMainAgent
                        ? 'Change'
                        : 'Rename'}
                  </button>
                </div>
              </AgentProfileDetails>
              <PersonalityEditor
                key={agent.id + '-personality'}
                agentId={agent.id}
                disabled={controlsDisabled}
                compactProfile
              />
            </div>
          </section>
          {!shows('model') && (
            <section
              className="settings-group"
              aria-labelledby="profile-model-title"
            >
              <h3 id="profile-model-title" className="settings-group-title">
                {isMainAgent ? 'Model and voice' : 'Model and access'}
              </h3>
              <div className="settings-list">
                <div className="settings-item">
                  <span
                    className="settings-item-icon profile-model-icon"
                    aria-hidden="true"
                  >
                    <PenIcon name="cpu-fill" size={16} />
                  </span>
                  <div className="settings-item-text">
                    <strong>
                      {helperMatchesMainModel
                        ? `Same as ${mainAgent.name}`
                        : agent.model || 'No model chosen'}
                    </strong>
                    <small>
                      {!isMainAgent && (
                        <>
                          {agent.model || 'No model chosen'}
                          {' · '}
                        </>
                      )}
                      {helperMatchesMainModel
                        ? `matches ${mainAgent.name}'s current model`
                        : agent.provider === 'chatgpt'
                          ? 'ChatGPT plan'
                          : (providers?.find(
                              (item) => item.id === agent.provider,
                            )?.label ?? agent.provider)}
                    </small>
                  </div>
                  <a
                    className="settings-item-action profile-model-action"
                    href="#/model"
                    aria-disabled={controlsDisabled || dirty || undefined}
                    title={
                      dirty
                        ? 'Save changes before switching models.'
                        : undefined
                    }
                    onClick={(event) => {
                      if (controlsDisabled || dirty) event.preventDefault();
                    }}
                  >
                    <PenIcon name="caret-down-fill" size={10} />
                    Change
                  </a>
                </div>
                <ProfileVoice
                  agentName={agent.name}
                  inheritedFrom={!isMainAgent ? mainAgent.name : undefined}
                />
              </div>
            </section>
          )}
          {!isMainAgent && (
            <>
              <section
                className="settings-group settings-helper-capabilities"
                aria-labelledby="settings-helper-capabilities-title"
              >
                <h3
                  id="settings-helper-capabilities-title"
                  className="settings-group-title"
                >
                  What it can do
                </h3>
                <p className="settings-group-help">
                  Choose the helper’s default access. Its permission rules still
                  apply.
                </p>
                <fieldset
                  disabled={controlsDisabled}
                  className="settings-fieldset"
                >
                  <div className="settings-access-options">
                    {ACCESS_ORDER.map((profileName) => {
                      const profile = ACCESS_PROFILE_COPY[profileName];
                      const inputId = `settings-helper-access-${profileName}`;
                      return (
                        <div
                          key={profileName}
                          className={`settings-access-option${accessSelection === profileName ? ' is-selected' : ''}`}
                        >
                          <input
                            id={inputId}
                            type="radio"
                            name="settings-helper-access"
                            value={profileName}
                            checked={accessSelection === profileName}
                            onChange={() => {
                              setAccessSelection(profileName);
                              setAccessChanged(true);
                              setAllowMemoryRemove(false);
                              setAllowMemoryConsolidate(false);
                              setAllowDocumentRead(false);
                              setAllowWebSearch(profileName === 'operate');
                              setAllowWebFetch(false);
                              setAllowToolChains(false);
                              setBrowserGrants([]);
                            }}
                          />
                          <label htmlFor={inputId}>
                            <span className="settings-access-name">
                              {profile.label}
                            </span>
                            <span className="settings-access-summary">
                              {profile.summary}
                            </span>
                            {profile.kicker && (
                              <span className="settings-access-kicker">
                                {profile.kicker}
                              </span>
                            )}
                          </label>
                        </div>
                      );
                    })}
                  </div>
                </fieldset>
              </section>
              <label className="settings-item settings-helper-main-chat">
                <span className="settings-item-text">
                  <strong>Shows up in the main chat</strong>
                  <small>
                    Posts results to your main chat as well as its own
                  </small>
                </span>
                <input
                  type="checkbox"
                  role="switch"
                  aria-label="Shows up in the main chat"
                  checked={showInMainChat}
                  disabled={controlsDisabled}
                  onChange={(event) => setShowInMainChat(event.target.checked)}
                />
              </label>
            </>
          )}
          <section
            className="settings-group"
            aria-labelledby="settings-advanced-title"
          >
            <h3 id="settings-advanced-title" className="settings-group-title">
              Advanced
            </h3>
            <div className="settings-list">
              <button
                type="button"
                className="settings-item profile-soul-open"
                onClick={() => setPersonalityEditing(true)}
              >
                <span className="settings-item-icon" aria-hidden="true">
                  <PenIcon name="file-text-fill" size={16} />
                </span>
                <span className="settings-item-text">
                  <strong>SOUL.md</strong>
                  <small>
                    The personality file {agent.name} reads before every reply
                  </small>
                </span>
                <span className="settings-item-action">Edit</span>
              </button>
              {dangerActions.map((action) => (
                <section
                  key={action.id}
                  className="settings-item settings-danger"
                  aria-labelledby={`settings-${action.id}`}
                >
                  <span
                    className="settings-item-icon profile-reset-icon"
                    aria-hidden="true"
                  >
                    <PenIcon name={action.icon} size={16} />
                  </span>
                  <div className="settings-item-text">
                    <h3 id={`settings-${action.id}`}>{action.title}</h3>
                    <p>{action.help}</p>
                  </div>
                  <button
                    type="button"
                    onClick={() => setConfirmDanger(action.id)}
                    aria-describedby={
                      resetError ? 'settings-reset-error' : undefined
                    }
                    disabled={resetting || saving}
                  >
                    {resetting &&
                    (lastDanger ?? dangerActions[0].id) === action.id
                      ? 'Working…'
                      : action.open}
                  </button>
                </section>
              ))}
              {confirming && (
                <div
                  className="settings-danger-confirm"
                  role="alertdialog"
                  aria-labelledby="settings-danger-confirm-title"
                  aria-describedby="settings-danger-confirm-copy"
                >
                  <div>
                    <strong id="settings-danger-confirm-title">
                      {confirming.confirmTitle}
                    </strong>
                    <p id="settings-danger-confirm-copy">
                      {confirming.confirmCopy}
                    </p>
                  </div>
                  <button type="button" onClick={() => setConfirmDanger(null)} disabled={resetting}>
                    Cancel
                  </button>
                  <button
                    type="button"
                    disabled={resetting || saving}
                    onClick={() => {
                      setConfirmDanger(null);
                      setLastDanger(confirming.id);
                      confirming.run();
                    }}
                  >
                    {confirming.confirm}
                  </button>
                </div>
              )}
            </div>
          </section>
        </>
        )
      )}

      {shows('model') && (
        <section
          className="settings-group model-section"
          aria-labelledby="settings-model"
        >
          <h3 id="settings-model" className="settings-group-title">
            Main model
          </h3>
          <p className="settings-group-help">
            Used for every reply unless a task asks for something stronger.
          </p>
          <div className="settings-model-block">
            <div className="settings-model-summary" data-testid="main-model">
              <span className="settings-model-mark" aria-hidden="true">
                <PenIcon name="cpu-fill" size={16} />
              </span>
              <div className="settings-model-text">
                <strong>
                  {(model === '__custom__' ? customModel : model).trim() ||
                    agent.model ||
                    'No model chosen'}
                </strong>
                <span>
                  {provider === 'chatgpt'
                    ? 'ChatGPT plan'
                    : (providers?.find((p) => p.id === provider)?.label ??
                      provider)}
                  {providers
                    ? providers.find((p) => p.id === provider)?.configured
                      ? ' · ready'
                      : ' · not connected'
                    : ''}
                </span>
              </div>
            </div>
            <details className="settings-model-more">
              <summary>Change</summary>
              <div className="settings-model-change">
                <label className={labelCls} htmlFor="settings-provider">
                  Provider
                </label>
                <select
                  id="settings-provider"
                  aria-label="Provider"
                  value={provider}
                  disabled={controlsDisabled}
                  onChange={(e) => {
                    const next = e.target.value;
                    setProvider(next);
                    const options = MODEL_SUGGESTIONS[next] ?? [];
                    setModel(
                      next === 'chatgpt' ? '' : (options[0] ?? '__custom__'),
                    );
                  }}
                  className="field"
                >
                  {/* Keep the current provider selectable even if the daemon
              catalog is still loading or lacks it. */}
                  {(providers ?? [])
                    .map((p) => p.id)
                    .concat(
                      providers?.some((p) => p.id === agent.provider) ||
                        !agent.provider
                        ? []
                        : [agent.provider],
                    )
                    .filter((id, i, all) => all.indexOf(id) === i)
                    .map((id) => (
                      <option key={id} value={id}>
                        {id === 'chatgpt'
                          ? 'ChatGPT plan'
                          : (providers?.find((p) => p.id === id)?.label ?? id)}
                      </option>
                    ))}
                </select>
                {providers && provider !== 'chatgpt' && (
                  <p className="settings-field-note">
                    {providers.find((p) => p.id === provider)?.configured
                      ? 'Key configured on the server.'
                      : 'No key configured for this provider on the server.'}
                  </p>
                )}
              </div>
              {provider === 'chatgpt' ? (
                <ChatGptModelSelect
                  id="settings-chatgpt-model"
                  scopeKey={JSON.stringify([agent.id, modelScope.revision])}
                  connectionPending={modelScope.pending}
                  model={model}
                  customModel={customModel}
                  onModelChange={setModel}
                  onCustomModelChange={setCustomModel}
                  disabled={controlsDisabled}
                />
              ) : (
                <>
                  <div>
                    <label className={labelCls} htmlFor="settings-model-id">
                      Model
                    </label>
                    <select
                      id="settings-model-id"
                      aria-label="Model"
                      value={model}
                      onChange={(e) => setModel(e.target.value)}
                      disabled={controlsDisabled}
                      className="field"
                    >
                      {(MODEL_SUGGESTIONS[provider] ?? []).map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                      <option value="__custom__">custom…</option>
                    </select>
                  </div>
                  {model === '__custom__' && (
                    <input
                      aria-label="Custom model"
                      value={customModel}
                      onChange={(e) => setCustomModel(e.target.value)}
                      disabled={controlsDisabled}
                      placeholder="model id, e.g. llama3.1"
                      className="field animate-fade-in"
                    />
                  )}
                </>
              )}
              {provider === 'chatgpt' && (
                <ChatGptConnection
                  key={agent.id}
                  onConnectionChange={refreshProviders}
                  onModelScopeChange={(pending) =>
                    setModelScope((current) => ({
                      revision: current.revision + 1,
                      pending,
                    }))
                  }
                />
              )}
            </details>
          </div>
          <FallbackModels
            key={'fallbacks:' + agent.id}
            primary={{ provider: agent.provider, model: agent.model }}
            fallbacks={agent.fallbacks ?? []}
            providers={providers}
            agentName={agent.name}
            disabled={controlsDisabled}
            save={(fallbacks) => saveSettings({ fallbacks })}
          />
          <DelegationSettings
            key={'delegation:' + agent.id}
            primary={{ provider: agent.provider, model: agent.model }}
            policy={agent.delegation}
            providers={providers}
            disabled={controlsDisabled}
            save={(delegation) => saveSettings({ settings: { delegation } })}
          />
        </section>
      )}

      {shows('model') && (
        <>
          <ProviderKeys
            providers={providers}
            onChanged={refreshProviders}
            disabled={controlsDisabled}
          />
          <SideModel providers={providers} disabled={controlsDisabled} />
        </>
      )}

      {(shows('access') || shows('builtins')) && (
        <section className="settings-group" aria-labelledby="settings-access">
          <h3 id="settings-access" className="settings-group-title">
            {shows('builtins') ? 'Built-in' : 'Default access'}
          </h3>
          {shows('access') && (
            <p className="settings-group-help">
              Applies to {agent.name} and every helper it brings in.
            </p>
          )}
          {shows('builtins') && (
            <p className="settings-group-help">
              Turn built-in tools on or off for this agent. Permission rules
              still decide when an action asks first.
            </p>
          )}
          {shows('builtins') && agent.toolNames.some((name) =>
            /^mcp_[0-9a-f]{32}_[0-9a-f]{16}$/.test(name),
          ) && (
            <p className="settings-group-help">
              External tool permissions are managed in Connectors. Choosing a
              standard access profile replaces these permissions when you save;
              saved connections and reviews are kept.
            </p>
          )}
          {shows('access') && derivedAccess === 'custom' ? (
            <div className="settings-custom-access">
              <p className="font-semibold text-ink">Custom access</p>
              <p>
                This agent has a custom tool set. Choose a standard profile to
                replace it when you save.
              </p>
              <p className="settings-field-note">
                {agent.toolNames.length} tools configured
              </p>
              <p className="settings-field-note break-words">
                {agent.toolNames.join(', ') || 'No tools configured'}
              </p>
            </div>
          ) : null}
          <fieldset disabled={controlsDisabled} className="settings-fieldset">
            {shows('access') && (
              <>
                <legend className={labelCls}>Access profile</legend>
                <div className="settings-access-options">
              {ACCESS_ORDER.map((profileName) => {
                const profile = ACCESS_PROFILE_COPY[profileName];
                const inputId = `settings-access-${profileName}`;
                return (
                  <div
                    key={profileName}
                    className={`settings-access-option${accessSelection === profileName ? ' is-selected' : ''}`}
                  >
                    <input
                      id={inputId}
                      type="radio"
                      name="settings-access"
                      value={profileName}
                      checked={accessSelection === profileName}
                      onChange={() => {
                        setAccessSelection(profileName);
                        setAccessChanged(true);
                        setAllowMemoryRemove(false);
                        setAllowMemoryConsolidate(false);
                        setAllowDocumentRead(false);
                        setAllowWebSearch(profileName === 'operate');
                        setAllowWebFetch(false);
                        setAllowToolChains(false);
                        setBrowserGrants([]);
                      }}
                    />
                    <label htmlFor={inputId}>
                      <span className="settings-access-name">
                        {profile.label}
                      </span>
                      <span className="settings-access-summary">
                        {profile.summary}
                      </span>
                    </label>
                    {profile.kicker && (
                      <span
                        className={`settings-access-kicker${profileName === 'collaborate' ? ' is-recommended' : ''}`}
                      >
                        {profile.kicker}
                      </span>
                    )}
                  </div>
                );
              })}
                </div>
              </>
            )}
            {shows('access') && accessSelection === 'operate' && (
              <p className="settings-access-note">
                <span>
                  Helpers get the same access as {agent.name}. Commands run in
                  a Docker sandbox.
                </span>
                <a
                  className="settings-access-note-link"
                  href="#/runtime"
                >
                  Change in Runtime
                </a>
              </p>
            )}
            {shows('builtins') && (
              <div className="settings-tool-groups">
                {builtInToolGroups.map((group) => (
                  <label
                    key={group.id}
                    className="settings-tool-group"
                    data-mixed={group.mixed || undefined}
                  >
                    <span className="settings-tool-group-copy">
                      <strong>{group.label}</strong>
                      <small>
                        {group.enabledCount} of {group.tools.length} tools on
                      </small>
                      <span>{group.help}</span>
                    </span>
                    <input
                      type="checkbox"
                      role="switch"
                      aria-label={group.label}
                      aria-checked={group.mixed ? 'mixed' : group.enabled}
                      checked={group.enabled}
                      disabled={controlsDisabled || (group.id === 'document-editing' && agent.workspaceRole === 'helper')}
                      onChange={(event) =>
                        setBuiltInToolGroup(group.id, event.target.checked)
                      }
                    />
                  </label>
                ))}
              </div>
            )}
            {shows('builtins') && (
              <details className="settings-tools-advanced">
                <summary>Specific permissions</summary>
                <SectionTitle>Individual capabilities</SectionTitle>
                <div className="settings-abilities">
                  {abilities.map(
                    ({ id, label, checked, setChecked, help, disabled }) => (
                      <AbilityToggle
                        key={id}
                        id={id}
                        label={label}
                        help={help}
                        checked={checked}
                        disabled={disabled}
                        onChange={setChecked}
                      />
                    ),
                  )}
                </div>
                <SectionTitle>Browser tools</SectionTitle>
                <div className="settings-ability is-list">
                  <p>
                    Each chat task also needs an installed browser policy. Existing
                    action approval rules still apply.
                  </p>
                  {BROWSER_TOOL_ACCESS.map(({ name, label }) => (
                    <label key={name}>
                      <input
                        type="checkbox"
                        checked={browserGrants.includes(name)}
                        onChange={(event) =>
                          setBrowserGrants((current) =>
                            event.target.checked
                              ? [...current.filter((tool) => tool !== name), name]
                              : current.filter((tool) => tool !== name),
                          )
                        }
                      />
                      {label}
                    </label>
                  ))}
                </div>
              </details>
            )}
          </fieldset>
        </section>
      )}

      <div className={`settings-save${dirty && !saving ? ' is-dirty' : ''}`}>
        {saveError ? (
          <div
            id="settings-save-error"
            ref={saveErrorRef}
            role="alert"
            aria-live="assertive"
            aria-atomic="true"
            tabIndex={-1}
          >
            <ErrorBanner message={saveError} icon={<AlertIcon size={14} />} />
          </div>
        ) : null}
        {resetError ? (
          <div
            id="settings-reset-error"
            ref={resetErrorRef}
            role="alert"
            aria-live="assertive"
            aria-atomic="true"
            tabIndex={-1}
          >
            <ErrorBanner message={resetError} icon={<AlertIcon size={14} />} />
          </div>
        ) : null}
        {resetting ? (
          <p
            role="status"
            aria-live="polite"
            aria-atomic="true"
            className="sr-only"
          >
            Resetting agent…
          </p>
        ) : null}
        <button
          onClick={save}
          aria-describedby={saveError ? 'settings-save-error' : undefined}
          disabled={
            saving || resetting || !dirty || !resolvedModel || !name.trim()
          }
          className={`${primaryBtnCls} settings-save-button`}
          // Nothing to save: the model page leaves the button out.
          data-idle={(!saving && !savedFlash && !dirty) || undefined}
        >
          {saving
            ? 'Saving…'
            : savedFlash
              ? 'Saved ✓'
              : dirty
                ? 'Save changes'
                : 'No changes'}
        </button>
      </div>
      {orbStyleOpen && (
        <OrbStyleDialog
          agent={agent}
          agents={agents}
          onClose={() => setOrbStyleOpen(false)}
        />
      )}
    </div>
  );
}
