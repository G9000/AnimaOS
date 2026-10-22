import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { WorkflowTemplate } from '@animaos/sdk';
import { WorkflowTemplates } from './WorkflowTemplates';

const api = vi.hoisted(() => ({
  listWorkflowTemplates: vi.fn(),
  saveWorkflowTemplate: vi.fn(),
  runWorkflowTemplate: vi.fn(),
  deleteWorkflowTemplate: vi.fn(),
}));
vi.mock('../lib/daemon-api', () => ({ daemon: api }));
const template: WorkflowTemplate = {
  agentId: 'alpha',
  slug: 'brief',
  title: 'Brief ${topic}',
  revision: 2,
  parameters: [{ name: 'topic', default: null }],
  steps: [
    {
      id: 'check',
      title: 'Review',
      kind: 'approval',
      prompt: 'Review ${topic}',
    },
  ],
  saveHistory: [],
  createdAtMs: 1,
  updatedAtMs: 2,
  deleted: false,
};
beforeEach(() => {
  sessionStorage.clear();
  api.listWorkflowTemplates.mockResolvedValue([template]);
});
afterEach(() => vi.clearAllMocks());

it('runs a pinned template with fresh input keys and renders input as safe text', async () => {
  const onRun = vi.fn();
  api.runWorkflowTemplate.mockResolvedValue({ id: 'new-run' });
  const view = render(
    <WorkflowTemplates agentId="alpha" online onRun={onRun} />,
  );
  fireEvent.change(await screen.findByLabelText('Template to run'), {
    target: { value: 'brief' },
  });
  expect(screen.getByRole('button', { name: 'Run template' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('Input topic'), {
    target: { value: '<script>alert(1)</script>' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Run template' }));
  await waitFor(() => expect(onRun).toHaveBeenCalled());
  expect(api.runWorkflowTemplate.mock.calls[0]).toEqual([
    'alpha',
    'brief',
    {
      revision: 2,
      requestKey: expect.any(String),
      inputs: { topic: '<script>alert(1)</script>' },
    },
  ]);
  fireEvent.click(screen.getByRole('button', { name: 'Run template' }));
  await waitFor(() => expect(api.runWorkflowTemplate).toHaveBeenCalledTimes(2));
  expect(api.runWorkflowTemplate.mock.calls[0][2].requestKey).not.toBe(
    api.runWorkflowTemplate.mock.calls[1][2].requestKey,
  );
  expect(view.container.querySelector('script')).toBeNull();
});

it('persists the exact uncertain run request across remount and template edits', async () => {
  api.runWorkflowTemplate
    .mockRejectedValueOnce(new Error('Response lost'))
    .mockResolvedValue({ id: 'saved' });
  const view = render(<WorkflowTemplates agentId="alpha" online />);
  fireEvent.change(await screen.findByLabelText('Template to run'), {
    target: { value: 'brief' },
  });
  fireEvent.change(screen.getByLabelText('Input topic'), {
    target: { value: 'Alpha' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Run template' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Response lost');
  view.unmount();
  api.listWorkflowTemplates.mockResolvedValue([{ ...template, revision: 3 }]);
  render(<WorkflowTemplates agentId="alpha" online />);
  fireEvent.click(
    await screen.findByRole('button', { name: 'Retry exact template request' }),
  );
  await waitFor(() => expect(api.runWorkflowTemplate).toHaveBeenCalledTimes(2));
  expect(api.runWorkflowTemplate.mock.calls[1]).toEqual(
    api.runWorkflowTemplate.mock.calls[0],
  );
});

it('saves a named parameterized template and leaves fields disabled offline', async () => {
  api.saveWorkflowTemplate.mockResolvedValue(template);
  const view = render(<WorkflowTemplates agentId="alpha" online />);
  await screen.findByLabelText('Template to run');
  fireEvent.click(screen.getByRole('button', { name: 'New template' }));
  fireEvent.change(screen.getByLabelText('Template slug'), {
    target: { value: 'weekly' },
  });
  fireEvent.change(screen.getByLabelText('Template title'), {
    target: { value: 'Weekly ${topic}' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
  fireEvent.change(screen.getByLabelText('Parameter 1 name'), {
    target: { value: 'topic' },
  });
  fireEvent.change(screen.getByLabelText('Template step 1 title'), {
    target: { value: 'Review' },
  });
  fireEvent.change(screen.getByLabelText('Template step 1 prompt'), {
    target: { value: 'Review ${topic}' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save template' }));
  await waitFor(() =>
    expect(api.saveWorkflowTemplate).toHaveBeenCalledWith(
      'alpha',
      expect.objectContaining({
        slug: 'weekly',
        revision: 0,
        title: 'Weekly ${topic}',
        parameters: [{ name: 'topic', default: null }],
      }),
    ),
  );
  view.rerender(<WorkflowTemplates agentId="alpha" online={false} />);
  expect(screen.getByRole('button', { name: 'Run template' })).toBeDisabled();
});

it('notifies the originating agent of a successful run after the source view is replaced', async () => {
  let finish!: (value: unknown) => void;
  api.runWorkflowTemplate.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const onAlphaRun = vi.fn();
  const onBetaRun = vi.fn();
  const view = render(
    <WorkflowTemplates key="alpha" agentId="alpha" online onRun={onAlphaRun} />,
  );
  fireEvent.change(await screen.findByLabelText('Template to run'), {
    target: { value: 'brief' },
  });
  fireEvent.change(screen.getByLabelText('Input topic'), {
    target: { value: 'Alpha' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Run template' }));
  expect(api.runWorkflowTemplate).toHaveBeenCalledWith(
    'alpha',
    'brief',
    expect.anything(),
  );
  view.rerender(
    <WorkflowTemplates key="beta" agentId="beta" online onRun={onBetaRun} />,
  );
  await act(async () => finish({ id: 'alpha-run' }));
  expect(onAlphaRun).toHaveBeenCalledTimes(1);
  expect(onBetaRun).not.toHaveBeenCalled();
  expect(sessionStorage.getItem('anima:template-command:alpha')).toBeNull();
});
