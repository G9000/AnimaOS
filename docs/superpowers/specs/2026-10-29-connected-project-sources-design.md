# Connected project sources

Date: 2026-10-29. Authorized by the owner together with click-to-edit previews, with explicit subagent delegation. Base: `170207f3274f0bd01dabfd4364f003a1ad6dbde1`. Work remains uncommitted unless the owner asks to publish it.

## Outcome

In Projects → Sources, the owner can find selected content from an existing Google Drive, Microsoft files, or mail connection and explicitly share that one source with the project's current member agents. Search and project-chat retrieval use the existing bounded passage index and verified citations. Refresh updates the selected source; removing the source or revoking its account access fences retrieval. Ordinary project references and selected local folders retain their behavior.

This connects existing native account access to project knowledge. It does not create accounts, duplicate OAuth configuration, enroll an entire account, or grant member agents arbitrary tools or credentials.

## Domain and admission

Add a host-admitted `ConnectedSource` target containing the account-owning agent ID, connector ID/provider, native authorization generation, connected-search enrollment generation/epoch, canonical selected remote ID, and source kind. Source metadata stays in acknowledged project persistence; derived passages remain in the existing private SQLite document store. The account owner must be a current project member. Ordinary `addResource` cannot manufacture a connected-source grant.

Discovery reuses enabled native connected search and returns bounded eligible item metadata. Admission resolves a current citation/selection on the server, rechecks owner membership and native read access, and requires explicit `shareWithProject: true` plus the expected project revision and idempotency request key. Provider URLs, display names and client-supplied remote IDs never authorize a read. Renewed account enrollment cannot revive an old grant.

Proposed additive owner APIs:

- `GET /api/projects/{id}/knowledge/connected-sources?asAgentId=...&query=...`: eligible bounded choices with provider/account identity and truthful retained coverage.
- `POST /api/projects/{id}/knowledge/connected-sources`: selected canonical citation, account/connector binding, project revision, request key and explicit sharing consent.

Both retain owner authentication, origin/mutation checks, no-store responses, finite input limits, redacted failures, and existing persistence rollback discipline. Existing status, refresh, search, citation and resource-removal APIs handle the resulting source.

## Retrieval, freshness and revocation

Generalize the knowledge source witness from a folder to a folder or connected source. Preserve corpus/source isolation, atomic replacement, deterministic revision-bound opaque citations, query-centered excerpts, quotas, and existing context budgets. Check project/member generations, selected-source existence, account owner membership, native connector/read generation, file selection revision where applicable, and connected-search enrollment epoch before and after provider I/O and before publication/admission. Do not hold control-plane locks across network calls. Revalidate current remote content/version for citation and new-turn admission; active admitted context remains immutable.

Use canonical selected-file readers and existing physically owned extraction for supported bounded documents. Initial bounds: 2 MiB file input, 1 MiB extracted text/source, 32 MiB read bytes per project indexing pass, and existing project source/context limits. Mail thread reads retain at most 25 messages and 1 MiB text. Surface pagination, truncation and provider limitations. IMAP retains only the selected indexed message as partial thread coverage unless the real transport supplies a reliable thread identity; never invent grouping from a subject line.

An unavailable connected source does not hide healthy sources. A failed/incomplete refresh preserves the previous acknowledged derived generation but cannot serve it after its authority is revoked. Status distinguishes current coverage, partial content, indexing and unavailable access; UI shows the original provider/account and last refresh time. Removal/reauthorization/re-enrollment invalidates old citations conservatively. Derived content is quoted untrusted material, never instructions or new authority.

## Validation

Meaningful tests cover all native providers, canonical selection admission and consent, forged/stale IDs, nonmember account owners, private account boundaries, reconnect/enrollment ABA, source/member removal during I/O, source version changes, inaccessible source isolation, thread pagination/partial coverage, exact citations, context revalidation, quotas and persistence rollback/restart. SDK tests verify paths and consent/bindings. Desktop/mobile UI checks exercise choice → consent → indexing → search/citation → refresh/removal, including stale responses and agent/project switches.

Run full wrapped native tests and lint, SDK test/build/typecheck/lint, focused web tests and web build/typecheck/lint. Synthetic provider fixtures are local integration evidence only; actual provider/account acceptance requires a separately authorized real account exercise and is not claimed from mocks.
