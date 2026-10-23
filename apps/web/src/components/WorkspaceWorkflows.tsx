import { useEffect, useRef, useState } from 'react';
import {
  DaemonHttpError,
  type WorkflowInput,
  type WorkflowResumeInput,
  type WorkflowRun,
  type WorkflowStepInput,
} from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { WorkflowTemplates } from './WorkflowTemplates';
import { useLatest } from '../hooks/useLatest';
import { useRenderScope } from '../hooks/useRenderScope';

const buttonClass =
  'rounded-xl border border-line px-4 py-3 text-sm disabled:opacity-50';
// Work stays open alongside the mounted chat; owner changes must refresh both.
const workflowChanges = new EventTarget();
const notifyWorkflowChange = (agentId: string) =>
  workflowChanges.dispatchEvent(new CustomEvent('change', { detail: agentId }));

const labels = {
  pending: 'Pending',
  running: 'Running',
  awaiting_approval: 'Awaiting approval',
  awaiting_input: 'Awaiting answer',
  needs_review: 'Needs review',
  completed: 'Completed',
  cancelled: 'Cancelled',
};
type Command =
  | { type: 'create'; input: WorkflowInput }
  | { type: 'resume'; id: string; input: WorkflowResumeInput }
  | { type: 'cancel'; id: string; input: { revision: number } };
type Context = ReturnType<typeof useRenderScope>;
const bytes = (text: string) => new TextEncoder().encode(text).byteLength;
const textValid = (text: string) =>
  text.trim().length > 0 && bytes(text) <= 8192;
const newKey = () => crypto.randomUUID();
const newStep = (): WorkflowStepInput => ({
  id: newKey(),
  title: '',
  kind: 'task',
  prompt: '',
});
const newDraft = (): WorkflowInput => ({
  title: '',
  requestKey: newKey(),
  steps: [newStep()],
});

function stored<T>(key: string): T | null {
  try {
    return JSON.parse(sessionStorage.getItem(key) ?? 'null') as T | null;
  } catch {
    return null;
  }
}
function save(key: string, value: unknown) {
  try {
    if (value === null) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* In-memory exact replay still works when browser storage is unavailable. */
  }
}
function definitionError(draft: WorkflowInput): string | null {
  if (!draft.title.trim() || Array.from(draft.title).length > 160)
    return 'Use a workflow title of 1–160 characters.';
  if (draft.steps.length < 1 || draft.steps.length > 16)
    return 'Use 1–16 steps.';
  for (let index = 0; index < draft.steps.length; index++) {
    const step = draft.steps[index];
    if (!step.title.trim() || Array.from(step.title).length > 160)
      return `Step ${index + 1} needs a title of 1–160 characters.`;
    if (!textValid(step.prompt))
      return `Step ${index + 1} needs a prompt of 1–8192 UTF-8 bytes.`;
  }
  if (!draft.requestKey.trim() || bytes(draft.requestKey) > 128)
    return 'Use a request key of 1–128 UTF-8 bytes.';
  if (
    new Set(draft.steps.map((step) => step.id)).size !== draft.steps.length ||
    draft.steps.some((step) => !/^[A-Za-z0-9_-]{1,64}$/.test(step.id))
  )
    return 'Step IDs must be unique and use 1–64 letters, numbers, underscores or hyphens.';
  const text =
    draft.title +
    draft.requestKey +
    draft.steps.map((step) => step.id + step.title + step.prompt).join('');
  return bytes(text) > 32768
    ? 'The complete definition must fit within 32768 UTF-8 bytes.'
    : null;
}

