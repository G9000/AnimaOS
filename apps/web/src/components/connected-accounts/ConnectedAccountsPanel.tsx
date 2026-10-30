import { useEffect, useRef, useState } from 'react';
import {
  createDaemonClient,
  type DaemonClient,
  type ConnectedSource,
  type ConnectedSearchHit,
  type ConnectedSearchItem,
  type AccountWorkflowRule,
  type AccountWorkflowInput,
  type AccountWorkflowEvent,
  type WorkflowTemplate,
  type WorkflowRun,
} from '@animaos/sdk';
import { ownerFetch } from '../../lib/owner-fetch';
import { useRenderScope } from '../../hooks/useRenderScope';
import { AccountRuleEditor } from './AccountRuleEditor';
import { SourceSearchSettings } from './SourceSearchSettings';
import {
  accountButton,
  providerName,
  sourceStatus,
  workflowStatus,
} from './presentation';

export type AccountsApi = Pick<
  DaemonClient,
  'connectedSearch' | 'accountWorkflows' | 'workflowTemplates' | 'workflows'
>;
const defaultApi = createDaemonClient({ baseUrl: '', fetch: ownerFetch });
const date = (value: number | null) =>
  value ? new Date(value).toLocaleString() : 'Not synced yet';
const message = (cause: unknown) =>
  cause instanceof Error
    ? cause.message
    : 'Could not complete this action. Refresh and try again.';

