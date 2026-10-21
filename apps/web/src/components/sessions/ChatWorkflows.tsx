import type { AgentDetail } from '../../lib/types';
import { WorkflowAgentView } from '../WorkspaceWorkflows';

/** Chat surfaces active work and owner checkpoints; the full manager lives under Work. */
export function ChatWorkflows({
  agent,
  online,
  refreshKey,
}: {
  agent: AgentDetail;
  online: boolean;
  refreshKey?: string | number;
}) {
  return (
    <WorkflowAgentView
      key={agent.id}
      agentId={agent.id}
      membership={agent.id}
      online={online}
      surface="chat"
      refreshKey={refreshKey}
    />
  );
}
