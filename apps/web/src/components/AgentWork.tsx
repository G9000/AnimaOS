import { useCallback, useEffect, useId, useRef, useState } from 'react';
import type { AgentTask, AgentTasks } from '@animaos/sdk';
import {
  daemon,
  type DaemonSchedule,
  type TelegramConnector,
} from '../lib/daemon-api';
import { primaryBtnCls } from './ui-bits';
import { describeTrigger } from '../lib/automations';

interface TasksContext {
  agentId: string;
  online: boolean;
  generation: number;
  owner: object;
}

interface TasksEditor {
  owner: object;
  snapshot: AgentTasks | null;
  tasks: AgentTask[];
  content: string;
  read: {
    context: TasksContext;
    status: 'loading' | 'loaded' | 'error';
    latest: AgentTasks | null;
    error: string | null;
  } | null;
  actionError: string | null;
  review: boolean;
}

interface PendingTasksSave {
  context: TasksContext;
  notify?: (busy: boolean) => void;
  ownsBusy: boolean;
}

function emptyTasksEditor(context: TasksContext): TasksEditor {
  return {
    owner: context.owner,
    snapshot: null,
    tasks: [],
    content: '',
    read: null,
    actionError: null,
    review: false,
  };
}

function changedTasks(editor: TasksEditor): boolean {
  return (
    editor.snapshot !== null &&
    JSON.stringify(editor.tasks) !== JSON.stringify(editor.snapshot.tasks)
  );
}

function hasTaskDraft(editor: TasksEditor): boolean {
  return changedTasks(editor) || editor.content !== '';
}

function releaseTasksBusy(operation: PendingTasksSave) {
  if (!operation.ownsBusy) return;
  operation.ownsBusy = false;
  operation.notify?.(false);
}

