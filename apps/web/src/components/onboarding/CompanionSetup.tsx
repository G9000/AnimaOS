import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import {
  daemon,
  installKnowHow,
  MODEL_SUGGESTIONS,
  type CodingApp,
  type CodingKnowHow,
  type DaemonProvider,
  type DaemonSnapshot,
  type DaemonWorkspaceState,
  type WorkspaceInspectFound,
} from '../../lib/daemon-api';
import {
  toolNamesForProfile,
  type AccessProfile,
} from '../../lib/agent-access';
import { savePreferredVoice } from '../../lib/voice-preference';
import { ChatGptConnection } from '../ChatGptConnection';
import { ChatGptModelSelect } from '../ChatGptModelSelect';
import {
  ArrowRightIcon,
  BoltIcon,
  ChevronIcon,
  EyeIcon,
  FolderIcon,
  GlobeIcon,
  HardDriveIcon,
  KeyRoundIcon,
  MoonIcon,
  PencilIcon,
  RocketIcon,
  ShieldCheckIcon,
  SlidersIcon,
  SquareDotIcon,
  SquareIcon,
  TerminalIcon,
  UserRoundIcon,
} from '../icons';
import { AddKeyForm } from '../models/AddKeyForm';
import { ResumeCard } from './ResumeCard';
import { MicCheck, VoiceCheck } from './VoiceCheck';
import { OnboardingScene } from './OnboardingScene';
import './companion-onboarding.css';
import { useResetOnChange } from '../../hooks/useResetOnChange';

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
const FLOW_LABEL: Record<string, string> = {
  model: 'Brain',
  you: 'You',
  access: 'Access',
  voice: 'Voice',
};
const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];
type BrowserLevel = 'off' | 'read' | 'act' | 'files';
const BROWSER_LEVELS: readonly {
  value: BrowserLevel;
  title: string;
  summary: string;
  tools: readonly string[];
}[] = [
  { value: 'off', title: 'Off', summary: 'No web at all.', tools: [] },
  {
    value: 'read',
    title: 'Read pages',
    summary: 'Search and read. The default.',
    tools: ['browser_navigate', 'browser_observe'],
  },
  {
    value: 'act',
    title: 'Click and type',
    summary: 'Fill forms, press buttons. Asks before submitting.',
    tools: ['browser_navigate', 'browser_observe', 'browser_act'],
  },
  {
    value: 'files',
    title: 'Files too',
    summary: 'Download and upload files.',
    tools: [
      'browser_navigate',
      'browser_observe',
      'browser_act',
      'browser_transfer',
    ],
  },
];
const BROWSER_SUMMARY: Record<BrowserLevel, string> = {
  off: 'No web access.',
  read: 'Reads web pages. Can’t click or download.',
  act: 'Click and type · asks before submitting.',
  files: 'Click, type, download and upload files.',
};

/** What each level lets anima do, in plain words, before the owner picks. */
const ACCESS_LEVELS: Record<
  AccessProfile,
  {
    title: string;
    summary: string;
    icon: ReactNode;
    can: readonly string[];
    cannot: readonly string[];
    warning?: string;
  }
> = {
  observe: {
    title: 'Look only',
    summary: 'Reads, never touches.',
    icon: <EyeIcon size={16} />,
    can: [
      'Read files in its home folder',
      'Search its memory and your past chats',
      'Read your todos',
    ],
    cannot: ['Change or delete any file', 'Run programs or commands'],
  },
  collaborate: {
    title: 'Edit files',
    summary: 'Reads and writes in its folder. Runs nothing.',
    icon: <PencilIcon size={16} />,
    can: [
      'Everything in Look only',
      'Create and edit files in its home folder',
      'Keep todos, reminders and automations',
      'Remember things about you',
    ],
    cannot: ['Run commands or programs', 'Touch files outside its folder'],
  },
  operate: {
    title: 'Run commands',
    summary: 'Full hands on this computer.',
    icon: <TerminalIcon size={16} />,
    can: [
      'Everything in Edit files',
      'Run terminal commands',
      'Start background jobs and apps',
      'Hand work to coding agents like Claude Code or Codex',
    ],
    cannot: ['Ignore your ground rules'],
    warning:
      'Commands can do anything you can. Pick this if you’d hand a capable intern your terminal.',
  },
};
const ACCESS_NOTE =
  'Always asks before deleting things, going outside its folder, or sending anything as you.';

