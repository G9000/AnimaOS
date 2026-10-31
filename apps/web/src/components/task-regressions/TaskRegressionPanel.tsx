import { useEffect, useId, useRef, useState } from 'react';
import type {
  RegressionSuite,
  TaskRegressionConfiguration,
  TaskRegressionReport,
  TaskRegressionSuiteSummary,
  TaskRegressionToolPolicy,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { ghostBtnCls, primaryBtnCls } from '../ui-bits';
import { TaskRegressionCard } from './TaskRegressionCard';

const SYNTHETIC_SUITE: RegressionSuite = {
  name: 'Synthetic file task',
  fixtures: [
    {
      id: 'write-result',
      actor: {
        prompt:
          'Write exactly done (without a newline) to result.txt using the file tool. Then answer exactly Done.',
        initialFiles: {},
        readableFiles: [],
        writableFiles: ['result.txt'],
        mockEffects: false,
      },
      oracle: {
        expectedFiles: { 'result.txt': 'done' },
        expectedOutput: 'Done.',
        expectedMockEffects: [],
      },
    },
  ],
};
const SUITE_JSON_LIMIT = 65_536;

interface SimpleTaskDefinition {
  name: string;
  instructions: string;
  inputFile: string;
  inputContents: string;
  resultFile: string;
  resultContents: string;
  expectedReply: string;
}

function suiteForSimpleTask(task: SimpleTaskDefinition): RegressionSuite {
  const inputFile = task.inputFile.trim();
  const resultFile = task.resultFile.trim();
  const initialFiles: Record<string, string> = inputFile
    ? { [inputFile]: task.inputContents }
    : {};
  return {
    name: task.name.trim(),
    fixtures: [
      {
        id: 'write-result',
        actor: {
          prompt: task.instructions,
          initialFiles,
          readableFiles: Object.keys(initialFiles),
          writableFiles: [resultFile],
          mockEffects: false,
        },
        oracle: {
          expectedFiles: { ...initialFiles, [resultFile]: task.resultContents },
          expectedOutput: task.expectedReply,
          expectedMockEffects: [],
        },
      },
    ],
  };
}

interface PanelProps {
  agentId: string;
  disabled?: boolean;
  hasUnsavedChanges?: boolean;
}

export function TaskRegressionPanel({
  agentId,
  disabled = false,
  hasUnsavedChanges = false,
}: PanelProps) {
  // A different profile starts closed, without fetching the new agent's config.
  return (
    <TaskRegressionEntry
      key={agentId}
      agentId={agentId}
      disabled={disabled}
      hasUnsavedChanges={hasUnsavedChanges}
    />
  );
}

function TaskRegressionEntry({
  agentId,
  disabled,
  hasUnsavedChanges,
}: Required<PanelProps>) {
  const [expanded, setExpanded] = useState(false);
  const [opened, setOpened] = useState(false);
  const panelId = useId();
  return (
    <section className="settings-group" aria-label="Saved task comparisons">
      <button
        type="button"
        className={ghostBtnCls}
        disabled={disabled}
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={() => {
          setOpened(true);
          setExpanded((value) => !value);
        }}
      >
        Compare on saved tasks
      </button>
      {opened && (
        <div id={panelId} className="mt-4" hidden={!expanded}>
          <TaskRegressionForm
            agentId={agentId}
            disabled={disabled}
            hasUnsavedChanges={hasUnsavedChanges}
          />
        </div>
      )}
    </section>
  );
}

function TaskRegressionForm({
  agentId,
  disabled,
  hasUnsavedChanges,
}: Required<PanelProps>) {
  const [suites, setSuites] = useState<TaskRegressionSuiteSummary[]>([]);
  const [suiteId, setSuiteId] = useState('');
  const [configuration, setConfiguration] =
    useState<TaskRegressionConfiguration | null>(null);
  const [model, setModel] = useState('');
  const [provider, setProvider] = useState('');
  const [system, setSystem] = useState('');
  const [toolPolicy, setToolPolicy] =
    useState<TaskRegressionToolPolicy>('files');
  const [repetitions, setRepetitions] = useState(1);
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [defining, setDefining] = useState(false);
  const [task, setTask] = useState<SimpleTaskDefinition>({
    name: SYNTHETIC_SUITE.name,
    instructions: SYNTHETIC_SUITE.fixtures[0].actor.prompt,
    inputFile: '',
    inputContents: '',
    resultFile: 'result.txt',
    resultContents: 'done',
    expectedReply: SYNTHETIC_SUITE.fixtures[0].oracle.expectedOutput,
  });
  const [advancedJson, setAdvancedJson] = useState<string | null>(null);
  const [report, setReport] = useState<TaskRegressionReport | null>(null);
  const [reportId, setReportId] = useState('');
  const mounted = useRef(true);
  const actionLock = useRef(false);
  const startAttempt = useRef<{ signature: string; requestKey: string } | null>(
    null,
  );
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    let ignore = false;
    Promise.all([
      daemon.taskRegressions.suites(),
      daemon.taskRegressions.configuration(agentId),
    ])
      .then(([saved, config]) => {
        if (ignore) return;
        setSuites(saved);
        setSuiteId(saved[0]?.id ?? '');
        setConfiguration(config);
        setModel(config.model);
        setProvider(config.provider ?? '');
        setSystem(config.system ?? '');
      })
      .catch((caught) => {
        if (!ignore) setError(errorMessage(caught));
      })
      .finally(() => {
        if (!ignore) setLoading(false);
      });
    return () => {
      ignore = true;
    };
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- revision is the explicit owner refresh trigger.
  }, [agentId, revision]);

  async function action(operation: () => Promise<void>) {
    if (disabled || actionLock.current) return;
    actionLock.current = true;
    setBusy(true);
    setError(null);
    try {
      await operation();
    } catch (caught) {
      if (mounted.current) setError(errorMessage(caught));
    } finally {
      actionLock.current = false;
      if (mounted.current) setBusy(false);
    }
  }
  function receiveReport(next: TaskRegressionReport) {
    if (!mounted.current) return;
    setReport(next);
    setReportId(next.id);
  }
  const selectedSuite = suites.find((suite) => suite.id === suiteId);
  const running = report?.status === 'running';
  const locked = disabled || busy || loading;
  const simpleSuite = suiteForSimpleTask(task);
  const suiteJson = advancedJson ?? JSON.stringify(simpleSuite, null, 2);

  function changeTask(field: keyof SimpleTaskDefinition, value: string) {
    setTask((current) => ({ ...current, [field]: value }));
  }

  async function compare() {
    if (!configuration || !selectedSuite || running || hasUnsavedChanges)
      return;
    if (
      !model.trim() ||
      !Number.isInteger(repetitions) ||
      repetitions < 1 ||
      repetitions > 3
    )
      throw new Error('Choose a candidate model and 1 to 3 repetitions.');
    const selection = {
      agentId: configuration.agentId,
      configDigest: configuration.configDigest,
    };
    const input = {
      suiteId: selectedSuite.id,
      suiteDigest: selectedSuite.digest,
      baseline: selection,
      candidate: {
        ...selection,
        model: model.trim(),
        ...(provider.trim() ? { provider: provider.trim() } : {}),
        ...(system !== (configuration.system ?? '') ? { system } : {}),
        toolPolicy,
      },
      repetitions,
    };
    const signature = JSON.stringify(input);
    if (startAttempt.current?.signature !== signature)
      startAttempt.current = { signature, requestKey: crypto.randomUUID() };
    // Keep this key on an uncertain transport failure so a manual retry joins it.
    receiveReport(
      await daemon.taskRegressions.compare({
        ...input,
        requestKey: startAttempt.current.requestKey,
      }),
    );
    startAttempt.current = null;
  }
  async function saveSuite(suite: RegressionSuite) {
    if (
      new TextEncoder().encode(JSON.stringify(suite)).length > SUITE_JSON_LIMIT
    )
      throw new Error('Keep the task suite within 64 KB.');
    const saved = await daemon.taskRegressions.define(suite);
    if (!mounted.current) return;
    setSuites((existing) => [
      ...existing.filter((item) => item.id !== saved.id),
      saved,
    ]);
    setSuiteId(saved.id);
    setDefining(false);
  }
  async function defineSimple() {
    if (
      !task.name.trim() ||
      !task.instructions.trim() ||
      !task.resultFile.trim()
    )
      throw new Error(
        'Add a suite name, task instructions and result file name.',
      );
    if (!task.inputFile.trim() && task.inputContents)
      throw new Error('Add an input file name, or clear its contents.');
    await saveSuite(simpleSuite);
  }
  async function defineAdvanced() {
    if (new TextEncoder().encode(suiteJson).length > SUITE_JSON_LIMIT)
      throw new Error('Keep the task suite within 64 KB.');
    let suite: RegressionSuite;
    try {
      suite = JSON.parse(suiteJson) as RegressionSuite;
    } catch {
      throw new Error('Enter valid JSON for the synthetic suite.');
    }
    await saveSuite(suite);
  }
  return (
    <div className="space-y-4 text-sm">
      <p className="text-ink-3">
        Compare saved sample tasks before changing this agent. Only the explicit
        run action starts the selected models. Reports stay in this owner view.
      </p>
      {hasUnsavedChanges && (
        <p className="text-ink-3">
          Save profile changes and capture the current configuration before
          starting a comparison.
        </p>
      )}
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {loading && (
        <p role="status">Loading saved suites and current configuration…</p>
      )}
      <button
        type="button"
        className={ghostBtnCls}
        disabled={locked || running}
        onClick={() => {
          setLoading(true);
          setError(null);
          setConfiguration(null);
          setRevision((value) => value + 1);
        }}
      >
        Capture current configuration
      </button>
      {configuration && (
        <>
          <p className="break-words text-xs text-ink-3">
            Baseline: {configuration.provider ?? 'default provider'} /{' '}
            {configuration.model}.
          </p>
          <details>
            <summary className="cursor-pointer">Version details</summary>
            <dl className="mt-2 space-y-2 break-all text-xs text-ink-3">
              <div>
                <dt>Captured configuration</dt>
                <dd>{configuration.configDigest}</dd>
              </div>
              {selectedSuite && (
                <div>
                  <dt>Saved task suite</dt>
                  <dd>{selectedSuite.digest}</dd>
                </div>
              )}
            </dl>
          </details>
          <p className="text-xs text-ink-3">
            Baseline tools: files and simulated actions allowed by each saved
            task. Candidate tools can use the same access or files only.
          </p>
          <details>
            <summary className="cursor-pointer">
              Captured baseline instructions
            </summary>
            <pre className="mt-2 whitespace-pre-wrap break-words text-xs">
              {configuration.system === null
                ? "Uses the task runner's default instructions."
                : configuration.system || '(empty)'}
            </pre>
          </details>
          <fieldset disabled={locked || running} className="space-y-3">
            <label className="block">
              Saved task suite
              <select
                className="field mt-1"
                value={suiteId}
                onChange={(event) => setSuiteId(event.target.value)}
              >
                {!suites.length && <option value="">No saved suites</option>}
                {suites.map((suite) => (
                  <option key={suite.id} value={suite.id}>
                    {suite.name} ({suite.fixtureCount} tasks)
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              className={ghostBtnCls}
              onClick={() => setDefining((value) => !value)}
            >
              Create task suite
            </button>
            {defining && (
              <div className="space-y-2">
                <p className="text-xs text-ink-3">
                  Describe a sample task and the result you expect. Expected
                  results stay private and are used to check the finished task.
                  Saving does not contact models.
                </p>
                <label className="block">
                  Suite name
                  <input
                    className="field mt-1"
                    maxLength={128}
                    value={task.name}
                    onChange={(event) => changeTask('name', event.target.value)}
                  />
                </label>
                <label className="block">
                  Task instructions
                  <textarea
                    className="field mt-1"
                    rows={3}
                    maxLength={16_384}
                    value={task.instructions}
                    onChange={(event) =>
                      changeTask('instructions', event.target.value)
                    }
                  />
                </label>
                <details>
                  <summary className="cursor-pointer">
                    Sample input (optional)
                  </summary>
                  <div className="mt-2 space-y-2">
                    <label className="block">
                      Input file name
                      <input
                        className="field mt-1"
                        placeholder="sample.txt"
                        maxLength={128}
                        value={task.inputFile}
                        onChange={(event) =>
                          changeTask('inputFile', event.target.value)
                        }
                      />
                    </label>
                    <label className="block">
                      Input file contents
                      <textarea
                        className="field mt-1"
                        rows={3}
                        maxLength={SUITE_JSON_LIMIT}
                        value={task.inputContents}
                        onChange={(event) =>
                          changeTask('inputContents', event.target.value)
                        }
                      />
                    </label>
                  </div>
                </details>
                <label className="block">
                  Expected result file
                  <input
                    className="field mt-1"
                    maxLength={128}
                    value={task.resultFile}
                    onChange={(event) =>
                      changeTask('resultFile', event.target.value)
                    }
                  />
                </label>
                <label className="block">
                  Expected result contents
                  <textarea
                    className="field mt-1"
                    rows={3}
                    maxLength={SUITE_JSON_LIMIT}
                    value={task.resultContents}
                    onChange={(event) =>
                      changeTask('resultContents', event.target.value)
                    }
                  />
                </label>
                <label className="block">
                  Expected reply
                  <textarea
                    className="field mt-1"
                    rows={2}
                    maxLength={16_384}
                    value={task.expectedReply}
                    onChange={(event) =>
                      changeTask('expectedReply', event.target.value)
                    }
                  />
                </label>
                <button
                  type="button"
                  className={ghostBtnCls}
                  onClick={() => void action(defineSimple)}
                >
                  Save task suite
                </button>
                <details>
                  <summary className="cursor-pointer">
                    Advanced: multiple tasks and mock effects
                  </summary>
                  <div className="mt-2 space-y-2">
                    <p className="text-xs text-ink-3">
                      Use flat file names and synthetic data. Advanced JSON
                      edits are saved separately from the simple task above.
                    </p>
                    <label className="block">
                      Synthetic suite JSON
                      <textarea
                        className="field mt-1 font-mono text-xs"
                        rows={12}
                        maxLength={SUITE_JSON_LIMIT}
                        value={suiteJson}
                        onChange={(event) =>
                          setAdvancedJson(event.target.value)
                        }
                      />
                    </label>
                    <button
                      type="button"
                      className={ghostBtnCls}
                      onClick={() => void action(defineAdvanced)}
                    >
                      Save advanced suite
                    </button>
                  </div>
                </details>
              </div>
            )}
            <label className="block">
              Candidate model
              <input
                className="field mt-1"
                value={model}
                maxLength={256}
                onChange={(event) => setModel(event.target.value)}
              />
            </label>
            <label className="block">
              Candidate provider
              <input
                className="field mt-1"
                value={provider}
                maxLength={256}
                onChange={(event) => setProvider(event.target.value)}
              />
            </label>
            <label className="block">
              Candidate instructions
              <textarea
                className="field mt-1"
                rows={4}
                value={system}
                maxLength={32_768}
                onChange={(event) => setSystem(event.target.value)}
              />
            </label>
            <label className="block">
              Candidate task tools
              <select
                className="field mt-1"
                value={toolPolicy}
                onChange={(event) =>
                  setToolPolicy(event.target.value as TaskRegressionToolPolicy)
                }
              >
                <option value="files">Files only</option>
                <option value="filesAndMockEffects">
                  Files and simulated actions
                </option>
              </select>
            </label>
            <label className="block">
              Repetitions per task
              <input
                className="field mt-1"
                type="number"
                min={1}
                max={3}
                value={repetitions}
                onChange={(event) => setRepetitions(Number(event.target.value))}
              />
            </label>
            {selectedSuite && (
              <p className="text-xs text-ink-3">
                {selectedSuite.fixtureCount * repetitions * 2} task runs across
                both selections. Each task may contact its model more than once;
                total usage may be incomplete.
              </p>
            )}
            <p className="text-xs text-ink-3">
              Task instructions and sample inputs are sent to the selected
              providers and may use paid credits.
            </p>
            <button
              type="button"
              className={primaryBtnCls}
              disabled={hasUnsavedChanges || !selectedSuite || !model.trim()}
              onClick={() => void action(compare)}
            >
              Run comparison (uses selected models)
            </button>
          </fieldset>
        </>
      )}
      <div className="flex flex-wrap items-end gap-2">
        <label className="min-w-0 flex-1">
          Report ID
          <input
            className="field mt-1"
            value={reportId}
            disabled={disabled || busy}
            onChange={(event) => setReportId(event.target.value)}
          />
        </label>
        <button
          type="button"
          className={ghostBtnCls}
          disabled={disabled || busy || !reportId.trim()}
          onClick={() =>
            void action(async () =>
              receiveReport(
                await daemon.taskRegressions.report(reportId.trim()),
              ),
            )
          }
        >
          Open report
        </button>
      </div>
      {report && (
        <TaskRegressionCard
          report={report}
          busy={disabled || busy}
          onRefresh={() =>
            void action(async () =>
              receiveReport(await daemon.taskRegressions.report(report.id)),
            )
          }
          onCancel={() =>
            void action(async () =>
              receiveReport(await daemon.taskRegressions.cancel(report.id)),
            )
          }
        />
      )}
    </div>
  );
}

function errorMessage(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}
