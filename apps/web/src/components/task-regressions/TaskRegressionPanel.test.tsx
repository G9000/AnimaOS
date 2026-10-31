import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TaskRegressionReport, TraceMetrics } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { TaskRegressionPanel } from './TaskRegressionPanel';
import { TaskRegressionCard } from './TaskRegressionCard';

const api = vi.hoisted(() => ({
  suites: vi.fn(),
  define: vi.fn(),
  configuration: vi.fn(),
  compare: vi.fn(),
  report: vi.fn(),
  cancel: vi.fn(),
}));
vi.mock('../../lib/daemon-api', () => ({ daemon: { taskRegressions: api } }));
const suite = {
  id: 'suite-one',
  digest: 'suite-v1',
  name: 'File task',
  fixtureCount: 1,
  createdAtMs: 1,
};
const config = {
  agentId: 'agent-one',
  configDigest: 'config-v1',
  model: 'baseline-model',
  provider: 'test-provider',
  system: 'Baseline instructions',
};
const metrics: TraceMetrics = {
  modelCalls: 1,
  toolCalls: 1,
  toolErrors: 0,
  runErrors: 0,
  exactRepeatCalls: 0,
  recoveredToolResults: 0,
  resultBytes: 4,
  finalOutputBytes: 5,
  knownUsageCalls: 0,
  unknownUsageCalls: 1,
  knownUsage: null,
  elapsedMs: 10,
};
function report(
  patch: Partial<TaskRegressionReport> = {},
): TaskRegressionReport {
  const pin = {
    agentId: config.agentId,
    configDigest: config.configDigest,
    effectiveConfigDigest: 'effective-v1',
    provider: config.provider,
    model: config.model,
    toolPolicy: 'files' as const,
  };
  return {
    id: 'report-one',
    status: 'running',
    suite,
    baseline: pin,
    candidate: {
      ...pin,
      model: 'candidate-model',
      effectiveConfigDigest: 'candidate-v1',
    },
    repetitions: 1,
    plannedTrials: 2,
    completedTrials: 0,
    createdAtMs: 2,
    finishedAtMs: null,
    retiredAtMs: null,
    evidenceScope: 'selectedAdapterTasks',
    evaluatorDigest: 'evaluator-v1',
    toolImplementationDigest: 'tools-v1',
    hostSourceDigest: 'host-v1',
    planDigest: 'plan-v1',
    comparison: null,
    observations: [],
    usageComplete: false,
    limitations: ['Deterministic file checks only.'],
    error: null,
    ...patch,
  };
}
beforeEach(() => {
  vi.resetAllMocks();
  api.suites.mockResolvedValue([suite]);
  api.configuration.mockResolvedValue(config);
  api.define.mockResolvedValue({
    ...suite,
    id: 'suite-new',
    name: 'Synthetic file task',
  });
  api.compare.mockResolvedValue(report());
  api.report.mockResolvedValue(
    report({ status: 'completed', completedTrials: 2 }),
  );
  api.cancel.mockResolvedValue(report({ status: 'cancelled' }));
});
afterEach(() => vi.restoreAllMocks());