/** The "Coder" starting point: Operate, the coding apps' know-how, and a
 *  persona that hands implementation to them and verifies their work. */
export const CODER_TEMPLATE = {
  name: 'Coder',
  purpose:
    'Hands coding work to the coding apps on this computer, then reads the changes and runs the checks itself.',
  instruction:
    'You delegate implementation to the coding apps on this computer (such as Claude Code, Codex, OpenCode or Gemini CLI) and verify their work: read the diff and run the checks yourself before you report. Never type credentials into an app; if one is not signed in, ask the owner to sign in.',
} as const;

type CodingApps = { apps: CodingApp[]; handoff: CodingKnowHow | null };

function codingAppState(app: CodingApp): string {
  if (app.found && !app.problem)
    return app.version ? `Found · ${app.version}` : 'Found';
  return app.problem ? 'Needs reinstall' : 'Not found';
}

const SOURCES: Record<
  ModelSource,
  { title: string; summary: string; icon: ReactNode }
> = {
  local: {
    title: 'Run it locally',
    summary: 'Ollama, vLLM, LM Studio, anything OpenAI-compatible.',
    icon: <HardDriveIcon size={15} />,
  },
  chatgpt: {
    title: 'Use my ChatGPT plan',
    summary: 'Sign in with the plan you already pay for.',
    icon: <UserRoundIcon size={15} />,
  },
  key: {
    title: 'Paste an API key',
    summary: 'OpenAI, Anthropic, Google, OpenRouter. They bill you, not us.',
    icon: <KeyRoundIcon size={15} />,
  },
};
const SOURCE_GROUPS: readonly {
  label: string;
  sources: readonly ModelSource[];
}[] = [
  { label: 'Total freedom · your machine', sources: ['local'] },
  { label: 'Join the corpo · cloud', sources: ['chatgpt', 'key'] },
];

const PERSONALITIES: Record<
  Personality,
  {
    label: string;
    summary: string;
    instruction: string;
    icon: ReactNode;
    traits: readonly (readonly [string, string])[];
    reply: string;
  }
