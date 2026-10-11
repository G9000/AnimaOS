//! Pure, transient rendering for accepted standard Markdown agent files.
//! Discovery, provenance and filtering belong to the host.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePromptIdentity {
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedAgentFiles {
    pub identity: Option<RuntimePromptIdentity>,
    pub project_context: String,
    pub estimated_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentFileSection {
    pub label: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentFileRenderBudget {
    pub max_identity_chars: usize,
    pub max_project_chars: usize,
    pub max_total_chars: usize,
}

/// Labels must be relative filenames. Invalid labels are replaced, never reflected.
fn safe_label(label: &str) -> &str {
    if !label.is_empty()
        && label.len() <= 256
        && !label.starts_with('/')
        && !label.contains('\\')
        && !label.contains(':')
        && label
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
    {
        label
    } else {
        "agent-file.md"
    }
}

/// Unicode scalar truncation; the marker is included in the supplied limit.
/// A limit unable to hold the marker and useful head/tail returns no content.
pub fn truncate_agent_file(text: &str, label: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let marker = format!("\n[{} truncated]\n", safe_label(label));
    let reserve = marker.chars().count();
    if max_chars < reserve.saturating_add(2) {
        return String::new();
    }
    let available = max_chars - reserve;
    let head = available * 7 / 10;
    let tail = available - head;
    let mut result: String = text.chars().take(head).collect();
    result.push_str(&marker);
    result.extend(text.chars().skip(count - tail));
    result
}

fn render_section(section: &AgentFileSection, kind: &str, cap: usize) -> String {
    let text = section.text.replace("\r\n", "\n");
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let header = format!("## {kind} ({})\n", safe_label(&section.label));
    let overhead = header.chars().count();
    if cap <= overhead {
        return String::new();
    }
    let body = truncate_agent_file(text, &section.label, cap - overhead);
    if body.is_empty() {
        return String::new();
    }
    format!("{header}{body}")
}

/// Identity receives allocation first, then the entire project chain shares one cap.
/// Empty/excluded identity means callers retain their complete legacy persona.
pub fn render_agent_files_with_admissions(
    identity: Option<&AgentFileSection>,
    project_sections: &[AgentFileSection],
    budget: AgentFileRenderBudget,
) -> (RenderedAgentFiles, Vec<usize>) {
    let mut admissions = Vec::new();
    let has_identity = identity.is_some();
    let identity = identity
        .map(|section| {
            render_section(
                section,
                "Primary Identity",
                budget.max_identity_chars.min(budget.max_total_chars),
            )
        })
        .filter(|text| !text.is_empty())
        .map(|text| RuntimePromptIdentity { text });
    let identity_chars = identity.as_ref().map_or(0, |v| v.text.chars().count());
    if has_identity {
        admissions.push(identity.as_ref().map_or(0, |v| {
            v.text
                .split_once('\n')
                .map_or(0, |(_, body)| body.chars().count())
        }));
    }
    let mut remaining = budget
        .max_project_chars
        .min(budget.max_total_chars.saturating_sub(identity_chars));
    // The prompt separator is part of the total rendered allocation.
    if identity.is_some() {
        remaining = remaining.saturating_sub(2);
    }
    let mut project_context = String::new();
    for section in project_sections {
        let separator = if project_context.is_empty() { 0 } else { 2 };
        let rendered = render_section(
            section,
            "Project Instructions",
            remaining.saturating_sub(separator),
        );
        admissions.push(
            rendered
                .split_once('\n')
                .map_or(0, |(_, body)| body.chars().count()),
        );
        if rendered.is_empty() {
            continue;
        }
        if separator > 0 {
            project_context.push_str("\n\n");
        }
        remaining -= separator + rendered.chars().count();
        project_context.push_str(&rendered);
    }
    let chars = identity_chars
        + project_context.chars().count()
        + if identity.is_some() && !project_context.is_empty() {
            2
        } else {
            0
        };
    (
        RenderedAgentFiles {
            identity,
            project_context,
            estimated_tokens: (chars as u64).div_ceil(crate::CHARS_PER_TOKEN),
        },
        admissions,
    )
}

/// Render without per-source admission metadata.
pub fn render_agent_files(
    identity: Option<&AgentFileSection>,
    project_sections: &[AgentFileSection],
    budget: AgentFileRenderBudget,
) -> RenderedAgentFiles {
    render_agent_files_with_admissions(identity, project_sections, budget).0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_truncation_retains_tail_and_counts_marker() {
        let text = format!("{}END", "🦀界".repeat(100));
        let result = truncate_agent_file(&text, "SOUL.md", 60);
        assert_eq!(result.chars().count(), 60);
        assert!(result.ends_with("END"));
        assert!(result.contains("[SOUL.md truncated]"));
        assert!(truncate_agent_file(&text, "SOUL.md", 3).is_empty());
    }
    #[test]
    fn aggregate_budget_and_identity_precedence() {
        let soul = AgentFileSection {
            label: "SOUL.md".into(),
            text: "persona".repeat(100),
        };
        let projects = vec![
            AgentFileSection {
                label: "AGENTS.md".into(),
                text: "project".repeat(100)
            };
            30
        ];
        let result = render_agent_files(
            Some(&soul),
            &projects,
            AgentFileRenderBudget {
                max_identity_chars: 100,
                max_project_chars: 1000,
                max_total_chars: 200,
            },
        );
        assert_eq!(result.identity.as_ref().unwrap().text.chars().count(), 100);
        assert!(result.project_context.chars().count() <= 98);
        assert!(result.estimated_tokens <= 50);
        let tiny = render_agent_files(
            Some(&soul),
            &projects,
            AgentFileRenderBudget {
                max_identity_chars: 1,
                max_project_chars: 1,
                max_total_chars: 1,
            },
        );
        assert_eq!(tiny, RenderedAgentFiles::default());
    }
    #[test]
    fn labels_cannot_reflect_absolute_paths_or_markdown() {
        let section = AgentFileSection {
            label: "C:/secret\n# inject".into(),
            text: "ok\r\nnext".into(),
        };
        let result = render_agent_files(
            Some(&section),
            &[],
            AgentFileRenderBudget {
                max_identity_chars: 200,
                max_project_chars: 0,
                max_total_chars: 200,
            },
        );
        let text = result.identity.unwrap().text;
        assert!(!text.contains("secret"));
        assert!(text.ends_with("ok\nnext"));
    }
}
