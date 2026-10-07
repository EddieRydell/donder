//! Where each reference in the project's data documents points, recorded while
//! loading resolves them: the language server's definitions, references and
//! renames. Script declarations are named, not located, and the server finds
//! them in its script analysis.
use donder_language::dsl::TextSpan;
use donder_language::identity::DocumentId;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectIndex {
    pub links: Vec<Link>,
}

/// One name in a data document and the object it names.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub document: DocumentId,
    pub span: TextSpan,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkTarget {
    /// A name declared in a data document: a declaration, an item, a port.
    Data {
        document: DocumentId,
        span: TextSpan,
    },
    /// An import's alias.
    Import {
        document: DocumentId,
        span: TextSpan,
    },
    /// An effect or operator, or one of its members.
    Script {
        document: DocumentId,
        declaration: String,
        member: ScriptMember,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScriptMember {
    Declaration,
    Param(String),
    Option { param: String, option: String },
    Input(String),
}

impl ProjectIndex {
    /// The link whose name covers `offset` in `document`.
    pub fn link_at(&self, document: &DocumentId, offset: usize) -> Option<&Link> {
        self.links.iter().find(|link| {
            &link.document == document && link.span.start <= offset && offset <= link.span.end
        })
    }

    /// Every link to `target`.
    pub fn links_to<'a>(&'a self, target: &'a LinkTarget) -> impl Iterator<Item = &'a Link> {
        self.links.iter().filter(move |link| &link.target == target)
    }
}
