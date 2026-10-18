# anima-service-adapters

Reusable contracts, bounded validation and content projection for service/tool
adapters. Browser operations, connector descriptors, bundle manifests, speech and
transcription ports, and derived DOCX/PDF/XLSX content belong here. Concrete
provider clients remain in `anima-model-adapters`, parsers in
`anima-document-content`, and selected host integrations in `hosts/rust-daemon`.
MCP servers expose additional services through the host's existing MCP adapter.

The dependency runs from this crate to `anima-core`. Core never imports or
reexports service contracts. Core owns execution, capabilities, approvals,
budgets, cancellation and recovery through generic interfaces.

Hosts using the document attachment projection must use `ServiceTokenEstimator`
for admission, context selection, compaction and calibration. It implements
core's `ContextTokenEstimator` and adds exact rendered document UTF-8 bytes to
ordinary text/tool estimates. Calibration never discounts that extra reserve.
It derives the charge from validated document content, not a caller-supplied
token-count field. This accounting does not grant attachment authority: the host
must still verify ownership and source hashes before sending content.

Run `bun x nx run core-rust:test --skipNxCache` from the workspace root. The
existing core Rust build, test and lint targets include this crate explicitly.
