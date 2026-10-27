import { useEffect, useRef, useState, type ReactNode } from 'react';
import type {
  Calculation,
  ChatResult,
  ChatResultPayload,
  ChatResultsClient,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { MarkdownMessage } from '../MarkdownMessage';
import { useLatest } from '../../hooks/useLatest';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useRenderScope } from '../../hooks/useRenderScope';
import { TrackerResult } from './TrackerResult';
import { InteractiveResultEditor } from './InteractiveResultEditor';

type ResultsApi = Pick<ChatResultsClient, 'list' | 'pin'> &
  Partial<Pick<ChatResultsClient, 'save'>>;
type ResultsState = {
  scope: string;
  results: ChatResult[];
  error: string | null;
  loading: boolean;
};

/** Every async result belongs to an exact agent, conversation and reset generation. */
export function useChatResults({
  agentId,
  sessionId,
  clearGeneration,
  refreshKey,
  api = daemon.chatResults,
}: {
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  refreshKey?: unknown;
  api?: ResultsApi;
}) {
  const { generation } = useRenderScope(agentId, sessionId, clearGeneration);
  const scope = JSON.stringify([agentId, sessionId, clearGeneration, generation]);
  const currentScope = useLatest(scope);
  const [state, setState] = useState<ResultsState>({
    scope,
    results: [],
    error: null,
    loading: true,
  });
  const [retry, setRetry] = useState(0);
  const [pending, setPending] = useState<{ scope: string; id: string } | null>(
    null,
  );
  const mutation = useRef<string | null>(null);
  useResetOnChange(
    [agentId, sessionId, clearGeneration, scope, refreshKey, retry, api],
    () => {
      setState((previous) => ({
        scope,
        results: previous.scope === scope ? previous.results : [],
        error: null,
        loading: true,
      }));
    },
  );
  useEffect(() => {
    const abort = new AbortController();
    let active = true;
    api
      .list(agentId, sessionId, { signal: abort.signal })
      .then((response) => {
        if (!active || currentScope.current !== scope) return;
        if (
          response.clearGeneration !== clearGeneration ||
          response.results.some(
            (result) => !owns(result, agentId, sessionId, clearGeneration),
          )
        ) {
          setState({
            scope,
            results: [],
            loading: false,
            error: 'The conversation changed; refresh its results.',
          });
          return;
        }
        setState((previous) => ({
          scope,
          loading: false,
          error: null,
          results: response.results.map((result) => {
            const newer =
              previous.scope === scope
                ? previous.results.find(
                    (old) =>
                      old.id === result.id && old.revision > result.revision,
                  )
                : null;
            return newer ?? result;
          }),
        }));
      })
      .catch((error: unknown) => {
        if (active && currentScope.current === scope)
          setState((previous) => ({
            scope,
            results: previous.scope === scope ? previous.results : [],
            loading: false,
            error: errorText(error),
          }));
      });
    return () => {
      active = false;
      abort.abort();
    };
  }, [
    agentId,
    sessionId,
    clearGeneration,
    scope,
    refreshKey,
    retry,
    api,
    currentScope,
  ]);

  async function pin(result: ChatResult) {
    if (
      mutation.current === scope ||
      pending?.scope === scope ||
      !owns(result, agentId, sessionId, clearGeneration)
    )
      return;
    mutation.current = scope;
    setPending({ scope, id: result.id });
    try {
      const response = await api.pin(agentId, sessionId, result.id, {
        clearGeneration,
        expectedRevision: result.revision,
        pinned: !result.pinned,
      });
      if (currentScope.current !== scope) return;
      if (
        !owns(response.result, agentId, sessionId, clearGeneration) ||
        response.result.id !== result.id
      ) {
        setState({
          scope,
          results: [],
          loading: false,
          error: 'The saved result no longer belongs to this conversation.',
        });
        return;
      }
      setState((previous) =>
        previous.scope !== scope
          ? previous
          : {
              ...previous,
              error: null,
              results: previous.results.map((old) =>
                old.id === result.id && old.revision <= response.result.revision
                  ? response.result
                  : old,
              ),
            },
      );
    } catch (error) {
      if (currentScope.current === scope)
        setState((previous) =>
          previous.scope !== scope
            ? previous
            : { ...previous, error: errorText(error) },
        );
    } finally {
      if (mutation.current === scope) mutation.current = null;
      setPending((previous) =>
        previous?.scope === scope && previous.id === result.id
          ? null
          : previous,
      );
    }
  }
  async function save(
    result: ChatResult,
    payload: ChatResultPayload,
  ): Promise<ChatResult> {
    if (
      !api.save ||
      mutation.current === scope ||
      !owns(result, agentId, sessionId, clearGeneration)
    )
      throw new Error(
        'This result is busy or no longer belongs to this conversation.',
      );
    mutation.current = scope;
    setPending({ scope, id: result.id });
    try {
      const response = await api.save(agentId, sessionId, clearGeneration, {
        id: result.id,
        expectedRevision: result.revision,
        requestKey: `edit_${crypto.randomUUID().replaceAll('-', '')}`,
        title: result.title,
        payload,
      });
      if (
        currentScope.current !== scope ||
        !owns(response.result, agentId, sessionId, clearGeneration) ||
        response.result.id !== result.id
      )
        throw new Error('The conversation changed while saving.');
      setState((previous) =>
        previous.scope !== scope
          ? previous
          : {
              ...previous,
              error: null,
              results: previous.results.map((old) =>
                old.id === result.id && old.revision <= response.result.revision
                  ? response.result
                  : old,
              ),
            },
      );
      return response.result;
    } finally {
      if (mutation.current === scope) mutation.current = null;
      setPending((previous) =>
        previous?.scope === scope && previous.id === result.id
          ? null
          : previous,
      );
    }
  }
  const visible =
    state.scope === scope
      ? state
      : { scope, results: [], loading: true, error: null };
  return {
    ...visible,
    pendingId: pending?.scope === scope ? pending.id : null,
    pin,
    save: api.save ? save : undefined,
    refresh: () => setRetry((n) => n + 1),
  };
}

