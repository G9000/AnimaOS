use anima_core::workflow_templates::{
    WorkflowTemplate, WorkflowTemplateInput, WorkflowTemplateParameter, WorkflowTemplateRunInput,
};
use anima_core::workflows::{WorkflowStepKind, WorkflowStepSpec};
use std::collections::BTreeMap;

fn input() -> WorkflowTemplateInput {
    WorkflowTemplateInput {
        slug: "daily-brief".into(),
        revision: 0,
        title: "Brief for ${name}".into(),
        parameters: vec![WorkflowTemplateParameter {
            name: "name".into(),
            default: Some("Owner".into()),
        }],
        steps: vec![WorkflowStepSpec {
            id: "review".into(),
            title: "Review ${name}".into(),
            kind: WorkflowStepKind::Approval,
            prompt: "Review ${name}. Literal $${name}. Shell $(whoami) and $HOME are plain text."
                .into(),
        }],
    }
}

#[test]
fn workflow_templates_resolve_defaults_once_and_preserve_literal_data() {
    let template = WorkflowTemplate::new("agent", input(), 10).unwrap();
    let request = WorkflowTemplateRunInput {
        request_key: "run-1".into(),
        revision: 1,
        inputs: BTreeMap::new(),
    };
    let run = template.materialize("id-1", &request, 11).unwrap();
    assert_eq!(run.title, "Brief for Owner");
    assert_eq!(
        run.steps[0].prompt,
        "Review Owner. Literal ${name}. Shell $(whoami) and $HOME are plain text."
    );
    assert_eq!(
        run.source_template.as_ref().unwrap().inputs["name"],
        "Owner"
    );
    assert_eq!(
        run.status,
        anima_core::workflows::WorkflowStatus::AwaitingApproval
    );
    let mut values = BTreeMap::new();
    values.insert("name".into(), "${other}<script>alert(1)</script>".into());
    let next = template
        .materialize(
            "id-2",
            &WorkflowTemplateRunInput {
                request_key: "run-2".into(),
                revision: 1,
                inputs: values,
            },
            12,
        )
        .unwrap();
    assert_eq!(next.title, "Brief for ${other}<script>alert(1)</script>");
    assert_ne!(run.id, next.id);
    assert_eq!(run.title, "Brief for Owner");
    run.validate().unwrap();
    next.validate().unwrap();
}

#[test]
fn workflow_templates_reject_unknown_missing_malformed_and_oversized_values() {
    let mut definition = input();
    definition.parameters[0].default = None;
    let template = WorkflowTemplate::new("agent", definition, 10).unwrap();
    let mut request = WorkflowTemplateRunInput {
        request_key: "run".into(),
        revision: 1,
        inputs: BTreeMap::new(),
    };
    assert!(template
        .materialize("id", &request, 11)
        .unwrap_err()
        .contains("Missing"));
    request.inputs.insert("unknown".into(), "a".into());
    assert!(template
        .materialize("id", &request, 11)
        .unwrap_err()
        .contains("Unknown"));
    request.inputs.clear();
    request.inputs.insert("name".into(), "a".repeat(8193));
    assert!(template.materialize("id", &request, 11).is_err());
    for text in ["${missing}", "${name", "${name.toUpperCase()}"] {
        let mut definition = input();
        definition.steps[0].prompt = text.into();
        assert!(WorkflowTemplate::new("agent", definition, 10).is_err());
    }
    let mut definition = input();
    definition.parameters.push(definition.parameters[0].clone());
    assert!(WorkflowTemplate::new("agent", definition, 10).is_err());
}

#[test]
fn workflow_templates_revision_updates_replay_exact_requests_and_pin_old_runs() {
    let mut template = WorkflowTemplate::new("agent", input(), 10).unwrap();
    let old = template
        .materialize(
            "old",
            &WorkflowTemplateRunInput {
                request_key: "old-run".into(),
                revision: 1,
                inputs: BTreeMap::new(),
            },
            11,
        )
        .unwrap();
    let mut changed = input();
    changed.revision = 1;
    changed.steps[0].prompt = "New ${name}".into();
    assert!(template.save(changed.clone(), 12).unwrap());
    assert!(!template.save(changed.clone(), 1).unwrap());
    assert_eq!(template.revision, 2);
    let mut conflict = changed;
    conflict.title = "Conflict".into();
    assert!(template.save(conflict, 13).is_err());
    assert!(!template.save(input(), 1).unwrap());
    let restored: WorkflowTemplate =
        serde_json::from_str(&serde_json::to_string(&template).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(old.source_template.as_ref().unwrap().revision, 1);
    assert!(old.steps[0].prompt.starts_with("Review Owner"));
    let mut invalid = old;
    invalid.source_template.as_mut().unwrap().revision = 0;
    assert!(invalid.validate().is_err());
}