describe('owner task comparison', () => {
  it.each([null, 'Baseline instructions'])(
    'omits unchanged candidate instructions for captured source %s',
    async (sourceSystem) => {
      api.configuration.mockResolvedValue({ ...config, system: sourceSystem });
      render(<TaskRegressionPanel agentId="agent-one" />);
      fireEvent.click(
        screen.getByRole('button', { name: 'Compare on saved tasks' }),
      );
      expect(
        await screen.findByLabelText('Candidate instructions'),
      ).toHaveValue(sourceSystem ?? '');
      const baselineInstructions = screen.getByText(
        'Captured baseline instructions',
      );
      fireEvent.click(baselineInstructions);
      expect(
        within(baselineInstructions.parentElement!).getByText(
          sourceSystem ?? "Uses the task runner's default instructions.",
        ),
      ).toBeVisible();
      fireEvent.click(
        screen.getByRole('button', {
          name: 'Run comparison (uses selected models)',
        }),
      );
      await waitFor(() => expect(api.compare).toHaveBeenCalledTimes(1));
      expect(api.compare.mock.calls[0][0].candidate).not.toHaveProperty(
        'system',
      );
    },
  );

  it('preserves an intentional empty override when existing source instructions are cleared', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    expect(await screen.findByLabelText('Candidate instructions')).toHaveValue(
      'Baseline instructions',
    );
    fireEvent.change(screen.getByLabelText('Candidate instructions'), {
      target: { value: '' },
    });
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    await waitFor(() => expect(api.compare).toHaveBeenCalledTimes(1));
    expect(api.compare.mock.calls[0][0].candidate).toHaveProperty('system', '');
  });

  it('loads only after expanding, captures configuration, and starts only on the explicit selected-model action', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    expect(api.suites).not.toHaveBeenCalled();
    expect(api.configuration).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    expect(await screen.findByLabelText('Candidate model')).toHaveValue(
      'baseline-model',
    );
    expect(api.configuration).toHaveBeenCalledWith('agent-one');
    expect(api.compare).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Candidate model'), {
      target: { value: 'candidate-model' },
    });
    fireEvent.change(screen.getByLabelText('Candidate provider'), {
      target: { value: 'candidate-provider' },
    });
    fireEvent.change(screen.getByLabelText('Candidate instructions'), {
      target: { value: 'Candidate instructions' },
    });
    fireEvent.change(screen.getByLabelText('Candidate task tools'), {
      target: { value: 'filesAndMockEffects' },
    });
    fireEvent.change(screen.getByLabelText('Repetitions per task'), {
      target: { value: '2' },
    });
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    await waitFor(() => expect(api.compare).toHaveBeenCalledTimes(1));
    expect(api.compare).toHaveBeenCalledWith({
      suiteId: suite.id,
      suiteDigest: suite.digest,
      baseline: { agentId: 'agent-one', configDigest: 'config-v1' },
      candidate: {
        agentId: 'agent-one',
        configDigest: 'config-v1',
        model: 'candidate-model',
        provider: 'candidate-provider',
        system: 'Candidate instructions',
        toolPolicy: 'filesAndMockEffects',
      },
      repetitions: 2,
      requestKey: expect.any(String),
    });
    expect(
      await screen.findByRole('article', { name: 'Task comparison report' }),
    ).toHaveTextContent('report-one');
    expect(api.report).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh report' }));
    await waitFor(() => expect(api.report).toHaveBeenCalledWith('report-one'));
  });

  it('creates one complete file task from the default simple form while keeping expected results private', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(screen.getByRole('button', { name: 'Create task suite' }));
    expect(screen.getByLabelText('Task instructions')).toBeVisible();
    expect(screen.getByLabelText('Synthetic suite JSON')).not.toBeVisible();
    fireEvent.change(screen.getByLabelText('Suite name'), {
      target: { value: 'Rewrite sample' },
    });
    fireEvent.change(screen.getByLabelText('Task instructions'), {
      target: {
        value: 'Rewrite the sample in plain language, then acknowledge.',
      },
    });
    fireEvent.click(screen.getByText('Sample input (optional)'));
    fireEvent.change(screen.getByLabelText('Input file name'), {
      target: { value: 'sample.txt' },
    });
    fireEvent.change(screen.getByLabelText('Input file contents'), {
      target: { value: 'Public sample input' },
    });
    fireEvent.change(screen.getByLabelText('Expected result file'), {
      target: { value: 'summary.txt' },
    });
    fireEvent.change(screen.getByLabelText('Expected result contents'), {
      target: { value: 'PRIVATE_EXPECTED_FILE' },
    });
    fireEvent.change(screen.getByLabelText('Expected reply'), {
      target: { value: 'PRIVATE_EXPECTED_REPLY' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save task suite' }));
    await waitFor(() =>
      expect(api.define).toHaveBeenCalledWith({
        name: 'Rewrite sample',
        fixtures: [
          {
            id: 'write-result',
            actor: {
              prompt: 'Rewrite the sample in plain language, then acknowledge.',
              initialFiles: { 'sample.txt': 'Public sample input' },
              readableFiles: ['sample.txt'],
              writableFiles: ['summary.txt'],
              mockEffects: false,
            },
            oracle: {
              expectedFiles: {
                'sample.txt': 'Public sample input',
                'summary.txt': 'PRIVATE_EXPECTED_FILE',
              },
              expectedOutput: 'PRIVATE_EXPECTED_REPLY',
              expectedMockEffects: [],
            },
          },
        ],
      }),
    );
    expect(
      JSON.stringify(api.define.mock.calls[0][0].fixtures[0].actor),
    ).not.toContain('PRIVATE_EXPECTED');
    expect(screen.getByLabelText('Saved task suite')).toHaveValue('suite-new');
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('requires a name for sample input contents instead of silently dropping them', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(screen.getByRole('button', { name: 'Create task suite' }));
    fireEvent.click(screen.getByText('Sample input (optional)'));
    fireEvent.change(screen.getByLabelText('Input file contents'), {
      target: { value: 'Keep this input' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save task suite' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'input file name',
    );
    expect(api.define).not.toHaveBeenCalled();
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('keeps version hashes folded and shows the provider and paid-credit notice before the explicit run action', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    expect(
      screen.getByText(/Baseline: test-provider \/ baseline-model/),
    ).toBeVisible();
    expect(screen.getByText('config-v1')).not.toBeVisible();
    expect(screen.getByText('suite-v1')).not.toBeVisible();
    const notice = screen.getByText(
      'Task instructions and sample inputs are sent to the selected providers and may use paid credits.',
    );
    expect(notice).toBeVisible();
    const run = screen.getByRole('button', {
      name: 'Run comparison (uses selected models)',
    });
    expect(
      notice.compareDocumentPosition(run) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    fireEvent.click(screen.getByText('Version details'));
    expect(screen.getByText('config-v1')).toBeVisible();
    expect(screen.getByText('suite-v1')).toBeVisible();
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('supports multi-task and mock-effect JSON only through Advanced and reports invalid JSON locally', async () => {
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(screen.getByRole('button', { name: 'Create task suite' }));
    const editor = screen.getByLabelText('Synthetic suite JSON');
    expect(editor).not.toBeVisible();
    fireEvent.click(
      screen.getByText('Advanced: multiple tasks and mock effects'),
    );
    expect(editor).toBeVisible();
    expect(
      JSON.parse((editor as HTMLTextAreaElement).value).fixtures[0].oracle
        .expectedFiles,
    ).toEqual({ 'result.txt': 'done' });
    fireEvent.change(editor, { target: { value: '{bad json' } });
    fireEvent.click(
      screen.getByRole('button', { name: 'Save advanced suite' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('valid JSON');
    expect(api.define).not.toHaveBeenCalled();
    const advancedSuite = {
      name: 'Mixed sample tasks',
      fixtures: [
        {
          id: 'summary',
          actor: {
            prompt: 'Write a short summary.',
            initialFiles: {},
            readableFiles: [],
            writableFiles: ['summary.txt'],
            mockEffects: false,
          },
          oracle: {
            expectedFiles: { 'summary.txt': 'Brief.' },
            expectedOutput: 'Done.',
            expectedMockEffects: [],
          },
        },
        {
          id: 'mock',
          actor: {
            prompt: 'Record a synthetic acknowledgement.',
            initialFiles: {},
            readableFiles: [],
            writableFiles: [],
            mockEffects: true,
          },
          oracle: {
            expectedFiles: {},
            expectedOutput: 'Recorded.',
            expectedMockEffects: [{ key: 'receipt', payload: 'accepted' }],
          },
        },
      ],
    };
    fireEvent.change(editor, {
      target: { value: JSON.stringify(advancedSuite) },
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Save advanced suite' }),
    );
    await waitFor(() => expect(api.define).toHaveBeenCalledWith(advancedSuite));
    expect(screen.getByLabelText('Saved task suite')).toHaveValue('suite-new');
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('reopens a saved report without starting models and can cancel a running comparison', async () => {
    api.report.mockResolvedValue(report());
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.change(screen.getByLabelText('Report ID'), {
      target: { value: 'report-one' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open report' }));
    expect(
      await screen.findByRole('article', { name: 'Task comparison report' }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel comparison' }));
    await waitFor(() => expect(api.cancel).toHaveBeenCalledWith('report-one'));
    expect(await screen.findByText('cancelled')).toBeVisible();
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('discards previous agent responses when switching profiles', async () => {
    let resolveFirst!: (value: typeof config) => void;
    api.configuration.mockImplementation((id) =>
      id === 'agent-one'
        ? new Promise((resolve) => {
            resolveFirst = resolve;
          })
        : Promise.resolve({ ...config, agentId: id, model: 'second-model' }),
    );
    const view = render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await waitFor(() => expect(api.configuration).toHaveBeenCalled());
    view.rerender(<TaskRegressionPanel agentId="agent-two" />);
    expect(api.configuration).toHaveBeenCalledTimes(1);
    await act(async () => resolveFirst(config));
    expect(screen.queryByLabelText('Candidate model')).toBeNull();
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    expect(await screen.findByLabelText('Candidate model')).toHaveValue(
      'second-model',
    );
    expect(api.compare).not.toHaveBeenCalled();
  });

  it('shows admission errors without inventing a pass or retrying dispatch', async () => {
    api.compare.mockRejectedValue(
      new Error('Configuration changed. Capture it again.'),
    );
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Configuration changed',
    );
    expect(api.compare).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('article')).toBeNull();
  });

  it('reuses an uncertain start key after folding, then gives a new explicit run its own key', async () => {
    api.compare
      .mockRejectedValueOnce(new Error('Connection lost'))
      .mockResolvedValue(report({ status: 'completed', completedTrials: 2 }));
    render(<TaskRegressionPanel agentId="agent-one" />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Connection lost',
    );
    const firstKey = api.compare.mock.calls[0][0].requestKey;
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    await screen.findByText('completed');
    expect(api.compare.mock.calls[1][0].requestKey).toBe(firstKey);
    expect(api.configuration).toHaveBeenCalledTimes(1);
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    );
    await waitFor(() => expect(api.compare).toHaveBeenCalledTimes(3));
    expect(api.compare.mock.calls[2][0].requestKey).not.toBe(firstKey);
  });

  it('blocks starting from unsaved profile changes while keeping report cancellation available', async () => {
    api.report.mockResolvedValue(report());
    render(<TaskRegressionPanel agentId="agent-one" hasUnsavedChanges />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Compare on saved tasks' }),
    );
    await screen.findByLabelText('Candidate model');
    expect(
      screen.getByRole('button', {
        name: 'Run comparison (uses selected models)',
      }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Report ID'), {
      target: { value: 'report-one' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open report' }));
    expect(
      await screen.findByRole('button', { name: 'Cancel comparison' }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel comparison' }));
    await waitFor(() => expect(api.cancel).toHaveBeenCalledWith('report-one'));
    expect(api.compare).not.toHaveBeenCalled();
  });
});

it('shows pinned evidence, exact task counts, failures and unknown usage without claiming rendered quality', () => {
  render(
    <TaskRegressionCard
      report={report({
        status: 'completed',
        completedTrials: 2,
        comparison: {
          matrixComplete: true,
          baseline: { trials: 1, correct: 1, metrics },
          candidate: { trials: 1, correct: 0, metrics },
          trials: [
            {
              fixtureId: 'file',
              repetition: 1,
              variant: 'candidate',
              failures: ['result.txt differs'],
            },
          ],
        },
      })}
      onRefresh={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  const card = screen.getByRole('article', { name: 'Task comparison report' });
  expect(card).toHaveTextContent('2 / 2 trials completed');
  expect(card).toHaveTextContent('Baseline: 1 / 1 tasks passed');
  expect(card).toHaveTextContent('Candidate: 0 / 1 tasks passed');
  expect(card).toHaveTextContent('result.txt differs');
  expect(card).toHaveTextContent('Usage incomplete');
  expect(card).toHaveTextContent('suite-v1');
  expect(card).toHaveTextContent('config-v1');
  expect(card).toHaveTextContent('evaluator-v1');
  expect(card).toHaveTextContent('tools-v1');
  expect(card).toHaveTextContent('host-v1');
  expect(
    within(card).queryByRole('button', { name: 'Cancel comparison' }),
  ).toBeNull();
  expect(daemon.taskRegressions.compare).not.toHaveBeenCalled();
});
