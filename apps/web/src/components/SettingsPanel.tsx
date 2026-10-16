import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  ACCESS_PROFILES,
  BROWSER_TOOL_ACCESS,
  isBrowserTool,
  deriveAccessProfile,
  toolNamesForProfile,
  type AccessProfile,
} from '../lib/agent-access';
import type { AgentDetail } from '../lib/types';
import {
  daemon,
  MODEL_SUGGESTIONS,
  type AgentUpdateInput,
  type DaemonProvider,
  type DaemonWorkspaceState,
} from '../lib/daemon-api';
import { AlertIcon, TrashIcon, XIcon } from './icons';
import { ChatGptConnection } from './ChatGptConnection';
import { ChatGptModelSelect } from './ChatGptModelSelect';
import { AgentProfileDetails, AgentMemoryView } from './AgentProfileDetails';
import { AgentFilesPanel } from './AgentFilesPanel';
import { AgentTasksView, AgentProactiveView } from './AgentWork';
import {
  LearningReviewPanel,
  type LearningReviewApi,
} from './learning/LearningReviewPanel';
import {
  ErrorBanner,
  SectionTitle,
  formatTokens,
  labelCls,
  primaryBtnCls,
} from './ui-bits';

const FOCUSABLE_SETTINGS_SELECTOR = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function InfoRow({
  label,
  value,
  mono = true,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-lg border border-line bg-white/[0.02] px-3.5 py-2.5">
      <span className="shrink-0 font-mono text-[11px] uppercase tracking-wider text-ink-3">
        {label}
      </span>
      <span
        className={`min-w-0 break-all text-right text-xs text-ink ${mono ? 'font-mono' : ''}`}
        title={value}
      >
        {value}
      </span>
    </div>
  );
}

const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];

const DEFAULT_LEARNING_API: LearningReviewApi = {
  policy: (agentId, options) => daemon.learningPolicy(agentId, options),
  setPolicy: (agentId, input) => daemon.setLearningPolicy(agentId, input),
  listReviews: (agentId, options) =>
    daemon.listLearningReviews(agentId, options),
  getReview: (agentId, reviewId, options) =>
    daemon.getLearningReview(agentId, reviewId, options),
  cancelReview: (agentId, reviewId) =>
    daemon.cancelLearningReview(agentId, reviewId),
  decideProposal: (agentId, reviewId, proposalId, input) =>
    daemon.decideLearningProposal(agentId, reviewId, proposalId, input),
  listSkills: (options) => daemon.listSkills(options),
  skill: (slug, options) => daemon.skill(slug, options),
  createSkillDraft: (agentId, reviewId, proposalId, input) =>
    daemon.createLearningSkillDraft(agentId, reviewId, proposalId, input),
};

/**
 * Editable agent settings. Saves through the daemon's
 * PATCH /api/agents/:id endpoint — the conversation is preserved and the
 * new system prompt takes effect on the next run.
 */
