import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ACCESS_PROFILES,
  BROWSER_TOOL_ACCESS,
  isBrowserTool,
  PROGRAMMATIC_TOOL_ACCESS,
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
import { AlertIcon, TrashIcon } from './icons';
import { ChatGptConnection } from './ChatGptConnection';
import { ChatGptModelSelect } from './ChatGptModelSelect';
import { AgentProfileDetails } from './AgentProfileDetails';
import { AgentFilesPanel } from './AgentFilesPanel';
import { FallbackModels } from './models/FallbackModels';
import { ProviderKeys } from './models/ProviderKeys';
import { SideModel } from './models/SideModel';
import { ErrorBanner, SectionTitle, labelCls, primaryBtnCls } from './ui-bits';

/** The parts of the companion's own configuration. Each Settings page shows
 *  the ones it is about; one save covers whatever the page shows. */
export type SettingsSection = 'profile' | 'model' | 'access';
const ALL_SECTIONS: readonly SettingsSection[] = ['profile', 'model', 'access'];

const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];

const PERSONALITY_STYLES: Record<string, string> = {
  calm: 'Be calm, patient, and thoughtful. Explain clearly and ask when the request is ambiguous.',
  creative:
    'Be imaginative, curious, and encouraging. Offer concrete creative options and help develop promising ideas.',
  direct:
    'Be direct, practical, and concise. Lead with the outcome and explain important trade-offs.',
  proactive:
    'Be organized and proactive during assigned work. Identify useful next steps within the agreed scope and existing tool permissions. Do not assume permission for external actions or background work.',
};

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
  refreshProviders,
  sections = ALL_SECTIONS,
}: {
  agent: AgentDetail;
  providers: DaemonProvider[] | null;
  saving: boolean;
  resetting: boolean;
  saveError: string | null;
  resetError: string | null;
  saveSettings: (patch: AgentUpdateInput) => Promise<boolean>;
  resetAgent: () => void;
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
  const derivedAccess = deriveAccessProfile(
    agent.toolNames.filter(
      (tool) =>
        tool !== 'memory_remove' &&
        tool !== 'memory_consolidate' &&
        !isBrowserTool(tool) &&
        tool !== 'read_conversation_window' &&
        tool !== 'read_document' &&
        tool !== 'web_search' &&
        tool !== 'web_fetch' &&
        tool !== 'execute_code',
    ),
  );

  const [name, setName] = useState(agent.name);
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
  const [allowToolChains, setAllowToolChains] = useState(hasToolChains);
  const [toolChainStatus, setToolChainStatus] = useState('loading');
  const [savedFlash, setSavedFlash] = useState(false);
  const saveErrorRef = useRef<HTMLDivElement>(null);
  const resetErrorRef = useRef<HTMLDivElement>(null);
  const controlsDisabled = saving || resetting;
  const shows = (section: SettingsSection) => sections.includes(section);
  useEffect(() => {
    if (!sections.includes('access')) return;
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
    setAllowToolChains(hasToolChains);
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
    hasToolChains,
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
    allowWebFetch !== hasWebFetch ||
    allowToolChains !== hasToolChains;
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
          (tool !== 'web_fetch' || allowWebFetch) &&
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

  const abilities = [
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

  return (
    <div className="settings-form" aria-busy={controlsDisabled}>
      {shows('profile') && (
        <>
          <section
            className="settings-group"
            aria-labelledby="settings-identity"
          >
            <h3 id="settings-identity" className="settings-group-title">
              Identity
            </h3>
            <AgentProfileDetails
              key={agent.id}
              agent={agent}
              disabled={controlsDisabled}
            />
            <div>
              <label className={labelCls} htmlFor="settings-name">
                Name
              </label>
              <input
                id="settings-name"
                aria-label="Name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                disabled={controlsDisabled}
                className="field"
              />
            </div>
          </section>

          <section
            className="settings-group"
            aria-labelledby="settings-personality"
          >
            <h3 id="settings-personality" className="settings-group-title">
              Personality & instructions
            </h3>
            <p className="settings-group-help">
              Their role, voice, priorities, and how you want them to work.
              Changes apply from the next reply.
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
                const style = PERSONALITY_STYLES[event.target.value];
                if (style)
                  setSystem(
                    (current) =>
                      `${current.replace(/(?:^|\n\n)\[Personality style\][\s\S]*?\[\/Personality style\]/g, '').trim()}\n\n[Personality style]\n${style}\n[/Personality style]`,
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
              rows={7}
              placeholder="Leave empty to use the default built from this profile."
              className="field resize-y leading-relaxed"
            />
          </section>

          <details className="settings-group settings-advanced">
            <summary>Instruction files</summary>
            <AgentFilesPanel agentId={agent.id} />
          </details>
        </>
      )}

      {shows('model') && (
        <section className="settings-group" aria-labelledby="settings-model">
          <h3 id="settings-model" className="settings-group-title">
            Provider and model
          </h3>
          <p className="settings-group-help">
            The model {agent.name} thinks with. Switching keeps every
            conversation.
          </p>
          <div>
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
                      ? 'ChatGPT subscription'
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
          <FallbackModels
            key={agent.id}
            primary={{ provider: agent.provider, model: agent.model }}
            fallbacks={agent.fallbacks ?? []}
            providers={providers}
            disabled={controlsDisabled}
            save={(fallbacks) => saveSettings({ fallbacks })}
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

      {shows('access') && (
        <section className="settings-group" aria-labelledby="settings-access">
          <h3 id="settings-access" className="settings-group-title">
            What {agent.name} can do
          </h3>
          <p className="settings-group-help">
            Pick how far {agent.name} may act on its own. Rules below decide
            when it still asks you first.
          </p>
          {agent.toolNames.some((name) =>
            /^mcp_[0-9a-f]{32}_[0-9a-f]{16}$/.test(name),
          ) && (
            <p className="settings-group-help">
              External tool permissions are managed in Connectors. Choosing a
              standard access profile replaces these permissions when you save;
              saved connections and reviews are kept.
            </p>
          )}
          {derivedAccess === 'custom' ? (
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
            <legend className={labelCls}>Access profile</legend>
            <div className="settings-access-options">
              {ACCESS_ORDER.map((profileName) => {
                const profile = ACCESS_PROFILES[profileName];
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
                        setAllowConversationRead(false);
                        setAllowDocumentRead(false);
                        setAllowWebSearch(false);
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
                      <span className="settings-access-risk">
                        {profile.risk}
                      </span>
                    </label>
                  </div>
                );
              })}
            </div>
            {accessSelection === 'operate' && (
              <p className="settings-access-note">
                <span aria-hidden="true">↳</span>
                <span>
                  Operate lets {agent.name} run commands and apps. Autonomy
                  decides when it asks first.
                </span>
                <button
                  type="button"
                  className="settings-access-note-link"
                  onClick={() =>
                    document
                      .getElementById('approvals-autonomy')
                      ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                  }
                >
                  Change in Autonomy
                </button>
              </p>
            )}
            <SectionTitle>Extra abilities</SectionTitle>
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
            <SectionTitle>Browser</SectionTitle>
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

      {shows('profile') && (
        <section className="settings-danger" aria-labelledby="settings-reset">
          <div>
            <h3 id="settings-reset">Reset {agent.name}</h3>
            <p>Deletes the companion and every conversation with it.</p>
          </div>
          <button
            onClick={resetAgent}
            aria-describedby={resetError ? 'settings-reset-error' : undefined}
            disabled={resetting || saving}
          >
            <TrashIcon size={12} />
            {resetting ? 'Resetting…' : 'Reset'}
          </button>
        </section>
      )}
    </div>
  );
}
