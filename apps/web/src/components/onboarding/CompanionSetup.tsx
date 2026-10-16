import { useEffect, useRef, useState } from 'react';
import {
  daemon,
  MODEL_SUGGESTIONS,
  type DaemonWorkspaceState,
  type WorkspaceInspectFound,
} from '../../lib/daemon-api';
import {
  ACCESS_PROFILES,
  toolNamesForProfile,
  type AccessProfile,
} from '../../lib/agent-access';
import type { OnboardingFlowProps } from './OnboardingFlow';
import { ModelStep } from './ModelStep';
import { AccessStep } from './AccessStep';
import { ResumeCard } from './ResumeCard';

export function CompanionSetup({
  providers,
  providersError,
  retryProviders,
  onCreated,
}: OnboardingFlowProps) {
  const [name, setName] = useState('Anima');
  const [priorities, setPriorities] = useState('');
  const [provider, setProvider] = useState('');
  const [model, setModel] = useState('');
  const [customModel, setCustomModel] = useState('');
  const [access, setAccess] = useState<AccessProfile>('collaborate');
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
        if (current)
          setWorkspaceError(
            caught instanceof Error ? caught.message : String(caught),
          );
      });
    return () => {
      current = false;
    };
  }, [loadAttempt]);
  useEffect(() => {
    if (provider) return;
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
      setModel(
        available.id === 'chatgpt'
          ? ''
          : (MODEL_SUGGESTIONS[available.id]?.[0] ?? '__custom__'),
      );
    }
  }, [providers, provider]);

  const refreshProviders = async () => {
    if (retryingRef.current) return;
    retryingRef.current = true;
    setRetrying(true);
    setError(null);
    try {
      await retryProviders();
    } catch (caught) {
      if (mounted.current)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      retryingRef.current = false;
      if (mounted.current) setRetrying(false);
    }
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
        setError(caught instanceof Error ? caught.message : String(caught));
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
      if (mounted.current)
        setError(caught instanceof Error ? caught.message : String(caught));
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
      if (mounted.current)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (mounted.current) setResuming(false);
    }
  };
  const ready =
    !!workspace &&
    !workspaceError &&
    !providersError &&
    !retrying &&
    !browsing &&
    !inspecting &&
    !!providers?.some((item) => item.id === provider && item.configured) &&
    !!resolvedModel &&
    !!name.trim() &&
    !!rootPath.trim();
  const submit = async () => {
    if (!ready || preview || submitting.current) return;
    submitting.current = true;
    setCreating(true);
    setError(null);
    const selectedRoot = rootPath.trim();
    const system = [
      `You are ${name.trim()}, the owner's personal companion. Keep a consistent identity across conversations and channels.`,
      'Help with everyday plans, research, files, reminders, and connected tools. Be warm, concise, practical, and honest about what you can actually do.',
      'Use available memory tools to recall relevant preferences. Distinguish facts the owner stated from your own guesses and accept corrections.',
      'Handle simple work yourself. When available and useful, spawn or reuse a helper for a bounded part of the current task. Remain responsible for checking the result and responding to the owner. Helpers cannot gain permissions you do not have.',
      'Only act within the requested scope and granted tool permissions. Ask before spending money, publishing, deleting important data, or sending consequential external messages. Never claim a task, reminder, delegation, or recovery succeeded without a recorded result.',
      'Background work requires an actual daemon job or schedule. Do not promise to keep working after a reply unless such work has been registered.',
      ...(workspace?.workspace?.mission
        ? [`Workspace context: ${workspace.workspace.mission}`]
        : []),
      ...(priorities.trim() ? [`Owner preferences: ${priorities.trim()}`] : []),
    ].join('\n\n');
    try {
      if (!workspace?.configured) {
        const inspected = await inspectFolder(selectedRoot);
        if (
          !inspected ||
          inspected.found ||
          rootPathRef.current.trim() !== selectedRoot
        )
          return;
      }
      const config = {
        name: name.trim(),
        provider,
        model: resolvedModel,
        system,
        tools: toolNamesForProfile(access),
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
      if (mounted.current) onCreated(result.agent);
    } catch (caught) {
      if (mounted.current)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      submitting.current = false;
      if (mounted.current) setCreating(false);
    }
  };

  return (
    <main className="companion-setup min-h-0 flex-1 overflow-y-auto px-5 py-10">
      <form
        className="mx-auto max-w-xl space-y-8"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <header>
          <p className="mb-4 text-sm text-ink-3">✳ Anima</p>
          <h1 className="text-3xl font-semibold tracking-tight">
            Set up your companion
          </h1>
          <p className="mt-3 text-sm leading-relaxed text-ink-2">
            One companion, your own space. Connect a model and start a
            conversation. You can add channels and schedules later.
          </p>
        </header>
        <div>
          <label
            htmlFor="companion-folder"
            className="mb-2 block text-sm font-medium"
          >
            Workspace folder on the server
          </label>
          <div className="flex flex-wrap gap-2">
            <input
              id="companion-folder"
              className="field min-w-0 flex-1 basis-full sm:basis-0"
              value={rootPath}
              readOnly={workspace?.configured}
              disabled={creating || resuming}
              onChange={(event) => changeRootPath(event.target.value)}
              spellCheck={false}
            />
            {!workspace?.configured && (
              <>
                <button
                  type="button"
                  onClick={() => void inspectFolder(rootPath.trim())}
                  disabled={
                    !rootPath.trim() ||
                    inspecting ||
                    browsing ||
                    creating ||
                    resuming
                  }
                  className="rounded-xl border border-line px-4 py-2 text-sm font-medium disabled:opacity-50"
                >
                  {inspecting ? 'Checking…' : 'Check folder'}
                </button>
                <button
                  type="button"
                  onClick={() => void browseFolder()}
                  disabled={browsing || inspecting || creating || resuming}
                  className="rounded-xl border border-line px-4 py-2 text-sm font-medium disabled:opacity-50"
                >
                  {browsing ? 'Choosing folder…' : 'Browse…'}
                </button>
              </>
            )}
          </div>
          <p className="mt-2 text-xs text-ink-3">
            Choose a folder on the machine running Anima. Existing files are
            preserved.
          </p>
          {folderNote && !preview && (
            <p role="status" className="mt-2 text-sm text-ink-2">
              {folderNote}
            </p>
          )}
        </div>
        {preview ? (
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
        ) : (
          <fieldset disabled={creating} className="space-y-8">
            <div>
              <label
                htmlFor="companion-name"
                className="mb-2 block text-sm font-medium"
              >
                Companion name
              </label>
              <input
                id="companion-name"
                className="field w-full"
                value={name}
                maxLength={80}
                onChange={(event) => setName(event.target.value)}
                required
              />
            </div>
            <ModelStep
              providers={providers}
              catalogState={
                retrying
                  ? 'retrying'
                  : providersError
                    ? 'error'
                    : providers === null
                      ? 'loading'
                      : providers.length
                        ? 'ready'
                        : 'empty'
              }
              providerError={providersError}
              provider={provider}
              model={model}
              customModel={customModel}
              onProviderChange={(value) => {
                if (value === provider) return;
                setProvider(value);
                setModel(
                  value === 'chatgpt'
                    ? ''
                    : (MODEL_SUGGESTIONS[value]?.[0] ?? '__custom__'),
                );
                setCustomModel('');
              }}
              onModelChange={setModel}
              onCustomModelChange={setCustomModel}
              onRetryProviders={() => void refreshProviders()}
              modelSelectRef={modelSelectRef}
              customModelInputRef={customModelInputRef}
            />
            <details className="rounded-xl border border-line p-4">
              <summary className="cursor-pointer text-sm font-medium">
                Personalize and set permissions
              </summary>
              <div className="mt-5 space-y-6">
                <div>
                  <label
                    htmlFor="companion-preferences"
                    className="mb-2 block text-sm"
                  >
                    What should your companion know about you?
                  </label>
                  <textarea
                    id="companion-preferences"
                    className="field min-h-24 w-full"
                    value={priorities}
                    onChange={(event) => setPriorities(event.target.value)}
                    placeholder="Your name, priorities, and how you like to work…"
                  />
                </div>
                <AccessStep access={access} onAccessChange={setAccess} />
              </div>
            </details>
          </fieldset>
        )}
        {!workspace && !workspaceError && (
          <p role="status" className="text-sm text-ink-3">
            Loading your workspace…
          </p>
        )}
        {workspaceError && (
          <div role="alert" className="text-sm text-danger">
            <p>{workspaceError}</p>
            <button
              type="button"
              className="mt-2 underline"
              onClick={() => setLoadAttempt((value) => value + 1)}
            >
              Retry workspace
            </button>
          </div>
        )}
        {error && !preview && (
          <p role="alert" className="text-sm text-danger">
            {error}
          </p>
        )}
        {!preview && (
          <div>
            <button
              type="submit"
              disabled={!ready || creating}
              className="w-full rounded-xl bg-accent px-5 py-3 text-sm font-semibold text-accent-fg disabled:opacity-40"
            >
              {creating ? 'Setting things up…' : 'Start chatting'}
            </button>
            <p className="mt-3 text-center text-xs text-ink-3">
              {ACCESS_PROFILES[access].risk}
            </p>
          </div>
        )}
      </form>
    </main>
  );
}
