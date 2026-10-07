//! Data documents: text to typed declarations and back. Parsing and printing
//! are the shared project language's (`donder_language::data`); this module
//! knows which declarations a document may hold.
pub(crate) mod encode;
pub(crate) mod save;
pub(crate) mod types;

use crate::source::SourceObjectKind;
use donder_language::data::schema::{Data, Decoder, Record, Schema, declaration};
use donder_language::data::tree::{DataDeclaration, DataDocument, DataImport, Spanned};
use donder_language::dsl::{Diagnostic, Identifier, TextSpan};

/// One top-level declaration of a data document.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Declaration {
    Project(types::Project),
    Setup(types::Setup),
    Controller(types::Controller),
    Layout(types::Layout),
    Patch(types::Patch),
    FixtureDefinition(types::FixtureDefinition),
    Sequence(types::Sequence),
    Curve(types::Curve),
    Gradient(types::Gradient),
}

impl Declaration {
    pub(crate) fn kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Setup(_) => SourceObjectKind::Setup,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Gradient(_) => SourceObjectKind::Gradient,
        }
    }

    pub(crate) fn encode(&self, name: &Identifier) -> DataDeclaration {
        match self {
            Self::Project(value) => declaration(name, value),
            Self::Setup(value) => declaration(name, value),
            Self::Controller(value) => declaration(name, value),
            Self::Layout(value) => declaration(name, value),
            Self::Patch(value) => declaration(name, value),
            Self::FixtureDefinition(value) => declaration(name, value),
            Self::Sequence(value) => declaration(name, value),
            Self::Curve(value) => declaration(name, value),
            Self::Gradient(value) => declaration(name, value),
        }
    }
}

/// Every type a data document may hold, with the declaration types first.
pub fn document_schema() -> Schema {
    let mut schema = Schema::default();
    types::Project::shape(&mut schema);
    types::Setup::shape(&mut schema);
    types::Controller::shape(&mut schema);
    types::Layout::shape(&mut schema);
    types::Patch::shape(&mut schema);
    types::FixtureDefinition::shape(&mut schema);
    types::Sequence::shape(&mut schema);
    types::Curve::shape(&mut schema);
    types::Gradient::shape(&mut schema);
    schema
}

/// The types a data document declares at its top level.
pub const DECLARATION_TYPE_NAMES: [&str; 9] = [
    "Project",
    "Setup",
    "Controller",
    "Layout",
    "Patch",
    "FixtureDefinition",
    "Sequence",
    "Curve",
    "Gradient",
];

const DECLARATION_TYPES: &str = "`Project`, `Setup`, `Controller`, `Layout`, `Patch`, `FixtureDefinition`, `Sequence`, `Curve` or `Gradient`";

/// A parsed data document: its imports and declarations by name, in order.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParsedDocument {
    pub(crate) imports: Vec<DataImport>,
    pub(crate) declarations: Vec<(Spanned<Identifier>, TextSpan, Declaration)>,
}

/// Parse and decode a data document. Every syntax and schema error is
/// reported; declarations that decode are kept even when others fail.
pub(crate) fn read(source: &str) -> (ParsedDocument, Vec<Diagnostic>) {
    let (tree, mut diagnostics) = donder_language::data::parse(source);
    let mut decoder = Decoder::default();
    let mut document = ParsedDocument {
        imports: tree.imports,
        declarations: Vec::new(),
    };
    for item in &tree.declarations {
        let fields = &item.fields;
        let decoded = match item.ty.value.as_str() {
            types::Project::TYPE => {
                types::Project::decode_fields(fields, &mut decoder).map(Declaration::Project)
            }
            types::Setup::TYPE => {
                types::Setup::decode_fields(fields, &mut decoder).map(Declaration::Setup)
            }
            types::Controller::TYPE => {
                types::Controller::decode_fields(fields, &mut decoder).map(Declaration::Controller)
            }
            types::Layout::TYPE => {
                types::Layout::decode_fields(fields, &mut decoder).map(Declaration::Layout)
            }
            types::Patch::TYPE => {
                types::Patch::decode_fields(fields, &mut decoder).map(Declaration::Patch)
            }
            types::FixtureDefinition::TYPE => {
                types::FixtureDefinition::decode_fields(fields, &mut decoder)
                    .map(Declaration::FixtureDefinition)
            }
            types::Sequence::TYPE => {
                types::Sequence::decode_fields(fields, &mut decoder).map(Declaration::Sequence)
            }
            types::Curve::TYPE => {
                types::Curve::decode_fields(fields, &mut decoder).map(Declaration::Curve)
            }
            types::Gradient::TYPE => {
                types::Gradient::decode_fields(fields, &mut decoder).map(Declaration::Gradient)
            }
            other => {
                decoder.error(
                    item.ty.span,
                    format!("`{other}` is not a declaration type; use {DECLARATION_TYPES}"),
                );
                None
            }
        };
        if let Some(decoded) = decoded {
            let span = item.ty.span.to(item.fields.span);
            document
                .declarations
                .push((item.name.clone(), span, decoded));
        }
    }
    let mut names = Vec::<&Identifier>::new();
    for (name, _, _) in &document.declarations {
        if names.contains(&&name.value) {
            decoder.error(
                name.span,
                format!("`{}` is declared twice", name.value.as_str()),
            );
        }
        names.push(&name.value);
    }
    diagnostics.extend(decoder.diagnostics);
    diagnostics.sort_by_key(|diagnostic| (diagnostic.span.start, diagnostic.span.end));
    (document, diagnostics)
}

/// The canonical text of a data document.
pub(crate) fn write(
    imports: Vec<DataImport>,
    declarations: &[(Identifier, Declaration)],
) -> String {
    donder_language::data::print(&DataDocument {
        imports,
        declarations: declarations
            .iter()
            .map(|(name, declaration)| declaration.encode(name))
            .collect(),
    })
}
