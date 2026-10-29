/** Selected connected content shares derived text with project members, never account credentials. */
export interface ProjectConnectedSourceGrant {
  ownerAgentId: string; connectorId: string; provider: 'gdrive' | 'onedrive' | 'sharepoint' | 'gmail' | 'outlook' | 'email';
  remoteId: string; citationId: string; connectionGeneration: string; searchGeneration: string;
  grantEpoch: string; selectionRevision: number; streamEpoch: string | null;
  sourceKind: 'file' | 'email_thread';
}
export interface ProjectConnectedSourceChoice {
  ownerAgentId: string; connectorId: string; provider: string; citationId: string; remoteId: string;
  ownerName?: string;
  title: string; accountLabel: string | null; sourceKind: 'file' | 'email_thread'; coverage: string;
}
export interface ProjectConnectedSourceChoices { choices: ProjectConnectedSourceChoice[]; omittedCount: number }
export interface ProjectAddConnectedSourceInput {
  expectedRevision: number; requestKey: string; asAgentId: string; ownerAgentId: string;
  connectorId: string; citationId: string; shareWithProject: true; label?: string;
}
