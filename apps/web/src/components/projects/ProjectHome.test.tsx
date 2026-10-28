import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Project } from '@animaos/sdk';
import { ProjectHome } from './ProjectHome';

const fixtures = vi.hoisted(() => ({ view: {} as Record<string, unknown>, createSession: vi.fn() }));
vi.mock('../../hooks/useProjects', () => ({ useProjects: () => fixtures.view }));
vi.mock('../../lib/daemon-api', () => ({ daemon: { createSession: fixtures.createSession } }));
vi.mock('./ProjectSourcePicker', () => ({ ProjectSourcePicker: () => <div>Source picker</div> }));
const project: Project = { id: 'project_shared', title: 'House research', status: 'active', revision: 4, accessEpoch: 1,
  members: [{ agentId: 'a', generation: 1, joinedAtMs: 1 }, { agentId: 'b', generation: 1, joinedAtMs: 1 }],
  instructions: 'Current owner brief', resources: [], createdAtMs: 1, updatedAtMs: 2, chatCount: 1, resourceCount: 0 };
const agents = [{ id: 'a', name: 'Alice' }, { id: 'b', name: 'Bob' }];
function setup(agentId = 'a') {
  const navigate = vi.fn(); const select = vi.fn();
  return { navigate, select, ...render(<ProjectHome agentId={agentId} agents={agents} projectId={project.id} onSelectAgent={select} navigate={navigate} />) };
}
describe('shared project home', () => {
  beforeEach(() => {
    fixtures.createSession.mockReset();
    fixtures.view = { project, projects: [project], chats: [{ ownerAgentId: 'b', sessionId: 'chat:b', title: 'Bob’s research', archived: false, activeRuns: 0 }],
      loading: false, error: null, live: true, preview: null, coverage: null, refresh: vi.fn(), readCoverage: vi.fn(), readPreview: vi.fn(),
      api: { update: vi.fn().mockRejectedValue(new Error('Project changed. Refresh and retry.')) },
      mutate: vi.fn((action: (api: unknown) => Promise<unknown>) => action((fixtures.view as { api: unknown }).api)) };
  });
  it('opens a member chat with its original owner and keeps the shared project identity on agent selection', () => {
    const { select, navigate } = setup();
    fireEvent.change(screen.getByLabelText('Working as'), { target: { value: 'b' } });
    expect(select).toHaveBeenLastCalledWith('b'); expect(navigate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Bob’s research' }));
    expect(navigate).toHaveBeenCalledWith({ kind: 'session', agentId: 'b', sessionId: 'chat:b' });
  });
  it('keeps an edited brief after a conflict and requires review against the latest revision', async () => {
    const view = setup();
    fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    fireEvent.change(screen.getByLabelText('Project instructions'), { target: { value: 'My unsaved changes' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save brief' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Project changed'));
    fixtures.view.project = { ...project, revision: 5, instructions: 'Another owner update' };
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.getByLabelText('Project instructions')).toHaveValue('My unsaved changes');
    expect(screen.getByRole('button', { name: 'Save brief' })).toBeDisabled();
  });
  it('hides cached source payloads when project access is invalidated', () => {
    fixtures.view.preview = { text: 'Selected donor payload' };
    const view = setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    expect(screen.getByText('Selected donor payload')).toBeVisible();
    fixtures.view.project = null; fixtures.view.live = false;
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.queryByText('Selected donor payload')).not.toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Reconnecting');
  });
  it('does not navigate to a late new chat after switching member agents', async () => {
    let resolve!: (value: { id: string }) => void;
    fixtures.createSession.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const view = setup(); fireEvent.click(screen.getByRole('button', { name: 'New chat with Alice' }));
    view.rerender(<ProjectHome agentId="b" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    await act(async () => resolve({ id: 'chat:late-a' }));
    expect(view.navigate).not.toHaveBeenCalled();
  });
});
