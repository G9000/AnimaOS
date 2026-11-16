# Native memory with an independent shared garden

AnimaOS can use M.O.T.H.E.R as an additive memory provider for one selected agent. AnimaOS keeps native memory tools, recall, task-result capture and learning. M.O.T.H.E.R holds shared context contributed by permitted harnesses. Choosing where personality comes from is independent of choosing additive memory.

## Additive mode and explicit conversion

In **Profile → M.O.T.H.E.R**, convert an existing Companion or Shared-context binding to the shared garden. Review the selected connection and access role before confirming. Conversion preserves chats, local data and remote grants; it resumes local capture and recall for a legacy Companion. The default personality choice preserves the existing source. A Companion grant can use either local or workspace personality; a Shared grant cannot read workspace personality.

Existing installations keep their saved legacy behavior until this explicit conversion. An ordinary upgrade does not silently reinterpret it. Native-only M.O.T.H.E.R has retired its old pairing application and `mother connect animaos` command; the historical setup section below is not an installation guide for that release.

Additive recall uses only the selected agent's reviewed MCP connection. Before each model request, the host validates workspace identity, restore generation, role and subject, and resolves only owner-pinned and assigned records. Bootstrap does not infer intent or search private facts from user-message words. Complete selection stays within four records and 8 KiB, with a 16 KiB per-read envelope cap. Oversized or flagged records are omitted whole. Returned records retain IDs, revisions, timestamps and attribution and are treated as untrusted reference data.

The conversation model owns query interpretation and reformulation, including mixed languages. `memory_search` defaults to native memory. With `source: "mother"`, it uses the current additive binding and existing read grants. The model supplies one concise query (1–200 UTF-16 units), `mode` (`hybrid`, `semantic` or `literal`), optional v1 `kind`, optional `exact_names` (at most two), and `include_graph` (default true). Each explicit search requests eight previews and fetches at most four complete records under a three-second deadline and 8 KiB output budget. An explicitly partial envelope permits one fresh four-preview retry; authority/transport/malformed failures do not. Pins do not consume the explicit search result budget.

Provider results include `source: "mother"`, distinct locators, complete current record data and coverage. Mother locators are not native IDs for editing, deletion, consolidation or sharing. The model compares evidence with the original question and can refine, translate or change modes through another ordinary tool call. Literal mode matches one contiguous substring. Search metadata preserves actual mode/fallback, index coverage and server omissions; local coverage reports bounded candidate/record exclusions and graph status. Empty or bounded retrieval does not establish absence. Provider errors never silently substitute native results; native recall remains independently available.

No English stop-word, personal-intent, species, capitalization or citation-acronym classifier chooses queries. Native `task_result` and reflection memories remain attributed local history; old failed-recall claims do not establish current garden contents. The model uses inventory/connection state without requesting unrelated personal facts when only checking availability. The same bootstrap behavior applies in every language.

Optional exact-name constraints are model-authored, normalized with Unicode NFKC/lowercase and rechecked on complete current content. Alphanumeric boundaries distinguish names such as `AsterOS` and `AsterOS2`; CJK/Kana/Hangul names can match within continuous text. These are literal entity constraints, not translations or proof of identity. Semantic/paraphrase results need not share query words, and learned/provider order is preserved. Inspect actual model coverage: Unicode handling alone is not multilingual semantics. Native recall uses its existing configured multilingual embeddings; Mother independently reports its own model and any literal fallback.

Mother's MCP adapter limits context/search/history navigation JSON to 12 KiB by removing whole previews and updating omission counts, preserving category balance for unfiltered context. Required persona/pins/assigned context is never shortened. Known read replies must also fit a 48 KiB serialized MCP result budget; a full record is returned whole or produces `READ_RESULT_TOO_LARGE`. These output limits do not alter stored records or tool descriptors. AnimaOS retains its own tighter per-read/recall budgets. Transport size violations explain the size cause while keeping `outcome_unknown`, retiring the connection, joining physical work and preventing replay. Additive fallback keeps native memory active.

### Optional temporal graph recall

When the selected connection already has reviewed `mother_get_capabilities`,
`mother_garden_query` and `mother_garden_get` tools, the additive adapter checks
for the available v2 query contract and supplements v1 recall with current graph
records. It does not review or grant tools automatically. Capability availability
does not establish read permission: Mother independently checks `garden:read`
on each query and Get. Existing context-only connections retain their v1 behavior;
a graph-only credential does not replace the existing v1 binding/bootstrap.

An explicit Mother search performs at most one literal graph text query and one one-hop neighbor query
with two starts, eight edges and 32 work units. It reads at most four current
graph records, in addition to the four-record v1 fetch budget. Both sources share
the four-item, 8 KiB complete-output limit and the absolute three-second deadline.
Optional graph work receives at most 750 ms, reserving 500 ms for the final v1
authority check. Query pages are bounded selections, not exhaustive graph recall.