export function ConnectedAccountsPanel({
  agentId,
  online = true,
  epoch = 0,
  disabled = false,
  api = defaultApi,
  onGrantsChanged,
}: {
  agentId: string;
  online?: boolean;
  epoch?: number;
  disabled?: boolean;
  api?: AccountsApi;
  onGrantsChanged?: (toolNames: string[]) => void | Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const scope = useRenderScope(agentId, online, epoch);
  return (
    <section
      className="connections-section space-y-4"
      aria-label="Connected account search and workflows"
    >
      <div className="sk-card">
        <button
          type="button"
          className="sk-row sk-nav"
          aria-label="Search and account workflows"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <span className="sk-row-text">
            <span className="sk-row-title">Search and account workflows</span>
            <span className="sk-row-desc">
              Search across connected accounts and set what it may do with
              them.
            </span>
          </span>
          <span aria-hidden="true">{open ? '−' : '+'}</span>
        </button>
      </div>
      {open &&
        (online ? (
          <AccountContents
            key={scope.generation}
            agentId={agentId}
            api={api}
            disabled={disabled}
            onGrantsChanged={onGrantsChanged}
          />
        ) : (
          <p role="status">Reconnect to search accounts and manage rules.</p>
        ))}
    </section>
  );
}

function AccountContents({
  agentId,
  api,
  disabled,
  onGrantsChanged,
}: {
  agentId: string;
  api: AccountsApi;
  disabled: boolean;
  onGrantsChanged?: (toolNames: string[]) => void | Promise<void>;
}) {
  const [sources, setSources] = useState<ConnectedSource[]>([]);
  const [rules, setRules] = useState<AccountWorkflowRule[]>([]);
  const [templates, setTemplates] = useState<WorkflowTemplate[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [kind, setKind] = useState('');
  const [sourceFilter, setSourceFilter] = useState('');
  const [hits, setHits] = useState<ConnectedSearchHit[] | null>(null);
  const [item, setItem] = useState<ConnectedSearchItem | null>(null);
  const [workflow, setWorkflow] = useState<WorkflowRun | null>(null);
  const [editor, setEditor] = useState<AccountWorkflowRule | 'new' | null>(
    null,
  );
  const [history, setHistory] = useState<{
    ruleId: string;
    events: AccountWorkflowEvent[];
  } | null>(null);
  const alive = useRef(false),
    locked = useRef(false),
    readSequence = useRef(0);
  const currentSources = useRef<ConnectedSource[]>([]);
  const canMutate = useRef(!disabled);
  canMutate.current = !disabled;
  const acceptSources = (next: ConnectedSource[]) => {
    // An authorization transition invalidates retained previews immediately.
    const authority = (items: ConnectedSource[]) =>
      JSON.stringify(
        items.map((s) => [
          s.connectorId,
          s.generation,
          s.enabled,
          s.available,
          s.status === 'reauthRequired',
        ]),
      );
    if (authority(currentSources.current) !== authority(next)) {
      setHits(null);
      setItem(null);
    }
    currentSources.current = next;
    setSources(next);
  };
  useEffect(() => {
    alive.current = true;
    const sequenceRef = readSequence;
    const controller = new AbortController();
    const load = async () => {
      if (locked.current) return;
      const sequence = ++readSequence.current;
      try {
        const [sourceResult, ruleResult, templateResult] =
          await Promise.allSettled([
            api.connectedSearch.sources(agentId, { signal: controller.signal }),
            api.accountWorkflows.list(agentId, { signal: controller.signal }),
            api.workflowTemplates.list(agentId, { signal: controller.signal }),
          ]);
        if (!alive.current || sequence !== readSequence.current) return;
        if (sourceResult.status === 'fulfilled')
          acceptSources(sourceResult.value.sources);
        else {
          acceptSources([]);
          setHits(null);
          setItem(null);
        }
        if (ruleResult.status === 'fulfilled') setRules(ruleResult.value);
        if (templateResult.status === 'fulfilled')
          setTemplates(templateResult.value.filter((t) => !t.deleted));
        const failed = [sourceResult, ruleResult, templateResult].find(
          (result) => result.status === 'rejected',
        );
        setError(failed?.status === 'rejected' ? message(failed.reason) : null);
      } catch (cause) {
        if (alive.current && sequence === readSequence.current)
          setError(message(cause));
      } finally {
        if (alive.current && sequence === readSequence.current)
          setLoading(false);
      }
    };
    void load();
    const timer = window.setInterval(() => {
      if (document.visibilityState !== 'hidden') void load();
    }, 15000);
    return () => {
      alive.current = false;
      ++sequenceRef.current;
      controller.abort();
      window.clearInterval(timer);
    };
    // This subtree is remounted for each owner/connection generation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, api]);

  async function run(action: () => Promise<void>, success?: string) {
    if (locked.current || !alive.current || !canMutate.current) return;
    locked.current = true;
    ++readSequence.current;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (alive.current && success) setNotice(success);
    } catch (cause) {
      if (alive.current) setError(message(cause));
    } finally {
      if (alive.current) {
        locked.current = false;
        setBusy(false);
        setLoading(false);
      }
    }
  }
  const blocked = disabled || busy || loading;
  const eligibleSources = sources.filter((s) => s.enabled && s.available);
  const ruleSources = eligibleSources.filter((source) => !source.capabilities || source.capabilities.workflowEventKinds.length > 0);
  const selectedFilter = eligibleSources.some(
    (s) => s.connectorId === sourceFilter,
  )
    ? sourceFilter
    : '';
  const ruleSaved = async (input: AccountWorkflowInput) => {
    const saved =
      editor && editor !== 'new'
        ? await api.accountWorkflows.update(agentId, editor.id, {
            ...input,
            revision: editor.revision,
          })
        : await api.accountWorkflows.create(agentId, input);
    if (alive.current) {
      setRules((previous) => [
        ...previous.filter((rule) => rule.id !== saved.id),
        saved,
      ]);
      setEditor(null);
    }
  };
  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-base font-semibold">Search your accounts</h2>
        <p className="mt-1 text-xs text-ink-3">
          Choose what your companion can search in this workspace. Enabled
          accounts refresh about every two minutes while AnimaOS is running.
          Search covers the retained scope below.
        </p>
      </div>
      {loading && <p role="status">Loading account sources…</p>}
      {error && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-xs text-accent">
          {notice}
        </p>
      )}
      {!loading && sources.length === 0 && (
        <p className="text-xs text-ink-3">
          Connect a mail, Drive or Calendar account in Services to get started.
        </p>
      )}
      <div className="space-y-3">
        {sources.map((source) => (
          <article
            key={source.connectorId}
            className="connections-card space-y-2"
          >
            <label className="flex items-start gap-3 text-sm">
              <input
                type="checkbox"
                checked={source.enabled}
                disabled={blocked || (!source.available && !source.enabled)}
                aria-label={`Use ${providerName(source.provider)} ${source.accountLabel ?? ''} for search and workflows`}
                onChange={(event) => {
                  const enabled = event.target.checked;
                  if (!enabled) {
                    setHits(null);
                    setItem(null);
                  }
                  void run(async () => {
                    const next = await api.connectedSearch.setSource(
                      agentId,
                      source.connectorId,
                      { enabled, expectedGeneration: source.generation },
                    );
                    if (alive.current)
                      acceptSources(
                        currentSources.current.map((s) =>
                          s.connectorId === next.connectorId ? next : s,
                        ),
                      );
                    if (alive.current && enabled)
                      await onGrantsChanged?.(['connected_search']);
                  }, 'Account settings saved.');
                }}
              />
              <span>
                <strong>{providerName(source.provider)}</strong>
                <span className="block break-all text-xs text-ink-3">
                  {source.accountLabel}
                </span>
              </span>
            </label>
            <p className="text-xs text-ink-2">{source.scope}</p>
            <p className="text-xs text-ink-3">
              {sourceStatus(source.status)} · {source.itemCount} indexed ·{' '}
              {date(source.lastSuccessAtMs)}
            </p>
            {source.error && (
              <p role="status" className="text-xs text-danger">
                {source.error}
              </p>
            )}
            {source.enabled && source.settings && source.settingsRevision && source.capabilities && (
              <SourceSearchSettings
                key={`${source.connectorId}:${source.generation}:${source.settingsRevision}`}
                source={source}
                disabled={blocked}
                onSave={(settings) => {
                  if (settings.historyDepth === 'recent' || settings.attachmentText === 'metadataOnly') {
                    setHits(null);
                    setItem(null);
                  }
                  void run(async () => {
                    const next = await api.connectedSearch.setSource(agentId, source.connectorId, {
                      enabled: true,
                      expectedGeneration: source.generation,
                      expectedSettingsRevision: source.settingsRevision,
                      settings,
                    });
                    if (alive.current) acceptSources(currentSources.current.map((item) => item.connectorId === next.connectorId ? next : item));
                  }, 'Search settings saved.');
                }}
              />
            )}
          </article>
        ))}
      </div>
      <button
        type="button"
        className={accountButton}
        disabled={blocked || !eligibleSources.length}
        onClick={() => {
          setHits(null);
          setItem(null);
          void run(async () => {
            const result = await api.connectedSearch.refresh(agentId);
            if (alive.current) acceptSources(result.sources);
          }, 'Account refresh requested. Sources update as syncing finishes.');
        }}
      >
        Refresh account content
      </button>
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          setItem(null);
          void run(async () => {
            const result = await api.connectedSearch.query(agentId, {
              query: query.trim(),
              ...(kind ? { kinds: [kind as ConnectedSource['kind']] } : {}),
              ...(selectedFilter ? { connectorIds: [selectedFilter] } : {}),
              limit: 30,
            });
            if (alive.current) {
              acceptSources(result.sources);
              setHits(result.hits);
            }
          });
        }}
      >
        <label className="block text-xs">
          Search connected accounts
          <input
            type="search"
            className="field mt-1 w-full"
            value={query}
            maxLength={512}
            required
            onChange={(e) => setQuery(e.target.value)}
            disabled={blocked}
          />
        </label>
        <div className="flex flex-wrap gap-3">
          <label className="text-xs">
            Content type
            <select
              className="field ml-2"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
              disabled={blocked}
            >
              <option value="">All content</option>
              <option value="mail">Mail</option>
              <option value="file">Files</option>
              <option value="event">Calendar</option>
            </select>
          </label>
          <label className="text-xs">
            Account
            <select
              className="field ml-2"
              value={selectedFilter}
              onChange={(e) => setSourceFilter(e.target.value)}
              disabled={blocked}
            >
              <option value="">All enabled accounts</option>
              {eligibleSources.map((s) => (
                <option key={s.connectorId} value={s.connectorId}>
                  {providerName(s.provider)} · {s.accountLabel}
                </option>
              ))}
            </select>
          </label>
        </div>
        <button
          type="submit"
          className={accountButton}
          disabled={blocked || !query.trim() || !eligibleSources.length}
        >
          Search accounts
        </button>
      </form>
      {hits && (
        <div className="space-y-3" aria-label="Account search results">
          <p className="text-xs text-ink-3">
            {hits.length
              ? `${hits.length} ${hits.length === 1 ? 'result' : 'results'} in retained account content`
              : 'No match in retained account content. Some older or unsupported content may be missing.'}
          </p>
          {hits.map((hit) => (
            <article
              key={hit.citationId}
              className="rounded-xl border border-line p-3 text-sm"
            >
              <button
                type="button"
                className="font-medium text-accent underline"
                disabled={blocked}
                aria-label={`Read ${hit.title}`}
                onClick={() =>
                  void run(async () => {
                    const result = await api.connectedSearch.item(
                      agentId,
                      hit.citationId,
                    );
                    if (alive.current) setItem(result);
                  })
                }
              >
                {hit.title || '(Untitled)'}
              </button>
              <p className="mt-1 whitespace-pre-wrap break-words text-xs">
                {hit.snippet}
              </p>
              <p className="mt-2 text-xs text-ink-3">
                {providerName(hit.provider)} · indexed {date(hit.indexedAtMs)}
                {hit.truncated ? ' · shortened text' : ''}
                {hit.contentRole === 'attachment' ? ' · attachment' : ''}
                {hit.extractionStatus && hit.extractionStatus !== 'ready' ? ` · ${hit.extractionStatus}` : ''}
              </p>
            </article>
          ))}
        </div>
      )}
      {item && (
        <section
          aria-label="Cited account content"
          className="space-y-3 rounded-xl border border-line p-4"
        >
          <div className="flex justify-between gap-3">
            <h3 className="font-medium">{item.title}</h3>
            <button
              type="button"
              className={accountButton}
              onClick={() => setItem(null)}
            >
              Close source
            </button>
          </div>
          <p className="text-xs text-ink-3">
            {item.sender} · {providerName(item.provider)} · indexed{' '}
            {date(item.indexedAtMs)}
          </p>
          {item.eventStart && (
            <p className="text-xs">
              {item.eventStart} – {item.eventEnd}
            </p>
          )}
          <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
            {item.text}
          </pre>
          {item.truncated && (
            <p className="text-xs text-ink-3">
              Text was shortened to fit the local index. Open the original for
              the complete content.
            </p>
          )}
          {item.extractionStatus && item.extractionStatus !== 'ready' && (
            <p className="text-xs text-ink-3">Some content could not be included: {item.extractionStatus}.</p>
          )}
          {item.omissions && item.omissions.length > 0 && (
            <p className="text-xs text-ink-3">The retained text covers only supported parts of this document.</p>
          )}
          {safeSourceUrl(item.sourceUrl) && (
            <a
              className="text-xs text-accent underline"
              href={safeSourceUrl(item.sourceUrl)!}
              target="_blank"
              rel="noopener noreferrer"
            >
              Open original
            </a>
          )}
        </section>
      )}
      <section
        className="space-y-4 border-t border-line pt-5"
        aria-label="Account workflow rules"
      >
        <h2 className="text-base font-semibold">
          When account activity changes
        </h2>
        <p className="text-xs leading-relaxed text-ink-3">
          Start a saved workflow when new mail is observed, a selected file
          changes, or a calendar event changes. Each enabled rule first takes a
          fresh baseline. Polling covers retained content and can miss activity
          between refreshes. Workflow approval steps still apply.
        </p>
        <a href="#/work" className="text-xs text-accent underline">
          Manage saved workflows and review runs
        </a>
        <div>
          <button
            type="button"
            className={accountButton}
            disabled={blocked || !ruleSources.length || !templates.length}
            onClick={() => setEditor('new')}
          >
            New account rule
          </button>
        </div>
        {!templates.length && !loading && (
          <p className="text-xs text-ink-3">
            Save a workflow under Work before creating an account rule.
          </p>
        )}
        {editor && (
          <AccountRuleEditor
            key={editor === 'new' ? 'new' : `${editor.id}:${editor.revision}`}
            sources={ruleSources}
            templates={templates}
            rule={editor === 'new' ? undefined : editor}
            disabled={blocked}
            onCancel={() => setEditor(null)}
            onSave={(input) =>
              void run(() => ruleSaved(input), 'Account rule saved.')
            }
          />
        )}
        {rules.map((rule) => (
          <article
            key={rule.id}
            className="space-y-2 rounded-xl border border-line p-3 text-xs"
          >
            <h3 className="font-medium text-sm">{rule.name}</h3>
            <p>
              {rule.enabled
                ? rule.baselineAtMs
                  ? 'Watching for activity'
                  : 'Waiting for a fresh baseline'
                : 'Paused'}{' '}
              · {rule.templateSlug} version {rule.templateRevision}
            </p>
            {rule.lastOutcome && (
              <p className="text-ink-3">{rule.lastOutcome.message}</p>
            )}
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className={accountButton}
                disabled={blocked}
                onClick={() => setEditor(rule)}
              >
                Edit {rule.name}
              </button>
              <button
                type="button"
                className={accountButton}
                disabled={blocked}
                onClick={() =>
                  void run(async () => {
                    const saved = await api.accountWorkflows.update(
                      agentId,
                      rule.id,
                      {
                        revision: rule.revision,
                        enabled: !rule.enabled,
                      },
                    );
                    if (alive.current)
                      setRules((previous) =>
                        previous.map((entry) =>
                          entry.id === saved.id ? saved : entry,
                        ),
                      );
                  }, 'Account rule updated.')
                }
              >
                {rule.enabled ? 'Pause' : 'Enable'} {rule.name}
              </button>
              <button
                type="button"
                className={accountButton}
                disabled={blocked}
                onClick={() =>
                  void run(async () => {
                    const events = await api.accountWorkflows.events(
                      agentId,
                      rule.id,
                    );
                    if (alive.current) setHistory({ ruleId: rule.id, events });
                  })
                }
              >
                History for {rule.name}
              </button>
              <button
                type="button"
                className={accountButton}
                disabled={blocked}
                onClick={() =>
                  void run(async () => {
                    await api.accountWorkflows.delete(agentId, rule.id, {
                      revision: rule.revision,
                    });
                    if (alive.current) {
                      setRules((previous) =>
                        previous.filter((entry) => entry.id !== rule.id),
                      );
                      if (history?.ruleId === rule.id) setHistory(null);
                    }
                  }, 'Account rule removed.')
                }
              >
                Remove {rule.name}
              </button>
            </div>
          </article>
        ))}
        {history && (
          <div aria-label="Account workflow history" className="space-y-2">
            <h3 className="text-sm font-medium">Started workflows</h3>
            {!history.events.length && (
              <p className="text-xs">No workflows started by this rule yet.</p>
            )}
            {history.events.map((event) => (
              <div key={event.id} className="break-words text-xs space-y-2">
                {date(event.createdAtMs)} · rule version {event.ruleRevision}
                <div>
                  <button
                    type="button"
                    className={accountButton}
                    disabled={blocked}
                    onClick={() =>
                      void run(async () => {
                        const result = await api.workflows.get(
                          agentId,
                          event.workflowId,
                        );
                        if (alive.current) setWorkflow(result);
                      })
                    }
                  >
                    Review workflow
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
        {workflow && (
          <section
            aria-label="Triggered workflow"
            className="space-y-3 rounded-xl border border-line p-4"
          >
            <h3 className="font-medium">{workflow.title}</h3>
            <p className="text-xs">
              {workflowStatus(workflow.status)} · version {workflow.revision}
            </p>
            {workflow.steps.map((step) => (
              <div key={step.id} className="text-xs space-y-1">
                <strong>{step.title}</strong>
                <p>{workflowStatus(step.status)}</p>
                {step.result && (
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words font-sans">
                    {step.result}
                  </pre>
                )}
              </div>
            ))}
            <a href="#/work" className="text-xs text-accent underline">
              Open Work for approvals and questions
            </a>
            <div>
              <button
                type="button"
                className={accountButton}
                onClick={() => setWorkflow(null)}
              >
                Close workflow
              </button>
            </div>
          </section>
        )}
      </section>
    </div>
  );
}

function safeSourceUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && !url.username && !url.password
      ? url.href
      : null;
  } catch {
    return null;
  }
}
