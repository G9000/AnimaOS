use super::*;
use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
use anima_core::agent_files::{render_agent_files_with_admissions, AgentFileSection};
use std::collections::HashSet;

struct Candidate {
    entry: AgentFileManifestEntry,
    text: Option<String>,
    directory: usize,
}
struct Reader<'a> {
    bytes: WorkspaceBytes,
    used: usize,
    candidates: usize,
    cancel: &'a AtomicBool,
    deadline: Instant,
}
impl Reader<'_> {
    fn read(&mut self, relative: String, kind: AgentFileKind, directory: usize) -> Candidate {
        let mut candidate = Candidate {
            entry: AgentFileManifestEntry::new(relative, kind),
            text: None,
            directory,
        };
        let entry = &mut candidate.entry;
        if let Some(state) = stopped(self.cancel, self.deadline) {
            entry.state = state;
            return candidate;
        }
        self.candidates += 1;
        if self.candidates > MAX_CANDIDATES {
            entry.state = AgentFileState::BudgetExcluded;
            entry.reason_code = Some(ReasonCode::CandidateLimit);
            return candidate;
        }
        // Reserve max+1 for the actual bounded read, even on oversize/error. This
        // upper bound prevents failed/changing files from escaping the I/O cap.
        let remaining = MAX_TOTAL_READ_BYTES.saturating_sub(self.used);
        if remaining <= 1 {
            entry.state = AgentFileState::BudgetExcluded;
            entry.reason_code = Some(ReasonCode::TotalReadLimit);
            return candidate;
        }
        let max = MAX_FILE_BYTES.min(remaining - 1);
        let bytes = match self.bytes.read_instruction_file(&entry.relative_path, max) {
            Ok(bytes) => {
                self.used += bytes.len();
                bytes
            }
            Err(error) => {
                entry.state = match error {
                    InstructionReadError::Missing => AgentFileState::Missing,
                    InstructionReadError::Linked => AgentFileState::Linked,
                    InstructionReadError::NonRegular => AgentFileState::NonRegular,
                    InstructionReadError::Oversized => {
                        self.used += max + 1;
                        AgentFileState::Oversized
                    }
                    InstructionReadError::Unreadable => {
                        self.used += max + 1;
                        AgentFileState::Unreadable
                    }
                };
                entry.reason_code = match entry.state {
                    AgentFileState::Oversized => Some(ReasonCode::FileTooLarge),
                    AgentFileState::Unreadable => Some(ReasonCode::ReadFailed),
                    _ => None,
                };
                return candidate;
            }
        };
        entry.raw_byte_count = bytes.len() as u64;
        entry.digest = Some(format!("{:x}", Sha256::digest(&bytes)));
        let decoded = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                entry.state = AgentFileState::InvalidUtf8;
                return candidate;
            }
        };
        let text = decoded
            .strip_prefix('\u{feff}')
            .unwrap_or(&decoded)
            .replace("\r\n", "\n");
        if let Some(state) = stopped(self.cancel, self.deadline) {
            entry.state = state;
            return candidate;
        }
        // Scan the WHOLE source, before frontmatter removal and render truncation.
        if text.chars().count() > MAX_SCAN_CHARS {
            entry.state = AgentFileState::BudgetExcluded;
            entry.reason_code = Some(ReasonCode::ScanLimit);
            return candidate;
        }
        entry.threat_ids = scan::scan(&text);
        if !entry.threat_ids.is_empty() {
            entry.state = AgentFileState::Blocked;
            entry.reason_code = Some(ReasonCode::ThreatDetected);
            return candidate;
        }
        let normalized = text;
        let text = if kind == AgentFileKind::Hermes {
            selection::strip_frontmatter(&normalized)
        } else {
            &normalized
        }
        .trim()
        .to_owned();
        if text.is_empty() {
            entry.state = AgentFileState::Empty;
        } else {
            entry.state = AgentFileState::Shadowed;
            candidate.text = Some(text);
        }
        candidate
    }
}
pub(super) fn load_scope(
    scope: &AgentFileScope,
    budget: AgentFileRenderBudget,
    cancel: &AtomicBool,
    deadline: Instant,
) -> AcceptedAgentFiles {
    if let Some(state) = stopped(cancel, deadline) {
        return fallback(scope, state);
    }
    if scope.workspace_root.as_os_str().is_empty() {
        return AcceptedAgentFiles::empty(Availability::UnavailableWorkspace);
    }
    let Some(mut chain) = selection::chain(&scope.project_relative) else {
        let mut result = AcceptedAgentFiles::empty(Availability::Available);
        let mut entry = AgentFileManifestEntry::new("AGENTS.md".into(), AgentFileKind::Agents);
        entry.state = AgentFileState::Unreadable;
        entry.reason_code = Some(ReasonCode::InvalidScope);
        result.manifest.entries.push(entry);
        result.seal();
        return result;
    };
    let bytes = match WorkspaceBytes::open(&scope.workspace_root) {
        Ok(bytes) => bytes,
        Err(_) => return AcceptedAgentFiles::empty(Availability::UnavailableWorkspace),
    };
    let mut reader = Reader {
        bytes,
        used: 0,
        candidates: 0,
        cancel,
        deadline,
    };
    // A nested repository is its own project-instruction boundary. This only
    // probes marker metadata under held capabilities, never .git contents.
    if chain.len() > 1 {
        for index in (0..chain.len()).rev() {
            if let Some(state) = stopped(cancel, deadline) {
                return fallback(scope, state);
            }
            reader.candidates += 1;
            if reader
                .bytes
                .instruction_marker_exists(&selection::path(&chain[index], ".git"))
            {
                chain = chain[index..].to_vec();
                break;
            }
        }
    }
    let mut candidates = vec![];
    let soul = selection::soul(scope).map(|path| {
        candidates.push(reader.read(path, AgentFileKind::Soul, 0));
        0
    });
    for (index, directory) in chain.iter().enumerate().rev() {
        for name in [".hermes.md", "HERMES.md"] {
            candidates.push(reader.read(
                selection::path(directory, name),
                AgentFileKind::Hermes,
                index,
            ));
        }
    }
    for (index, directory) in chain.iter().enumerate() {
        for name in ["AGENTS.override.md", "AGENTS.md", "agents.md"] {
            candidates.push(reader.read(
                selection::path(directory, name),
                AgentFileKind::Agents,
                index,
            ));
        }
    }
    let last = chain.last().expect("root directory");
    for name in ["CLAUDE.md", "claude.md"] {
        candidates.push(reader.read(
            selection::path(last, name),
            AgentFileKind::Claude,
            chain.len() - 1,
        ));
    }
    candidates.push(reader.read(
        selection::path(last, ".cursorrules"),
        AgentFileKind::Cursor,
        chain.len() - 1,
    ));
    if let Some(state) = stopped(cancel, deadline) {
        return fallback(scope, state);
    }
    let budget = AgentFileRenderBudget {
        max_identity_chars: budget.max_identity_chars.min(MAX_RENDER_CHARS),
        max_project_chars: budget.max_project_chars.min(MAX_RENDER_CHARS),
        max_total_chars: budget.max_total_chars.min(MAX_RENDER_CHARS),
    };
    let identity = soul.and_then(|index| {
        candidates[index]
            .text
            .as_ref()
            .map(|text| AgentFileSection {
                label: candidates[index].entry.relative_path.clone(),
                text: text.clone(),
            })
    });
    // Eligibility includes render budget, so an unusable high-priority candidate
    // cannot shadow a shorter lower-priority source.
    let mut selected = vec![];
    let mut selected_kind = None;
    for (kind, project_kind) in [
        (AgentFileKind::Hermes, ProjectSourceKind::Hermes),
        (AgentFileKind::Agents, ProjectSourceKind::Agents),
        (AgentFileKind::Claude, ProjectSourceKind::Claude),
        (AgentFileKind::Cursor, ProjectSourceKind::Cursor),
    ] {
        let mut directories = HashSet::new();
        let mut texts = HashSet::new();
        for index in 0..candidates.len() {
            if candidates[index].entry.kind != kind || candidates[index].text.is_none() {
                continue;
            }
            if kind == AgentFileKind::Agents && directories.contains(&candidates[index].directory) {
                continue;
            }
            let text = candidates[index].text.as_ref().unwrap().clone();
            if texts.contains(&text) {
                candidates[index].entry.state = AgentFileState::Duplicate;
                directories.insert(candidates[index].directory);
                continue;
            }
            let mut sections: Vec<_> = selected
                .iter()
                .map(|&i: &usize| section(&candidates[i]))
                .collect();
            sections.push(section(&candidates[index]));
            let (_, counts) =
                render_agent_files_with_admissions(identity.as_ref(), &sections, budget);
            if counts.last().copied().unwrap_or(0) == 0 {
                candidates[index].entry.state = AgentFileState::BudgetExcluded;
                candidates[index].entry.reason_code = Some(ReasonCode::RenderBudget);
                continue;
            }
            selected.push(index);
            texts.insert(text);
            directories.insert(candidates[index].directory);
            if kind != AgentFileKind::Agents {
                break;
            }
        }
        if !selected.is_empty() {
            selected_kind = Some(project_kind);
            break;
        }
    }
    let sections: Vec<_> = selected.iter().map(|&i| section(&candidates[i])).collect();
    if let Some(state) = stopped(cancel, deadline) {
        return fallback(scope, state);
    }
    let (rendered, counts) =
        render_agent_files_with_admissions(identity.as_ref(), &sections, budget);
    let mut count_index = 0;
    if identity.is_some() {
        let i = soul.unwrap();
        apply_admission(&mut candidates[i], counts[0]);
        count_index = 1;
    }
    for i in selected {
        apply_admission(&mut candidates[i], counts[count_index]);
        count_index += 1;
    }
    let mut result = AcceptedAgentFiles::empty(Availability::Available);
    result.manifest.identity_source = if rendered.identity.is_none() {
        IdentitySource::Legacy
    } else if scope.profile_enabled {
        IdentitySource::ProfileSoul
    } else {
        IdentitySource::WorkspaceSoul
    };
    result.manifest.project_source_kind = selected_kind;
    result.manifest.entries = candidates
        .into_iter()
        .map(|candidate| candidate.entry)
        .collect();
    result.rendered = rendered;
    if let Some(state) = stopped(cancel, deadline) {
        return fallback(scope, state);
    }
    result.seal();
    result
}
fn section(candidate: &Candidate) -> AgentFileSection {
    AgentFileSection {
        label: candidate.entry.relative_path.clone(),
        text: candidate.text.as_ref().unwrap().clone(),
    }
}
fn apply_admission(candidate: &mut Candidate, count: usize) {
    candidate.entry.admitted_char_count = count as u64;
    if count == 0 {
        candidate.entry.state = AgentFileState::BudgetExcluded;
        candidate.entry.reason_code = Some(ReasonCode::RenderBudget);
    } else if count < candidate.text.as_ref().unwrap().chars().count() {
        candidate.entry.state = AgentFileState::Truncated;
        candidate.entry.truncated = true;
    } else {
        candidate.entry.state = AgentFileState::Loaded;
    }
}