export function AgentTasksView({
  agentId,
  name,
  online,
  running = false,
  onBusyChange,
}: {
  agentId: string;
  name: string;
  online: boolean;
  running?: boolean;
  onBusyChange?: (busy: boolean) => void;
}) {
  const mounted = useRef(true);
  const sequence = useRef(0);
  const contextRef = useRef<TasksContext | null>(null);
  if (
    !contextRef.current ||
    contextRef.current.agentId !== agentId ||
    contextRef.current.online !== online
  ) {
    contextRef.current = {
      agentId,
      online,
      generation: (contextRef.current?.generation ?? 0) + 1,
      owner:
        contextRef.current?.agentId === agentId ? contextRef.current.owner : {},
    };
    sequence.current += 1;
  }
  const context = contextRef.current!;
  const isCurrent = useCallback(
    (origin: TasksContext) =>
      mounted.current && contextRef.current === origin && origin.online,
    [],
  );
  const [state, setState] = useState(() => emptyTasksEditor(context));
  const editorRef = useRef(state);
  editorRef.current = state;
  const updateEditor = useCallback(
    (update: (previous: TasksEditor) => TasksEditor) => {
      const next = update(editorRef.current);
      editorRef.current = next;
      setState(next);
    },
    [],
  );
  // This reservation survives connection generations until the physical PUT
  // settles. Display freshness alone must never authorize another submission.
  const pendingRef = useRef<PendingTasksSave | null>(null);
  const [pending, setPending] = useState<PendingTasksSave | null>(null);
  const runningRef = useRef(running);
  runningRef.current = running;

  const load = useCallback(
    async (manual = false) => {
      if (!isCurrent(context)) return;
      const request = ++sequence.current;
      updateEditor((previous) => {
        if (!isCurrent(context)) return previous;
        const base =
          previous.owner === context.owner
            ? previous
            : emptyTasksEditor(context);
        return {
          ...base,
          read: { context, status: 'loading', latest: null, error: null },
          actionError: null,
        };
      });
      try {
        const latest = await daemon.agentTasks(context.agentId);
        if (!isCurrent(context) || sequence.current !== request) return;
        updateEditor((previous) => {
          if (!isCurrent(context) || sequence.current !== request)
            return previous;
          const base =
            previous.owner === context.owner
              ? previous
              : emptyTasksEditor(context);
          const retain = base.snapshot !== null && hasTaskDraft(base);
          return {
            ...base,
            snapshot: retain ? base.snapshot : latest,
            tasks: retain ? base.tasks : latest.tasks,
            content: retain ? base.content : '',
            read: { context, status: 'loaded', latest, error: null },
            actionError: null,
            review:
              retain && (manual || base.snapshot!.revision !== latest.revision),
          };
        });
      } catch (caught) {
        if (!isCurrent(context) || sequence.current !== request) return;
        updateEditor((previous) => {
          if (!isCurrent(context) || sequence.current !== request)
            return previous;
          return {
            ...previous,
            read: {
              context,
              status: 'error',
              latest: null,
              error: caught instanceof Error ? caught.message : String(caught),
            },
          };
        });
      }
    },
    [context, isCurrent, updateEditor],
  );

  // Establish mount ownership before starting reads so root StrictMode replay
  // starts a live request while the discarded replay response stays fenced.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current += 1;
      if (pendingRef.current) releaseTasksBusy(pendingRef.current);
    };
  }, []);
  useEffect(
    () => () => {
      const operation = pendingRef.current;
      if (operation?.context === context) releaseTasksBusy(operation);
    },
    [context],
  );
  useEffect(() => {
    if (context.online) void load();
  }, [context, load]);

  // Project an agent/connection transition during render, before the effects.
  // Same-agent drafts remain visible, but old clean rows cannot appear fresh.
  const editor =
    state.owner === context.owner ? state : emptyTasksEditor(context);
  const currentRead = editor.read?.context === context ? editor.read : null;
  const loaded = online && currentRead?.status === 'loaded';
  const draft = hasTaskDraft(editor);
  const snapshot = loaded || draft ? editor.snapshot : null;
  const { tasks, content } = editor;
  const dirty = changedTasks(editor);
  const busy = pending !== null;
  const editable = loaded && snapshot !== null && !busy;
  const review = loaded && editor.review;
  const error = online
    ? (currentRead?.error ??
      (loaded ? editor.actionError : null) ??
      (review
        ? 'Tasks changed or were refreshed. Review your draft or discard it to load the saved list.'
        : null))
    : null;
  const canEdit = () => {
    const current = editorRef.current;
    return (
      isCurrent(context) &&
      current.owner === context.owner &&
      current.snapshot !== null &&
      current.read?.context === context &&
      current.read.status === 'loaded' &&
      pendingRef.current === null
    );
  };
  const edit = (update: (previous: TasksEditor) => TasksEditor) => {
    if (!canEdit()) return;
    updateEditor((previous) => (canEdit() ? update(previous) : previous));
  };
  const save = async () => {
    if (!canEdit() || runningRef.current) return;
    const origin = editorRef.current;
    if (
      !origin.snapshot ||
      origin.review ||
      !changedTasks(origin) ||
      origin.tasks.some((task) => !task.content.trim())
    )
      return;
    const operation: PendingTasksSave = {
      context,
      notify: onBusyChange,
      ownsBusy: true,
    };
    const payload = {
      tasks: origin.tasks.map((task) => ({ ...task })),
      revision: origin.snapshot.revision,
    };
    pendingRef.current = operation;
    setPending(operation);
    updateEditor((previous) => ({ ...previous, actionError: null }));
    try {
      operation.notify?.(true);
      if (!isCurrent(context)) return;
      const saved = await daemon.updateAgentTasks(context.agentId, payload);
      if (!isCurrent(context)) return;
      updateEditor((previous) => {
        if (!isCurrent(context)) return previous;
        return {
          ...previous,
          snapshot: saved,
          tasks: saved.tasks,
          read: { context, status: 'loaded', latest: saved, error: null },
          actionError: null,
          review: false,
        };
      });
    } catch (caught) {
      if (!isCurrent(context)) return;
      updateEditor((previous) =>
        isCurrent(context)
          ? {
              ...previous,
              actionError:
                caught instanceof Error ? caught.message : String(caught),
            }
          : previous,
      );
    } finally {
      if (pendingRef.current === operation) {
        pendingRef.current = null;
        if (mounted.current)
          setPending((current) => (current === operation ? null : current));
      }
      if (isCurrent(context)) releaseTasksBusy(operation);
    }
  };
  const discard = () => {
    if (!canEdit()) return;
    updateEditor((previous) => {
      const latest = previous.read?.latest;
      if (!canEdit() || !previous.review || !latest) return previous;
      return {
        ...previous,
        snapshot: latest,
        tasks: latest.tasks,
        content: '',
        actionError: null,
        review: false,
      };
    });
  };
  return (
    <section className="space-y-5" aria-label={`${name}'s tasks`}>
      <div className="flex items-center justify-between gap-3">
        <h4 className="font-semibold">Tasks for {name}</h4>
        <button
          type="button"
          disabled={!online || busy}
          className="text-xs underline"
          onClick={() => {
            if (isCurrent(context) && !pendingRef.current) void load(true);
          }}
        >
          Refresh tasks
        </button>
      </div>
      <p className="text-sm leading-relaxed text-ink-3">
        This is {name}’s own task list. Their todo tools read and update this
        same list. Add a proactive schedule separately to have them check it
        automatically.
      </p>
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      {running && (
        <p role="status" className="text-sm text-ink-3">
          {name} is working. You can draft tasks here; wait for the run to
          finish before saving.
        </p>
      )}
      {!online && (
        <p role="status">Offline. Reconnect to refresh and edit tasks.</p>
      )}
      {online && !loaded && !error && <p role="status">Loading tasks…</p>}
      {review && (
        <button
          type="button"
          className="text-sm underline"
          disabled={busy}
          onClick={discard}
        >
          Discard draft and load saved tasks
        </button>
      )}
      {snapshot && (
        <>
          <form
            className="space-y-3"
            onSubmit={(event) => {
              event.preventDefault();
              edit((previous) => {
                if (!previous.content.trim()) return previous;
                return {
                  ...previous,
                  tasks: [
                    ...previous.tasks,
                    {
                      ...(previous.tasks.some(
                        (task) => task.id !== undefined,
                      ) ||
                      previous.snapshot?.tasks.some(
                        (task) => task.id !== undefined,
                      )
                        ? { id: crypto.randomUUID() }
                        : {}),
                      content: previous.content.trim(),
                      activeForm: previous.content.trim(),
                      status: 'pending',
                    },
                  ],
                  content: '',
                };
              });
            }}
          >
            <textarea
              className="field"
              aria-label="New agent task"
              placeholder="What should this agent work on? Include the details they need."
              value={content}
              disabled={!editable}
              onChange={(event) => {
                const value = event.target.value;
                edit((previous) => ({ ...previous, content: value }));
              }}
              rows={3}
            />
            <button
              className="rounded-lg border border-line px-4 py-2 text-sm"
              disabled={!editable || !content.trim()}
            >
              Add task
            </button>
          </form>
          {!tasks.length && (
            <p className="rounded-xl border border-line p-5 text-sm text-ink-3">
              No tasks yet for {name}.
            </p>
          )}
          {tasks.map((task, index) => (
            <article
              className="space-y-3 rounded-xl border border-line p-4"
              key={index}
            >
              <textarea
                aria-label={`Task ${index + 1}`}
                className="field"
                rows={2}
                disabled={!editable}
                value={task.content}
                onChange={(event) => {
                  const value = event.target.value;
                  edit((previous) => ({
                    ...previous,
                    tasks: previous.tasks.map((item, i) =>
                      i === index
                        ? {
                            ...item,
                            content: value,
                            activeForm: value,
                          }
                        : item,
                    ),
                  }));
                }}
              />
              <div className="flex items-center justify-between gap-3">
                <select
                  aria-label={`Task ${index + 1} status`}
                  className="field w-auto text-sm"
                  value={task.status}
                  disabled={!editable}
                  onChange={(event) => {
                    const value = event.target.value as AgentTask['status'];
                    edit((previous) => ({
                      ...previous,
                      tasks: previous.tasks.map((item, i) =>
                        i === index
                          ? {
                              ...item,
                              status: value,
                            }
                          : item,
                      ),
                    }));
                  }}
                >
                  <option value="pending">Pending</option>
                  <option value="in_progress">In progress</option>
                  <option value="completed">Completed</option>
                  {task.status === 'cancelled' && (
                    <option value="cancelled" disabled>
                      Cancelled
                    </option>
                  )}
                </select>
                <button
                  type="button"
                  aria-label={`Remove task ${index + 1}`}
                  className="text-xs text-danger"
                  disabled={!editable}
                  onClick={() =>
                    edit((previous) => ({
                      ...previous,
                      tasks: previous.tasks.filter((_, i) => i !== index),
                    }))
                  }
                >
                  Remove
                </button>
              </div>
            </article>
          ))}
          <button
            type="button"
            className={`${primaryBtnCls} w-full`}
            disabled={
              !editable ||
              review ||
              running ||
              !dirty ||
              tasks.some((task) => !task.content.trim())
            }
            onClick={() => void save()}
          >
            {busy ? 'Saving tasks…' : dirty ? 'Save tasks' : 'Tasks saved'}
          </button>
          {draft && (
            <p className="text-xs text-ink-3">
              Unsaved changes. Refresh keeps this draft until you choose to
              discard it.
            </p>
          )}
        </>
      )}
    </section>
  );
}

