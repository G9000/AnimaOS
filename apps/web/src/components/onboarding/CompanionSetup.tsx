import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  daemon,
  MODEL_SUGGESTIONS,
  type DaemonProvider,
  type DaemonSnapshot,
  type DaemonWorkspaceState,
  type WorkspaceInspectFound,
} from '../../lib/daemon-api';
import {
  ACCESS_PROFILES,
  toolNamesForProfile,
  type AccessProfile,
} from '../../lib/agent-access';
import { savePreferredVoice } from '../../lib/voice-preference';
import { ChatGptConnection } from '../ChatGptConnection';
import { ChatGptModelSelect } from '../ChatGptModelSelect';
import { ResumeCard } from './ResumeCard';
import { VoiceCheck } from './VoiceCheck';
import { HaloOrb } from '../halo/HaloOrb';
import './companion-onboarding.css';

export interface CompanionSetupProps {
  providers: DaemonProvider[] | null;
  providersError: string | null;
  retryProviders(): void | Promise<void>;
  onCreated(snapshot: DaemonSnapshot): void;
}

type Step = 'welcome' | 'existing' | 'model' | 'you' | 'access' | 'voice';
type ModelSource = 'chatgpt' | 'key' | 'local';
type Personality = 'quiet' | 'balanced' | 'driven';

const FLOW: readonly Step[] = ['model', 'you', 'access', 'voice'];
const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];
const WEB_READ_TOOLS = ['browser_navigate', 'browser_observe'];

const SOURCES: Record<ModelSource, { title: string; summary: string }> = {
  chatgpt: {
    title: 'Use my ChatGPT plan',
    summary: 'Sign in with your account. No keys to manage.',
  },
  key: {
    title: 'Use an API key',
    summary:
      'OpenAI, Anthropic, Google and more. Keys live on the server running anima.',
  },
  local: {
    title: 'Run a local model',
    summary: 'Ollama or another model on that server. Private, works offline.',
  },
};

const PERSONALITIES: Record<
  Personality,
  { label: string; summary: string; instruction: string }
> = {
  quiet: {
    label: 'Quiet',
    summary: 'Waits for you. Short answers.',
    instruction:
      'Wait for the owner to ask before suggesting more work. Keep answers short.',
  },
  balanced: {
    label: 'Balanced',
    summary: 'Suggests next steps when useful.',
    instruction:
      'Suggest a useful next step when it helps, and keep answers concise.',
  },
  driven: {
    label: 'Driven',
    summary: 'Takes the lead and explains its thinking.',
    instruction:
      'Take the lead on clearly useful next steps and briefly explain your reasoning, while staying within granted permissions.',
  },
};

function sourceOf(provider: DaemonProvider): ModelSource {
  if (provider.id === 'chatgpt') return 'chatgpt';
  return provider.requiresKey ? 'key' : 'local';
}

function defaultModel(provider: string): string {
  return provider === 'chatgpt'
    ? ''
    : (MODEL_SUGGESTIONS[provider]?.[0] ?? '__custom__');
}

function providerGuidance(provider: DaemonProvider): string {
  if (provider.configured) return 'Ready';
  if (!provider.requiresKey) return 'Not running on the server';
  return `Set ${provider.apiKeyEnvs[0] ?? 'its API key'} on the server`;
}

function greeting(owner: string, personality: Personality): string {
  const hour = new Date().getHours();
  const part = hour < 12 ? 'morning' : hour < 18 ? 'afternoon' : 'evening';
  const hello = `Good ${part}${owner ? `, ${owner}` : ''}.`;
  if (personality === 'quiet') return `${hello} I'm here when you need me.`;
  if (personality === 'driven')
    return `${hello} I have a few ideas for today. Want to hear them?`;
  return `${hello} Want to pick up where you left off, or start something new?`;
}

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

