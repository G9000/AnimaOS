import { useEffect, useState } from 'react';
import { daemon } from '../lib/daemon-api';
import { describeTrigger } from '../lib/automations';
import type { AgentDetail } from '../lib/types';
import { AgentRunsView } from './AgentRuns';
import { WorkspaceGoals } from './WorkspaceGoals';
import { WorkspaceWorkflows } from './WorkspaceWorkflows';
import { AgentAvatar } from './AgentAvatar';
import { AgentProactiveView, AgentTasksView } from './AgentWork';
import { useLatest } from '../hooks/useLatest';
import { useRenderScope } from '../hooks/useRenderScope';

type Section = 'Notes' | 'Tasks' | 'Schedules' | 'Runs' | 'Workflows' | 'Goals';
type Entry = {
  id: string;
  agentId: string;
  content: string;
  detail: string;
  timestamp?: number;
};
type LoadResult = {
  entries: Entry[];
  errors: { agentId: string; message: string }[];
};

function noteText(content: string, tags?: string[] | null): string {
  if (!tags?.includes('user-stated')) return content;
  const text = content
    .replace(/^user stated (?:memory|preference|profile):\s*/i, '')
    .trim();
  // Older extractors saved requests containing "my" as profile facts.
  // Hide those records without deleting the underlying memory.
  if (
    /^(?:(?:can|could|would|will) you\b|(?:please )?remind me\b|(?:please )?remember to\b|i want you to\b)/i.test(
      text,
    )
  )
    return '';
  return text.replace(/^(?:please )?remember (?:that\s+)?/i, '');
}

function noteLabel(
  content: string,
  type: string,
  tags?: string[] | null,
): string {
  if (tags?.includes('user-stated')) {
    if (/^user stated preference:/i.test(content)) return 'Preference';
    if (/^user stated profile:/i.test(content)) return 'About you';
  }
  if (tags?.includes('tool-memory-add')) return 'Saved note';
  return type === 'observation' ? 'Observation' : 'Saved fact';
}

async function loadEntries(
  agentId: string,
  section: Section,
): Promise<Entry[]> {
  if (section === 'Notes') {
    const { memories } = await daemon.recentAgentMemories(agentId);
    // Runtime task results and evaluator reflections are internal records.
    // Preserve deliberate memory_add saves, regardless of their chosen type.
    return memories
      .filter(
        (item) =>
          noteText(item.content, item.tags).trim().length > 0 &&
          (item.type === 'fact' ||
            item.type === 'observation' ||
            item.tags?.includes('tool-memory-add')),
      )
      .map((item) => ({
        id: item.id,
        agentId,
        content: noteText(item.content, item.tags),
        detail: noteLabel(item.content, item.type, item.tags),
        timestamp: new Date(item.createdAt).getTime(),
      }));
  }
  if (section === 'Tasks') {
    const { tasks } = await daemon.agentTasks(agentId);
    return tasks.map((item, index) => ({
      id: String(index),
      agentId,
      content: item.content,
      detail: {
        pending: 'Pending',
        in_progress: 'In progress',
        completed: 'Completed',
        cancelled: 'Cancelled',
      }[item.status],
    }));
  }
  const { schedules } = await daemon.listSchedules(agentId);
  return schedules.map((item) => ({
    id: item.id,
    agentId,
    content: item.prompt,
    detail: [
      item.enabled ? 'Enabled' : 'Paused',
      describeTrigger(item.trigger),
      item.target.type === 'workspace'
        ? 'Workspace'
        : `Connector: ${item.target.connectorId}`,
      item.enabled
        ? item.trigger.type === 'event'
          ? 'Waiting for event'
          : `Next: ${new Date(item.nextDueAtMs).toLocaleString()}`
        : '',
      item.lastOutcome
        ? `Last run: ${item.lastOutcome.status}${item.lastOutcome.errorCode ? ` (${item.lastOutcome.errorCode})` : ''}`
        : '',
    ]
      .filter(Boolean)
      .join(' · '),
    timestamp:
      item.trigger.type === 'event' ? item.createdAtMs : item.nextDueAtMs,
  }));
}

