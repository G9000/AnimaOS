import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
const calls = vi.hoisted(() => ({ claim: vi.fn(), owner: vi.fn(), viewer: vi.fn() }));
vi.mock('./app', () => ({ App: () => { calls.owner(); return <p>Owner bootstrap</p>; } }));
vi.mock('../lib/console-session', () => ({ claimConsoleSession: calls.claim }));
vi.mock('../components/sessions/SharedConversation', () => ({ SharedConversation: ({ shareId }: {shareId:string}) => { calls.viewer(); return <p>Viewer {shareId}</p>; } }));
import { AppEntry } from './AppEntry';
describe('AppEntry viewer isolation', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  it('mounts only the dedicated viewer surface', () => {
    window.location.hash = '#/shared/share-12345678-1234-1234-1234-123456789abc';
    render(<AppEntry />);
    expect(screen.getByText(/Viewer share-/)).toBeTruthy();
    expect(calls.owner).not.toHaveBeenCalled();
    expect(calls.claim).not.toHaveBeenCalled();
  });
  it('keeps malformed viewer links away from owner bootstrap', () => {
    window.location.hash = '#/shared/not-valid';
    render(<AppEntry />);
    expect(screen.getByText(/link is invalid/)).toBeTruthy();
    expect(calls.owner).not.toHaveBeenCalled();
    expect(calls.claim).not.toHaveBeenCalled();
  });
});
