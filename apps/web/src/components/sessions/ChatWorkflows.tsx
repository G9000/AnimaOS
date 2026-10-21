import {
  WORKFLOW_TEMPLATE_TOOL_ACCESS,
  WORKFLOW_TOOL_ACCESS,
} from '../../lib/agent-access';
import type { AgentDetail } from '../../lib/types';
import { WorkflowAgentView } from '../WorkspaceWorkflows';

/** Saved workflows belong to the agent, including those started outside this chat. */
export function ChatWorkflows({
  agent,
  online,
  refreshKey,
}: {
  agent: AgentDetail;
  online: boolean;
  refreshKey?: string | number;
}) {
  const allowed = WORKFLOW_TOOL_ACCESS.tools.every((tool) =>
    agent.toolNames.includes(tool),
  );
  const templatesAllowed = WORKFLOW_TEMPLATE_TOOL_ACCESS.tools.every((tool) =>
    agent.toolNames.includes(tool),
  );
  return (
    <section aria-label={`Workflows for ${agent.name}`} className="space-y-4">
      <h2 className="text-base font-semibold">Workflows</h2>
      <p className="text-sm text-ink-3">
        Saved workflows for {agent.name}, including those created outside this
        chat.
        {
          ' Open Reusable templates to save named steps or run them again with new inputs.'
        }
        {allowed
          ? ' Ask in chat to create a workflow with tasks, approval checkpoints, and questions.'
          : ' Enable “Allow workflows in chat” in Permissions to create workflows by asking in chat. You can still manage saved workflows here.'}
      </p>
      <p className="text-sm text-ink-3">
        {templatesAllowed
          ? 'Ask in chat to save a reusable template or run one with named inputs.'
          : 'Enable “Allow workflow templates in chat” in Permissions to save and run templates by asking in chat.'}
      </p>
      <WorkflowAgentView
        key={agent.id}
        agentId={agent.id}
        membership={agent.id}
        online={online}
        surface="chat"
        refreshKey={refreshKey}
      />
    </section>
  );
}
