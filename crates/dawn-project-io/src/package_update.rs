use std::collections::BTreeMap;

use dawn_language::identity::SourceIdentity;

use crate::{ProjectSession, SourceObjectKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageCompatibilityIssueKind {
    ModuleRemoved,
    ExportGroupRemoved,
    ExportDocumentRemoved,
    DocumentRemoved,
    ObjectRemoved,
    ObjectKindChanged,
    EffectSchemaChanged,
    OperatorSchemaChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageCompatibilityIssue {
    pub package: String,
    pub kind: PackageCompatibilityIssueKind,
    pub message: String,
    pub breaking: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageCompatibilityReport {
    pub issues: Vec<PackageCompatibilityIssue>,
}

impl PackageCompatibilityReport {
    pub fn has_breaking_changes(&self) -> bool {
        self.issues.iter().any(|issue| issue.breaking)
    }
}

pub fn analyze_package_candidate(
    current: &ProjectSession,
    candidate: &ProjectSession,
) -> PackageCompatibilityReport {
    let current_modules = dependency_modules(current);
    let candidate_modules = dependency_modules(candidate);
    let mut issues = Vec::new();

    for (module_id, package) in &current_modules {
        let Some(candidate_package) = candidate_modules.get(module_id) else {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::ModuleRemoved,
                format!("dependency module `{module_id}` was removed"),
            ));
            continue;
        };
        compare_exports(package, current, candidate, *module_id, &mut issues);
        compare_source_objects(package, current, candidate, *module_id, &mut issues);
        if package != candidate_package {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::ModuleRemoved,
                format!(
                    "module `{module_id}` changed package identity from `{package}` to `{candidate_package}`"
                ),
            ));
        }
    }

    compare_definition_schemas(current, candidate, &current_modules, &mut issues);
    issues.sort_by(|left, right| {
        (&left.package, &left.message).cmp(&(&right.package, &right.message))
    });
    PackageCompatibilityReport { issues }
}

fn dependency_modules(session: &ProjectSession) -> BTreeMap<uuid::Uuid, String> {
    session
        .source
        .source_graph
        .modules()
        .iter()
        .filter_map(|(module_id, module)| match &module.origin {
            dawn_package::ResolvedModuleOrigin::RegistryDependency { package, .. } => {
                Some((*module_id, package.to_string()))
            }
            dawn_package::ResolvedModuleOrigin::PathDependency { declared_path, .. } => {
                Some((*module_id, format!("path:{declared_path}")))
            }
            dawn_package::ResolvedModuleOrigin::Project => None,
        })
        .collect()
}

fn compare_exports(
    package: &str,
    current: &ProjectSession,
    candidate: &ProjectSession,
    module_id: uuid::Uuid,
    issues: &mut Vec<PackageCompatibilityIssue>,
) {
    let Some(current_module) = current.source.source_graph.modules().get(&module_id) else {
        return;
    };
    let Some(candidate_module) = candidate.source.source_graph.modules().get(&module_id) else {
        return;
    };
    for (group, export) in &current_module.manifest.exports {
        let Some(candidate_export) = candidate_module.manifest.exports.get(group) else {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::ExportGroupRemoved,
                format!("export group `{group}` was removed"),
            ));
            continue;
        };
        for document in &export.documents {
            if !candidate_export.documents.contains(document) {
                issues.push(issue(
                    package,
                    PackageCompatibilityIssueKind::ExportDocumentRemoved,
                    format!("document `{document}` was removed from export group `{group}`"),
                ));
            }
        }
    }
}

fn compare_source_objects(
    package: &str,
    current: &ProjectSession,
    candidate: &ProjectSession,
    module_id: uuid::Uuid,
    issues: &mut Vec<PackageCompatibilityIssue>,
) {
    for (document_id, document) in &current.source.documents {
        if document_id.module_id() != module_id {
            continue;
        }
        let Some(candidate_document) = candidate.source.documents.get(document_id) else {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::DocumentRemoved,
                format!("document `{}` was removed", document_id.path()),
            ));
            continue;
        };
        let candidate_objects = candidate_document
            .objects()
            .iter()
            .map(|object| (object.id(), object.kind()))
            .collect::<BTreeMap<_, _>>();
        for object in document.objects() {
            match candidate_objects.get(object.id()) {
                None => {
                    issues.push(issue(
                        package,
                        PackageCompatibilityIssueKind::ObjectRemoved,
                        format!(
                            "{} `{}` was removed from `{}`",
                            object_kind_name(object.kind()),
                            object.id(),
                            document_id.path()
                        ),
                    ));
                }
                Some(kind) if *kind != object.kind() => {
                    issues.push(issue(
                        package,
                        PackageCompatibilityIssueKind::ObjectKindChanged,
                        format!(
                            "object `{}` in `{}` changed from {} to {}",
                            object.id(),
                            document_id.path(),
                            object_kind_name(object.kind()),
                            object_kind_name(kind)
                        ),
                    ));
                }
                Some(_) => {}
            }
        }
    }
}

fn compare_definition_schemas(
    current: &ProjectSession,
    candidate: &ProjectSession,
    modules: &BTreeMap<uuid::Uuid, String>,
    issues: &mut Vec<PackageCompatibilityIssue>,
) {
    for (id, definition) in &current.project.definitions.effects.definitions {
        let Some(package) = modules.get(&identity_module(&id.0)) else {
            continue;
        };
        let Some(candidate_definition) = candidate.project.definitions.effects.definitions.get(id)
        else {
            continue;
        };
        if definition.kind != candidate_definition.kind
            || definition.params != candidate_definition.params
        {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::EffectSchemaChanged,
                format!(
                    "effect `{}` changed its kind or parameter schema",
                    id.0.object()
                ),
            ));
        }
    }
    for (id, definition) in &current.project.definitions.operators.definitions {
        let Some(package) = modules.get(&identity_module(&id.0)) else {
            continue;
        };
        let Some(candidate_definition) =
            candidate.project.definitions.operators.definitions.get(id)
        else {
            continue;
        };
        if definition.inputs != candidate_definition.inputs
            || definition.output != candidate_definition.output
            || definition.params != candidate_definition.params
        {
            issues.push(issue(
                package,
                PackageCompatibilityIssueKind::OperatorSchemaChanged,
                format!(
                    "operator `{}` changed its port or parameter schema",
                    id.0.object()
                ),
            ));
        }
    }
}

fn identity_module(identity: &SourceIdentity) -> uuid::Uuid {
    identity.module_id()
}

fn issue(
    package: &str,
    kind: PackageCompatibilityIssueKind,
    message: String,
) -> PackageCompatibilityIssue {
    PackageCompatibilityIssue {
        package: package.to_string(),
        kind,
        message,
        breaking: true,
    }
}

fn object_kind_name(kind: &SourceObjectKind) -> &'static str {
    match kind {
        SourceObjectKind::Project => "project",
        SourceObjectKind::Setup => "setup",
        SourceObjectKind::Controller => "controller",
        SourceObjectKind::Layout => "layout",
        SourceObjectKind::Patch => "patch",
        SourceObjectKind::FixtureDefinition => "fixture definition",
        SourceObjectKind::Curve => "curve",
        SourceObjectKind::Gradient => "gradient",
        SourceObjectKind::Sequence => "sequence",
        SourceObjectKind::EffectDefinition => "effect",
        SourceObjectKind::OperatorDefinition => "operator",
        SourceObjectKind::EffectInstance => "effect instance",
    }
}