export function AgentProactiveView({
  agentId,
  name,
  onBusyChange,
}: {
  agentId: string;
  name: string;
  onBusyChange?: (busy: boolean) => void;
}) {
  const [schedules, setSchedules] = useState<DaemonSchedule[] | null>(null);
  // Check-ins can be delivered to Telegram only once a chat is approved.
  const [telegram, setTelegram] = useState<TelegramConnector | null>(null);
  const [connectors, setConnectors] = useState<'loading' | 'loaded' | 'failed'>(
    'loading',
  );
  const [target, setTarget] = useState<'workspace' | 'telegram'>('workspace');
  const [prompt, setPrompt] = useState('');
  const [minutes, setMinutes] = useState(60);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setError(null);
    daemon.listSchedules(agentId).then(
      (result) => {
        if (!cancelled) setSchedules(result.schedules);
      },
      (reason: unknown) => {
        if (!cancelled)
          setError(reason instanceof Error ? reason.message : String(reason));
      },
    );
    // Connectors load on their own, so a slow or failed read never holds up
    // the schedules; Telegram stays unavailable until one succeeds.
    daemon.listConnectors(agentId).then(
      (result) => {
        if (cancelled) return;
        setTelegram(
          result.connectors.find(
            (connector) => connector.approvedChat !== null,
          ) ?? null,
        );
        setConnectors('loaded');
      },
      () => {
        if (cancelled) return;
        setTelegram(null);
        setConnectors('failed');
      },
    );
    return () => {
      cancelled = true;
    };
  }, [agentId, reload]);
  const telegramUnavailable = target === 'telegram' && telegram === null;
  const telegramHintId = useId();
  const mutate = async (action: () => Promise<unknown>, created = false) => {
    if (busy) return;
    setBusy(true);
    onBusyChange?.(true);
    setError(null);
    try {
      await action();
      if (created) setPrompt('');
      setReload((value) => value + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      onBusyChange?.(false);
    }
  };
  return (
    <section className="space-y-5" aria-label={`${name}'s proactive settings`}>
      <div className="flex items-center justify-between">
        <h4 className="font-semibold">Proactive work for {name}</h4>
        <button
          type="button"
          disabled={busy}
          className="text-xs underline"
          onClick={() => setReload((value) => value + 1)}
        >
          Refresh schedules
        </button>
      </div>
      <p className="text-sm leading-relaxed text-ink-3">
        Choose when {name} wakes up and what they should do. Schedules belong
        only to this agent and run while the daemon is active, even with the UI
        closed. Their existing tool permissions still apply.
      </p>
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      {schedules === null && !error && <p role="status">Loading schedules…</p>}
      {schedules?.length === 0 && (
        <p className="rounded-xl border border-line p-4 text-sm text-ink-3">
          Proactive work is off. No schedules configured for {name}.
        </p>
      )}
      {schedules?.map((schedule) => (
        <article
          key={schedule.id}
          className="space-y-3 rounded-xl border border-line p-4"
        >
          <p className="whitespace-pre-wrap break-words text-sm">
            {schedule.prompt}
          </p>
          <p className="text-xs text-ink-3">
            {schedule.enabled
              ? schedule.trigger.type === 'event'
                ? 'Waiting for event'
                : `Next run: ${new Date(schedule.nextDueAtMs).toLocaleString()}`
              : 'Paused'}{' '}
            · {describeTrigger(schedule.trigger)}
          </p>
          {schedule.lastOutcome && (
            <p className="text-xs text-ink-3">
              Last result:{' '}
              {schedule.lastOutcome.status === 'silent'
                ? 'No update needed'
                : schedule.lastOutcome.status === 'spoke'
                  ? 'Posted an update'
                  : schedule.lastOutcome.status === 'stopped'
                    ? 'Stopped by owner'
                    : 'Run failed'}
              {schedule.lastOutcome.errorCode
                ? ` (${schedule.lastOutcome.errorCode})`
                : ''}
            </p>
          )}
          <div className="flex gap-4">
            <button
              type="button"
              className="text-sm underline"
              disabled={busy}
              onClick={() =>
                void mutate(() =>
                  daemon.updateSchedule(agentId, schedule.id, {
                    enabled: !schedule.enabled,
                  }),
                )
              }
            >
              {schedule.enabled ? 'Pause' : 'Resume'}
            </button>
            <button
              type="button"
              className="text-sm text-danger"
              disabled={busy}
              onClick={() =>
                void mutate(() => daemon.deleteSchedule(agentId, schedule.id))
              }
            >
              Remove schedule
            </button>
          </div>
        </article>
      ))}
      <form
        className="space-y-4 border-t border-line pt-5"
        onSubmit={(event) => {
          event.preventDefault();
          if (
            !prompt.trim() ||
            !Number.isInteger(minutes) ||
            minutes < 1 ||
            telegramUnavailable
          )
            return;
          void mutate(
            () =>
              daemon.createSchedule(agentId, {
                prompt: prompt.trim(),
                trigger: { type: 'interval', intervalMs: minutes * 60000 },
                target:
                  target === 'telegram' && telegram
                    ? { type: 'connector', connectorId: telegram.id }
                    : { type: 'workspace' },
                enabled: true,
              }),
            true,
          );
        }}
      >
        <label className="block text-sm">
          What should they do?
          <textarea
            aria-label="Proactive instructions"
            rows={5}
            className="field mt-2"
            value={prompt}
            disabled={busy}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder="Describe the goal, what to check, and when to report back."
          />
        </label>
        <button
          type="button"
          className="text-xs underline"
          disabled={busy}
          onClick={() =>
            setPrompt(
              'Read your own task list. Work on the next clear, pending task within your existing permissions and agreed scope. Update task progress with todo_write. Ask before consequential external actions. Report meaningful progress or blockers; otherwise return CHECKIN_OK.',
            )
          }
        >
          Use my task list
        </button>
        <label className="flex items-center gap-3 text-sm">
          Every{' '}
          <input
            aria-label="Proactive interval minutes"
            type="number"
            min={1}
            step={1}
            className="field w-24"
            value={minutes}
            disabled={busy}
            onChange={(event) => setMinutes(Number(event.target.value))}
          />{' '}
          minutes
        </label>
        <label className="flex flex-wrap items-center gap-3 text-sm">
          Deliver to
          <select
            aria-label="Deliver to"
            aria-describedby={
              telegram === null && connectors !== 'loading'
                ? telegramHintId
                : undefined
            }
            className="field w-auto"
            value={target}
            disabled={busy}
            onChange={(event) =>
              setTarget(event.target.value as 'workspace' | 'telegram')
            }
          >
            <option value="workspace">Workspace</option>
            <option value="telegram" disabled={telegram === null}>
              Telegram
            </option>
          </select>
        </label>
        {connectors === 'failed' ? (
          <p id={telegramHintId} className="text-xs text-danger">
            Connectors could not be loaded, so Telegram delivery is unavailable.
            Refresh schedules to try again.
          </p>
        ) : connectors === 'loaded' && telegram === null ? (
          <p id={telegramHintId} className="text-xs text-ink-3">
            Approve a Telegram chat in Connectors to deliver check-ins there.
          </p>
        ) : null}
        <p className="text-xs text-ink-3">
          {target === 'telegram'
            ? 'Updates are sent to your approved Telegram chat.'
            : 'Updates appear in this agent’s own check-in session.'}{' '}
          Scheduled runs use your configured model.
        </p>
        <button
          className={`${primaryBtnCls} w-full`}
          disabled={
            busy ||
            !prompt.trim() ||
            !Number.isInteger(minutes) ||
            minutes < 1 ||
            telegramUnavailable
          }
        >
          {busy ? 'Saving…' : 'Enable schedule'}
        </button>
      </form>
    </section>
  );
}
