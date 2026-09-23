use crate::dto::{DiagnosticSeverity, ProjectDiagnostic, RelatedDiagnosticLocation};
use donder_project_io::{IoDiagnostic, IoDiagnosticSeverity, ProjectCheckReport};

pub(crate) fn project_diagnostic(diagnostic: &IoDiagnostic) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: diagnostic.path.to_string(),
        range: diagnostic
            .range
            .as_ref()
            .map(|range| crate::dto::TextRange {
                start: crate::dto::TextPosition {
                    line: range.start.line,
                    character: range.start.character,
                },
                end: crate::dto::TextPosition {
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
                range: related.range.as_ref().map(|range| crate::dto::TextRange {
                    start: crate::dto::TextPosition {
                        line: range.start.line,
                        character: range.start.character,
                    },
                    end: crate::dto::TextPosition {
                        line: range.end.line,
                        character: range.end.character,
                    },
                }),
                message: related.message.clone(),
            })
            .collect(),
    }
}

pub(crate) fn project_diagnostics(report: &ProjectCheckReport) -> Vec<ProjectDiagnostic> {
    report.diagnostics.iter().map(project_diagnostic).collect()
}
