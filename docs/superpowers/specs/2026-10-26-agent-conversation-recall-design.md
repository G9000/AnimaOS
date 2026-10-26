# Agent conversation recall

Agents must be able to answer questions in their ordinary owner chat about requests received from other agents, and the owner must be able to inspect the actual exchange.

Use the existing durable sessions, run ledger, and transcripts. Incoming delegated and peer sessions of an existing agent are its own history. Temporary generated helpers remain isolated. Own-history search and anchored reading may include incoming agent sessions, with sender attribution, without reading the sender's private chat. Recent incoming requests are supplied as bounded, quoted evidence in ordinary owner chats, including sender, timestamp, run state, session/anchor references, and a short request excerpt. Evidence is not a new instruction and does not prove effects outside the recorded run. Missing history is not proof that no contact occurred.

The owner opens the selected agent's menu, then Other conversations, then the Agent conversations history filter. Incoming/outgoing sessions use the existing searchable, paged, archivable history list. Each row identifies the participants and opens the existing read-only transcript. Keep this entry point out of the regular chat header. Do not merge transcripts or add model-generated summaries as provenance.

Existing records, including Mom's two delegated exchanges, work without a migration or rewriting history. Deleted/pending-deletion sessions and generated helper configurations remain excluded from agent recall. Bound context, search, and transcript windows. Test sender attribution, cross-session recall, archived/cold history, failed/in-progress runs, isolation, and UI navigation.
