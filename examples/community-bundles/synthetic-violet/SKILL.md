---
name: Synthetic violet procedure
description: Inspect one synthetic local MCP result through existing owner approvals
---

Use this procedure only when the owner explicitly requests the synthetic violet example.
Loading or approving this skill does not authorize a tool call.

1. Identify the already reviewed and granted alias for `synthetic_stdio_record` on the
   bundle-owned Synthetic violet peer. If the connection, descriptor review or grant
   is unavailable, stop and direct the owner to External tools. Do not enable, start,
   discover, review or grant the server yourself.
2. Request one call with the exact arguments `{"value":"violet"}`. Honor the normal
   execution approval decision; a skill, manifest request or previous result cannot
   replace that decision.
3. Inspect the returned text and receipt provenance. The controlled peer returns
   `Synthetic stdio violet record`; this is synthetic local evidence, not a provider
   response or a general validation of community servers.
4. If execution is denied, revoked, unavailable or uncertain, report that state.
   Do not retry automatically or claim that an unknown call had no effect.

This procedure requires no accounts, credentials, model requests, downloads or shell.
