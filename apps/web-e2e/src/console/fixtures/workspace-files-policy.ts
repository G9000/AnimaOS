/** Fixture-only admission for the one M69 report-download workflow. */
export type WorkspaceFilesStep = 'list' | 'preview' | 'download';

export interface WorkspaceFilesRequest {
  method: string;
  url: string;
}

export type WorkspaceFilesAdmission =
  | { allowed: true; kind: WorkspaceFilesStep; path: string }
  | { allowed: false; reason: string };

export interface WorkspaceFilesPolicySnapshot {
  admitted: WorkspaceFilesStep[];
  refused: number;
}

export interface WorkspaceFilesPolicy {
  admit(request: WorkspaceFilesRequest): WorkspaceFilesAdmission;
  snapshot(): WorkspaceFilesPolicySnapshot;
}

export function createWorkspaceFilesPolicy(): WorkspaceFilesPolicy {
  const steps: { kind: WorkspaceFilesStep; path: string }[] = [
    { kind: 'list', path: '/api/workspace/files' },
    { kind: 'preview', path: '/api/workspace/file?path=reports%2Fledger.csv' },
    {
      kind: 'download',
      path: '/api/workspace/file/download?path=reports%2Fledger.csv',
    },
  ];
  const admitted: WorkspaceFilesStep[] = [];
  let refused = 0;
  return {
    admit(request) {
      const next = steps[admitted.length];
      // Compare the original URL, before URL parsing can normalize aliases,
      // dot segments, escaping, fragments or an empty query delimiter.
      if (
        next &&
        request.method === 'GET' &&
        request.url === `http://127.0.0.1:18753${next.path}`
      ) {
        admitted.push(next.kind);
        return { allowed: true, kind: next.kind, path: next.path };
      }
      refused += 1;
      return {
        allowed: false,
        reason: 'Refused workspace request outside the fixed report sequence',
      };
    },
    snapshot() {
      return { admitted: [...admitted], refused };
    },
  };
}