> = {
  quiet: {
    label: 'Quiet',
    summary: 'Waits for you. Short answers.',
    instruction:
      'Wait for the owner to ask before suggesting more work. Keep answers short.',
    icon: <MoonIcon size={15} />,
    traits: [
      ['Starts things', 'Only when you ask'],
      ['Answers', 'A line or two'],
      ['Explains its thinking', 'Only if you ask'],
    ],
    reply: 'Deploy failed: migration 0042 timed out. Want me to retry it?',
  },
  balanced: {
    label: 'Balanced',
    summary: 'Suggests next steps when useful.',
    instruction:
      'Suggest a useful next step when it helps, and keep answers concise.',
    icon: <SlidersIcon size={15} />,
    traits: [
      ['Starts things', 'When it’s clearly useful'],
      ['Answers', 'Short, longer when needed'],
      ['Explains its thinking', 'When it matters'],
    ],
    reply:
      'Migration 0042 timed out again, same as Tuesday. I can retry with a longer timeout or roll back. Retry?',
  },
  driven: {
    label: 'Driven',
    summary: 'Takes the lead and explains its thinking.',
    instruction:
      'Take the lead on clearly useful next steps and briefly explain your reasoning, while staying within granted permissions.',
    icon: <RocketIcon size={15} />,
    traits: [
      ['Starts things', 'Takes the next step itself'],
      ['Answers', 'As long as it’s useful'],
      ['Explains its thinking', 'Always, briefly'],
    ],
    reply:
      'Migration 0042 timed out again, second time this week, so I raised the timeout to 120 s and reran it. The real cause looks like the missing index on events. Want a fix for that?',
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
  return 'Add an API key below to use it';
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

/** One choice in a list. The chosen one is the live item: tinted, with square
 *  handles, and it can open a panel of its own right underneath. */
function Option({
  name,
  checked,
  disabled,
  onChange,
  title,
  summary,
  icon,
  mark,
  detail,
}: {
  name: string;
  checked: boolean;
  disabled?: boolean;
  onChange(): void;
  title: ReactNode;
  summary?: string;
  icon?: ReactNode;
  /** A square radio on the right; choices with an icon go without. */
  mark?: ReactNode;
  detail?: ReactNode;
}) {
  return (
    <div className="onboarding__choice" data-checked={checked || undefined}>
      <label className="onboarding__option">
        <input
          type="radio"
          name={name}
          checked={checked}
          disabled={disabled}
          onChange={onChange}
        />
        {icon ? (
          <span className="onboarding__icon" aria-hidden="true">
            {icon}
          </span>
        ) : null}
        <span className="onboarding__option-text">
          <span className="onboarding__option-title">{title}</span>
          {summary ? (
            <span className="onboarding__option-desc">{summary}</span>
          ) : null}
        </span>
        {mark ?? (icon ? null : <span className="onboarding__mark" />)}
      </label>
      {detail ? <div className="onboarding__detail">{detail}</div> : null}
    </div>
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
  const [sourceChosen, setSourceChosen] = useState(false);
  const [modelScope, setModelScope] = useState({ revision: 0, pending: false });
  const [access, setAccess] = useState<AccessProfile>('collaborate');
  // How much it asks, separate from what it can use; new agents start Free.
  const [autonomy, setAutonomy] = useState<'free' | 'strict'>('free');
  const [coder, setCoder] = useState(false);
  const [purpose, setPurpose] = useState('');
  const [codingApps, setCodingApps] = useState<CodingApps | null>(null);
  const [codingAppsError, setCodingAppsError] = useState(false);
  // Set when the agent exists but some know-how did not install: a quiet
  // note, then Continue.
  const [knowHowNote, setKnowHowNote] = useState<{
    note: string;
    agent: DaemonSnapshot;
  } | null>(null);
  // Small windows have no stage, so the approval choice sits in the card.
  const [narrow, setNarrow] = useState(
    () =>
      typeof window !== 'undefined' &&
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(max-width: 900px)').matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const query = window.matchMedia('(max-width: 900px)');
    const update = () => setNarrow(query.matches);
    query.addEventListener?.('change', update);
    return () => query.removeEventListener?.('change', update);
  }, []);
  // Personality opens to show how it answers once the owner picks one.
  const [personalityOpen, setPersonalityOpen] = useState(false);
  const [browser, setBrowser] = useState<BrowserLevel>('read');
  const [browserOpen, setBrowserOpen] = useState(false);
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

  useResetOnChange([loadAttempt], () => setWorkspaceError(null));

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    let current = true;
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
  useResetOnChange([providers, provider, sourceChosen, source], () => {
    if (provider || sourceChosen) return;
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
  });
  useResetOnChange([providers, source, provider], () => {
    // Connecting ChatGPT while its option is open selects the subscription.
    const chatGpt = providers?.find((item) => item.id === 'chatgpt');
    if (source === 'chatgpt' && chatGpt?.configured && provider !== 'chatgpt') {
      setProvider('chatgpt');
      setModel('');
      setCustomModel('');
    }
  });
  useEffect(
    () => () => {
      if (avatar) URL.revokeObjectURL(avatar.url);
    },
    [avatar],
  );
  useResetOnChange([coder], () => {
    if (coder) setCodingAppsError(false);
  });
  useEffect(() => {
    headingRef.current?.focus();
  }, [step]);
  useEffect(() => {
    if (!coder) return;
    let current = true;
    daemon.codingApps().then(
      (result) => {
        if (current)
          setCodingApps({ apps: result.apps, handoff: result.handoff ?? null });
      },
      () => {
        if (current) setCodingAppsError(true);
      },
    );
    return () => {
      current = false;
    };
  }, [coder]);
  const startFromCoder = () => {
    setCoder(true);
    setName(CODER_TEMPLATE.name);
    setPurpose(CODER_TEMPLATE.purpose);
    setAccess('operate');
  };
  const startBlank = () => {
    setCoder(false);
    setName('Anima');
    setPurpose('');
    setAccess('collaborate');
  };

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
    setSourceChosen(true);
    setSource(value);
    if (
      provider &&
      providers?.some(
        (item) => item.id === provider && sourceOf(item) === value,
      )
    )
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
      ...(purpose.trim() ? [`What you are for: ${purpose.trim()}`] : []),
      ...(coder ? [CODER_TEMPLATE.instruction] : []),
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
          ...(BROWSER_LEVELS.find((level) => level.value === browser)?.tools ??
            []),
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
        await daemon
          .setAgentAvatar(agentId, avatar.file)
          .catch(() => undefined);
      }
      if (agentId) {
        // The daemon starts new agents Free; the choice is saved either way,
        // and one that fails to save never blocks the new agent.
        await daemon
          .setAutonomy(agentId, { mode: autonomy })
          .catch(() => undefined);
      }
      const missed =
        agentId && coder ? await installCodingKnowHow(agentId) : [];
      if (keepVoice) savePreferredVoice(voiceURI);
      if (!mounted.current) return;
      if (missed.length > 0)
        setKnowHowNote({
          note: `${result.agent?.state?.name ?? name.trim()} is ready, but know-how for ${missed.join(', ')} didn't install. You can add it later in Settings › Tools › Community bundles.`,
          agent: result.agent,
        });
      else onCreated(result.agent);
    } catch (caught) {
      if (mounted.current) setError(errorText(caught));
    } finally {
      submitting.current = false;
      if (mounted.current) setCreating(false);
    }
  };
  /** Installs know-how for every coding app that is found and starts,
   *  plus the generic handoff; returns what did not install. A broken
   *  install gets none: the agent could not use it anyway. */
  const installCodingKnowHow = async (agentId: string): Promise<string[]> => {
    const found =
      codingApps?.apps.filter((app) => app.found && !app.problem) ?? [];
    const bundles = [
      ...found.map((app) => ({ label: app.name, manifest: app.manifestText })),
      ...(codingApps?.handoff
        ? [
            {
              label: 'handing off work',
              manifest: codingApps.handoff.manifestText,
            },
          ]
        : []),
    ];
    const missed: string[] = [];
    for (const bundle of bundles) {
      try {
        await installKnowHow(agentId, bundle.manifest);
      } catch {
        missed.push(bundle.label);
      }
    }
    if (codingAppsError) missed.push('the coding apps');
    return missed;
  };
  const next = async () => {
    if (preview || knowHowNote) return;
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
  const sources = new Set(
    (['chatgpt', 'key', 'local'] as const).filter((value) =>
      providers?.some((item) => sourceOf(item) === value),
    ),
  );
  const sourceProviders = (providers ?? [])
    .filter(
      (item) => source && item.id !== 'chatgpt' && sourceOf(item) === source,
    )
    .sort((left, right) => Number(right.configured) - Number(left.configured));
  // Providers that only need a key from the owner.
  const keyProviders = sourceProviders.filter(
    (item) => item.requiresKey && !item.configured,
  );

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
        <span className="onboarding__field">
          <FolderIcon size={15} aria-hidden="true" />
          <input
            id="companion-folder"
            className="onboarding__input onboarding__input--mono"
            value={rootPath}
            readOnly={workspace?.configured}
            disabled={busy}
            onChange={(event) => changeRootPath(event.target.value)}
            spellCheck={false}
          />
        </span>
        {!workspace?.configured && (
          <button
            type="button"
            onClick={() => void browseFolder()}
            disabled={browsing || inspecting || busy}
            className="onboarding__button onboarding__button--secondary"
          >
            {browsing ? 'Choosing…' : 'Browse'}
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
  let extras: ReactNode = null;
  const primary = (label: string, enabled: boolean) => (
    <button
      type="submit"
      disabled={!enabled || busy}
      className="onboarding__button onboarding__button--primary"
    >
      {label}
      <ArrowRightIcon size={15} />
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

  const autonomyChoice = (
    <div className="onboarding__section">
      <span
        id="onboarding-autonomy-label"
        className="onboarding__mono onboarding__mono--lead"
      >
        When it checks with you
      </span>
      <div
        role="radiogroup"
        aria-labelledby="onboarding-autonomy-label"
        className="onboarding__options"
      >
        {(
          [
            {
              value: 'free',
              icon: <BoltIcon size={16} />,
              title: (
                <>
                  Only for risky things
                  <span className="onboarding__tag">Recommended</span>
                </>
              ),
              summary:
                'Works on its own and tells you what it did. Risky things still ask.',
            },
            {
              value: 'strict',
              icon: <ShieldCheckIcon size={16} />,
              title: 'Before every change',
              summary: 'Asks before it changes or runs anything.',
            },
          ] as const
        ).map((choice) => (
          <Option
            key={choice.value}
            name="onboarding-autonomy"
            checked={autonomy === choice.value}
            onChange={() => setAutonomy(choice.value)}
            title={choice.title}
            summary={choice.summary}
            icon={choice.icon}
            mark={
              <span className="onboarding__square" aria-hidden="true">
                {autonomy === choice.value ? (
                  <SquareDotIcon size={16} />
                ) : (
                  <SquareIcon size={16} />
                )}
              </span>
            }
          />
        ))}
      </div>
    </div>
  );

  /** What opens under the chosen way of reaching a model. */
  const sourceDetail = (value: ModelSource): ReactNode => {
    if (value === 'chatgpt') {
      return chatGpt ? (
        <div className="onboarding__section">
          <ChatGptConnection
            compact
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
      ) : null;
    }
    return (
      <>
        <div className="onboarding__section">
          <span id="onboarding-provider-label" className="onboarding__mono">
            Provider
          </span>
          <div
            role="radiogroup"
            aria-labelledby="onboarding-provider-label"
            className="onboarding__options onboarding__options--inner"
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
          {value === 'key' && keyProviders.length > 0 && (
            <>
              <span className="onboarding__mono">Add API key</span>
              <AddKeyForm
                providers={keyProviders}
                buttonLabel="Add API key"
                variant="onboarding"
                warnIfNotPersisted
                onAdded={refreshProviders}
              />
            </>
          )}
        </div>
        {provider && (
          <div className="onboarding__section">
            <label htmlFor="onboarding-model" className="onboarding__mono">
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
                  className="onboarding__mono"
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
    );
  };

  if (knowHowNote) {
    body = (
      <>
        {heading('Almost there', 'One thing to know before you start.')}
        <p className="onboarding__hint" role="status">
          {knowHowNote.note}
        </p>
      </>
    );
    footer = (
      <button
        type="button"
        className="onboarding__button onboarding__button--primary"
        onClick={() => onCreated(knowHowNote.agent)}
      >
        Continue
        <ArrowRightIcon size={15} />
      </button>
    );
  } else if (step === 'welcome') {
    const host = typeof window === 'undefined' ? null : window.location;
    const onThisComputer =
      !host || ['localhost', '127.0.0.1', '[::1]'].includes(host.hostname);
    body = (
      <>
        {heading(
          "I'm at your command",
          "It runs on your own machine and answers to you. No account with us, no data sold, no rules you didn't set. Call it whatever you want.",
          true,
        )}
        {workspaceError && (
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
        )}
      </>
    );
    footer = (
      <>
        {workspaceError ? (
          <span />
        ) : workspace ? (
          <div className="ob-runtime" role="status">
            <span className="ob-runtime__label">
              {onThisComputer
                ? 'Running on this computer'
                : 'Running on your server'}
            </span>
            {host && <span className="ob-runtime__addr">{host.host}</span>}
          </div>
        ) : (
          <p role="status" className="onboarding__status" data-tone="pending">
            Loading your workspace…
          </p>
        )}
        {primary('Set it up', workspaceReady)}
      </>
    );
    extras = (
      <>
        <div className="ob-import" aria-disabled="true">
          <div>
            <strong>Leaving OpenClaw or Hermes?</strong>
            <span>Bring its memory, skills and chats.</span>
          </div>
          <em>Soon</em>
        </div>
        {!workspace?.configured && (
          <button
            type="button"
            className="ob-open"
            disabled={!workspaceReady}
            onClick={() => goTo('existing')}
          >
            Open an existing workspace
            <ArrowRightIcon size={11} />
          </button>
        )}
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
          'Choose a brain.',
          'Total freedom on your own machine, or join the corpo in the cloud. Swap it whenever.',
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
              <p
                role="status"
                className="onboarding__status"
                data-tone="pending"
              >
                Refreshing providers…
              </p>
            )}
            <div
              role="radiogroup"
              aria-label="Model source"
              className="onboarding__options"
            >
              {SOURCE_GROUPS.map((group) => {
                const available = group.sources.filter((value) =>
                  sources.has(value),
                );
                if (!available.length) return null;
                return (
                  <div key={group.label} className="onboarding__group">
                    <span className="onboarding__group-label">
                      {group.label}
                    </span>
                    {available.map((value) => (
                      <Option
                        key={value}
                        name="onboarding-model-source"
                        checked={source === value}
                        onChange={() => chooseSource(value)}
                        title={SOURCES[value].title}
                        summary={SOURCES[value].summary}
                        icon={SOURCES[value].icon}
                        detail={source === value ? sourceDetail(value) : null}
                      />
                    ))}
                  </div>
                );
              })}
            </div>
            {provider && selectedProvider && !selectedProvider.configured && (
              <p role="status" className="onboarding__notice">
                {selectedProvider.label} is unavailable right now. Reconnect it,
                or choose another option to continue.
              </p>
            )}
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
        <p className="onboarding__hint onboarding__template">
          {coder ? (
            <>
              Starting from Coder ·{' '}
              <button
                type="button"
                className="onboarding__link"
                onClick={startBlank}
              >
                Start blank
              </button>
            </>
          ) : (
            <>
              Start from:{' '}
              <button
                type="button"
                className="onboarding__link"
                onClick={startFromCoder}
              >
                Coder
              </button>
            </>
          )}
        </p>
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
          <div className="onboarding__row onboarding__row--agent">
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
                <label className="onboarding__link onboarding__link--small">
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
                    className="onboarding__link onboarding__link--small"
                    onClick={() => setAvatar(null)}
                  >
                    Use the orb
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
        {(coder || purpose) && (
          <div className="onboarding__section">
            <label htmlFor="companion-purpose" className="onboarding__label">
              What it's for
            </label>
            <textarea
              id="companion-purpose"
              className="onboarding__input"
              rows={3}
              maxLength={600}
              value={purpose}
              onChange={(event) => setPurpose(event.target.value)}
            />
          </div>
        )}
        <div className="onboarding__section">
          <span id="onboarding-personality-label" className="onboarding__label">
            Personality
          </span>
          <div
            role="radiogroup"
            aria-labelledby="onboarding-personality-label"
            className="onboarding__options"
          >
            {(Object.keys(PERSONALITIES) as Personality[]).map((value) => {
              const trait = PERSONALITIES[value];
              return (
                <Option
                  key={value}
                  name="onboarding-personality"
                  checked={personality === value}
                  onChange={() => {
                    setPersonality(value);
                    setPersonalityOpen(true);
                  }}
                  title={trait.label}
                  summary={trait.summary}
                  icon={trait.icon}
                  detail={
                    personality === value && personalityOpen ? (
                      <>
                        <dl className="ob-traits">
                          {trait.traits.map(([label, text]) => (
                            <div key={label}>
                              <dt>{label}</dt>
                              <dd>{text}</dd>
                            </div>
                          ))}
                        </dl>
                        <span className="onboarding__mono">
                          Sample · you say
                        </span>
                        <p className="ob-bubble">The deploy failed again.</p>
                        <span className="onboarding__mono">It answers</span>
                        <p className="ob-bubble ob-bubble--reply">
                          {trait.reply}
                        </p>
                        {value === 'driven' && (
                          <p className="onboarding__hint">
                            Still only does what Access allows. Risky steps
                            still ask.
                          </p>
                        )}
                      </>
                    ) : null
                  }
                />
              );
            })}
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
                What it can do
              </span>
              <div
                role="radiogroup"
                aria-labelledby="onboarding-access-label"
                className="onboarding__options"
              >
                {ACCESS_ORDER.map((value) => {
                  const level = ACCESS_LEVELS[value];
                  return (
                    <Option
                      key={value}
                      name="onboarding-access"
                      checked={access === value}
                      onChange={() => setAccess(value)}
                      title={
                        <>
                          {level.title}
                          {value === 'collaborate' && (
                            <span className="onboarding__tag">Recommended</span>
                          )}
                        </>
                      }
                      summary={level.summary}
                      icon={level.icon}
                      detail={
                        access === value ? (
                          <>
                            <span className="onboarding__mono">Can</span>
                            <ul className="ob-list" data-kind="can">
                              {level.can.map((line) => (
                                <li key={line}>{line}</li>
                              ))}
                            </ul>
                            <span className="onboarding__mono">Can’t</span>
                            <ul className="ob-list" data-kind="cannot">
                              {level.cannot.map((line) => (
                                <li key={line}>{line}</li>
                              ))}
                            </ul>
                            {level.warning && (
                              <p className="onboarding__notice">
                                {level.warning}
                              </p>
                            )}
                            <p className="onboarding__hint">{ACCESS_NOTE}</p>
                          </>
                        ) : null
                      }
                    />
                  );
                })}
              </div>
            </div>
            {coder && (
              <div className="onboarding__section">
                <span className="onboarding__label">
                  Coding apps on this computer{' '}
                  <span className="onboarding__hint">
                    know-how installs for the ones found
                  </span>
                </span>
                {codingAppsError ? (
                  <p className="onboarding__hint">
                    Couldn't check for coding apps. You can add their know-how
                    later in Settings › Tools.
                  </p>
                ) : codingApps === null ? (
                  <p className="onboarding__hint">Looking for coding apps…</p>
                ) : (
                  <ul
                    className="coding-app-chips"
                    aria-label="Coding apps on this computer"
                  >
                    {codingApps.apps.map((app) => {
                      const state = codingAppState(app);
                      return (
                        <li
                          key={app.id}
                          className="coding-app-chip"
                          data-state={
                            app.found && !app.problem
                              ? 'found'
                              : app.problem
                                ? 'problem'
                                : 'missing'
                          }
                        >
                          <span className="coding-app-chip-name">
                            <span aria-hidden="true">
                              {app.found && !app.problem
                                ? '✓'
                                : app.problem
                                  ? '!'
                                  : '○'}
                            </span>{' '}
                            {app.name}
                          </span>
                          <span className="coding-app-chip-state">{state}</span>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
            )}
            <div className="onboarding__options">
              <div
                className="onboarding__choice onboarding__choice--browser"
                data-open={browserOpen || undefined}
              >
                <div className="onboarding__option">
                  <span className="onboarding__icon" aria-hidden="true">
                    <GlobeIcon size={16} />
                  </span>
                  <span className="onboarding__option-text">
                    <span className="onboarding__option-title">Browser</span>
                    <span className="onboarding__option-desc">
                      {BROWSER_SUMMARY[browser]}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="onboarding__action"
                    aria-expanded={browserOpen}
                    aria-controls="onboarding-browser-levels"
                    onClick={() => setBrowserOpen((open) => !open)}
                  >
                    Customize
                    {browserOpen && <ChevronIcon size={13} />}
                  </button>
                </div>
                {browserOpen && (
                  <div
                    id="onboarding-browser-levels"
                    className="onboarding__detail"
                  >
                    <span
                      id="onboarding-browser-label"
                      className="onboarding__mono"
                    >
                      What it can do online
                    </span>
                    <div
                      role="radiogroup"
                      aria-labelledby="onboarding-browser-label"
                      className="onboarding__options onboarding__options--inner"
                    >
                      {BROWSER_LEVELS.map((level) => (
                        <Option
                          key={level.value}
                          name="onboarding-browser"
                          checked={browser === level.value}
                          onChange={() => setBrowser(level.value)}
                          title={level.title}
                          summary={level.summary}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </div>
            {narrow && autonomyChoice}
          </>
        )}
      </>
    );
    footer = (
      <>
        {back('you')}
        {primary(
          inspecting ? 'Checking…' : 'Continue',
          accessReady && !preview,
        )}
      </>
    );
  } else {
    body = (
      <>
        {heading('Give it a voice.', "Or don't. Typing works just as well.")}
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
        {primary(creating ? 'Setting things up…' : 'Finish', ready)}
      </>
    );
  }

  return (
    <main className="onboarding" data-step={step}>
      <span className="onboarding__wordmark">anima</span>
      <div className="onboarding__dock">
        <section className="onboarding__panel">
          <header>
            <div className="onboarding__steps">
              <span className="onboarding__trail" aria-hidden="true">
                {FLOW.map((item, index) => (
                  <span
                    key={item}
                    data-state={
                      index < flowIndex
                        ? 'complete'
                        : index === flowIndex
                          ? 'current'
                          : 'upcoming'
                    }
                  >
                    {index > 0 && <i>/</i>}
                    {FLOW_LABEL[item]}
                  </span>
                ))}
              </span>
              <span className="onboarding__counter">
                {String(Math.max(flowIndex + 1, 0)).padStart(2, '0')} /{' '}
                {String(FLOW.length).padStart(2, '0')}
                {flowIndex >= 0 && (
                  <span className="sr-only">
                    Step {flowIndex + 1} of {FLOW.length}
                  </span>
                )}
              </span>
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
        {extras}
      </div>
      <OnboardingScene
        step={step}
        modelLabel={resolvedModel}
        sourceLabel={selectedProvider?.label ?? ''}
        rootPath={rootPath}
      >
        {step === 'you' && (
          <div
            className="ob-float ob-preview"
            style={{ '--x': 968, '--y': 610, '--w': 300 } as CSSProperties}
          >
            <span className="onboarding__kicker">Preview</span>
            <div className="ob-preview__message">
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
        {step === 'access' && !narrow && !preview && (
          <div
            className="ob-float ob-approval"
            style={{ '--x': 938, '--y': 540, '--w': 340 } as CSSProperties}
          >
            {autonomyChoice}
          </div>
        )}
        {step === 'voice' && (
          <>
            {speakingVoice && (
              <div
                className="ob-float ob-playing"
                style={{ '--x': 242, '--y': 684, '--w': 330 } as CSSProperties}
              >
                <div className="ob-playing__row">
                  <span>✦ Speaking · {speakingVoice}</span>
                </div>
                <span className="ob-wave" aria-hidden="true">
                  {Array.from({ length: 70 }, (_, index) => (
                    <i
                      key={index}
                      style={
                        {
                          '--i': index,
                          '--h': 4 + ((index * 37) % 17),
                        } as CSSProperties
                      }
                    />
                  ))}
                </span>
                <p>
                  “Hi{ownerName.trim() ? ` ${ownerName.trim()}` : ''}. This is
                  how I'll sound.”
                </p>
              </div>
            )}
            <MicCheck />
          </>
        )}
      </OnboardingScene>
    </main>
  );
}