export function SettingsPanel({
  agent,
  online,
  providers,
  workspace = null,
  saving,
  resetting,
  saveError,
  resetError,
  saveSettings,
  resetAgent,
  close,
  integrations,
  refreshProviders,
  learningApi,
  onOpenSkills,
}: {
  agent: AgentDetail;
  online: boolean;
  providers: DaemonProvider[] | null;
  workspace?: DaemonWorkspaceState | null;
  saving: boolean;
  resetting: boolean;
  saveError: string | null;
  resetError: string | null;
  saveSettings: (patch: AgentUpdateInput) => Promise<boolean>;
  resetAgent: () => void;
  close: () => void;
  integrations?: ReactNode;
  refreshProviders?: () => void | Promise<void>;
  learningApi?: LearningReviewApi;
  onOpenSkills?: () => void;
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
  const derivedAccess = deriveAccessProfile(
    agent.toolNames.filter(
      (tool) =>
        tool !== 'memory_remove' &&
        tool !== 'memory_consolidate' &&
        !isBrowserTool(tool) &&
        tool !== 'read_conversation_window' &&
        tool !== 'read_document' &&
        tool !== 'web_search' &&
        tool !== 'web_fetch',
    ),
  );

  const [name, setName] = useState(agent.name);
  const [profileTab, setProfileTab] = useState<
    'profile' | 'tasks' | 'proactive' | 'memory' | 'activity'
  >('profile');
  const [provider, setProvider] = useState(agent.provider);
  const [model, setModel] = useState(seedModel);
  const [customModel, setCustomModel] = useState(
    seedModel === '__custom__' ? agent.model : '',
  );
  const [modelScope, setModelScope] = useState({ revision: 0, pending: false });
  const [system, setSystem] = useState(agent.system ?? '');
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
  const [savedFlash, setSavedFlash] = useState(false);
  const saveErrorRef = useRef<HTMLDivElement>(null);
  const resetErrorRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const controlsDisabled = saving || resetting;

  const requestClose = () => {
    if (controlsDisabled) return;
    close();
  };

  // Re-seed after an accepted agent update or a main-agent transition. Tool
  // order alone is not a config change and must not clobber an in-progress draft.
  useEffect(() => {
    const options = MODEL_SUGGESTIONS[agent.provider] ?? [];
    const seeded =
      agent.provider === 'chatgpt' || options.includes(agent.model)
        ? agent.model
        : '__custom__';
    setName(agent.name);
    setProvider(agent.provider);
    setModel(seeded);
    setCustomModel(seeded === '__custom__' ? agent.model : '');
    setSystem(agent.system ?? '');
    setAccessSelection(derivedAccess === 'custom' ? null : derivedAccess);
    setAccessChanged(false);
    setBrowserGrants(agent.toolNames.filter(isBrowserTool));
    setAllowMemoryRemove(hasMemoryRemove);
    setAllowMemoryConsolidate(hasMemoryConsolidate);
    setAllowConversationRead(hasConversationRead);
    setAllowDocumentRead(hasDocumentRead);
    setAllowWebSearch(hasWebSearch);
    setAllowWebFetch(hasWebFetch);
  }, [
    agent.id,
    agent.name,
    agent.provider,
    agent.model,
    agent.system,
    agentToolsKey,
    hasMemoryRemove,
    hasMemoryConsolidate,
    hasConversationRead,
    hasDocumentRead,
    hasWebSearch,
    hasWebFetch,
    derivedAccess,
  ]);

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

  useEffect(() => {
    const closeButton = closeButtonRef.current;
    if (closeButton && !closeButton.disabled) {
      closeButton.focus();
    } else {
      headingRef.current?.focus();
    }
  }, []);

  useEffect(() => {
    const handleDialogKeyboard = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (!saving && !resetting) close();
        return;
      }
      if (event.key !== 'Tab') return;

      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SETTINGS_SELECTOR),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      const activeIndex = focusable.indexOf(active as HTMLElement);
      if (event.shiftKey && activeIndex <= 0) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && activeIndex === focusable.length - 1) {
        event.preventDefault();
        first.focus();
      } else if (!dialog.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      }
    };

    document.addEventListener('keydown', handleDialogKeyboard);
    return () => document.removeEventListener('keydown', handleDialogKeyboard);
  }, [close, resetting, saving]);

  const resolvedModel = model === '__custom__' ? customModel.trim() : model;
  const accessDirty =
    accessChanged &&
    accessSelection !== null &&
    accessSelection !== derivedAccess;
  const toolsDirty =
    accessDirty ||
    BROWSER_TOOL_ACCESS.some(
      ({ name }) =>
        browserGrants.includes(name) !== agent.toolNames.includes(name),
    ) ||
    allowMemoryRemove !== hasMemoryRemove ||
    allowMemoryConsolidate !== hasMemoryConsolidate ||
    allowConversationRead !== hasConversationRead ||
    allowDocumentRead !== hasDocumentRead ||
    allowWebSearch !== hasWebSearch ||
    allowWebFetch !== hasWebFetch;
  const dirty =
    name.trim() !== agent.name ||
    provider !== agent.provider ||
    resolvedModel !== agent.model ||
    system !== (agent.system ?? '') ||
    toolsDirty;

  const save = async () => {
    const patch: AgentUpdateInput = {};
    if (name.trim() && name.trim() !== agent.name) patch.name = name.trim();
    if (provider !== agent.provider) patch.provider = provider;
    if (resolvedModel && resolvedModel !== agent.model)
      patch.model = resolvedModel;
    // Empty string clears the prompt back to the daemon default.
    if (system !== (agent.system ?? '')) patch.system = system;
    if (toolsDirty) {
      const selectedTools =
        accessDirty && accessSelection
          ? toolNamesForProfile(accessSelection)
          : agent.toolNames;
      patch.tools = selectedTools.filter(
        (tool) =>
          (tool !== 'memory_remove' || allowMemoryRemove) &&
          !isBrowserTool(tool) &&
          (tool !== 'memory_consolidate' || allowMemoryConsolidate) &&
          (tool !== 'read_conversation_window' || allowConversationRead) &&
          (tool !== 'read_document' || allowDocumentRead) &&
          (tool !== 'web_search' || allowWebSearch) &&
          (tool !== 'web_fetch' || allowWebFetch),
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
      patch.tools.push(
        ...BROWSER_TOOL_ACCESS.filter(({ name }) =>
          browserGrants.includes(name),
        ).map(({ name }) => name),
      );
    }
    if (Object.keys(patch).length === 0) return;
    if (await saveSettings(patch)) {
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 2000);
    }
  };
  const closeBusyDescription = saving
    ? 'Settings cannot be closed while changes are saving.'
    : resetting
      ? 'Settings cannot be closed while the agent is resetting.'
      : null;

  return (
    <>
      {closeBusyDescription ? (
        <p id="settings-close-busy" className="sr-only">
          {closeBusyDescription}
        </p>
      ) : null}
      <div
        data-testid="settings-backdrop"
        className="animate-fade-in fixed inset-0 z-10 bg-black/50 backdrop-blur-[2px]"
        onClick={requestClose}
      />
      <aside
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        data-surface="settings-sheet-drawer"
        aria-labelledby="settings-heading"
        aria-busy={controlsDisabled}
        tabIndex={-1}
        className="studio-settings safe-settings-sheet animate-slide-in-right fixed inset-0 z-20 flex w-full flex-col shadow-2xl shadow-black/20 sm:inset-y-0 sm:left-auto sm:right-0 sm:max-w-lg sm:border-l sm:border-line"
      >
        {/* Panel header */}
        <div className="flex items-center justify-between border-b border-line px-5 py-4 sm:px-6">
          <div>
            <h3
              ref={headingRef}
              id="settings-heading"
              tabIndex={-1}
              className="font-display text-base font-semibold tracking-tight text-ink"
            >
              Agent settings
            </h3>
            <p className="mt-0.5 font-mono text-[11px] text-ink-3">
              edits keep the conversation · apply on next run
            </p>
          </div>
          <button
            ref={closeButtonRef}
            onClick={requestClose}
            disabled={controlsDisabled}
            aria-describedby={
              closeBusyDescription ? 'settings-close-busy' : undefined
            }
            className="flex h-8 w-8 cursor-pointer items-center justify-center rounded-lg border border-line text-ink-3 transition hover:border-line-strong hover:text-ink disabled:cursor-not-allowed disabled:opacity-50"
            aria-label="Close settings"
          >
            <XIcon size={14} />
          </button>
        </div>

        <nav
          aria-label="Agent profile sections"
          className="flex shrink-0 gap-1 overflow-x-auto border-b border-line px-3 py-3"
        >
          {(
            ['profile', 'tasks', 'proactive', 'memory', 'activity'] as const
          ).map((tab) => (
            <button
              key={tab}
              type="button"
              aria-current={profileTab === tab ? 'page' : undefined}
              className={`shrink-0 rounded-lg px-3 py-2 text-sm capitalize ${profileTab === tab ? 'bg-accent/15 font-semibold' : 'text-ink-3'}`}
              onClick={() => setProfileTab(tab)}
            >
              {tab}
            </button>
          ))}
        </nav>
        <div className="flex flex-1 flex-col gap-7 overflow-y-auto px-5 py-6 sm:px-6">
          {profileTab === 'tasks' && (
            <AgentTasksView
              key={agent.id}
              agentId={agent.id}
              online={online}
              name={agent.name}
              running={agent.status === 'Running'}
            />
          )}
          {profileTab === 'proactive' && (
            <AgentProactiveView
              key={agent.id}
              agentId={agent.id}
              name={agent.name}
            />
          )}
          {profileTab === 'memory' && (
            <AgentMemoryView key={agent.id} agentId={agent.id} />
          )}
          {profileTab === 'activity' && (
            <section className="space-y-5" aria-label="Agent activity">
              <h4 className="font-semibold">Recent activity</h4>
              <InfoRow label="Status" value={agent.status.toLowerCase()} />
              <p className="text-sm text-ink-3">
                {agent.toolNames.length} tools available ·{' '}
                {agent.messages.length} conversation entries
              </p>
              {agent.messages.length === 0 && (
                <p className="text-sm text-ink-3">
                  No activity yet. Start a conversation with {agent.name}.
                </p>
              )}
              {agent.messages
                .slice(-30)
                .reverse()
                .map((message) => (
                  <article
                    key={message.id}
                    className="space-y-2 rounded-xl border border-line p-4"
                  >
                    <p className="text-xs text-ink-3">
                      {message.role} ·{' '}
                      {message.roomId?.startsWith('peer:')
                        ? 'Agent conversation'
                        : 'Direct conversation'}{' '}
                      · {new Date(message.created_at_ms).toLocaleString()}
                    </p>
                    <p className="whitespace-pre-wrap break-words text-sm leading-relaxed">
                      {message.content.text}
                    </p>
                  </article>
                ))}
            </section>
          )}
          {profileTab === 'profile' && (
            <>
              <AgentProfileDetails
                key={agent.id}
                agent={agent}
                disabled={controlsDisabled}
              />
              <AgentFilesPanel agentId={agent.id} />
              {/* System prompt */}
              <section className="space-y-2.5">
                <SectionTitle>Personality & instructions</SectionTitle>
                <p className="text-sm leading-relaxed text-ink-3">
                  Describe their role, voice, priorities, and how you want them
                  to work. These instructions shape the next conversation turn.
                </p>
                <label className={labelCls} htmlFor="personality-preset">
                  Add a personality style
                </label>
                <select
                  id="personality-preset"
                  className="field"
                  disabled={controlsDisabled}
                  value=""
                  onChange={(event) => {
                    const styles: Record<string, string> = {
                      calm: 'Be calm, patient, and thoughtful. Explain clearly and ask when the request is ambiguous.',
                      creative:
                        'Be imaginative, curious, and encouraging. Offer concrete creative options and help develop promising ideas.',
                      direct:
                        'Be direct, practical, and concise. Lead with the outcome and explain important trade-offs.',
                      proactive:
                        'Be organized and proactive during assigned work. Identify useful next steps within the agreed scope and existing tool permissions. Do not assume permission for external actions or background work.',
                    };
                    if (styles[event.target.value])
                      setSystem(
                        (current) =>
                          `${current.replace(/(?:^|\n\n)\[Personality style\][\s\S]*?\[\/Personality style\]/g, '').trim()}\n\n[Personality style]\n${styles[event.target.value]}\n[/Personality style]`,
                      );
                  }}
                >
                  <option value="">Choose a style…</option>
                  <option value="calm">Calm & thoughtful</option>
                  <option value="creative">Creative & curious</option>
                  <option value="direct">Direct & practical</option>
                  <option value="proactive">Organized & proactive</option>
                </select>
                <textarea
                  aria-label="System prompt"
                  value={system}
                  onChange={(e) => setSystem(e.target.value)}
                  disabled={controlsDisabled}
                  rows={6}
                  placeholder="Leave empty for the daemon default."
                  className="field resize-y leading-relaxed"
                />
                <p className="font-mono text-[10px] text-ink-3">
                  cleared = daemon builds its default prompt from the agent
                  profile
                </p>
              </section>

              {/* Runtime info (read-only) */}
              <section className="space-y-2.5">
                <SectionTitle>Runtime</SectionTitle>
                <InfoRow label="Status" value={agent.status.toLowerCase()} />
                <InfoRow
                  label="Created"
                  value={new Date(agent.created_at_ms).toLocaleString([], {
                    month: 'short',
                    day: 'numeric',
                    hour: '2-digit',
                    minute: '2-digit',
                  })}
                />
                <InfoRow
                  label="Tokens"
                  value={formatTokens(agent.token_usage.total_tokens)}
                />
              </section>

              {/* Workspace identity (read-only — editing is a follow-up) */}
              {workspace?.configured && workspace.workspace !== null ? (
                <section className="space-y-2.5">
                  <SectionTitle>Workspace</SectionTitle>
                  <InfoRow
                    label="Company"
                    value={workspace.workspace.companyName}
                    mono={false}
                  />
                  <InfoRow
                    label="Mission"
                    value={workspace.workspace.mission}
                    mono={false}
                  />
                  <InfoRow
                    label="Folder"
                    value={workspace.workspace.rootPath}
                  />
                </section>
              ) : null}

              {/* Editable identity */}
              <section className="space-y-2.5">
                <SectionTitle>Identity</SectionTitle>
                <div>
                  <label className={labelCls}>Name</label>
                  <input
                    aria-label="Name"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    disabled={controlsDisabled}
                    className="field"
                  />
                </div>
                <div>
                  <label className={labelCls}>Provider</label>
                  <select
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
                            ? 'ChatGPT subscription'
                            : (providers?.find((p) => p.id === id)?.label ??
                              id)}
                        </option>
                      ))}
                  </select>
                  {providers && provider !== 'chatgpt' && (
                    <p className="mt-1.5 font-mono text-[10px] text-ink-3">
                      {providers.find((p) => p.id === provider)?.configured
                        ? 'key configured on the daemon'
                        : 'no key configured for this provider on the daemon'}
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
                      <label className={labelCls}>Model</label>
                      <select
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
              </section>

              {/* Workspace access */}
              <section className="space-y-2.5">
                <SectionTitle>Workspace access</SectionTitle>
                {agent.toolNames.some((name) =>
                  /^mcp_[0-9a-f]{32}_[0-9a-f]{16}$/.test(name),
                ) && (
                  <p className="text-xs leading-relaxed text-ink-3">
                    External tool permissions are managed in Connectors.
                    Choosing a standard access profile replaces these
                    permissions when you save; saved connections and reviews are
                    kept.
                  </p>
                )}
                {derivedAccess === 'custom' ? (
                  <div className="rounded-xl border border-amber/25 bg-amber/[0.05] p-3.5">
                    <p className="text-sm font-semibold text-ink">
                      Custom access
                    </p>
                    <p className="mt-1 text-xs leading-relaxed text-ink-2">
                      This agent has a custom tool set. Choose a standard
                      profile to replace it when you save.
                    </p>
                    <p className="mt-2 font-mono text-[10px] uppercase tracking-wider text-ink-3">
                      {agent.toolNames.length} tools configured
                    </p>
                    <p className="mt-1 break-words font-mono text-[10px] leading-relaxed text-ink-3">
                      {agent.toolNames.join(', ') || 'No tools configured'}
                    </p>
                  </div>
                ) : null}
                <fieldset disabled={controlsDisabled} className="space-y-2">
                  <legend className={labelCls}>Access profile</legend>
                  {ACCESS_ORDER.map((profileName) => {
                    const profile = ACCESS_PROFILES[profileName];
                    const inputId = `settings-access-${profileName}`;

                    return (
                      <div
                        key={profileName}
                        className={`rounded-xl border p-3.5 transition ${
                          accessSelection === profileName
                            ? 'border-accent/50 bg-accent/[0.07]'
                            : 'border-line bg-white/[0.015] hover:border-line-strong'
                        }`}
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
                            setAllowConversationRead(false);
                            setAllowDocumentRead(false);
                            setAllowWebSearch(false);
                            setAllowWebFetch(false);
                            setBrowserGrants([]);
                          }}
                          className="mr-3 h-4 w-4 align-top"
                        />
                        <label
                          htmlFor={inputId}
                          className="inline cursor-pointer"
                        >
                          <span className="font-medium text-ink">
                            {profile.label}
                          </span>
                          <span className="mt-1 block pl-7 text-sm text-ink-2">
                            {profile.summary}
                          </span>
                          <span className="mt-1 block pl-7 text-xs text-ink-3">
                            {profile.risk}
                          </span>
                        </label>
                      </div>
                    );
                  })}
                  {[
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
                  ].map(({ id, label, checked, setChecked, help }) => (
                    <div
                      key={id}
                      className="rounded-xl border border-line bg-white/[0.015] p-3.5"
                    >
                      <div className="flex items-start gap-3">
                        <input
                          id={`settings-${id}`}
                          type="checkbox"
                          aria-describedby={`settings-${id}-help`}
                          checked={checked}
                          onChange={(event) => setChecked(event.target.checked)}
                          className="mt-0.5 h-4 w-4 shrink-0"
                        />
                        <label
                          htmlFor={`settings-${id}`}
                          className="cursor-pointer font-medium text-ink"
                        >
                          {label}
                        </label>
                      </div>
                      <p
                        id={`settings-${id}-help`}
                        className="mt-1 pl-7 text-sm text-ink-2"
                      >
                        {help}
                      </p>
                    </div>
                  ))}
                  <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
                    <p className="font-medium text-ink">Browser tool access</p>
                    <p className="mt-1 text-sm text-ink-2">
                      Each chat task also needs an installed browser policy.
                      Existing action approval rules still apply.
                    </p>
                    {BROWSER_TOOL_ACCESS.map(({ name, label }) => (
                      <label key={name} className="mt-2 block text-sm text-ink">
                        <input
                          type="checkbox"
                          checked={browserGrants.includes(name)}
                          onChange={(event) =>
                            setBrowserGrants((current) =>
                              event.target.checked
                                ? [
                                    ...current.filter((tool) => tool !== name),
                                    name,
                                  ]
                                : current.filter((tool) => tool !== name),
                            )
                          }
                          className="mr-2"
                        />
                        {label}
                      </label>
                    ))}
                  </div>
                  <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
                    <div className="flex items-start gap-3">
                      <input
                        id="settings-document-read"
                        type="checkbox"
                        aria-describedby="settings-document-read-help"
                        checked={allowDocumentRead}
                        onChange={(event) =>
                          setAllowDocumentRead(event.target.checked)
                        }
                        className="mt-0.5 h-4 w-4 shrink-0"
                      />
                      <label
                        htmlFor="settings-document-read"
                        className="cursor-pointer font-medium text-ink"
                      >
                        Allow reading workspace documents
                      </label>
                    </div>
                    <p
                      id="settings-document-read-help"
                      className="mt-1 pl-7 text-sm text-ink-2"
                    >
                      Allows this agent to read a selected DOCX, PDF, or XLSX as
                      bounded untrusted content. The original file is not
                      uploaded.
                    </p>
                  </div>
                  <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
                    <div className="flex items-start gap-3">
                      <input
                        id="settings-memory-remove"
                        type="checkbox"
                        aria-describedby="settings-memory-remove-help"
                        checked={allowMemoryRemove}
                        onChange={(event) =>
                          setAllowMemoryRemove(event.target.checked)
                        }
                        className="mt-0.5 h-4 w-4 shrink-0"
                      />
                      <label
                        htmlFor="settings-memory-remove"
                        className="cursor-pointer font-medium text-ink"
                      >
                        Allow removing saved facts
                      </label>
                    </div>
                    <p
                      id="settings-memory-remove-help"
                      className="mt-1 pl-7 text-sm text-ink-2"
                    >
                      Allows permanent removal of one active Fact only with its
                      exact current text. Chats and backups remain; existing
                      approval rules still apply.
                    </p>
                  </div>
                  <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
                    <div className="flex items-start gap-3">
                      <input
                        id="settings-memory-consolidate"
                        type="checkbox"
                        aria-describedby="settings-memory-consolidate-help"
                        checked={allowMemoryConsolidate}
                        onChange={(event) =>
                          setAllowMemoryConsolidate(event.target.checked)
                        }
                        className="mt-0.5 h-4 w-4 shrink-0"
                      />
                      <label
                        htmlFor="settings-memory-consolidate"
                        className="cursor-pointer font-medium text-ink"
                      >
                        Allow consolidating saved facts
                      </label>
                    </div>
                    <p
                      id="settings-memory-consolidate-help"
                      className="mt-1 pl-7 text-sm text-ink-2"
                    >
                      Allows saving one summary from 2–8 exact current facts and
                      keeping the originals in memory history. Original text and
                      citations remain available; existing approval rules still
                      apply.
                    </p>
                  </div>
                  <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
                    <div className="flex items-start gap-3">
                      <input
                        id="settings-conversation-read"
                        type="checkbox"
                        aria-describedby="settings-conversation-read-help"
                        checked={allowConversationRead}
                        disabled={
                          controlsDisabled || agent.workspaceRole === 'helper'
                        }
                        onChange={(event) =>
                          setAllowConversationRead(event.target.checked)
                        }
                        className="mt-0.5 h-4 w-4 shrink-0"
                      />
                      <label
                        htmlFor="settings-conversation-read"
                        className="cursor-pointer font-medium text-ink"
                      >
                        Allow reading conversation context
                      </label>
                    </div>
                    <p
                      id="settings-conversation-read-help"
                      className="mt-1 pl-7 text-sm text-ink-2"
                    >
                      Allows this agent to search your own past conversations
                      and read a bounded window around a selected message. Past
                      text is evidence, not current instructions.
                    </p>
                  </div>
                </fieldset>
              </section>

              <LearningReviewPanel
                agentId={agent.id}
                api={learningApi ?? DEFAULT_LEARNING_API}
                disabled={controlsDisabled}
                onOpenSkills={onOpenSkills}
              />

              {integrations}

              {saveError ? (
                <div
                  id="settings-save-error"
                  ref={saveErrorRef}
                  role="alert"
                  aria-live="assertive"
                  aria-atomic="true"
                  tabIndex={-1}
                >
                  <ErrorBanner
                    message={saveError}
                    icon={<AlertIcon size={14} />}
                  />
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
                  <ErrorBanner
                    message={resetError}
                    icon={<AlertIcon size={14} />}
                  />
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
                  saving ||
                  resetting ||
                  !dirty ||
                  !resolvedModel ||
                  !name.trim()
                }
                className={`${primaryBtnCls} w-full py-2.5`}
              >
                {saving
                  ? 'Saving…'
                  : savedFlash
                    ? 'Saved ✓'
                    : dirty
                      ? 'Save changes'
                      : 'No changes'}
              </button>

              {/* Danger zone */}
              <section className="rounded-xl border border-danger/20 bg-danger/[0.04] p-4">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="text-xs font-semibold text-danger">
                      Reset agent
                    </div>
                    <div className="mt-0.5 text-[11px] leading-relaxed text-danger">
                      deletes the agent and its entire conversation
                    </div>
                  </div>
                  <button
                    onClick={resetAgent}
                    aria-describedby={
                      resetError ? 'settings-reset-error' : undefined
                    }
                    disabled={resetting || saving}
                    className="flex shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs font-medium text-danger transition hover:bg-danger/20 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <TrashIcon size={12} />
                    {resetting ? 'Resetting…' : 'Reset'}
                  </button>
                </div>
              </section>
            </>
          )}
        </div>
      </aside>
    </>
  );
}