export function WorkspaceHub({
  agents,
  online,
  onSelectAgent,
  initialSection = 'Notes',
}: {
  agents: readonly AgentDetail[];
  online: boolean;
  onSelectAgent?: (id: string) => void;
  initialSection?: Section;
}) {
  const [section, setSection] = useState<Section>(initialSection);
  const [agentFilter, setAgentFilter] = useState('');
  const [query, setQuery] = useState('');
  const [revision, setRevision] = useState(0);
  const [loaded, setLoaded] = useState<{
    generation: object;
    result: LoadResult;
  } | null>(null);
  const [managedId, setManagedId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // Agent snapshots also change during chat polling. Only membership changes reload the hub.
  const agentIds = JSON.stringify(agents.map((agent) => agent.id).sort());
  // Connectivity only invalidates Tasks. A reconnect owns a new generation,
  // even when its membership and online value match an earlier request.
  const tasksOnline = section !== 'Tasks' || online;
  const contextKey = JSON.stringify([agentIds, section, revision, tasksOnline]);
  const generation = useRenderScope(contextKey);
  const generationRef = useLatest(generation);
  const result =
    loaded?.generation === generation && (section !== 'Tasks' || online)
      ? loaded.result
      : null;
  useEffect(() => {
    let cancelled = false;
    if (
      section === 'Runs' ||
      section === 'Goals' ||
      section === 'Workflows' ||
      !tasksOnline ||
      (section === 'Tasks' && generationRef.current !== generation)
    )
      return;
    const ids = JSON.parse(agentIds) as string[];
    void Promise.allSettled(ids.map((id) => loadEntries(id, section))).then(
      (responses) => {
        if (
          cancelled ||
          (section === 'Tasks' && generationRef.current !== generation)
        )
          return;
        const next: LoadResult = { entries: [], errors: [] };
        responses.forEach((response, index) => {
          if (response.status === 'fulfilled')
            next.entries.push(...response.value);
          else
            next.errors.push({
              agentId: ids[index],
              message:
                response.reason instanceof Error
                  ? response.reason.message
                  : String(response.reason),
            });
        });
        if (section !== 'Tasks')
          next.entries.sort((a, b) =>
            section === 'Notes'
              ? (b.timestamp ?? 0) - (a.timestamp ?? 0)
              : (a.timestamp ?? 0) - (b.timestamp ?? 0),
          );
        setLoaded({ generation, result: next });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [agentIds, section, revision, tasksOnline, generation, generationRef]);

  const selectedId = agents.some((agent) => agent.id === agentFilter)
    ? agentFilter
    : '';
  const managed = agents.find((agent) => agent.id === managedId);
  const entries = result?.entries.filter(
    (entry) =>
      (!selectedId || entry.agentId === selectedId) &&
      `${entry.content} ${entry.detail} ${agents.find((agent) => agent.id === entry.agentId)?.name ?? ''}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const errors = result?.errors.filter(
    (error) => !selectedId || error.agentId === selectedId,
  );

  return (
    <section className="work-page" aria-label="Work">
      <div className="work-stack">
        <h2 className="sr-only">Work</h2>
        {managed && section !== 'Notes' ? (
          <div className="space-y-6">
            <button
              type="button"
              className="text-sm underline disabled:opacity-50"
              disabled={saving}
              onClick={() => {
                setManagedId(null);
                setRevision((value) => value + 1);
              }}
            >
              ← Back to {section.toLowerCase()}
            </button>
            <h3 className="text-lg font-semibold">
              {managed.name} · {section}
            </h3>
            {section === 'Tasks' ? (
              <AgentTasksView
                key={managed.id}
                agentId={managed.id}
                online={online}
                name={managed.name}
                running={managed.status === 'Running'}
                onBusyChange={setSaving}
              />
            ) : (
              <AgentProactiveView
                onBusyChange={setSaving}
                key={managed.id}
                agentId={managed.id}
                name={managed.name}
              />
            )}
          </div>
        ) : (
          <>
            <div
              role="tablist"
              aria-label="Work categories"
              className="work-tabs"
            >
              {(
                [
                  'Notes',
                  'Tasks',
                  'Schedules',
                  'Runs',
                  'Workflows',
                  'Goals',
                ] as const
              ).map((item) => (
                <button
                  key={item}
                  id={`hub-tab-${item}`}
                  type="button"
                  role="tab"
                  tabIndex={section === item ? 0 : -1}
                  onKeyDown={(event) => {
                    const tabs: Section[] = [
                      'Notes',
                      'Tasks',
                      'Schedules',
                      'Runs',
                      'Workflows',
                      'Goals',
                    ];
                    const offset =
                      event.key === 'ArrowRight'
                        ? 1
                        : event.key === 'ArrowLeft'
                          ? -1
                          : 0;
                    const next =
                      event.key === 'Home'
                        ? tabs[0]
                        : event.key === 'End'
                          ? tabs[tabs.length - 1]
                          : offset
                            ? tabs[
                                (tabs.indexOf(item) + offset + tabs.length) %
                                  tabs.length
                              ]
                            : null;
                    if (!next) return;
                    event.preventDefault();
                    setSection(next);
                    setQuery('');
                    document.getElementById(`hub-tab-${next}`)?.focus();
                  }}
                  aria-selected={section === item}
                  aria-controls="hub-panel"
                  className="work-tab"
                  onClick={() => {
                    setSection(item);
                    setQuery('');
                  }}
                >
                  {item}
                </button>
              ))}
            </div>
            <div
              role="tabpanel"
              id="hub-panel"
              aria-labelledby={`hub-tab-${section}`}
              className="space-y-6"
            >
              {section === 'Workflows' ? (
                <WorkspaceWorkflows agents={agents} online={online} />
              ) : section === 'Goals' ? (
                <WorkspaceGoals agents={agents} />
              ) : section === 'Runs' ? (
                <div className="space-y-5">
                  <label className="block space-y-2 text-sm">
                    Agent
                    <select
                      className="field"
                      aria-label="Agent"
                      value={selectedId || agents[0]?.id || ''}
                      onChange={(event) => setAgentFilter(event.target.value)}
                    >
                      {agents.map((agent) => (
                        <option key={agent.id} value={agent.id}>
                          {agent.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  {agents.length ? (
                    <AgentRunsView agentId={selectedId || agents[0].id} />
                  ) : (
                    <p className="text-sm text-ink-3">
                      Create an agent to queue an assignment.
                    </p>
                  )}
                </div>
              ) : (
                <>
                  <p className="text-sm leading-relaxed text-ink-3">
                    {section === 'Notes'
                      ? 'Saved facts and notes, newest first. Recent notes only; search applies to the notes shown.'
                      : section === 'Tasks'
                        ? 'Each agent keeps their own task list. Choose an agent below to add tasks or update progress.'
                        : 'Schedules run through the daemon, even when this page is closed. Choose an agent to create, pause, or remove a schedule.'}
                  </p>
                  <div className="work-toolbar">
                    <label className="min-w-40 flex-1 space-y-2 text-sm">
                      Agent
                      <select
                        className="field"
                        aria-label="Agent"
                        value={selectedId}
                        onChange={(event) => setAgentFilter(event.target.value)}
                      >
                        <option value="">All agents</option>
                        {agents.map((agent) => (
                          <option key={agent.id} value={agent.id}>
                            {agent.name}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="min-w-40 flex-[2] space-y-2 text-sm">
                      Search
                      <input
                        className="field"
                        value={query}
                        placeholder={`Search ${section.toLowerCase()}…`}
                        onChange={(event) => setQuery(event.target.value)}
                      />
                    </label>
                    <button
                      type="button"
                      className="rounded-xl border border-line px-4 py-3 text-sm"
                      disabled={section === 'Tasks' && !online}
                      onClick={() => {
                        if (
                          section === 'Tasks' &&
                          (!online || generationRef.current !== generation)
                        )
                          return;
                        setRevision((value) => value + 1);
                      }}
                    >
                      Refresh
                    </button>
                  </div>
                  {section !== 'Notes' && (
                    <div className="flex flex-wrap gap-3">
                      {agents
                        .filter(
                          (agent) => !selectedId || agent.id === selectedId,
                        )
                        .map((agent) => (
                          <button
                            key={agent.id}
                            type="button"
                            className="flex items-center gap-2 rounded-xl border border-line px-4 py-3 text-sm"
                            onClick={() => setManagedId(agent.id)}
                          >
                            <AgentAvatar id={agent.id} name={agent.name} />
                            Manage {agent.name}’s {section.toLowerCase()}
                          </button>
                        ))}
                    </div>
                  )}
                  {errors?.map((error) => (
                    <p
                      key={error.agentId}
                      role="alert"
                      className="rounded-xl border border-danger/30 p-4 text-sm text-danger"
                    >
                      Could not load {section.toLowerCase()} for{' '}
                      {agents.find((agent) => agent.id === error.agentId)
                        ?.name ?? error.agentId}
                      : {error.message}. Use Refresh to retry.
                    </p>
                  ))}
                  {section === 'Tasks' && !online ? (
                    <p role="status">Offline. Reconnect to load tasks.</p>
                  ) : !result ? (
                    <p role="status">Loading {section.toLowerCase()}…</p>
                  ) : (
                    <>
                      <p className="text-xs text-ink-3">
                        {entries?.length}{' '}
                        {entries?.length === 1
                          ? section.toLowerCase().slice(0, -1)
                          : section.toLowerCase()}{' '}
                        shown
                        {errors?.length
                          ? ' · Some agents could not be loaded'
                          : ''}
                      </p>
                      {entries?.length === 0 && (
                        <p className="rounded-2xl border border-dashed border-line p-8 text-sm text-ink-3">
                          {query
                            ? 'No matches. Try another search.'
                            : errors?.length
                              ? 'No items available from the agents that loaded.'
                              : `No ${section.toLowerCase()} yet${selectedId ? ' for this agent' : ''}.`}
                        </p>
                      )}
                      <div className="settings-list work-list">
                        {entries?.map((entry) => {
                          const agent = agents.find(
                            (item) => item.id === entry.agentId,
                          );
                          return (
                            <article
                              key={`${entry.agentId}:${entry.id}`}
                              className="work-row"
                              data-section={section}
                            >
                              {section === 'Tasks' && (
                                <span
                                  className="work-status"
                                  data-status={entry.detail
                                    .toLowerCase()
                                    .replace(' ', '-')}
                                >
                                  {entry.detail}
                                </span>
                              )}
                              <div className="work-row-who">
                                <AgentAvatar
                                  id={entry.agentId}
                                  name={agent?.name ?? entry.agentId}
                                />
                                <span className="text-sm font-semibold">
                                  {agent?.name ?? entry.agentId}
                                </span>
                                {onSelectAgent && (
                                  <button
                                    type="button"
                                    className="ml-auto text-xs underline"
                                    onClick={() => onSelectAgent(entry.agentId)}
                                  >
                                    Chat with {agent?.name ?? entry.agentId}
                                  </button>
                                )}
                              </div>
                              <p className="work-row-text">{entry.content}</p>
                              {section !== 'Tasks' && (
                                <p className="work-row-detail">
                                  {entry.detail}
                                  {section === 'Notes' &&
                                  entry.timestamp != null
                                    ? ` · ${new Date(entry.timestamp).toLocaleString()}`
                                    : ''}
                                </p>
                              )}
                            </article>
                          );
                        })}
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          </>
        )}
      </div>
    </section>
  );
}
