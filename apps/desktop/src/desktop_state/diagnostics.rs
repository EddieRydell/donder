use donder_project_io::{IoDiagnostic, IoDiagnosticSeverity, IoFix, ProjectCheckReport};
use donder_sequence_api::{
    DiagnosticSeverity, DocumentInclusion, ProjectDiagnostic, RelatedDiagnosticLocation,
};

pub(crate) fn project_diagnostic(diagnostic: &IoDiagnostic) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: diagnostic.path.to_string(),
        range: diagnostic
            .range
            .as_ref()
            .map(|range| donder_sequence_api::TextRange {
                start: donder_sequence_api::TextPosition {
                    line: range.start.line,
                    character: range.start.character,
                },
                end: donder_sequence_api::TextPosition {
                    line: range.end.line,
                    character: range.end.character,
                },
            }),
        severity: match diagnostic.severity {
            IoDiagnosticSeverity::Error => DiagnosticSeverity::Error,
            IoDiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        },
        code: diagnostic.code.as_str().to_string(),
        message: diagnostic.message.clone(),
        detail: diagnostic.detail.clone(),
        related: diagnostic
            .related
            .iter()
            .map(|related| RelatedDiagnosticLocation {
                path: related.path.to_string(),
                range: related
                    .range
                    .as_ref()
                    .map(|range| donder_sequence_api::TextRange {
                        start: donder_sequence_api::TextPosition {
                            line: range.start.line,
                            character: range.start.character,
                        },
                        end: donder_sequence_api::TextPosition {
                            line: range.end.line,
                            character: range.end.character,
                        },
                    }),
                message: related.message.clone(),
            })
            .collect(),
        inclusion: match &diagnostic.fix {
            Some(IoFix::Include(inclusion)) => Some(DocumentInclusion {
                document: inclusion.document.to_string(),
                alias: inclusion.alias.as_str().to_string(),
                sequences: inclusion
                    .sequences
                    .iter()
                    .map(|sequence| sequence.as_str().to_string())
                    .collect(),
            }),
            Some(IoFix::Replace(_)) | None => None,
        },
    }
}

pub(crate) fn project_diagnostics(report: &ProjectCheckReport) -> Vec<ProjectDiagnostic> {
    report.diagnostics.iter().map(project_diagnostic).collect()
}