function owns(
  result: ChatResult,
  agent: string,
  session: string,
  generation: number,
) {
  return (
    result.agentId === agent &&
    result.sessionId === session &&
    result.clearGeneration === generation
  );
}
function errorText(error: unknown) {
  return error instanceof Error ? error.message : 'Results are unavailable';
}

export function ChatResultsPanel(props: {
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  refreshKey?: unknown;
  api?: ResultsApi;
}) {
  const results = useChatResults(props);
  return <InlineResults state={results} />;
}

/** One fetched state powers the transcript results and the pinned report shelf. */
export function ChatResultsLayout({
  children,
  ...props
}: {
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  refreshKey?: unknown;
  api?: ResultsApi;
  children: (inlineResults: ReactNode) => ReactNode;
}) {
  const results = useChatResults(props);
  const pinned = results.results.filter((result) => result.pinned);
  const scope = JSON.stringify([
    props.agentId,
    props.sessionId,
    props.clearGeneration,
  ]);
  const [preview, setPreview] = useState<{ scope: string; id: string } | null>(
    null,
  );
  useResetOnChange([scope], () => setPreview(null));
  const selected =
    preview?.scope === scope
      ? results.results.find((result) => result.id === preview.id)
      : undefined;
  return (
    <div className="chat-results-layout flex min-h-0 min-w-0 flex-1 flex-col xl:flex-row">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {children(
          <InlineResults
            state={results}
            onOpen={(result) => setPreview({ scope, id: result.id })}
          />,
        )}
      </div>
      {(selected || pinned.length > 0) && (
        <aside
          aria-label={selected ? 'Interactive preview' : 'Pinned reports'}
          className={`max-h-[65vh] shrink-0 space-y-3 overflow-auto border-t border-line bg-panel-2/50 p-3 xl:max-h-none xl:border-l xl:border-t-0 ${selected ? 'xl:w-[min(45%,42rem)] xl:min-w-80' : 'xl:w-80'}`}
        >
          {selected ? (
            <>
              <div className="flex items-center justify-between gap-3">
                <h2 className="text-xs font-medium uppercase tracking-wide text-ink-3">
                  Live preview
                </h2>
                <button
                  type="button"
                  className="text-xs underline"
                  onClick={() => setPreview(null)}
                >
                  Close preview
                </button>
              </div>
              <ChatResultCard
                key={`${scope}:${selected.id}`}
                result={selected}
                onSave={results.save}
                onPin={() => void results.pin(selected)}
                pinDisabled={results.pendingId !== null}
              />
              <button
                type="button"
                className="text-xs text-ink-2 underline"
                onClick={results.refresh}
              >
                Refresh saved results
              </button>
            </>
          ) : (
            <>
              <h2 className="text-xs font-medium uppercase tracking-wide text-ink-3">
                Pinned reports
              </h2>
              {pinned.map((result) => (
                <ChatResultCard
                  key={`${result.id}:${result.revision}`}
                  result={result}
                  onPin={() => void results.pin(result)}
                  pinDisabled={results.pendingId !== null}
                  onOpen={() => setPreview({ scope, id: result.id })}
                />
              ))}
            </>
          )}
        </aside>
      )}
    </div>
  );
}

