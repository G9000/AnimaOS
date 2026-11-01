# General skill setup

The owner authorized extending the current narrow setup to a Hermes-style
research, inspect, install, execute and verify workflow. This follows the
already accepted architecture: the agent chooses a method from the project's
documentation; native source adapters and execution custody implement it.

## Design

Three approaches were considered. Adding language-specific handlers is easy to
start but keeps imposing runtime-specific limits. Relying solely on ordinary
terminal grants provides flexibility but does not preserve reviewed skill
identity or setup state. The selected approach combines generic source
inspection/import with an exact-approved command environment, retaining the
existing optimized Python and MCP tools.

- `inspect_skill_source` reads untrusted original documentation and a bounded
  file index without installing or executing it. It supports public GitHub
  identifiers/URLs and public HTTPS skill documents. Optional file selection
  and bounded pagination let the agent read original supporting instructions.
  A bounded expiring native inspection capture/identity keeps later file pages
  and import on the same captured bytes (or exact resolved GitHub commit with
  checked file hashes). It must fail explicitly on expired/changed captures,
  rather than silently mixing versions. The model never has to discover a full
  commit hash through web search.
- `import_skill` uses those source adapters to create an inactive imported
  draft. GitHub remains commit-pinned and complete within its declared subtree.
  URL packages contain explicitly referenced/selected relative supporting
  files, each with captured byte hashes; they do not claim repository-wide
  completeness or an immutable remote commit. Missing required files fail the
  import. Existing `import_github_skill` remains compatible.
- URL provenance, original SKILL.md bytes and captured file hashes survive
  pending-review persistence and Keep. They appear in SDK and Skills review.
  Neither external source can be laundered into autoactivated authored content.
  Every initial URL, redirect and referenced-file fetch remains HTTPS and passes
  public-address/DNS validation. Requested and final URLs are retained.
- `manage_skill_setup` is a direct-owner control for an approved skill. It
  prepares an isolated dependency/work directory and executes the exact
  documented shell command after fresh owner approval, independent of language.
  Node/npm, Bun, Python/uv, shell scripts and other installed command tools use
  the same mechanism. It does not silently install missing OS runtimes.
- A preserved original package and a mutable command workspace are distinct.
  Each command is bound to the approved package hash and expected working-state
  hash. External changes invalidate the prepared state. Approved command changes
  are resealed and reported; a failed command never preserves a verified flag.
  The seal includes dependencies, private HOME/config/temp, relevant permissions,
  the resolved shell path/hash/invocation mode and safe link identities/targets.
  Escaping/cyclic links are rejected. Script bytes are preserved; where imported
  executable bits are unavailable, use an explicit installed interpreter or an
  exact-approved chmod step before a documented direct invocation.
- Shared native process custody, root serialization, durable operation markers,
  bounded output/storage/deadlines, Stop/revocation, private HOME and filtered
  PATH apply to both Python and general command setup. No ambient account
  credentials are inherited. The environment is dependency isolation, not an OS
  security boundary. Command approval permits the command's documented network
  and OS effects; local process cleanup does not prove remote rollback.
- Existing MCP configuration, stdio exact approval, browser OAuth, discovery and
  same-turn tool refresh remain the connector mechanism. A GitHub-hosted MCP
  project is identified by its documentation, not by treating GitHub and MCP as
  exclusive categories.
- General guidance asks the agent to inspect, choose a supported method, perform
  authorized work, repair concrete failures and verify the requested outcome.
  Only actual review, sign-in, missing input or failed operations form boundaries.

## Evidence and verification

Meaningful regressions cover source routing, SSRF/redirect/path/byte bounds,
missing files, provenance/original-byte integrity, pending review, exact command
approval, stale state, environment isolation, source changes, failed checks,
Stop/deadline retirement and unknown-custody blocking. Real local Node/Bun
fixtures exercise documented dependency/setup commands and a successful check.
No live account sign-in, publication or model-provider calls are required.

Work is isolated on `g9000/general-skill-setup`. All native checks are launched
through the original checkout's task wrapper with the feature checkout as cwd
and the existing absolute main `target/cargo/validation` output. The outer main
wrapper continues enforcing the combined 32 GiB cache ceiling. There are no
per-feature Cargo output directories.

The original checkout is clean at baseline `e02d3ae4`; its just-completed baseline
verification passed 5,747 Rust tests, formatting and Clippy correctness. Only new
regressions are rerun before implementation; the full matrix runs on completion.
