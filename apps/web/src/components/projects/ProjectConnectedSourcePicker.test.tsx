import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { ProjectConnectedSourcePicker } from './ProjectConnectedSourcePicker';

const project = { id: 'p', revision: 7, accessEpoch: 3, status: 'active', members: [{ agentId: 'owner' }, { agentId: 'reader' }] } as Project;
const choice = { ownerAgentId: 'owner', connectorId: 'drive', provider: 'gdrive', citationId: 'selected', remoteId: 'opaque', title: 'Annual report', accountLabel: 'owner@example.test', sourceKind: 'file', coverage: 'One selected file' };

describe('selected account sharing', () => {
  it('requires explicit consent and sends only the chosen canonical account citation', async () => {
    const api = { connectedSourceChoices: vi.fn().mockResolvedValue({ choices: [choice], omittedCount: 0 }), addConnectedSource: vi.fn().mockResolvedValue({}) };
    const saved = vi.fn();
    render(<ProjectConnectedSourcePicker project={project} agentId="reader" api={api as unknown as ProjectsClient} onSaved={saved} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole('radio', { name: /Annual report/ }));
    expect(screen.getByText(/owner@example.test/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Share selected source' })).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox', { name: /Share the selected content/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Share selected source' }));
    await waitFor(() => expect(saved).toHaveBeenCalled());
    expect(api.addConnectedSource).toHaveBeenCalledWith('p', { expectedRevision: 7, requestKey: expect.any(String), asAgentId: 'reader', ownerAgentId: 'owner', connectorId: 'drive', citationId: 'selected', shareWithProject: true });
  });

  it('drops pending choices when the project scope changes', async () => {
    let resolve!: (value: unknown) => void;
    const api = { connectedSourceChoices: vi.fn().mockImplementationOnce(() => new Promise((done) => { resolve = done; })).mockResolvedValue({ choices: [], omittedCount: 0 }) };
    const view = render(<ProjectConnectedSourcePicker project={project} agentId="reader" api={api as unknown as ProjectsClient} onSaved={vi.fn()} onClose={vi.fn()} />);
    view.rerender(<ProjectConnectedSourcePicker project={{ ...project, id: 'other' }} agentId="reader" api={api as unknown as ProjectsClient} onSaved={vi.fn()} onClose={vi.fn()} />);
    resolve({ choices: [choice], omittedCount: 0 });
    await screen.findByText(/No eligible selected sources/);
    expect(screen.queryByText('Annual report')).not.toBeInTheDocument();
  });

  it('cannot carry a selected consent into a newer project access binding', async () => {
    const api = { connectedSourceChoices: vi.fn().mockResolvedValueOnce({ choices: [choice], omittedCount: 0 }).mockImplementation(() => new Promise(() => {})), addConnectedSource: vi.fn() };
    const view = render(<ProjectConnectedSourcePicker project={project} agentId="reader" api={api as unknown as ProjectsClient} onSaved={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole('radio', { name: /Annual report/ }));
    fireEvent.click(screen.getByRole('checkbox', { name: /Share the selected content/ }));
    view.rerender(<ProjectConnectedSourcePicker project={{ ...project, revision: 8, accessEpoch: 4 }} agentId="reader" api={api as unknown as ProjectsClient} onSaved={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Share selected source' }));
    expect(api.addConnectedSource).not.toHaveBeenCalled();
    expect(screen.queryByRole('radio')).not.toBeInTheDocument();
  });
});