function Option({
  name,
  checked,
  disabled,
  onChange,
  title,
  summary,
  children,
}: {
  name: string;
  checked: boolean;
  disabled?: boolean;
  onChange(): void;
  title: string;
  summary?: string;
  children?: ReactNode;
}) {
  return (
    <label className="onboarding__option">
      <input
        type="radio"
        name={name}
        checked={checked}
        disabled={disabled}
        onChange={onChange}
      />
      <span className="onboarding__option-text">
        <span className="onboarding__option-title">{title}</span>
        {summary ? (
          <span className="onboarding__option-desc">{summary}</span>
        ) : null}
      </span>
      {children}
      <span className="onboarding__mark" aria-hidden="true" />
    </label>
  );
}

export function CompanionSetup({
  providers,
  providersError,
  retryProviders,
  onCreated,
}: CompanionSetupProps) {
  const [step, setStep] = useState<Step>('welcome');
  const [name, setName] = useState('Anima');
  const [ownerName, setOwnerName] = useState('');
  const [personality, setPersonality] = useState<Personality>('balanced');
  const [avatar, setAvatar] = useState<{ file: File; url: string } | null>(
    null,
  );
  const [provider, setProvider] = useState('');
  const [model, setModel] = useState('');
  const [customModel, setCustomModel] = useState('');
  const [source, setSource] = useState<ModelSource | null>(null);
  const sourceChosen = useRef(false);
  const [modelScope, setModelScope] = useState({ revision: 0, pending: false });
  const [access, setAccess] = useState<AccessProfile>('collaborate');
  const [browseWeb, setBrowseWeb] = useState(false);
  const [voiceURI, setVoiceURI] = useState<string | null>(null);
  const [speakingVoice, setSpeakingVoice] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState<DaemonWorkspaceState | null>(null);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [rootPath, setRootPath] = useState('');
  const rootPathRef = useRef('');
  const [preview, setPreview] = useState<{
    rootPath: string;
    workspace: WorkspaceInspectFound;
  } | null>(null);
  const [folderNote, setFolderNote] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [resuming, setResuming] = useState(false);
  const folderRequest = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const mounted = useRef(false);
  const submitting = useRef(false);
  const retryingRef = useRef(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const modelSelectRef = useRef<HTMLSelectElement>(null);
  const customModelInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    let current = true;
    setWorkspaceError(null);
    void daemon
      .getWorkspace()
      .then((state) => {
        if (!current) return;
        setWorkspace(state);
        const path = state.workspace?.rootPath ?? state.defaultRoot;
        rootPathRef.current = path;
        setRootPath(path);
      })
      .catch((caught) => {
        if (current) setWorkspaceError(errorText(caught));
      });
    return () => {
      current = false;
    };
  }, [loadAttempt]);
  useEffect(() => {
    if (provider || sourceChosen.current) return;
    // Keyless local providers are advertised even when no local server is running.
    // Keep those (and the test adapter) explicit choices instead of cloud defaults.
    const available =
      providers?.find((item) => item.id === 'chatgpt' && item.configured) ??
      providers?.find(
        (item) =>
          item.configured && item.requiresKey && item.id !== 'deterministic',
      );
    if (available) {
      setProvider(available.id);
      setModel(defaultModel(available.id));
      setSource(sourceOf(available));
    } else if (!source && providers?.some((item) => item.id === 'chatgpt')) {
      setSource('chatgpt');
    }
  }, [providers, provider, source]);
  useEffect(() => {
    // Connecting ChatGPT while its option is open selects the subscription.
    const chatGpt = providers?.find((item) => item.id === 'chatgpt');
    if (source === 'chatgpt' && chatGpt?.configured && provider !== 'chatgpt') {
      setProvider('chatgpt');
      setModel('');
      setCustomModel('');
    }
  }, [providers, source, provider]);
  useEffect(() => () => {
    if (avatar) URL.revokeObjectURL(avatar.url);
  }, [avatar]);
  useEffect(() => {
    headingRef.current?.focus();
  }, [step]);

  const refreshProviders = async () => {
    if (retryingRef.current) return;
    retryingRef.current = true;
    setRetrying(true);
    setError(null);
    try {
      await retryProviders();
    } catch (caught) {
      if (mounted.current) setError(errorText(caught));
    } finally {
      retryingRef.current = false;
      if (mounted.current) setRetrying(false);
    }
  };
  const changeProvider = (value: string) => {
    if (value === provider) return;
    setProvider(value);
    setModel(value ? defaultModel(value) : '');
    setCustomModel('');
  };
  const chooseSource = (value: ModelSource) => {
    sourceChosen.current = true;
    setSource(value);
    if (provider && providers?.some((item) => item.id === provider && sourceOf(item) === value))
      return;
    const first = (providers ?? [])
      .filter((item) => sourceOf(item) === value && item.configured)
      .sort(
        (left, right) =>
          Number(left.id === 'deterministic') -
          Number(right.id === 'deterministic'),
      )[0];
    changeProvider(first?.id ?? '');
  };
  const resolvedModel = (model === '__custom__' ? customModel : model).trim();
  const changeRootPath = (path: string) => {
    folderRequest.current += 1;
    setInspecting(false);
    rootPathRef.current = path;
    setRootPath(path);
    setPreview(null);
    setFolderNote(null);
    setError(null);
  };
  const inspectFolder = async (path: string) => {
    const request = ++folderRequest.current;
    setInspecting(true);
    setFolderNote(null);
    try {
      const result = await daemon.inspectWorkspace(path);
      if (
        !mounted.current ||
        request !== folderRequest.current ||
        rootPathRef.current.trim() !== path
      )
        return null;
      if (result.found) {
        setPreview({ rootPath: path, workspace: result });
      } else {
        setFolderNote(
          'No saved Anima workspace found here. This folder can be used for a new companion.',
        );
      }
      return result;
    } catch (caught) {
      if (mounted.current && request === folderRequest.current) {
        setError(errorText(caught));
      }
      return null;
    } finally {
      if (mounted.current && request === folderRequest.current)
        setInspecting(false);
    }
  };
  const browseFolder = async () => {
    if (browsing || inspecting) return;
    const request = folderRequest.current;
    setBrowsing(true);
    setError(null);
    try {
      const { rootPath: selected } = await daemon.pickWorkspaceFolder();
      if (mounted.current && request === folderRequest.current && selected) {
        changeRootPath(selected);
        await inspectFolder(selected.trim());
      }
    } catch (caught) {
      if (mounted.current) setError(errorText(caught));
    } finally {
      if (mounted.current) setBrowsing(false);
    }
  };
  const resume = async () => {
    if (!preview || resuming) return;
    setResuming(true);
    setError(null);
    try {
      const result = await daemon.resumeWorkspace(preview.rootPath);
      if (mounted.current) onCreated(result.orchestrator);
    } catch (caught) {
      if (mounted.current) setError(errorText(caught));
    } finally {
      if (mounted.current) setResuming(false);
    }
  };

  const catalogState = retrying
    ? 'retrying'
    : providersError
      ? 'error'
      : providers === null
        ? 'loading'
        : providers.length
          ? 'ready'
          : 'empty';
  const workspaceReady = !!workspace && !workspaceError;
  const modelReady =
    catalogState === 'ready' &&
    !!providers?.some((item) => item.id === provider && item.configured) &&
    !!resolvedModel;
  const youReady = !!name.trim();
  const accessReady =
    workspaceReady && !!rootPath.trim() && !browsing && !inspecting;
  const ready = workspaceReady && modelReady && youReady && accessReady;

  const goTo = (next: Step) => {
    setError(null);
    setStep(next);
  };
  const submit = async (keepVoice: boolean) => {
    if (!ready || preview || submitting.current) return;
    submitting.current = true;
    setCreating(true);
    setError(null);
    const selectedRoot = rootPath.trim();
    const owner = ownerName.trim();
    const system = [
      `You are ${name.trim()}, the owner's personal companion. Keep a consistent identity across conversations and channels.`,
      ...(owner ? [`The owner's name is ${owner}.`] : []),
      'Help with everyday plans, research, files, reminders, and connected tools. Be warm, concise, practical, and honest about what you can actually do.',
      PERSONALITIES[personality].instruction,
      'Use available memory tools to recall relevant preferences. Distinguish facts the owner stated from your own guesses and accept corrections.',
      'Handle simple work yourself. When available and useful, spawn or reuse a helper for a bounded part of the current task. Remain responsible for checking the result and responding to the owner. Helpers cannot gain permissions you do not have.',
      'Only act within the requested scope and granted tool permissions. Ask before spending money, publishing, deleting important data, or sending consequential external messages. Never claim a task, reminder, delegation, or recovery succeeded without a recorded result.',
      'Background work requires an actual daemon job or schedule. Do not promise to keep working after a reply unless such work has been registered.',
      ...(workspace?.workspace?.mission
        ? [`Workspace context: ${workspace.workspace.mission}`]
        : []),
    ].join('\n\n');
    try {
      if (!workspace?.configured) {
        const inspected = await inspectFolder(selectedRoot);
        if (
          !inspected ||
          inspected.found ||
          rootPathRef.current.trim() !== selectedRoot
        ) {
          if (inspected?.found && mounted.current) setStep('access');
          return;
        }
      }
      const config = {
        name: name.trim(),
        provider,
        model: resolvedModel,
        system,
        tools: [
          ...toolNamesForProfile(access),
          ...(browseWeb ? WEB_READ_TOOLS : []),
        ],
      };
      const result = workspace?.configured
        ? await daemon.createAgent({
            ...config,
            settings: { additional: { workspaceRole: 'lead' } },
          })
        : await daemon.bootstrapWorkspace({
            workspace: {
              rootPath: selectedRoot,
              companyName: 'Personal',
              mission:
                'A dependable personal companion for everyday life and work.',
              values: [],
            },
            agent: {
              ...config,
              presetId: 'chief-of-staff',
              bio: 'Your personal companion for everyday life and work.',
              adjectives: ['warm', 'dependable', 'honest'],
              style:
                'Lead with the outcome. Keep conversation natural and concise.',
            },
          });
      const agentId = result.agent?.state?.id;
      if (avatar && agentId) {
        // The companion exists either way; a failed upload keeps the default orb.
        await daemon.setAgentAvatar(agentId, avatar.file).catch(() => undefined);
      }
      if (keepVoice) savePreferredVoice(voiceURI);
      if (mounted.current) onCreated(result.agent);
    } catch (caught) {
      if (mounted.current) setError(errorText(caught));
    } finally {
      submitting.current = false;
      if (mounted.current) setCreating(false);
    }
  };
  const next = async () => {
    if (preview) return;
    if (step === 'welcome' && workspaceReady) goTo('model');
    else if (step === 'model' && modelReady) goTo('you');
    else if (step === 'you' && youReady) goTo('access');
    else if (step === 'access' && accessReady) {
      if (workspace?.configured) {
        goTo('voice');
        return;
      }
      const selectedRoot = rootPath.trim();
      const inspected = await inspectFolder(selectedRoot);
      if (
        mounted.current &&
        inspected &&
        !inspected.found &&
        rootPathRef.current.trim() === selectedRoot
      )
        goTo('voice');
    } else if (step === 'existing' && rootPath.trim() && !inspecting)
      await inspectFolder(rootPath.trim());
    else if (step === 'voice') await submit(true);
  };

  const flowIndex = FLOW.indexOf(step);
  const busy = creating || resuming;
  const chatGpt = providers?.find((item) => item.id === 'chatgpt');
  const selectedProvider = providers?.find((item) => item.id === provider);
  const sources = (['chatgpt', 'key', 'local'] as const).filter((value) =>
    providers?.some((item) => sourceOf(item) === value),
  );
  const sourceProviders = (providers ?? [])
    .filter((item) => source && item.id !== 'chatgpt' && sourceOf(item) === source)
    .sort((left, right) => Number(right.configured) - Number(left.configured));

  const heading = (title: string, description: string, hero = false) => (
    <div
      className={`onboarding__heading${hero ? ' onboarding__heading--hero' : ''}`}
    >
      <h1 id="onboarding-step-heading" ref={headingRef} tabIndex={-1}>
        {title}
      </h1>
      <p>{description}</p>
    </div>
  );
  const folderField = (allowCheck: boolean) => (
    <div className="onboarding__section">
      <label htmlFor="companion-folder" className="onboarding__label">
        Home folder
      </label>
      <div className="onboarding__row">
        <input
          id="companion-folder"
          className="onboarding__input onboarding__input--mono"
          value={rootPath}
          readOnly={workspace?.configured}
          disabled={busy}
          onChange={(event) => changeRootPath(event.target.value)}
          spellCheck={false}
        />
        {!workspace?.configured && (
          <button
            type="button"
            onClick={() => void browseFolder()}
            disabled={browsing || inspecting || busy}
            className="onboarding__button onboarding__button--secondary"
          >
            {browsing ? 'Choosing…' : 'Browse…'}
          </button>
        )}
        {!workspace?.configured && allowCheck && (
          <button
            type="button"
            onClick={() => void inspectFolder(rootPath.trim())}
            disabled={!rootPath.trim() || inspecting || browsing || busy}
            className="onboarding__button onboarding__button--secondary"
          >
            {inspecting ? 'Checking…' : 'Check folder'}
          </button>
        )}
      </div>
      <p className="onboarding__hint">
        {workspace?.configured
          ? 'anima already has a home on this server.'
          : 'A folder on the machine running anima. Existing files are kept.'}
      </p>
      {folderNote && !preview && (
        <p role="status" className="onboarding__hint">
          {folderNote}
        </p>
      )}
    </div>
  );
  const resumeCard = preview ? (
    <ResumeCard
      preview={preview.workspace}
      rootPath={preview.rootPath}
      resuming={resuming}
      resumeError={error}
      onResume={() => void resume()}
      onSetupFresh={() => {
        setPreview(null);
        setFolderNote('Choose another folder to create a new companion.');
      }}
      onSetupFreshLabel="Choose another folder"
    />
  ) : null;

  let body: ReactNode;
  let footer: ReactNode;
  const primary = (label: string, enabled: boolean) => (
    <button
      type="submit"
      disabled={!enabled || busy}
      className="onboarding__button onboarding__button--primary"
    >
      {label}
      <span aria-hidden="true">→</span>
    </button>
  );
  const back = (target: Step, label = 'Back') => (
    <button
      type="button"
      className="onboarding__link"
      disabled={busy}
      onClick={() => goTo(target)}
    >
      {label}
    </button>
  );

  if (step === 'welcome') {
    body = (
      <>
        {heading(
          'Meet anima.',
          'A personal agent you own. Run it on your computer or your own server, and talk to it here or from the chat apps you already use.',
          true,
        )}
        <ul className="onboarding__options" aria-label="What anima does">
          {[
            ['Yours to host', 'On this computer, or a server you own.'],
            ['Talk or type', 'Here, or from Telegram, WhatsApp and Discord.'],
            [
              'Gets better with use',
              'Learns skills from what works, and asks before keeping them.',
            ],
          ].map(([title, summary]) => (
            <li key={title} className="onboarding__option">
              <span className="onboarding__option-text">
                <span className="onboarding__option-title">{title}</span>
                <span className="onboarding__option-desc">{summary}</span>
              </span>
            </li>
          ))}
        </ul>
        {workspaceError ? (
          <div role="alert" className="onboarding__section">
            <p className="onboarding__alert">{workspaceError}</p>
            <button
              type="button"
              className="onboarding__link"
              onClick={() => setLoadAttempt((value) => value + 1)}
            >
              Retry workspace
            </button>
          </div>
        ) : workspace ? (
          <p className="onboarding__status">
            {workspace.configured
              ? 'Runtime connected. Your workspace is ready for a new companion.'
              : 'Runtime connected'}
          </p>
        ) : (
          <p role="status" className="onboarding__status" data-tone="pending">
            Loading your workspace…
          </p>
        )}
      </>
    );
    footer = (
      <>
        {workspace?.configured ? (
          <span />
        ) : (
          <button
            type="button"
            className="onboarding__link"
            disabled={!workspaceReady}
            onClick={() => goTo('existing')}
          >
            Open an existing workspace
          </button>
        )}
        {primary('Get started', workspaceReady)}
      </>
    );
  } else if (step === 'existing') {
    body = (
      <>
        {heading(
          'Open an existing workspace',
          'Point anima at a folder that already holds a workspace. Nothing in it is changed.',
        )}
        {folderField(true)}
        {resumeCard}
      </>
    );
    footer = (
      <>
        {back('welcome')}
        <span />
      </>
    );
  } else if (step === 'model') {
    body = (
      <>
        {heading(
          'Choose a model',
          'This is what anima thinks with. You can switch anytime.',
        )}
        {catalogState === 'loading' ||
        (catalogState === 'retrying' && !providers?.length) ? (
          <p role="status" className="onboarding__status" data-tone="pending">
            {catalogState === 'loading'
              ? 'Loading provider catalog…'
              : 'Retrying provider catalog…'}
          </p>
        ) : catalogState === 'error' || catalogState === 'empty' ? (
          <div className="onboarding__section">
            <p role="alert" className="onboarding__alert">
              {catalogState === 'error'
                ? providersError
                : 'No model providers are available yet. Add a provider key on the server, then retry.'}
            </p>
            <button
              type="button"
              className="onboarding__button onboarding__button--secondary"
              onClick={() => void refreshProviders()}
            >
              Retry providers
            </button>
          </div>
        ) : (
          <>
            {/* Keep the choices (and the ChatGPT connection) mounted while the
                catalog refreshes; remounting the connection would refresh again. */}
            {catalogState === 'retrying' && (
              <p role="status" className="onboarding__status" data-tone="pending">
                Refreshing providers…
              </p>
            )}
            <div
              role="radiogroup"
              aria-label="Model source"
              className="onboarding__options"
            >
              {sources.map((value) => (
                <Option
                  key={value}
                  name="onboarding-model-source"
                  checked={source === value}
                  onChange={() => chooseSource(value)}
                  title={SOURCES[value].title}
                  summary={SOURCES[value].summary}
                />
              ))}
            </div>
            {provider && selectedProvider && !selectedProvider.configured && (
              <p role="status" className="onboarding__notice">
                {selectedProvider.label} is unavailable right now. Reconnect
                it, or choose another option to continue.
              </p>
            )}
            {source === 'chatgpt' && chatGpt ? (
              <div className="onboarding__section">
                <ChatGptConnection
                  onConnectionChange={refreshProviders}
                  onModelScopeChange={(pending) =>
                    setModelScope((current) => ({
                      revision: current.revision + 1,
                      pending,
                    }))
                  }
                />
                {provider === 'chatgpt' && (
                  <ChatGptModelSelect
                    id="onboarding-model"
                    scopeKey={`onboarding:${modelScope.revision}`}
                    connectionPending={modelScope.pending}
                    model={model}
                    customModel={customModel}
                    onModelChange={setModel}
                    onCustomModelChange={setCustomModel}
                    modelSelectRef={modelSelectRef}
                    customModelInputRef={customModelInputRef}
                  />
                )}
              </div>
            ) : source ? (
              <>
                <div className="onboarding__section">
                  <span id="onboarding-provider-label" className="onboarding__label">
                    Provider
                  </span>
                  <div
                    role="radiogroup"
                    aria-labelledby="onboarding-provider-label"
                    className="onboarding__options"
                  >
                    {sourceProviders.map((item) => (
                      <Option
                        key={item.id}
                        name="onboarding-provider"
                        checked={provider === item.id}
                        disabled={!item.configured}
                        onChange={() => changeProvider(item.id)}
                        title={item.label}
                        summary={providerGuidance(item)}
                      />
                    ))}
                  </div>
                </div>
                {provider && (
                  <div className="onboarding__section">
                    <label htmlFor="onboarding-model" className="onboarding__label">
                      Model
                    </label>
                    <select
                      ref={modelSelectRef}
                      id="onboarding-model"
                      className="onboarding__input"
                      value={model}
                      onChange={(event) => setModel(event.target.value)}
                    >
                      <option value="" disabled>
                        Choose a model
                      </option>
                      {(MODEL_SUGGESTIONS[provider] ?? []).map((suggestion) => (
                        <option key={suggestion} value={suggestion}>
                          {suggestion}
                        </option>
                      ))}
                      <option value="__custom__">Custom model</option>
                    </select>
                    {model === '__custom__' && (
                      <>
                        <label
                          htmlFor="onboarding-custom-model"
                          className="onboarding__label"
                        >
                          Custom model
                        </label>
                        <input
                          ref={customModelInputRef}
                          id="onboarding-custom-model"
                          className="onboarding__input onboarding__input--mono"
                          value={customModel}
                          onChange={(event) => setCustomModel(event.target.value)}
                          placeholder="Provider model identifier"
                        />
                      </>
                    )}
                  </div>
                )}
              </>
            ) : null}
          </>
        )}
      </>
    );
    footer = (
      <>
        {back('welcome')}
        {primary('Continue', modelReady)}
      </>
    );
  } else if (step === 'you') {
    body = (
      <>
        {heading(
          'Introductions',
          'Defaults are fine. You can change all of this later.',
        )}
        <div className="onboarding__section">
          <label htmlFor="companion-owner" className="onboarding__label">
            Your name
          </label>
          <input
            id="companion-owner"
            className="onboarding__input"
            value={ownerName}
            maxLength={80}
            autoComplete="given-name"
            placeholder="What anima should call you"
            onChange={(event) => setOwnerName(event.target.value)}
          />
        </div>
        <div className="onboarding__section">
          <span className="onboarding__label">Your agent</span>
          <div className="onboarding__row">
            <span className="onboarding__avatar">
              {avatar ? <img src={avatar.url} alt="" /> : null}
            </span>
            <div className="onboarding__section" style={{ flex: 1 }}>
              <input
                id="companion-name"
                aria-label="Agent name"
                className="onboarding__input"
                value={name}
                maxLength={80}
                onChange={(event) => setName(event.target.value)}
                required
              />
              <div className="onboarding__row">
                <span className="onboarding__hint">
                  {avatar ? avatar.file.name : 'Default orb'} ·
                </span>
                <label className="onboarding__link" style={{ fontSize: 12 }}>
                  {avatar ? 'Change image' : 'Upload an image'}
                  <input
                    type="file"
                    accept="image/*"
                    hidden
                    onChange={(event) => {
                      const file = event.target.files?.[0];
                      event.target.value = '';
                      if (!file) return;
                      setAvatar({ file, url: URL.createObjectURL(file) });
                    }}
                  />
                </label>
                {avatar && (
                  <button
                    type="button"
                    className="onboarding__link"
                    style={{ fontSize: 12 }}
                    onClick={() => setAvatar(null)}
                  >
                    Use the orb
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
        <div className="onboarding__section">
          <span id="onboarding-personality-label" className="onboarding__label">
            Personality
          </span>
          <div
            role="radiogroup"
            aria-labelledby="onboarding-personality-label"
            className="onboarding__options"
          >
            {(Object.keys(PERSONALITIES) as Personality[]).map((value) => (
              <Option
                key={value}
                name="onboarding-personality"
                checked={personality === value}
                onChange={() => setPersonality(value)}
                title={PERSONALITIES[value].label}
                summary={PERSONALITIES[value].summary}
              />
            ))}
          </div>
        </div>
      </>
    );
    footer = (
      <>
        {back('model')}
        {primary('Continue', youReady)}
      </>
    );
  } else if (step === 'access') {
    body = (
      <>
        {heading(
          'Where anima works',
          'Its home folder, and how much it can do there.',
        )}
        {folderField(false)}
        {resumeCard ?? (
          <>
            <div className="onboarding__section">
              <span id="onboarding-access-label" className="onboarding__label">
                Permissions
              </span>
              <div
                role="radiogroup"
                aria-labelledby="onboarding-access-label"
                className="onboarding__options"
              >
                {ACCESS_ORDER.map((value) => (
                  <Option
                    key={value}
                    name="onboarding-access"
                    checked={access === value}
                    onChange={() => setAccess(value)}
                    title={ACCESS_PROFILES[value].label}
                    summary={ACCESS_PROFILES[value].summary}
                  />
                ))}
              </div>
              <p className="onboarding__hint">{ACCESS_PROFILES[access].risk}</p>
            </div>
            <label className="onboarding__option onboarding__options">
              <input
                type="checkbox"
                checked={browseWeb}
                onChange={(event) => setBrowseWeb(event.target.checked)}
              />
              <span className="onboarding__option-text">
                <span className="onboarding__option-title">
                  Let anima read the web
                </span>
                <span className="onboarding__option-desc">
                  Open and read pages. Clicking and downloads stay off.
                </span>
              </span>
              <span className="onboarding__switch" aria-hidden="true" />
            </label>
          </>
        )}
      </>
    );
    footer = (
      <>
        {back('you')}
        {primary(inspecting ? 'Checking…' : 'Continue', accessReady && !preview)}
      </>
    );
  } else {
    body = (
      <>
        {heading(
          'Talk out loud?',
          'Pick a voice and check your mic. You can always type instead.',
        )}
        <VoiceCheck
          voiceURI={voiceURI}
          onVoiceChange={setVoiceURI}
          sample={`Hi${ownerName.trim() ? ` ${ownerName.trim()}` : ''}. This is how I'll sound.`}
          onSpeakingChange={setSpeakingVoice}
        />
      </>
    );
    footer = (
      <>
        <button
          type="button"
          className="onboarding__link"
          disabled={!ready || busy}
          onClick={() => void submit(false)}
        >
          Skip, I'll type
        </button>
        {primary(creating ? 'Setting things up…' : 'Finish setup', ready)}
      </>
    );
  }

  return (
    <main className="onboarding">
      <section className="onboarding__panel">
        <header>
          <div className="onboarding__brand">
            <span className="onboarding__wordmark">anima</span>
            {flowIndex >= 0 && (
              <span className="onboarding__counter">
                Step {flowIndex + 1} of {FLOW.length}
              </span>
            )}
          </div>
          {flowIndex >= 0 && (
            <ol className="onboarding__progress" aria-label="Setup progress">
              {FLOW.map((item, index) => (
                <li
                  key={item}
                  data-state={
                    index < flowIndex
                      ? 'complete'
                      : index === flowIndex
                        ? 'current'
                        : 'upcoming'
                  }
                  aria-current={index === flowIndex ? 'step' : undefined}
                >
                  <span className="sr-only">{item}</span>
                </li>
              ))}
            </ol>
          )}
        </header>
        <form
          className="onboarding__form"
          aria-labelledby="onboarding-step-heading"
          onSubmit={(event) => {
            event.preventDefault();
            void next();
          }}
        >
          <fieldset
            disabled={creating}
            className="onboarding__body"
            style={{ border: 0, margin: 0, minWidth: 0 }}
          >
            {body}
            {error && !preview && (
              <p role="alert" className="onboarding__alert">
                {error}
              </p>
            )}
          </fieldset>
          <div className="onboarding__footer">{footer}</div>
        </form>
      </section>
      <div className="onboarding__scene" aria-hidden="true">
        <HaloOrb energy={0.45} className="onboarding__orb" />
        {step === 'you' && (
          <div className="onboarding__preview">
            <span className="onboarding__kicker">Preview</span>
            <div className="onboarding__preview-message">
              <span className="onboarding__avatar">
                {avatar ? <img src={avatar.url} alt="" /> : null}
              </span>
              <div>
                <strong style={{ fontSize: 13 }}>
                  {name.trim() || 'Anima'}
                </strong>
                <p>{greeting(ownerName.trim(), personality)}</p>
              </div>
            </div>
          </div>
        )}
        {step === 'voice' && speakingVoice && (
          <div className="onboarding__preview">
            <span className="onboarding__kicker">Now playing · {speakingVoice}</span>
            <div className="onboarding__preview-message">
              <span className="onboarding__avatar">
                {avatar ? <img src={avatar.url} alt="" /> : null}
              </span>
              <p>“Hi{ownerName.trim() ? ` ${ownerName.trim()}` : ''}. This is how I'll sound.”</p>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}
