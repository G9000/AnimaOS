import type { DriveConnector } from '@animaos/sdk';
export function driveFixture(
  overrides: Partial<DriveConnector> = {},
): DriveConnector {
  return {
    id: 'drive-1',
    agentId: 'agent-a',
    type: 'gdrive',
    status: 'active',
    accountLabel: 'owner@example.test',
    selectedFileIds: ['File_1'],
    requestedScope: 'https://www.googleapis.com/auth/drive.readonly',
    enforcement: 'animaSelectedFiles',
    cleanupPending: false,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides,
  };
}
