//! Data documents: text to typed declarations and back. Parsing and printing
//! are the shared project language's (`donder_language::data`); this module
//! knows which declarations a document may hold.
pub(crate) mod encode;
pub(crate) mod save;
pub(crate) mod text_cache;
pub(crate) mod types;

use donder_language::compiler::{Diagnostic, TextSpan};
use donder_language::data::{Choice, Data, Decoder, Record, Refers, Schema, declaration};
use donder_language::data::{DataDeclaration, DataDocument, DataField, DataImport, Spanned};
use donder_model::ObjectKind;
use donder_runtime_types::Identifier;

/// The declaration types, in schema order. Each is a `types` record and the
/// `ObjectKind` of the same name; everything a document knows about its
/// declarations follows from this list.
macro_rules! declarations {
    ($($ty:ident),+ $(,)?) => {
        /// One top-level declaration of a data document.
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum Declaration {
            $($ty(types::$ty),)+
        }

        impl Declaration {
            pub(crate) fn kind(&self) -> ObjectKind {
                match self {
                    $(Self::$ty(_) => ObjectKind::$ty,)+
                }
            }

            fn encode(&self, name: &Identifier) -> DataDeclaration {
                match self {
                    $(Self::$ty(value) => declaration(name, value),)+
                }
            }

            /// The declaration of type `ty`, or `None` with a diagnostic.
            fn decode(
                ty: &Spanned<Identifier>,
                fields: &Spanned<Vec<DataField>>,
                decoder: &mut Decoder,
            ) -> Option<Self> {
                match ty.value.as_str() {
                    $(types::$ty::TYPE => types::$ty::decode_fields(fields, decoder).map(Self::$ty),)+
                    other => {
                        let names = DECLARATION_TYPE_NAMES.map(|name| format!("`{name}`"));
                        let (last, rest) = names.split_last().unwrap_or_else(|| unreachable!());
                        decoder.error(
                            ty.span,
                            format!(
                                "`{other}` is not a declaration type; use {} or {last}",
                                rest.join(", ")
                            ),
                        );
                        None
                    }
                }
            }
        }

        /// Every type a data document may hold, with the declaration types first.
        pub fn document_schema() -> Schema {
            let mut schema = Schema::default();
            $(types::$ty::shape(&mut schema);)+
            schema
        }

        /// The types a data document declares at its top level.
        pub const DECLARATION_TYPE_NAMES: [&str; [$(stringify!($ty)),+].len()] =
            [$(types::$ty::TYPE),+];
    };
}

declarations! {
    Project,
    Setup,
    Controller,
    InputDevice,
    Layout,
    Patch,
    FixtureDefinition,
    Sequence,
    Console,
    Curve,
    Gradient,
}

/// The types whose objects a reference names: the declaration type of a
/// `Declaration` reference, or whose members a `Fixture` or `Control`
/// reference names as `declaration.member`, or the types of the objects of
/// the same document a `Layer`, `Clip`, `Marks` or `Node` reference names.
/// Effects and operators are scripts', not data types.
pub fn reference_targets(refers: Refers) -> Vec<&'static str> {
    match refers {
        Refers::Declaration(ty) => vec![ty],
        Refers::Effect | Refers::Operator => Vec::new(),
        Refers::Fixture => vec![types::Layout::TYPE],
        Refers::Control => vec![types::InputDevice::TYPE],
        Refers::Layer => vec![types::Layer::TYPE],
        Refers::Clip => vec![types::Clip::TYPE, types::ConsoleClip::TYPE],
        Refers::Marks => vec![types::MarkCollection::TYPE, types::LiveMarks::TYPE],
        Refers::Node => vec![types::Node::TYPE],
    }
}

/// What a parameter value of type `ty` names in a clip's or node's `params`.
pub fn param_refers(ty: &donder_runtime_types::Type) -> Option<Refers> {
    use donder_runtime_types::Type;
    match ty {
        Type::Marks => Some(Refers::Marks),
        Type::Curve => Some(Refers::Declaration(types::Curve::TYPE)),
        Type::Gradient => Some(Refers::Declaration(types::Gradient::TYPE)),
        _ => None,
    }
}

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
        let decoded = Declaration::decode(&item.ty, &item.fields, &mut decoder);
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

/// The canonical text of a data document; each splice prints a declaration's
/// list field from items rendered beforehand.
pub(crate) fn write(
    imports: Vec<DataImport>,
    declarations: &[(Identifier, Declaration)],
    splices: &[donder_language::data::ListSplice<'_>],
) -> String {
    donder_language::data::print_spliced(
        &DataDocument {
            imports,
            declarations: declarations
                .iter()
                .map(|(name, declaration)| declaration.encode(name))
                .collect(),
        },
        splices,
    )
}