function InlineResults({
  state: results,
  onOpen,
}: {
  state: ReturnType<typeof useChatResults>;
  onOpen?: (result: ChatResult) => void;
}) {
  if (!results.loading && !results.error && results.results.length === 0)
    return null;
  const ordered = [...results.results].sort(
    (a, b) =>
      Number(b.pinned) - Number(a.pinned) || a.createdAtMs - b.createdAtMs,
  );
  return (
    <section
      aria-label="Conversation results"
      className="my-4 min-w-0 space-y-3"
    >
      {results.loading && (
        <p role="status" className="text-xs text-ink-3">
          Loading results…
        </p>
      )}
      {results.error && (
        <div role="alert" className="text-sm text-ink-2">
          {results.error}{' '}
          <button type="button" onClick={results.refresh} className="underline">
            Refresh results
          </button>
        </div>
      )}
      {ordered.map((result) => (
        <ChatResultCard
          key={`${result.id}:${result.revision}`}
          result={result}
          onPin={() => void results.pin(result)}
          pinDisabled={results.pendingId !== null}
          onOpen={onOpen ? () => onOpen(result) : undefined}
        />
      ))}
    </section>
  );
}

export function ChatResultCard({
  result,
  onPin,
  pinDisabled = false,
  onOpen,
  onSave,
}: {
  result: ChatResult;
  onPin?: () => void;
  pinDisabled?: boolean;
  onOpen?: () => void;
  onSave?: (
    result: ChatResult,
    payload: ChatResultPayload,
  ) => Promise<ChatResult>;
}) {
  return (
    <article
      data-result-id={result.id}
      className="min-w-0 rounded-xl border border-line bg-panel/95 p-4 text-sm"
    >
      <header className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="break-words font-medium text-ink">{result.title}</h3>
          <p className="mt-1 text-xs text-ink-3">
            {result.pinned ? 'Pinned report' : 'Saved result'} · revision{' '}
            {result.revision}
          </p>
        </div>
        {onPin && (
          <button
            type="button"
            disabled={pinDisabled}
            onClick={onPin}
            className="shrink-0 rounded border border-line px-2 py-1 text-xs disabled:opacity-50"
          >
            {result.pinned ? 'Unpin report' : 'Pin report'}
          </button>
        )}
      </header>
      {onOpen && (
        <button
          type="button"
          className="mb-3 rounded border border-line px-2 py-1 text-xs"
          onClick={onOpen}
        >
          Open preview
        </button>
      )}
      {onSave ? (
        <InteractiveResultEditor result={result} onSave={onSave}>
          {(payload, change, disabled) => (
            <ResultContent
              payload={payload}
              onChange={change}
              disabled={disabled}
            />
          )}
        </InteractiveResultEditor>
      ) : (
        <ResultContent payload={result.payload} />
      )}
      <a
        className="mt-3 inline-block text-xs text-ink-2 underline"
        download={`result-${result.id}.json`}
        href={`data:application/json;charset=utf-8,${encodeURIComponent(JSON.stringify({ title: result.title, revision: result.revision, payload: result.payload }, null, 2))}`}
      >
        Download source
      </a>
    </article>
  );
}

function ResultContent({
  payload,
  onChange,
  disabled = false,
}: {
  payload: ChatResultPayload;
  onChange?: (payload: ChatResultPayload) => void;
  disabled?: boolean;
}) {
  switch (payload.kind) {
    case 'document':
      return (
        <div className="max-h-96 overflow-auto">
          {payload.format === 'markdown' ? (
            <MarkdownMessage>{payload.text}</MarkdownMessage>
          ) : (
            <pre className="whitespace-pre-wrap break-words font-sans">
              {payload.text}
            </pre>
          )}
        </div>
      );
    case 'comparison':
      return <Comparison payload={payload} />;
    case 'chart':
      return <Chart payload={payload} />;
    case 'calculator':
      return <Calculator payload={payload} />;
    case 'tracker':
      return (
        <TrackerResult
          payload={payload}
          disabled={disabled}
          onChange={onChange}
        />
      );
    case 'dashboard':
      return (
        <div className="grid gap-4 2xl:grid-cols-2">
          {payload.widgets.map((widget) => (
            <section
              key={widget.id}
              className="min-w-0 rounded-lg border border-line p-3"
            >
              <h4 className="mb-3 font-medium text-ink">{widget.title}</h4>
              <ResultContent
                payload={widget.payload}
                disabled={disabled}
                onChange={
                  onChange
                    ? (child) => {
                        if (child.kind !== 'dashboard')
                          onChange({
                            ...payload,
                            widgets: payload.widgets.map((item) =>
                              item.id === widget.id
                                ? { ...item, payload: child }
                                : item,
                            ),
                          });
                      }
                    : undefined
                }
              />
            </section>
          ))}
        </div>
      );
    default:
      return <p>Unsupported saved result.</p>;
  }
}