Graph output preserves typed IDs, revisions, valid/observed/transaction times,
attribution, evidence references, optional extensions, coverage, watermarks and
whether another page exists. A related entity's name is included only after a
fresh current Get. Unresolved references remain IDs. Locators use
`mother-garden:<workspace>/<generation>/<kind>/<id>@<revision>`; graph IDs must
not be passed to the v1 record reader. Unknown extensions remain opaque data.
Malformed core fields, mixed epochs/watermarks and inactive current records
discard the graph batch. No records, cursors or results persist between requests.

An optional graph failure leaves v1 recall available only when the final shared
authority check succeeds. A timeout that cancels an entered MCP call retires its
connection, so all shared results are withheld and native memory remains active.
A fresh turn uses existing reconnection checks. Workspace/generation mismatch or
restart-required errors likewise invalidate shared context. This read-only slice
does not publish graph facts, create proposals or change selected sharing.

If the garden is unavailable, revoked, restored or incompatible, the same chat continues with the saved local personality and native memory. The source's tools are withheld for that run; another agent's connection is unaffected. A fresh turn can attempt recovery. A restored generation requires explicit rebinding.

Ordinary local writes and full conversation turns are **not automatically uploaded**. Explicit `memory_share` selections enter a durable local queue; `memory_share_status` reports their current outcome. A queue acknowledgement confirms local storage only. Proposals remain pending until M.O.T.H.E.R owner review, and direct writes require their existing grants. Native memory saving and garden sharing are separate outcomes.

Disabling the provider removes automatic shared recall and persona loading. It does not revoke the MCP key or remove separately granted tools. Manage those separately when access should be removed entirely.

## Additive API contract

Owner-only `GET` and `PUT /api/agents/{agentId}/workspace-context` expose credential-free metadata and revision-checked updates. `providerBinding` uses this versioned schema:

```json
{"version":1,"provider":"mother","connectionId":"reviewed-connection","workspaceId":"workspace-id","generation":"restore-generation","subject":"agent-id","publication":"selected","personaSource":"local","accessMode":"companion"}
```

It is stored in `settings.additional.memoryProvider`. Conversion sends the current `expectedRevision`, `binding: null` and the complete `providerBinding` together. Omitting `providerBinding` preserves it for older clients; explicitly setting it to null removes it. Legacy and additive bindings cannot coexist. Saving the setting does not grant execution rights or prove a live read succeeded. After an unconfirmed save, reread owner metadata before attempting a different change.

### Native-only Mother pairing maintenance

Use the installed native executable with `mcp --config <owner-private-client-config>` on the selected agent's MCP connection. Reuse an existing compatible credential without changing its scopes. Connect, review the current discovered descriptors and grant the intended tools before saving the additive binding. Use the workspace, generation and agent subject from a fresh authorized context read; old pairing metadata can describe a retired generation.

For reviewed selected sharing, the installed pairing uses the six read tools (`mother_get_context`, `mother_search_context`, `mother_get_record`, `mother_get_record_history`, `mother_get_receipt`, `mother_get_proposal`) and `mother_propose_memory`. History requires a companion grant. This selection does not grant direct writes or owner approval. The additive setting uses `publication: "selected"` and `personaSource: "local"`; `accessMode: "companion"` describes the credential's access role and does not make Mother the agent's primary memory.

AnimaOS limits the MCP registry to four connections. When replacing an obsolete entry, first inspect selected-sharing outcomes and archive its metadata; do not discard pending or uncertain publications. Replace only the identified obsolete connection within that limit, preserving other connections and credentials. Verify the saved binding, ready connection, current and historical reads, and native memory/history after repair. A connected protocol session does not establish model-generated recall or owner approval acceptance.

## Selected sharing and review

`memory_share` selects one current native fact owned by the selected agent in the
`Shared` memory scope. Private, room-scoped, core-memory and projected provider
records are ineligible. Selection includes a stable publication UUID, local ID,
exact expected content, title, attribution and basis. Proposal delivery is the
default. An explicit correction also supplies the canonical target ID and its
expected revision; direct delivery must be selected explicitly and permitted by
both AnimaOS and M.O.T.H.E.R. There is no automatic transcript extraction or
publication on ordinary memory writes.

The private SQLite queue belongs to the daemon profile and is derived from its
configured persistence path. Without persistent storage, admission is unavailable.
Admission commits the selection, frozen request, source digest and operation UUID
before returning. A repeated UUID with the same selection returns its existing
state; changed input with that UUID conflicts. If a response is lost, read that ID
before taking another action. Queue bounds reject new selections visibly rather
than discarding acknowledged work. Defaults retain at most 10,000 records within
a 128 MiB database/journal budget; reserved outcome space can reject admission
earlier. A selected request must fit 6 KiB.
If persisting a delivery outcome fails, sharing reports storage unavailable and
stops admitting or dispatching new work. The original request remains retained
for startup recovery; the daemon does not guess whether the remote action saved.