export function WorkspaceWorkflows({
  agents,
  online,
}: {
  agents: readonly AgentDetail[];
  online: boolean;
}) {
  const [selection, setSelection] = useState('');
  const agentId = agents.some((agent) => agent.id === selection)
    ? selection
    : (agents[0]?.id ?? '');
  const membership = JSON.stringify(agents.map((agent) => agent.id).sort());
  return (
    <div className="space-y-5">
      <p className="text-sm leading-relaxed text-ink-3">
        Saved tasks, approval checkpoints, and questions run in order. Each task
        uses the agent’s current permissions.
      </p>
      <label className="block space-y-2 text-sm">
        Agent
        <select
          className="field"
          aria-label="Agent"
          value={agentId}
          onChange={(event) => setSelection(event.target.value)}
          disabled={!agents.length}
        >
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name}
            </option>
          ))}
        </select>
      </label>
      {agentId ? (
        <WorkflowAgentView
          key={agentId}
          agentId={agentId}
          membership={membership}
          online={online}
        />
      ) : (
        <p className="text-sm text-ink-3">
          Create an agent to start a workflow.
        </p>
      )}
    </div>
  );
}

export function WorkflowAgentView({
  agentId,
  membership,
  online,
  surface = 'work',
  refreshKey,
}: {
  agentId: string;
  membership: string;
  online: boolean;
  surface?: 'work' | 'chat';
  refreshKey?: string | number;
}) {
  const draftKey = `anima:workflow-draft:${agentId}`;
  const commandKey = `anima:workflow-command:${agentId}`;
  const [draft, setDraft] = useState<WorkflowInput>(() => {
    const candidate = stored<WorkflowInput>(draftKey);
    return candidate &&
      typeof candidate.title === 'string' &&
      typeof candidate.requestKey === 'string' &&
      Array.isArray(candidate.steps) &&
      candidate.steps.length > 0 &&
      candidate.steps.length <= 16 &&
      candidate.steps.every(
        (step) =>
          step &&
          typeof step.id === 'string' &&
          typeof step.title === 'string' &&
          typeof step.prompt === 'string' &&
          ['task', 'approval', 'input'].includes(step.kind),
      )
      ? candidate
      : newDraft();
  });
  const [pending, setPending] = useState<Command | null>(() =>
    stored<Command>(commandKey),
  );
  const [refresh, setRefresh] = useState(0);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  const contextKey = JSON.stringify([agentId, membership, online]);
  const context = useRenderScope(contextKey);
  const contextRef = useLatest(context);
  const mounted = useRef(false);
  const [loaded, setLoaded] = useState<{
    context: Context;
    workflows: WorkflowRun[] | null;
    error: string | null;
    loading: boolean;
    paused: boolean;
  } | null>(null);
  const [actionError, setActionError] = useState<{
    context: Context;
    message: string;
  } | null>(null);
  const [busy, setBusy] = useState<Context | null>(null);
  const busyRef = useRef<Context | null>(null);
  const readEpoch = useRef(0);
  const current = loaded?.context === context ? loaded : null;
  const saving = busy === context;
  const disabled =
    !online || !current || current.loading || !current.workflows || saving;
  const validContext = () =>
    mounted.current && contextRef.current === context && online;

  useEffect(() => {
    const update = (event: Event) => {
      if ((event as CustomEvent<string>).detail === agentId)
        setRefresh((value) => value + 1);
    };
    workflowChanges.addEventListener('change', update);
    return () => workflowChanges.removeEventListener('change', update);
  }, [agentId]);

  useEffect(() => {
    save(draftKey, draft);
  }, [draftKey, draft]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!online) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let count = 0;
    const controller = new AbortController();
    const load = async () => {
      if (cancelled || !mounted.current || contextRef.current !== context)
        return;
      if (busyRef.current === context) {
        timer = setTimeout(() => void load(), 5000);
        return;
      }
      const epoch = ++readEpoch.current;
      count += 1;
      setLoaded((previous) => ({
        context,
        workflows: previous?.context === context ? previous.workflows : null,
        error: null,
        loading: true,
        paused: false,
      }));
      try {
        const workflows = await daemon.listWorkflows(agentId, {
          signal: controller.signal,
        });
        if (
          cancelled ||
          !mounted.current ||
          contextRef.current !== context ||
          epoch !== readEpoch.current
        )
          return;
        const running = workflows.some(
          (workflow) => workflow.status === 'running',
        );
        setLoaded({
          context,
          workflows,
          error: null,
          loading: false,
          paused: running && count >= 60,
        });
        if (running && count < 60) timer = setTimeout(() => void load(), 5000);
      } catch (error) {
        if (
          cancelled ||
          !mounted.current ||
          contextRef.current !== context ||
          epoch !== readEpoch.current
        )
          return;
        setLoaded({
          context,
          workflows: null,
          error: error instanceof Error ? error.message : String(error),
          loading: false,
          paused: false,
        });
      }
    };
    void load();
    return () => {
      cancelled = true;
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [agentId, online, context, contextRef, refresh, refreshKey]);

  const remember = (command: Command | null) => {
    save(commandKey, command);
    setPending(command);
  };
  const mutate = async (command: Command) => {
    if (!validContext() || disabled || busyRef.current === context) return;
    busyRef.current = context;
    readEpoch.current += 1;
    setBusy(context);
    setActionError(null);
    remember(command);
    try {
      if (command.type === 'create')
        await daemon.createWorkflow(agentId, command.input);
      else if (command.type === 'resume')
        await daemon.resumeWorkflow(agentId, command.id, command.input);
      else await daemon.cancelWorkflow(agentId, command.id, command.input);
      notifyWorkflowChange(agentId);
      if (!validContext()) return;
      remember(null);
      if (command.type === 'create') setDraft(newDraft());
    } catch (error) {
      if (!validContext()) return;
      setActionError({
        context,
        message: error instanceof Error ? error.message : String(error),
      });
      if (
        error instanceof DaemonHttpError &&
        error.status >= 400 &&
        error.status < 500
      ) {
        remember(null);
        setRefresh((value) => value + 1);
      }
    } finally {
      if (busyRef.current === context) busyRef.current = null;
      if (validContext()) setBusy(null);
    }
  };
  const edit = (patch: Partial<WorkflowInput>) =>
    setDraft((value) => ({ ...value, ...patch, requestKey: newKey() }));
  const stepEdit = (index: number, patch: Partial<WorkflowStepInput>) =>
    edit({
      steps: draft.steps.map((step, position) =>
        position === index ? { ...step, ...patch } : step,
      ),
    });
  const moveStep = (index: number, offset: number) => {
    const steps = [...draft.steps];
    [steps[index], steps[index + offset]] = [
      steps[index + offset],
      steps[index],
    ];
    edit({ steps });
  };
  const validation = definitionError(draft);
  const error =
    actionError?.context === context ? actionError.message : current?.error;

  const chat = surface === 'chat';
  const visibleWorkflows = (current?.workflows ?? []).filter(
    (workflow) =>
      !chat ||
      (workflow.status !== 'completed' && workflow.status !== 'cancelled'),
  );
  if (chat && !visibleWorkflows.length && !pending && !error) return null;
  const Container = chat ? 'section' : 'div';

  return (
    <Container
      aria-label={chat ? 'Active workflows' : undefined}
      className="space-y-5"
    >
      {chat && visibleWorkflows.length > 0 && (
        <h2 className="text-base font-semibold">Active workflows</h2>
      )}
      {!chat && (
        <details
          onToggle={(event) => setTemplatesOpen(event.currentTarget.open)}
        >
          <summary className="cursor-pointer rounded-xl border border-line px-4 py-3 text-sm font-semibold">
            Reusable templates
          </summary>
          {templatesOpen && (
            <div className="pt-4">
              <WorkflowTemplates
                agentId={agentId}
                online={online}
                workflows={current?.workflows ?? []}
                refreshKey={refreshKey}
                onRun={() => notifyWorkflowChange(agentId)}
              />
            </div>
          )}
        </details>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          className={buttonClass}
          disabled={!online || saving || current?.loading}
          onClick={() => {
            if (validContext()) setRefresh((value) => value + 1);
          }}
        >
          Refresh workflows
        </button>
        {current?.paused && (
          <p className="text-sm text-ink-3">
            Automatic refresh paused. Use Refresh workflows to check again.
          </p>
        )}
      </div>
      {!online ? (
        <p role="status">Offline. Reconnect to load workflows.</p>
      ) : (
        (!current || current.loading) && <p role="status">Loading workflows…</p>
      )}
      {error && (
        <p
          role="alert"
          className="rounded-xl border border-danger/30 p-4 text-sm text-danger"
        >
          {error}
        </p>
      )}
      {pending && (
        <div className="space-y-3 rounded-xl border border-line p-4 text-sm">
          <p>
            A saved request may already have reached the daemon. Retry sends the
            same request and original revision.
          </p>
          <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
            {JSON.stringify(pending.input, null, 2)}
          </pre>
          <button
            type="button"
            className={buttonClass}
            disabled={disabled}
            onClick={() => void mutate(pending)}
          >
            {pending.type === 'create'
              ? 'Retry saved workflow'
              : pending.type === 'resume'
                ? 'Retry saved decision'
                : 'Retry saved cancellation'}
          </button>
        </div>
      )}
      {surface === 'work' && (
        <form
          className="space-y-4 rounded-2xl border border-line bg-surface p-5"
          onSubmit={(event) => {
            event.preventDefault();
            if (!validation && !pending)
              void mutate({ type: 'create', input: draft });
          }}
        >
          <h3 className="text-lg font-semibold">New workflow</h3>
          <fieldset
            disabled={disabled || !!pending}
            className="space-y-4 disabled:opacity-60"
          >
            <label className="block space-y-2 text-sm">
              Workflow title
              <input
                className="field"
                value={draft.title}
                maxLength={320}
                onChange={(event) => edit({ title: event.target.value })}
              />
            </label>
            {draft.steps.map((step, index) => (
              <div
                key={step.id}
                className="space-y-3 rounded-xl border border-line p-4"
              >
                <p className="text-sm font-semibold">Step {index + 1}</p>
                <label className="block space-y-2 text-sm">
                  Step {index + 1} kind
                  <select
                    className="field"
                    value={step.kind}
                    onChange={(event) =>
                      stepEdit(index, {
                        kind: event.target.value as WorkflowStepInput['kind'],
                      })
                    }
                  >
                    <option value="task">Task</option>
                    <option value="approval">Approval checkpoint</option>
                    <option value="input">Question</option>
                  </select>
                </label>
                <label className="block space-y-2 text-sm">
                  Step {index + 1} title
                  <input
                    className="field"
                    value={step.title}
                    maxLength={320}
                    onChange={(event) =>
                      stepEdit(index, { title: event.target.value })
                    }
                  />
                </label>
                <label className="block space-y-2 text-sm">
                  Step {index + 1} prompt
                  <textarea
                    className="field min-h-24"
                    value={step.prompt}
                    onChange={(event) =>
                      stepEdit(index, { prompt: event.target.value })
                    }
                  />
                </label>
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className={buttonClass}
                    disabled={index === 0}
                    aria-label={`Move step ${index + 1} up`}
                    onClick={() => moveStep(index, -1)}
                  >
                    Move up
                  </button>
                  <button
                    type="button"
                    className={buttonClass}
                    disabled={index === draft.steps.length - 1}
                    aria-label={`Move step ${index + 1} down`}
                    onClick={() => moveStep(index, 1)}
                  >
                    Move down
                  </button>
                  <button
                    type="button"
                    className={buttonClass}
                    disabled={draft.steps.length === 1}
                    aria-label={`Remove step ${index + 1}`}
                    onClick={() =>
                      edit({
                        steps: draft.steps.filter(
                          (_, position) => position !== index,
                        ),
                      })
                    }
                  >
                    Remove
                  </button>
                </div>
              </div>
            ))}
            <button
              type="button"
              className={buttonClass}
              disabled={draft.steps.length >= 16}
              onClick={() => edit({ steps: [...draft.steps, newStep()] })}
            >
              Add step
            </button>
          </fieldset>
          <p className="text-xs text-ink-3">
            Up to 16 steps; titles up to 160 characters; prompts up to 8192
            UTF-8 bytes each and 32768 bytes total.
          </p>
          {validation && <p className="text-sm text-ink-3">{validation}</p>}
          <button
            type="submit"
            className={buttonClass}
            disabled={disabled || !!pending || !!validation}
          >
            Create workflow
          </button>
        </form>
      )}
      {!chat &&
        online &&
        current?.workflows?.length === 0 &&
        !current.loading && (
          <p className="rounded-2xl border border-dashed border-line p-8 text-sm text-ink-3">
            No workflows yet for this agent.
          </p>
        )}
      {online &&
        visibleWorkflows.map((workflow) => (
          <WorkflowCard
            key={`${workflow.id}:${workflow.revision}`}
            workflow={workflow}
            disabled={disabled || !!pending}
            onResume={(input) =>
              void mutate({ type: 'resume', id: workflow.id, input })
            }
            onCancel={() =>
              void mutate({
                type: 'cancel',
                id: workflow.id,
                input: { revision: workflow.revision },
              })
            }
          />
        ))}
    </Container>
  );
}

function WorkflowCard({
  workflow,
  disabled,
  onResume,
  onCancel,
}: {
  workflow: WorkflowRun;
  disabled: boolean;
  onResume: (input: WorkflowResumeInput) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState('');
  const [acknowledged, setAcknowledged] = useState(false);
  const step = workflow.steps[workflow.currentStep];
  const terminal =
    workflow.status === 'completed' || workflow.status === 'cancelled';
  return (
    <article
      aria-label={workflow.title}
      className="space-y-4 rounded-2xl border border-line bg-surface p-5 sm:p-6"
    >
      <div className="flex flex-wrap items-center gap-3">
        <h3 className="text-lg font-semibold">{workflow.title}</h3>
        <span className="text-sm text-ink-3">{labels[workflow.status]}</span>
      </div>
      <p className="text-xs text-ink-3">
        Revision {workflow.revision} · Updated{' '}
        {new Date(workflow.updatedAtMs).toLocaleString()}
      </p>
      {workflow.sourceTemplate && (
        <details className="text-xs text-ink-3">
          <summary className="cursor-pointer">
            Template {workflow.sourceTemplate.slug} · revision{' '}
            {workflow.sourceTemplate.revision}
          </summary>
          <pre className="whitespace-pre-wrap break-words pt-2">
            {JSON.stringify(workflow.sourceTemplate.inputs, null, 2)}
          </pre>
        </details>
      )}
      {workflow.error && (
        <p className="whitespace-pre-wrap break-words text-sm text-danger">
          {workflow.error}
        </p>
      )}
      {workflow.status === 'running' &&
        step?.kind === 'task' &&
        !step.jobId &&
        (step.status === 'pending' || step.status === 'running') && (
          <p className="text-sm text-ink-3">Waiting for an execution slot</p>
        )}
      <ol className="space-y-3">
        {workflow.steps.map((saved, index) => (
          <li
            key={saved.id}
            className="space-y-2 rounded-xl border border-line p-4"
          >
            <p className="text-sm font-semibold">
              {index + 1}. {saved.title} ·{' '}
              {saved.kind === 'input'
                ? 'Question'
                : saved.kind === 'approval'
                  ? 'Approval'
                  : 'Task'}
            </p>
            <p className="text-xs text-ink-3">
              {labels[saved.status]}
              {saved.attempt ? ` · Attempt ${saved.attempt}/3` : ''}
            </p>
            <p className="whitespace-pre-wrap break-words text-sm">
              {saved.prompt}
            </p>
            {saved.jobId && (
              <p className="break-all text-xs text-ink-3">Job: {saved.jobId}</p>
            )}
            {saved.attempts.length > 0 && (
              <p className="break-all text-xs text-ink-3">
                Saved job attempts: {saved.attempts.join(', ')}
              </p>
            )}
            {saved.result !== null && (
              <div className="space-y-1">
                <p className="text-xs font-semibold text-ink-3">Saved result</p>
                <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
                  {saved.result}
                </pre>
              </div>
            )}
          </li>
        ))}
      </ol>
      {workflow.status === 'awaiting_approval' && (
        <button
          type="button"
          className={buttonClass}
          disabled={disabled}
          onClick={() =>
            onResume({ revision: workflow.revision, action: 'approve' })
          }
        >
          Approve checkpoint
        </button>
      )}
      {workflow.status === 'awaiting_input' && (
        <div className="space-y-3">
          <label className="block space-y-2 text-sm">
            Your answer
            <textarea
              className="field"
              disabled={disabled}
              value={value}
              onChange={(event) => setValue(event.target.value)}
            />
          </label>
          <p className="text-xs text-ink-3">{bytes(value)}/8192 UTF-8 bytes</p>
          <button
            type="button"
            className={buttonClass}
            disabled={disabled || !textValid(value)}
            onClick={() =>
              onResume({ revision: workflow.revision, action: 'answer', value })
            }
          >
            Submit answer
          </button>
        </div>
      )}
      {workflow.status === 'needs_review' && (
        <div className="space-y-3">
          <p className="text-sm text-ink-3">
            Inspect the saved job and any external receipts before deciding.
            Retrying can repeat effects; a verified result advances without
            rerunning this task.
          </p>
          <label className="flex items-start gap-3 text-sm">
            <input
              type="checkbox"
              disabled={disabled}
              checked={acknowledged}
              onChange={(event) => setAcknowledged(event.target.checked)}
            />
            I have reviewed the uncertain effects and understand retrying can
            repeat them.
          </label>
          <button
            type="button"
            className={buttonClass}
            disabled={disabled || !acknowledged || !step || step.attempt >= 3}
            onClick={() =>
              onResume({
                revision: workflow.revision,
                action: 'retry',
                acknowledgeUncertain: true,
              })
            }
          >
            Retry task
          </button>
          {step && step.attempt >= 3 && (
            <p className="text-sm text-ink-3">
              This task has reached its three-attempt limit.
            </p>
          )}
          <label className="block space-y-2 text-sm">
            Verified result
            <textarea
              className="field"
              disabled={disabled}
              value={value}
              onChange={(event) => setValue(event.target.value)}
            />
          </label>
          <p className="text-xs text-ink-3">{bytes(value)}/8192 UTF-8 bytes</p>
          <button
            type="button"
            className={buttonClass}
            disabled={disabled || !acknowledged || !textValid(value)}
            onClick={() =>
              onResume({
                revision: workflow.revision,
                action: 'resolve',
                value,
                acknowledgeUncertain: true,
              })
            }
          >
            Record verified result
          </button>
        </div>
      )}
      {!terminal && (
        <button
          type="button"
          className={buttonClass}
          disabled={disabled}
          onClick={onCancel}
        >
          Cancel workflow
        </button>
      )}
      {workflow.decisions.length > 0 && (
        <details className="text-sm">
          <summary className="cursor-pointer text-ink-3">
            Saved owner decisions
          </summary>
          <ol className="space-y-2 pt-3">
            {workflow.decisions.map((decision) => (
              <li
                key={decision.request.revision}
                className="whitespace-pre-wrap break-words"
              >
                Revision {decision.request.revision}: {decision.request.action}
                {decision.request.value
                  ? ` · ${decision.request.value}`
                  : ''} · {new Date(decision.decidedAtMs).toLocaleString()}
              </li>
            ))}
          </ol>
        </details>
      )}
    </article>
  );
}
