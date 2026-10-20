export type MemoryType = 'fact' | 'observation' | 'task_result' | 'reflection';
export type MemoryScope = 'shared' | 'private' | 'room';
export type RelationshipEndpointKind = 'agent' | 'user' | 'system' | 'external';

export interface Memory {
  id: string;
  agentId: string;
  agentName: string;
  type: MemoryType;
  content: string;
  /** 0-1 */
  importance: number;
  createdAt: number;
  tags?: string[];
  scope: MemoryScope;
  roomId?: string;
  worldId?: string;
  sessionId?: string;
}

export interface MemorySearchResult extends Memory {
  score: number;
}

export interface MemorySearchOptions {
  agentId?: string;
  agentName?: string;
  type?: MemoryType;
  scope?: MemoryScope;
  roomId?: string;
  worldId?: string;
  sessionId?: string;
  limit?: number;
  minImportance?: number;
}

export interface AgentRelationship {
  id: string;
  sourceKind: RelationshipEndpointKind;
  sourceAgentId: string;
  sourceAgentName: string;
  targetKind: RelationshipEndpointKind;
  targetAgentId: string;
  targetAgentName: string;
  relationshipType: string;
  summary?: string;
  strength: number;
  confidence: number;
  evidenceMemoryIds: string[];
  tags?: string[];
  roomId?: string;
  worldId?: string;
  sessionId?: string;
  createdAt: number;
  updatedAt: number;
}

export interface NewAgentRelationshipInput {
  sourceKind?: RelationshipEndpointKind;
  sourceAgentId: string;
  sourceAgentName: string;
  targetKind?: RelationshipEndpointKind;
  targetAgentId: string;
  targetAgentName: string;
  relationshipType: string;
  summary?: string;
  strength?: number;
  confidence?: number;
  evidenceMemoryIds?: string[];
  tags?: string[];
  roomId?: string;
  worldId?: string;
  sessionId?: string;
}

export interface AgentRelationshipOptions {
  entityId?: string;
  agentId?: string;
  sourceKind?: RelationshipEndpointKind;
  sourceAgentId?: string;
  targetKind?: RelationshipEndpointKind;
  targetAgentId?: string;
  relationshipType?: string;
  roomId?: string;
  worldId?: string;
  sessionId?: string;
  minStrength?: number;
  minConfidence?: number;
  limit?: number;
}
