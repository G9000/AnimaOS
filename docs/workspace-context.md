# A companion backed by an independent workspace

An agent can use an MCP workspace as its personality and primary durable memory. This is optional and configured separately for each agent. M.O.T.H.E.R provides the workspace, persona and access policy; AnimaOS remains the host running the conversation.

## Connect with M.O.T.H.E.R

Personal onboarding and **Add companion** offer **Memory and context → M.O.T.H.E.R**. This is optional; the default keeps memory in AnimaOS. Your chosen local personality remains the offline fallback.

In Settings, **New agent** starts directly with the model choice and uses a panel layout. Under **Access → Home folder → Change folder**, browse or enter the new folder, choose **Check folder**, then review **Use this folder**. This changes the shared project root for all agents on that server. Agents, chats and M.O.T.H.E.R memory are preserved; files are not moved and another folder's saved agents are not imported. Until you save or cancel, setup stays on this step. If a save is unconfirmed, **Check saved folder** reads its state without repeating the save.

After creating the companion, onboarding shows its name and ID and guides you through `mother connect animaos`. Select that new companion and the **Companion** role in M.O.T.H.E.R, review its separate key, and confirm. Return to onboarding and choose **Check connection**. **Saved connection ready** means the owner metadata contains this agent's companion binding and its exact reviewed reader is ready; the runtime still checks live access before each reply. Shared-context mode alone does not make M.O.T.H.E.R the primary memory or personality.

**Finish pairing later** opens the companion without claiming that M.O.T.H.E.R was activated. Until it is paired, the companion uses local memory and personality. If a companion binding already exists but its source is offline, the screen instead explains that the saved personality is available while M.O.T.H.E.R memory remains offline. Pairing checks never create the companion again, send a model prompt, or alter another agent's connection. Existing agents and chats remain intact when adding a companion or choosing a separate project folder.

Use versions of both applications supporting workspace context bindings. In M.O.T.H.E.R, open **Connect AnimaOS**, enter the local owner's credentials in the masked fields, and choose an agent.

For a dedicated M.O.T.H.E.R agent, create an agent with that name in AnimaOS first, then select it here. Your other agents keep their existing personalities and access.

Choose its role:

- **Companion** to use your stored M.O.T.H.E.R persona and its durable memory. The grant remains restricted to its selected read/proposal scopes; this role does not itself permit direct memory writes.
- **Shared context** to keep the agent's own personality and memory while sharing selected existing records. Choose records from the displayed list or enter their IDs. An empty selection shares nothing. The key cannot read the M.O.T.H.E.R persona or other records.
- **Legacy context tools** to preserve the previous MCP-only integration without adopting identity. Existing legacy read keys continue to have their previous workspace-wide access; they do not become scoped or companion keys automatically.

Review the agent, role and records before confirming. Setup creates a separate private key for that agent and role, then pins the workspace identity and restore generation in AnimaOS. It does not alter another agent's key. If an existing connection conflicts, disable it explicitly before replacing it; setup preserves it rather than silently widening access.

In AnimaOS, the agent's **Profile → M.O.T.H.E.R** panel shows its configured mode and connection state. **Use this agent independently** removes the identity/memory binding; its ordinary MCP connection and key remain separately managed under Tools and in M.O.T.H.E.R. Remove or revoke those too if the agent should have no access at all.

## Offline and recovery

Before a reply, AnimaOS checks the current connection, workspace, generation, granted role and agent subject. Companion identity is loaded transiently; your configured local personality is not overwritten.

If the source is offline, revoked, incompatible or mismatched, the conversation continues in the **same chat** with the saved local personality and a memory-unavailable notice. Previously loaded messages remain in chat history. The failed connection's tools cannot be used to bypass the unavailable binding. On a later turn, a successful read can restore companion mode without replaying any memory writes.

Companion mode keeps durable memory in M.O.T.H.E.R, including during fallback: AnimaOS does not silently capture a second local copy, automatically persist task-result memories, or mirror turns to another memory provider. Ordinary conversation history is still saved by AnimaOS. Memory proposals require the usual M.O.T.H.E.R owner review; offline or uncertain writes must never be reported as saved.

Closing M.O.T.H.E.R's dashboard does not necessarily stop its MCP service. Availability depends on the actual configured connection. Access changes cannot erase context already received in an existing conversation.

## Adapter contract

The host integration is generic MCP. It pins `settings.additional.workspaceContext` with a connection ID, remote reader name, mode, workspace ID, generation and subject. Owner-only `GET` and `PUT /api/agents/{agentId}/workspace-context` expose credential-free metadata and revision-checked updates. Saving a binding does not grant MCP execution rights or prove the next conversation has read it.

Readers return the existing context envelope plus `access: { version: 1, mode, subject }`. Companion mode requires the owner persona with its verified SHA-256 digest; shared mode omits persona. The host uses current per-agent MCP authority and retains its own permissions, instruction hierarchy, cancellation and execution limits. Workspace records remain attributed reference data.

Bootstrap requests one preview when the reader supports a limit, then uses the workspace tools for relevant searches and full records. The response must fit the host's existing 16 KiB MCP envelope budget. An oversized persona or response causes explicit fallback; personality text is never silently truncated. The separate 16,000-character persona ceiling does not guarantee that a document fits the byte budget, especially with multilingual text.

M.O.T.H.E.R enforces sharing independently across reads, counts, retrieval and search. Sharing an existing record does not automatically share its references, newly approved records, or future records. Credentials and their sharing policies are not portable memory and are excluded from exports/restores. A restored generation requires an explicit new binding.
