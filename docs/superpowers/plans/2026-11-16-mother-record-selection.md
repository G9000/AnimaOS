# Mother record selection in AnimaOS

The discovery repair is published on main as `c548989`. The retained real-chat
trace shows a separate selection failure: a long semantic query searched a
partially indexed garden, its long literal retry matched nothing, and the model
read a different project's checkpoint. A short exact project query finds the
intended current record. No data migration, pairing change or sharing change is
needed.

AnimaOS owns query planning and evidence selection. Keep Mother's generic storage,
search API and owner approval boundary unchanged. A prompt-only retry would leave
automatic recall dependent on model phrasing; expanding the semantic index alone
would leave exact lookup and project disambiguation unresolved. Use a bounded
adapter query plan with exact topic anchors before semantic fallback, then rank
candidate previews against the request and validate the fetched current record.
Similar project-name prefixes are not exact matches. Semantic-only candidates
must not override an explicitly named topic. Preserve complete records,
provenance, contradictions, existing pinned/assigned context, authority checks,
byte/record/deadline limits and local-memory fallback.

1. Add a synthetic partially indexed garden fixture and reproduce selection of a
   similarly named wrong project. Cover exact topic lookup, relevant older
   records, no-match ambiguity, changed current content and bounded query work.
2. Implement query planning and ranking inside the AnimaOS Mother adapter, without
   new dependencies, broad garden scans or automatic writes. Keep native memory,
   capture and local persona active; selected publication stays explicit.
3. Run focused regressions, the full managed Rust suite, lint and a native bundled
   build against fixed source hashes, preserving the 32 GiB cache ceiling.
4. Back up the active profile, install the qualified binary separately and verify
   graceful cold restart. Repeat the original natural-language real-chat task in
   a fresh conversation without any supplied Mother tool name, record ID, exact
   checkpoint title or answer. Verify actual current Mother evidence and native
   recall, unchanged original records/history and no shared publication/proposal.
5. Keep private evidence outside Git, preserve the original additive worktree and
   record the result and any remaining retrieval limitations.

The five original regressions reproduced wrong-project, unrelated-event and stale
current-record injection before production changes. A source review then found
two additional cases: title placement outweighed event overlap, and protocol
acronyms displaced quoted/mixed-case topics. Both were reproduced in three failing
checks against the exact pure Rust module before correction. The same six module
checks passed afterward; the integrated provider suite is being rechecked.

The first fully qualified selection build failed real acceptance: native recall
worked, but an eight-preview search with the real context payload exceeded the
host's portable-response budget and withheld Mother schemas. The failed chat and
private capture were retained, and the previous qualified discovery build was
restored without restoring or deleting profile data. A realistic synthetic
persona/preview response then reproduced the failure through `WorkspaceRunContext`.
Only explicitly partial search envelopes now permit one fresh read-only retry
with four previews. Partial data is discarded; every dispatch revalidates the
binding/grants and stays under the same three-second deadline. Authority,
transport and malformed-response failures do not retry. This permits at most six
search attempts. The corrected 32-check focused suite passed; full qualification
and fresh real-chat acceptance remain required.

The bounded retry build passed full qualification and cold restart. The second
real trial dispatched Mother searches and native recall, but the model assembled
long literal phrases instead of following the selected checkpoint's record ID.
It correctly declined to claim a fresh full-record read. That trial and its
private capture remain intact. The reference presentation now separates complete
harness-fetched records from previews and lists validated source locators before
the bodies, explaining how to request a fresh read of an already selected record.
This presentation change requires renewed full qualification and fresh acceptance.

Final qualification and acceptance completed: the exact final source passed the
full core, daemon and packaged-supervisor Rust suites, lint (existing warnings),
and bundled native build. The final executable was installed separately and
passed graceful cold restart with the existing workspace, account, credentials,
Mother pairings and local persona. A fresh real conversation using the unchanged
diagnostic prompt discovered Mother's reader, read the intended current complete
checkpoint and independently searched native memory. No Mother tool name, record
ID, exact checkpoint title or answer was supplied in the prompt. The answer
reported the correct published implementation and correctly distinguished cold
restart verification from an untested full Windows reboot.

All original native memory/graph entries and conversation history remained
unchanged. Normal diagnostic task captures stayed private. Agent configurations,
additive bindings and the empty selected-publication queue remained unchanged;
the final diagnostic made no canonical Mother changes. Original Mother records
and historical snapshots were audited through the owner service. Both earlier
failed trials and all backups remain owner-private, outside Git. This is one
successful final real conversation, not an exhaustive retrieval benchmark. The
selection changes remain local on `g9000/mother-record-selection`; only the
earlier discovery repair has been merged and pushed.
