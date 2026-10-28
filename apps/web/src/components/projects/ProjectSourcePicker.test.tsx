import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Project } from '@animaos/sdk';
import { ProjectSourcePicker } from './ProjectSourcePicker';
const api = vi.hoisted(() => ({ listWorkspaceFiles: vi.fn(), getSession: vi.fn(), sessionMessages: vi.fn(),
  listWorkflows: vi.fn(), listWorkflowTemplates: vi.fn(), chatResults: { list: vi.fn() } }));
vi.mock('../../lib/daemon-api', () => ({ daemon: api }));
const project = { id: 'project_shared', status: 'active' } as Project;
describe('exact project source selection', () => {
  beforeEach(() => {
    vi.resetAllMocks(); api.listWorkspaceFiles.mockResolvedValue({ files: [], truncated: false });
    api.getSession.mockResolvedValue({ clearGeneration: 7 });
    api.sessionMessages.mockResolvedValue({ messages: Array.from({ length: 5 }, (_, index) => ({ id: `message-${index}`,
      role: index % 2 ? 'assistant' : 'user', text: `Evidence ${index}`, hidden: false })) });
    api.listWorkflows.mockResolvedValue([]);
    api.listWorkflowTemplates.mockResolvedValue([{ slug: 'weekly', title: 'Weekly research' }]);
  });
  it('pins only explicitly checked donor message IDs with the canonical clear generation', async () => {
    const add = vi.fn().mockResolvedValue(undefined);
    render(<ProjectSourcePicker agentId="b" project={project} chats={[{ ownerAgentId: 'a', sessionId: 'chat:a', title: 'Donor chat',
      archived: false, activeRuns: 0, lastActivityAtMs: 1, projectBindingRevision: 1 }]} workflowsOnly={false} onAdd={add} onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Source type'), { target: { value: 'excerpt' } });
    fireEvent.change(screen.getByLabelText('Project chat'), { target: { value: 'a\0chat:a' } });
    await screen.findByText('Evidence 0', { exact: false });
    const checks = screen.getAllByRole('checkbox');
    fireEvent.click(checks[0]); fireEvent.click(checks[2]);
    fireEvent.click(screen.getByRole('button', { name: 'Link selected source' }));
    await waitFor(() => expect(add).toHaveBeenCalledWith({ kind: 'chat_excerpt', ownerAgentId: 'a', sessionId: 'chat:a',
      clearGeneration: 7, messageIds: ['message-0', 'message-2'] }, '', false, expect.any(String)));
    expect(api.sessionMessages).toHaveBeenCalledWith('a', 'chat:a', expect.objectContaining({ limit: 40 }));
  });
  it('keeps workflow templates as links without project execution or context inheritance', async () => {
    const add = vi.fn().mockResolvedValue(undefined);
    render(<ProjectSourcePicker agentId="b" project={project} chats={[]} workflowsOnly onAdd={add} onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Link type'), { target: { value: 'workflow_template' } });
    await screen.findByRole('option', { name: 'Weekly research' });
    fireEvent.change(screen.getByLabelText('Template owned by the selected agent'), { target: { value: 'weekly' } });
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Link selected source' })));
    expect(add).toHaveBeenCalledWith({ kind: 'workflow_template', ownerAgentId: 'b', slug: 'weekly' }, '', false, expect.any(String));
  });
});
