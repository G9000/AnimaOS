# Searchable project knowledge

Owner-approved scope: shared projects can select folders anywhere on the local computer, granting read-only knowledge access to each selected folder. No Kanban or new task-management surface. Existing agents keep their private memory, tools, credentials, and approval authority.

## User experience

The existing project Sources view gains Add folder and a search box. Adding a folder requires an explicit read-only grant and saves it as a canonical project resource. Status distinguishes indexing, ready, partial, and unavailable; coverage and errors remain visible. Search returns bounded relevant passages with document names and page, paragraph, cell, or line locations. Opening a citation reauthorizes the project membership, source selection, root identity, and current source hash. Changed or removed documents never silently show cached text as current evidence.

Project chats retrieve passages using the current user request, within the existing shared-project context budget. Quoted passages are data, never instructions or new authority. Active turns retain their admitted text and manifest through compaction. Revocation fences subsequent reads and model attempts. Recovery binds the admitted evidence manifest and cannot silently substitute changed documents.

## Architecture and limits

Reusable indexing lives in a separate SQLite document-store crate under packages/core-rust, separate from private agent memory and connected-account search. Bundled SQLite FTS5 supplies local lexical ranking; this version does not claim semantic/embedding search. Transactions replace complete document revisions and their chunks. Persistent source grants live with project resources; the index is derived, rebuildable cache.

Host folder grants pin a canonical physical root, provide only held read capabilities, and reject links, reparse escapes, hard links, protected/sensitive paths, and credential-bearing text. All operations require approved local-owner authentication plus current project membership. No generic endpoint reads arbitrary paths.

Folder scanning, reads, extraction, and index publication are bounded. Initial envelope: at most 200 documents/source, depth 8, 2 MiB input/document, 32 MiB original file bytes across the selected folders per indexing pass, and 1 MiB extracted text/document. Coverage counts omitted files and extraction limitations; later folders can be partial when a pass exhausts its budget. An explicit refresh can target one folder. The cache caps retained text at 256 MiB and the SQLite main database at 512 MiB. Unsupported, omitted, busy, changed, and failed files remain visible. Interactive Office work retains its existing physically owned process slot. Office/PDF parsing must remain in the owned worker; indexing extraction gets a separate larger bounded protocol rather than reusing the 16 KiB attachment preview. No OCR, passwords, remote downloads, remote embeddings, or evaluated spreadsheet formulas.

## API contract

All paths begin /api/projects/{projectId}/knowledge and use existing no-store owner authorization. Member reads use asAgentId.

* POST /sources: {expectedRevision, requestKey, asAgentId, rootPath, label?}; explicitly grants this root read-only access and returns {project}.
* GET /status?asAgentId: {projectRevision, accessEpoch, memberGeneration, sources:[{resourceId,status,documentCount,indexedAtMs,generation,omittedCount,error?}]}.
* POST /refresh: {asAgentId,resourceId?}; schedules/rebuilds only currently granted sources and returns status.
* GET /search?asAgentId&query&limit: {projectRevision,accessEpoch,memberGeneration,hits:[{citationId,resourceId,documentId,title,locator,excerpt}],sources:[status...]}. Limit defaults 8, max 20, query max 512 characters. No pagination needed for bounded top passages.
* GET /citations/{citationId}?asAgentId: verified hit with bounded passage text; changed bytes produce conflict, removed access refuses. Citation IDs are opaque server-minted identities, not file paths.

Existing resource remove and context selection APIs revoke/disable knowledge resources with the same access-epoch behavior. New resource target kind knowledge_folder contains the persisted server-validated grant; ordinary addResource must not manufacture it.

## Acceptance

Find a passage beyond the old 16 KiB extraction cutoff; rank useful passages; survive cache reopen; atomically replace changed documents; fence removed grants and memberships; reject path/link and query-injection attacks; avoid cross-project results; open only source/hash-matching citations; include query-specific citations in project chat within its budget; retain manifest on compaction/recovery. Validate SDK routing and UI stale-request clearing, read-only grant wording, status/search/error states, keyboard/mobile layout. Native process/cancellation tests must preserve physical custody. No live provider accounts are needed.