function Comparison({
  payload,
}: {
  payload: Extract<ChatResultPayload, { kind: 'comparison' }>;
}) {
  const [query, setQuery] = useState('');
  const rows = payload.rows.filter((row) =>
    row.some((cell) =>
      cell.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    ),
  );
  return (
    <div className="space-y-3">
      <label className="block text-xs text-ink-2">
        Filter comparison
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="mt-1 w-full rounded border border-line bg-panel px-2 py-1 text-sm"
        />
      </label>
      <DataTable columns={payload.columns} rows={rows} />
      {rows.length === 0 && (
        <p className="text-xs text-ink-3">No matching rows.</p>
      )}
    </div>
  );
}
function DataTable({
  columns,
  rows,
}: {
  columns: string[];
  rows: (string | number)[][];
}) {
  return (
    <div className="max-h-80 overflow-auto">
      <table className="w-full border-collapse text-left text-xs">
        <thead>
          <tr>
            {columns.map((column, index) => (
              <th
                key={index}
                scope="col"
                className="border-b border-line p-2 font-medium"
              >
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, columnIndex) => (
                <td
                  key={columnIndex}
                  className="border-b border-line p-2 align-top"
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
const COLORS = [
  '#6d77d5',
  '#52a99d',
  '#c08a43',
  '#bd71a1',
  '#5e9fc6',
  '#8ca65f',
  '#c97666',
  '#8874af',
];
function Chart({
  payload,
}: {
  payload: Extract<ChatResultPayload, { kind: 'chart' }>;
}) {
  const [hidden, setHidden] = useState<Set<number>>(() => new Set());
  const visible = payload.series
    .map((series, index) => ({ ...series, index }))
    .filter((series) => !hidden.has(series.index));
  const values = visible.flatMap((series) => series.values);
  const min = Math.min(0, ...values),
    max = Math.max(0, ...values),
    range = max - min || 1;
  const y = (value: number) => 180 - ((value - min) / range) * 150;
  const x = (index: number) =>
    45 + ((index + 0.5) / Math.max(1, payload.labels.length)) * 520;
  const groupWidth = 480 / Math.max(1, payload.labels.length);
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {payload.series.map((series, index) => (
          <button
            type="button"
            key={index}
            aria-pressed={!hidden.has(index)}
            onClick={() =>
              setHidden((old) => {
                const next = new Set(old);
                if (next.has(index)) next.delete(index);
                else next.add(index);
                return next;
              })
            }
            className="rounded border border-line px-2 py-1 text-xs"
            style={{ opacity: hidden.has(index) ? 0.5 : 1 }}
          >
            <span aria-hidden style={{ color: COLORS[index % COLORS.length] }}>
              ●{' '}
            </span>
            {series.label}
          </button>
        ))}
      </div>
      <div className="overflow-x-auto">
        <svg
          viewBox="0 0 600 220"
          role="img"
          aria-label={`${payload.chartType === 'bar' ? 'Bar' : 'Line'} chart`}
          className="w-full min-w-80"
        >
          <line
            x1="35"
            x2="575"
            y1={y(0)}
            y2={y(0)}
            stroke="currentColor"
            opacity="0.2"
          />
          <text x="5" y="28" fontSize="10" fill="currentColor">
            {max.toLocaleString()}
          </text>
          <text x="5" y="185" fontSize="10" fill="currentColor">
            {min.toLocaleString()}
          </text>
          {visible.map((series, group) => (
            <g
              key={series.index}
              fill={COLORS[series.index % COLORS.length]}
              stroke={COLORS[series.index % COLORS.length]}
            >
              {payload.chartType === 'line' && (
                <polyline
                  fill="none"
                  strokeWidth="2"
                  points={series.values
                    .map((value, index) => `${x(index)},${y(value)}`)
                    .join(' ')}
                />
              )}
              {series.values.map((value, index) =>
                payload.chartType === 'bar' ? (
                  <rect
                    key={index}
                    x={
                      x(index) -
                      groupWidth / 2 +
                      (group * groupWidth) / Math.max(1, visible.length)
                    }
                    y={Math.min(y(0), y(value))}
                    width={Math.max(
                      0.25,
                      groupWidth / Math.max(1, visible.length) - 1,
                    )}
                    height={Math.abs(y(value) - y(0))}
                  >
                    <title>
                      {payload.labels[index]} · {series.label}: {value}
                    </title>
                  </rect>
                ) : (
                  <circle key={index} cx={x(index)} cy={y(value)} r="3">
                    <title>
                      {payload.labels[index]} · {series.label}: {value}
                    </title>
                  </circle>
                ),
              )}
            </g>
          ))}
          {payload.labels.map((label, index) =>
            payload.labels.length <= 12 ||
            index === 0 ||
            index === payload.labels.length - 1 ? (
              <text
                key={index}
                x={x(index)}
                y="205"
                textAnchor="middle"
                fontSize="10"
                fill="currentColor"
              >
                {label.length > 16 ? `${label.slice(0, 15)}…` : label}
              </text>
            ) : null,
          )}
        </svg>
      </div>
      <details>
        <summary className="cursor-pointer text-xs text-ink-2">
          View chart data
        </summary>
        <DataTable
          columns={[
            'Category',
            ...payload.series.map((series) => series.label),
          ]}
          rows={payload.labels.map((label, index) => [
            label,
            ...payload.series.map((series) => series.values[index]),
          ])}
        />
      </details>
    </div>
  );
}

/** Closed arithmetic only; a bad or undefined operation never runs source text. */
export function evaluateCalculation(
  expression: Calculation,
  values: Record<string, number>,
): number | null {
  let nodes = 0;
  function calculate(node: Calculation, depth: number): number | null {
    if (!node || typeof node !== 'object' || ++nodes > 64 || depth > 8)
      return null;
    let result: number | null;
    if (node.op === 'input')
      result = Object.hasOwn(values, node.key) ? values[node.key] : null;
    else if (node.op === 'constant') result = node.value;
    else if (
      node.op === 'add' ||
      node.op === 'subtract' ||
      node.op === 'multiply' ||
      node.op === 'divide'
    ) {
      const left = calculate(node.left, depth + 1),
        right = calculate(node.right, depth + 1);
      if (
        left === null ||
        right === null ||
        (node.op === 'divide' && right === 0)
      )
        return null;
      result =
        node.op === 'add'
          ? left + right
          : node.op === 'subtract'
            ? left - right
            : node.op === 'multiply'
              ? left * right
              : left / right;
    } else return null;
    return typeof result === 'number' && Number.isFinite(result)
      ? result
      : null;
  }
  return calculate(expression, 0);
}
function Calculator({
  payload,
}: {
  payload: Extract<ChatResultPayload, { kind: 'calculator' }>;
}) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      payload.inputs.map((input) => [input.key, String(input.value)]),
    ),
  );
  const invalid = payload.inputs.some(
    (input) =>
      !values[input.key]?.trim() ||
      !Number.isFinite(Number(values[input.key])) ||
      (input.min !== undefined && Number(values[input.key]) < input.min) ||
      (input.max !== undefined && Number(values[input.key]) > input.max),
  );
  const result = invalid
    ? null
    : evaluateCalculation(
        payload.expression,
        Object.fromEntries(
          Object.entries(values).map(([key, value]) => [key, Number(value)]),
        ),
      );
  return (
    <div className="space-y-3">
      <div className="grid gap-3 sm:grid-cols-2">
        {payload.inputs.map((input) => (
          <label key={input.key} className="block text-xs text-ink-2">
            {input.label}
            <input
              type="number"
              step="any"
              min={input.min}
              max={input.max}
              value={values[input.key] ?? ''}
              onChange={(event) =>
                setValues((previous) => ({
                  ...previous,
                  [input.key]: event.target.value,
                }))
              }
              className="mt-1 w-full rounded border border-line bg-panel px-2 py-1 text-sm"
            />
          </label>
        ))}
      </div>
      <p className="font-medium" aria-live="polite">
        {payload.outputLabel}:{' '}
        {result === null
          ? 'Enter valid values'
          : result.toLocaleString(undefined, { maximumFractionDigits: 6 })}
      </p>
      <p className="text-xs text-ink-3">
        Input changes are local to this preview. The saved report keeps its
        original defaults.
      </p>
    </div>
  );
}
