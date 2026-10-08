//! What a name refers to, wherever it is written: definitions, references and
//! renames across data documents and scripts.
use donder_language::NameKind;
use donder_language::analysis::TokenClass;
use donder_language::analysis::{ScriptAnalysis, SymbolKind, analyze_script, data_tokens};
use donder_language::compiler::TextSpan;
use donder_project_io::{LinkTarget, ScriptMember};

use crate::workspace::{Kind, Workspace, kind};

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A name other documents may use: a data name, an import alias, or a
    /// script declaration or member.
    Link(LinkTarget),
    /// A name only its script sees: a function, argument, `let` or index, or
    /// any symbol of a script outside the project.
    Local {
        uri: String,
        span: TextSpan,
        kind: SymbolKind,
    },
}

/// A script symbol as a target, linked when the script is in the project.
pub fn script_symbol_target(
    workspace: &Workspace,
    uri: &str,
    analysis: &ScriptAnalysis,
    symbol: usize,
) -> Target {
    let local = || Target::Local {
        uri: uri.to_string(),
        span: analysis.symbols[symbol].span,
        kind: analysis.symbols[symbol].kind,
    };
    let entry = &analysis.symbols[symbol];
    let parent = |index: usize| analysis.symbols[index].parent;
    let member = match entry.kind {
        SymbolKind::Effect | SymbolKind::Operator => {
            Some((entry.name.clone(), ScriptMember::Declaration))
        }
        SymbolKind::Param => parent(symbol).map(|declaration| {
            (
                analysis.symbols[declaration].name.clone(),
                ScriptMember::Param(entry.name.clone()),
            )
        }),
        SymbolKind::Input => parent(symbol).map(|declaration| {
            (
                analysis.symbols[declaration].name.clone(),
                ScriptMember::Input(entry.name.clone()),
            )
        }),
        SymbolKind::EnumOption => parent(symbol).and_then(|param| {
            parent(param).map(|declaration| {
                (
                    analysis.symbols[declaration].name.clone(),
                    ScriptMember::Option {
                        param: analysis.symbols[param].name.clone(),
                        option: entry.name.clone(),
                    },
                )
            })
        }),
        _ => None,
    };
    match (workspace.document_id(uri), member) {
        (Some(document), Some((declaration, member))) => Target::Link(LinkTarget::Script {
            document,
            declaration,
            member,
        }),
        _ => local(),
    }
}

/// The symbol a script target names in its script's analysis.
pub fn script_member(
    analysis: &ScriptAnalysis,
    declaration: &str,
    member: &ScriptMember,
) -> Option<usize> {
    let declared = analysis.symbols.iter().position(|symbol| {
        matches!(symbol.kind, SymbolKind::Effect | SymbolKind::Operator)
            && symbol.name == declaration
    })?;
    let child = |kind: SymbolKind, name: &str, parent: usize| {
        analysis.symbols.iter().position(|symbol| {
            symbol.kind == kind && symbol.name == name && symbol.parent == Some(parent)
        })
    };
    match member {
        ScriptMember::Declaration => Some(declared),
        ScriptMember::Param(param) => child(SymbolKind::Param, param, declared),
        ScriptMember::Input(input) => child(SymbolKind::Input, input, declared),
        ScriptMember::Option { param, option } => {
            let param = child(SymbolKind::Param, param, declared)?;
            child(SymbolKind::EnumOption, option, param)
        }
    }
}

/// The target named at `offset` in a document.
pub fn target_at(workspace: &Workspace, uri: &str, text: &str, offset: usize) -> Option<Target> {
    match kind(uri) {
        Kind::Script => {
            let analysis = analyze_script(text);
            let (_, target) = analysis.target_at(offset)?;
            match target {
                donder_language::analysis::ScriptTarget::Symbol(symbol) => {
                    Some(script_symbol_target(workspace, uri, &analysis, symbol))
                }
                _ => None,
            }
        }
        Kind::Data => {
            let document = workspace.document_id(uri);
            if let (Some(document), Some(check)) = (&document, &workspace.check)
                && let Some(link) = check.report.index.link_at(document, offset)
            {
                return Some(Target::Link(link.target.clone()));
            }
            // A declared name: a declaration, an item, an import alias.
            let token = data_tokens(text).into_iter().find(|token| {
                token.declaration && token.span.start <= offset && offset <= token.span.end
            })?;
            let document = document?;
            Some(Target::Link(if token.class == TokenClass::Namespace {
                LinkTarget::Import {
                    document,
                    span: token.span,
                }
            } else {
                LinkTarget::Data {
                    document,
                    span: token.span,
                }
            }))
        }
        Kind::Other => None,
    }
}

/// Where a target is declared.
pub fn definition(workspace: &Workspace, target: &Target) -> Option<(String, TextSpan)> {
    match target {
        Target::Local { uri, span, .. } => Some((uri.clone(), *span)),
        Target::Link(LinkTarget::Data { document, span })
        | Target::Link(LinkTarget::Import { document, span }) => {
            Some((workspace.document_uri(document)?, *span))
        }
        Target::Link(LinkTarget::Script {
            document,
            declaration,
            member,
        }) => {
            let uri = workspace.document_uri(document)?;
            let analysis = analyze_script(&workspace.text(&uri)?);
            let symbol = script_member(&analysis, declaration, member)?;
            Some((uri, analysis.symbols[symbol].span))
        }
    }
}

/// The declaration and every use of a target.
pub fn occurrences(workspace: &Workspace, target: &Target) -> Vec<(String, TextSpan)> {
    let mut found = Vec::new();
    match target {
        Target::Local { uri, span, .. } => {
            if let Some(text) = workspace.text(uri) {
                let analysis = analyze_script(&text);
                if let Some(symbol) = analysis
                    .symbols
                    .iter()
                    .position(|symbol| symbol.span == *span)
                {
                    found.extend(
                        analysis
                            .occurrences(symbol)
                            .into_iter()
                            .map(|span| (uri.clone(), span)),
                    );
                }
            }
            return found;
        }
        Target::Link(LinkTarget::Script {
            document,
            declaration,
            member,
        }) => {
            if let Some(uri) = workspace.document_uri(document)
                && let Some(text) = workspace.text(&uri)
            {
                let analysis = analyze_script(&text);
                if let Some(symbol) = script_member(&analysis, declaration, member) {
                    found.extend(
                        analysis
                            .occurrences(symbol)
                            .into_iter()
                            .map(|span| (uri.clone(), span)),
                    );
                }
            }
        }
        Target::Link(_) => found.extend(definition(workspace, target)),
    }
    if let (Target::Link(link), Some(check)) = (target, &workspace.check) {
        for use_ in check.report.index.links_to(link) {
            if let Some(uri) = workspace.document_uri(&use_.document) {
                found.push((uri, use_.span));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.start.cmp(&b.1.start)));
    found.dedup();
    found
}

/// Whether `name` may replace the target's name.
pub fn valid_name(target: &Target, name: &str) -> Result<(), String> {
    let kind = match target {
        Target::Link(LinkTarget::Data { .. } | LinkTarget::Import { .. }) => NameKind::Object,
        Target::Link(LinkTarget::Script { member, .. }) => match member {
            ScriptMember::Declaration => NameKind::Definition,
            ScriptMember::Param(_) | ScriptMember::Input(_) => NameKind::Member,
            ScriptMember::Option { .. } => NameKind::EnumOption,
        },
        Target::Local { kind, .. } => kind.name_kind(),
    };
    kind.check(name).map_err(|error| error.message(name))
}
