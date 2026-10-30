/** A snapshot of the responding daemon. Configuration is not a provider health probe or a grant. */
export interface RuntimeCapabilityObservation {
  instanceId: string;
  observedAtMs: number;
  daemonVersion: string;
  buildRevision?: string | null;
  images: {
    ledgerVersion: 1;
    observationVersion: 1;
    configuration: {
      state: 'configured' | 'unconfigured' | 'unknown';
      reason: string;
    };
  };
  pdfPages: { version: 1 };
  limits: string;
}
/** Registered handlers are not a connection check or an agent permission grant. */
export interface DaemonCapabilityTool {
  name: string;
  description: string;
  category:
    | 'workspace'
    | 'terminal'
    | 'memory'
    | 'team'
    | 'research'
    | 'productivity'
    | 'browser'
    | 'documents'
    | 'utility';
  requirements: string[];
}

export interface DaemonCapabilities {
  schemaVersion: 1;
  /** Missing on older daemons. Do not infer API support from frontend code or health. */
  runtime?: RuntimeCapabilityObservation;
  /** Owner-selected shared context; membership never transfers agent permissions. */
  projects?: { version: 1; sharedAgents: true; selectedContext: true };
  /** Configuration inventory only; execution still validates interpreter/process custody. */
  programmaticTools?: {
    status: 'configured' | 'unconfigured' | 'unsupported';
    mode: 'one_shot';
    interpreterEnvironment: string;
  };
  tools: DaemonCapabilityTool[];
  persistence: {
    /** Configured storage backend, not a successful storage health probe. */
    controlPlane: string;
    memory: string;
    /** A step journal is configured; this does not promise automatic run resumption. */
    executionJournal: boolean;
  };
  extensions: Array<{
    id: string;
    label: string;
    status: 'planned' | 'partial';
    description: string;
  }>;
  limitations: string[];
}
