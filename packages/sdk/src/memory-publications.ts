/** Durable selected sharing; a submitted proposal is not canonical saved memory. */
export type MemoryPublicationState =
  | 'queued' | 'sending' | 'unknown' | 'submitted_for_review'
  | 'saved' | 'conflict' | 'blocked' | 'dismissed' | 'cancelled';

export interface MemoryPublication {
  id: string;
  operationId: string;
  revision: number;
  state: MemoryPublicationState;
  title: string;
  localId: string;
  attempts: number;
  createdAtMs: number;
  updatedAtMs: number;
  nextAttemptAtMs: number;
  reason: string | null;
  proposalId: string | null;
  canonical: { id: string; revision: number; ownerEdited: boolean } | null;
}

export interface MemoryPublicationsView {
  available: boolean;
  reason: string | null;
  items: MemoryPublication[];
  counts: Partial<Record<MemoryPublicationState, number>>;
  omitted: number;
}

export interface ShareMemory {
  /** Generate once; retain this UUID when checking or retrying uncertain admission. */
  publicationId: string;
  localId: string;
  expectedContent: string;
  title: string;
  source: string;
  basis: 'owner_statement' | 'source' | 'model_inference';
  /** Defaults to proposal. Direct writes still require current explicit permission. */
  delivery?: 'proposal' | 'direct';
  /** A deliberately selected correction; omission creates a new garden record. */
  target?: { id: string; expectedRevision: number };
}

export interface ControlMemoryPublication {
  expectedRevision: number;
  action: 'resume' | 'cancel';
}