The background worker checks current profile ownership, the exact agent binding,
workspace and generation, reviewed descriptors and current tool permissions.
`Ask` blocks delivery until the owner grants the tool permission and resumes;
`Deny` blocks delivery. An earlier chat approval is not background authority.
Paired-node connections are not supported for durable publication. Removing the
provider or changing its binding preserves queued work and blocks dispatch.
Resuming does not retarget old work to a new connection or restored generation.

States distinguish `queued`, `sending`, `unknown`, `submitted_for_review`, `saved`,
`conflict`, `blocked`, `dismissed` and `cancelled`. Interrupted sends recover as
uncertain and look up the same operation receipt before retrying the identical
request. Known proposals are polled for review outcomes, never submitted again.
Only a validated canonical write or approval receipt creates a saved mapping;
owner-edited approval content and its exact canonical revision are retained.
An unreadable approval remains unconfirmed, including when a shared allowlist
does not expose a newly approved record. Access is never widened automatically.

Changes to a selected source block its unsent snapshot. Deleting a source cancels
work that has never been claimed for sending. Once a send might have happened,
reconciliation preserves that uncertainty. Removing local memory never retracts
shared knowledge. Corrections require a fresh explicit selection and revision.

The agent's M.O.T.H.E.R panel has **Show selected shares** with counts, current
outcomes, manual refresh, resume and cancellation of never-sent work. Owner-only
API routes expose summaries without raw selected content:

| Method | Route | Behavior |
| --- | --- | --- |
| GET | `/api/agents/{agentId}/memory-publications` | Bounded status list, counts and omissions. |
| POST | `/api/agents/{agentId}/memory-publications` | Durable selection admission; not a remote save acknowledgement. |
| GET | `/api/agents/{agentId}/memory-publications/{publicationId}` | Reread one current outcome after an uncertain response. |
| POST | `/api/agents/{agentId}/memory-publications/{publicationId}/control` | Revision-checked `resume` or `cancel`. |

The SDK exposes these as `agentFiles.memoryPublications`, `memoryPublication`,
`shareMemory` and `controlMemoryPublication`. API and UI controls do not approve
M.O.T.H.E.R proposals; owner review remains in M.O.T.H.E.R.

## Historical legacy companion setup

Legacy Companion uses the MCP workspace as its personality and primary durable memory. The following describes the older pairing application and existing unconverted bindings. Its dashboard and setup commands are retired in native-only M.O.T.H.E.R; use the native pairing maintenance contract above for that release.

### Connect with M.O.T.H.E.R

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

### Legacy offline and recovery

Before a reply, AnimaOS checks the current connection, workspace, generation, granted role and agent subject. Companion identity is loaded transiently; your configured local personality is not overwritten.

If the source is offline, revoked, incompatible or mismatched, the conversation continues in the **same chat** with the saved local personality and a memory-unavailable notice. Previously loaded messages remain in chat history. The failed connection's tools cannot be used to bypass the unavailable binding. On a later turn, a successful read can restore companion mode without replaying any memory writes.

Companion mode keeps durable memory in M.O.T.H.E.R, including during fallback: AnimaOS does not silently capture a second local copy, automatically persist task-result memories, or mirror turns to another memory provider. Ordinary conversation history is still saved by AnimaOS. Memory proposals require the usual M.O.T.H.E.R owner review; offline or uncertain writes must never be reported as saved.

Closing M.O.T.H.E.R's dashboard does not necessarily stop its MCP service. Availability depends on the actual configured connection. Access changes cannot erase context already received in an existing conversation.

### Legacy adapter contract

The host integration is generic MCP. It pins `settings.additional.workspaceContext` with a connection ID, remote reader name, mode, workspace ID, generation and subject. Owner-only `GET` and `PUT /api/agents/{agentId}/workspace-context` expose credential-free metadata and revision-checked updates. Saving a binding does not grant MCP execution rights or prove the next conversation has read it.

Readers return the existing context envelope plus `access: { version: 1, mode, subject }`. Companion mode requires the owner persona with its verified SHA-256 digest; shared mode omits persona. The host uses current per-agent MCP authority and retains its own permissions, instruction hierarchy, cancellation and execution limits. Workspace records remain attributed reference data.

Bootstrap requests one preview when the reader supports a limit, then uses the workspace tools for relevant searches and full records. The response must fit the host's existing 16 KiB MCP envelope budget. An oversized persona or response causes explicit fallback; personality text is never silently truncated. The separate 16,000-character persona ceiling does not guarantee that a document fits the byte budget, especially with multilingual text.

M.O.T.H.E.R enforces sharing independently across reads, counts, retrieval and search. Sharing an existing record does not automatically share its references, newly approved records, or future records. Credentials and their sharing policies are not portable memory and are excluded from exports/restores. A restored generation requires an explicit new binding.
