//! Project loading. Documents are read and indexed, imports followed, and
//! declarations then resolved into typed state, where every object gets a
//! fresh session identity.
mod resolve;

use std::cell::RefCell;
use std::fs;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::controller::ControllerId;
use donder_language::data::schema::NO_SPAN;
use donder_language::dsl::{TextSpan, compile_script};
use donder_language::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use donder_language::fixture::FixtureDefinitionId;
use donder_language::identity::{DocumentId, SourceIdentity};
use donder_language::imports::{ImportAlias, ImportDeclaration, ImportSource, SourceReference};
use donder_language::layout::LayoutId;
use donder_language::model::{
    DonderProject, ProjectData, ProjectDefinitionStores, ProjectId, ProjectRoot,
};
use donder_language::operator::{OperatorDefinitionId, custom_operator_definition};
use donder_language::ownership::ValueSource;
use donder_language::patch::PatchId;
use donder_language::sequence::SequenceId;
use donder_language::setup::SetupId;
use indexmap::{IndexMap, IndexSet};

use crate::diagnostics::{byte_range, data_diagnostic, dsl_diagnostic};
use crate::document::{self, Declaration};
use crate::imports::ParsedImport;
use crate::index::{Link, LinkTarget, ScriptMember};
use crate::source::{
    ProjectSession, ReferencedAsset, SourceDocument, SourceDocumentKind, SourceObjectId,
    SourceObjectKind, SourceProject,
};
use crate::{IoDiagnosticCode, LoadProjectError, TextRange};
use resolve::DomainResolver;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResolvedObject {
    Project(ProjectId),
    Setup(SetupId),
    Controller(ControllerId),
    Layout(LayoutId),
    Patch(PatchId),
    FixtureDefinition(FixtureDefinitionId),
    Curve(CurveId),
    Gradient(GradientId),
    Sequence(SequenceId),
    EffectDefinition(EffectDefinitionId),
    OperatorDefinition(OperatorDefinitionId),
}

impl ResolvedObject {
    fn new(kind: SourceObjectKind, identity: SourceIdentity) -> Option<Self> {
        Some(match kind {
            SourceObjectKind::Project => Self::Project(ProjectId(identity)),
            SourceObjectKind::Setup => Self::Setup(SetupId(identity.into())),
            SourceObjectKind::Controller => Self::Controller(ControllerId(identity.into())),
            SourceObjectKind::Layout => Self::Layout(LayoutId(identity.into())),
            SourceObjectKind::Patch => Self::Patch(PatchId(identity.into())),
            SourceObjectKind::FixtureDefinition => {
                Self::FixtureDefinition(FixtureDefinitionId(identity))
            }
            SourceObjectKind::Curve => Self::Curve(CurveId(identity)),
            SourceObjectKind::Gradient => Self::Gradient(GradientId(identity)),
            SourceObjectKind::Sequence => Self::Sequence(SequenceId(identity.into())),
            SourceObjectKind::EffectDefinition => {
                Self::EffectDefinition(EffectDefinitionId(identity))
            }
            SourceObjectKind::OperatorDefinition => {
                Self::OperatorDefinition(OperatorDefinitionId(identity))
            }
            SourceObjectKind::EffectInstance => return None,
        })
    }

    pub(crate) fn source_identity(&self) -> &SourceIdentity {
        match self {
            Self::Project(id) => &id.0,
            Self::Setup(id) => id.0.root_source(),
            Self::Controller(id) => id.0.root_source(),
            Self::Layout(id) => id.0.root_source(),
            Self::Patch(id) => id.0.root_source(),
            Self::FixtureDefinition(id) => &id.0,
            Self::Curve(id) => &id.0,
            Self::Gradient(id) => &id.0,
            Self::Sequence(id) => id.0.root_source(),
            Self::EffectDefinition(id) => &id.0,
            Self::OperatorDefinition(id) => &id.0,
        }
    }

    pub(crate) fn source_kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Setup(_) => SourceObjectKind::Setup,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Gradient(_) => SourceObjectKind::Gradient,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::EffectDefinition(_) => SourceObjectKind::EffectDefinition,
            Self::OperatorDefinition(_) => SourceObjectKind::OperatorDefinition,
        }
    }
}

/// A data document's text and its declarations by name.
pub(crate) struct DataDocument {
    pub(crate) text: String,
    pub(crate) declarations: IndexMap<String, (TextSpan, Declaration)>,
    /// Each declaration's name span.
    pub(crate) names: IndexMap<String, TextSpan>,
}

pub(super) struct Loader {
    pub(crate) workspace: crate::ProjectWorkspace,
    pub(crate) entrypoint: DocumentId,
    pub(crate) documents: IndexMap<DocumentId, SourceDocument>,
    pub(crate) data: IndexMap<DocumentId, Arc<DataDocument>>,
    pub(crate) visible_objects: IndexMap<DocumentId, IndexMap<SourceReference, ResolvedObject>>,
    pub(crate) import_locations: IndexMap<DocumentId, Vec<ParsedImport>>,
    pub(crate) loading_documents: IndexSet<DocumentId>,
    pub(crate) definitions: ProjectDefinitionStores,
    pub(crate) referenced_assets: Vec<ReferencedAsset>,
    pub(crate) next_asset_id: u32,
    pub(crate) checked_scripts: IndexSet<Utf8PathBuf>,
    pub(crate) source_overrides: IndexMap<DocumentId, String>,
    /// Where each resolved name points, for the language server.
    pub(crate) links: RefCell<Vec<Link>>,
}

impl Loader {
    pub(super) fn new(workspace: crate::ProjectWorkspace) -> Result<Self, LoadProjectError> {
        workspace
            .metadata
            .validate()
            .map_err(|message| LoadProjectError::InvalidDocument {
                path: crate::PROJECT_ROOT_FILE.into(),
                range: None,
                message,
            })?;
        let entrypoint = DocumentId::new(
            workspace.metadata.project_id,
            crate::PROJECT_ROOT_FILE.into(),
        );
        Ok(Self {
            workspace,
            entrypoint,
            documents: IndexMap::new(),
            data: IndexMap::new(),
            visible_objects: IndexMap::new(),
            import_locations: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: ProjectDefinitionStores::default(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
            checked_scripts: IndexSet::new(),
            source_overrides: IndexMap::new(),
            links: RefCell::new(Vec::new()),
        })
    }

    pub(super) fn load(&mut self) -> Result<ProjectSession, LoadProjectError> {
        let entrypoint = self.entrypoint.clone();
        self.load_document(&entrypoint)?;
        self.build_document_scopes()?;
        let mut project = self.resolve_project(&entrypoint)?;
        self.resolve_loaded_objects(&mut project)?;
        let project =
            DonderProject::try_new(project).map_err(|error| LoadProjectError::InvalidDocument {
                path: entrypoint.path().to_path_buf(),
                range: None,
                message: format!("project validation failed: {error:?}"),
            })?;
        Ok(ProjectSession {
            project,
            source: SourceProject {
                workspace: self.workspace.clone(),
                entrypoint: Some(entrypoint),
                documents: std::mem::take(&mut self.documents),
                referenced_assets: std::mem::take(&mut self.referenced_assets),
            },
        })
    }

    /// The text range of `span` in a loaded data document.
    pub(crate) fn range(&self, document: &DocumentId, span: TextSpan) -> Option<TextRange> {
        (span != NO_SPAN)
            .then(|| self.data.get(document))
            .flatten()
            .map(|data| byte_range(&data.text, span.start, span.end))
    }

    pub(crate) fn invalid(
        &self,
        document: &DocumentId,
        span: TextSpan,
        message: impl Into<String>,
    ) -> LoadProjectError {
        LoadProjectError::InvalidDocument {
            path: document.path().to_path_buf(),
            range: self.range(document, span),
            message: message.into(),
        }
    }

    fn resolve_loaded_objects(
        &mut self,
        project: &mut ProjectData,
    ) -> Result<(), LoadProjectError> {
        // Every declared object gets typed state, including objects nothing
        // uses, so saving can always print every declaration.
        let objects: Vec<_> = self
            .visible_objects
            .iter()
            .flat_map(|(document, visible)| {
                visible
                    .iter()
                    .filter(|(key, _)| matches!(key, SourceReference::Local(_)))
                    .map(|(_, object)| (document.clone(), object.clone()))
            })
            .collect();
        let entrypoint = self.entrypoint.clone();
        let mut resolver = DomainResolver {
            loader: self,
            project,
        };
        for (document, object) in objects {
            match object {
                ResolvedObject::Project(id) => {
                    if entrypoint != document || resolver.project.root.id != id {
                        let span = resolver.loader.declaration_span(&id.0);
                        return Err(resolver.loader.invalid(
                            &document,
                            span,
                            format!("a project is declared only in {}", crate::PROJECT_ROOT_FILE),
                        ));
                    }
                }
                ResolvedObject::Setup(id) => resolver.resolve_setup(&id)?,
                ResolvedObject::Controller(id) => resolver.resolve_controller(&id)?,
                ResolvedObject::Layout(id) => resolver.resolve_layout(&id)?,
                ResolvedObject::Patch(id) => resolver.resolve_patch(&id)?,
                ResolvedObject::FixtureDefinition(id) => resolver.resolve_fixture(&id)?,
                ResolvedObject::Sequence(id) => resolver.resolve_sequence(&id)?,
                ResolvedObject::Curve(_)
                | ResolvedObject::Gradient(_)
                | ResolvedObject::EffectDefinition(_)
                | ResolvedObject::OperatorDefinition(_) => {}
            }
        }
        Ok(())
    }

    pub(super) fn load_document(
        &mut self,
        document_id: &DocumentId,
    ) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(document_id) {
            return Ok(());
        }
        if self.loading_documents.contains(document_id) {
            // Import cycles are valid: each document indexes its declarations
            // before following imports, so the active document is visible.
            return Ok(());
        }
        self.loading_documents.insert(document_id.clone());
        let absolute = self.absolute_document_path(document_id)?;
        let result = match crate::source_document_format(document_id.path()) {
            crate::SourceDocumentFormat::Script => self.load_script(document_id, &absolute),
            crate::SourceDocumentFormat::Data => self.load_data(document_id, &absolute),
            crate::SourceDocumentFormat::Other => Err(LoadProjectError::InvalidDocument {
                path: document_id.path().to_path_buf(),
                range: None,
                message: "not a Donder document".into(),
            }),
        };
        self.loading_documents.shift_remove(document_id);
        result
    }

    pub(super) fn absolute_document_path(
        &self,
        document_id: &DocumentId,
    ) -> Result<Utf8PathBuf, LoadProjectError> {
        if document_id.module_id() != self.workspace.metadata.project_id {
            return Err(LoadProjectError::InvalidDocument {
                path: document_id.path().to_owned(),
                range: None,
                message: "Document belongs to another project".into(),
            });
        }
        crate::validate_document_path(document_id.path().as_str()).map_err(|message| {
            LoadProjectError::InvalidDocument {
                path: document_id.path().to_owned(),
                range: None,
                message,
            }
        })?;
        let path = self.workspace.root.join(document_id.path());
        if path.exists() {
            let absolute = path
                .canonicalize_utf8()
                .map_err(|source| LoadProjectError::Io {
                    path: path.clone(),
                    source,
                })?;
            if !absolute.starts_with(&self.workspace.root) {
                return Err(LoadProjectError::InvalidDocument {
                    path: document_id.path().to_owned(),
                    range: None,
                    message: "Document escapes the project root".into(),
                });
            }
        }
        Ok(path)
    }

    fn read_source(
        &self,
        document_id: &DocumentId,
        absolute: &Utf8Path,
    ) -> Result<String, LoadProjectError> {
        if let Some(source) = self.source_overrides.get(document_id) {
            return Ok(source.clone());
        }
        fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })
    }

    fn load_script(
        &mut self,
        document_id: &DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
        self.checked_scripts.insert(relative.to_path_buf());
        let script =
            compile_script(&source).map_err(|diagnostics| LoadProjectError::InvalidScript {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::ScriptCompile,
                        )
                    })
                    .collect(),
            })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        let declared = script
            .effects
            .into_iter()
            .map(|effect| {
                let name = effect.name().clone();
                let id = EffectDefinitionId(SourceIdentity::from_document(
                    document_id.clone(),
                    name.as_str().to_string(),
                ));
                self.definitions
                    .effects
                    .insert(id.clone(), EffectDefinition::custom(id.clone(), effect));
                (name, ResolvedObject::EffectDefinition(id))
            })
            .collect::<Vec<_>>();
        let declared = declared
            .into_iter()
            .chain(script.operators.into_iter().map(|operator| {
                let name = operator.name().clone();
                let id = OperatorDefinitionId(SourceIdentity::from_document(
                    document_id.clone(),
                    name.as_str().to_string(),
                ));
                self.definitions
                    .operators
                    .insert(id.clone(), custom_operator_definition(id.clone(), operator));
                (name, ResolvedObject::OperatorDefinition(id))
            }));
        for (name, object) in declared.collect::<Vec<_>>() {
            objects.push(SourceObjectId {
                kind: object.source_kind(),
                id: name.as_str().to_string(),
            });
            if visible
                .insert(SourceReference::Local(name.clone()), object)
                .is_some()
            {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: format!("`{}` is declared twice", name.as_str()),
                });
            }
        }
        self.visible_objects.insert(document_id.clone(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Script { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.import_locations
            .insert(document_id.clone(), Vec::new());
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    fn load_data(
        &mut self,
        document_id: &DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let text = self.read_source(document_id, absolute)?;
        let (parsed, diagnostics) = document::read(&text);
        if !diagnostics.is_empty() {
            return Err(LoadProjectError::InvalidData {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| data_diagnostic(relative, &text, diagnostic))
                    .collect(),
            });
        }
        let imports = parsed
            .imports
            .iter()
            .map(|import| {
                let range = |span: TextSpan| Some(byte_range(&text, span.start, span.end));
                Ok(ParsedImport {
                    declaration: ImportDeclaration {
                        alias: ImportAlias::new(import.alias.value.as_str()).map_err(
                            |message| LoadProjectError::InvalidDocument {
                                path: relative.to_path_buf(),
                                range: range(import.alias.span),
                                message,
                            },
                        )?,
                        source: ImportSource::LocalDocuments {
                            documents: import
                                .paths
                                .iter()
                                .map(|path| Utf8PathBuf::from(&path.value))
                                .collect(),
                        },
                    },
                    range: range(import.alias.span),
                    alias_span: import.alias.span,
                    source_ranges: import.paths.iter().map(|path| range(path.span)).collect(),
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        let mut declarations = IndexMap::new();
        let mut names = IndexMap::new();
        for (name, span, declaration) in parsed.declarations {
            names.insert(name.value.as_str().to_string(), name.span);
            let identity =
                SourceIdentity::from_document(document_id.clone(), name.value.as_str().to_string());
            let kind = declaration.kind();
            match &declaration {
                Declaration::Curve(curve) => {
                    let value = resolve::curve(&curve.points).map_err(|message| {
                        LoadProjectError::InvalidDocument {
                            path: relative.to_path_buf(),
                            range: Some(byte_range(&text, span.start, span.end)),
                            message,
                        }
                    })?;
                    self.definitions.curves.insert(
                        CurveId(identity.clone()),
                        CurveDefinition {
                            description: curve.description.clone(),
                            curve: value,
                        },
                    );
                }
                Declaration::Gradient(gradient) => {
                    let value = resolve::gradient(&gradient.stops).map_err(|message| {
                        LoadProjectError::InvalidDocument {
                            path: relative.to_path_buf(),
                            range: Some(byte_range(&text, span.start, span.end)),
                            message,
                        }
                    })?;
                    self.definitions.gradients.insert(
                        GradientId(identity.clone()),
                        GradientDefinition {
                            description: gradient.description.clone(),
                            gradient: value,
                        },
                    );
                }
                _ => {}
            }
            let object = ResolvedObject::new(kind.clone(), identity)
                .unwrap_or_else(|| unreachable!("declarations are source objects"));
            objects.push(SourceObjectId {
                kind,
                id: name.value.as_str().to_string(),
            });
            visible.insert(SourceReference::Local(name.value.clone()), object);
            declarations.insert(name.value.as_str().to_string(), (span, declaration));
        }
        self.visible_objects.insert(document_id.clone(), visible);
        self.data.insert(
            document_id.clone(),
            Arc::new(DataDocument {
                text,
                declarations,
                names,
            }),
        );
        let import_edges = self.load_imports(document_id, &imports)?;
        let document = SourceDocument::new(import_edges, objects, SourceDocumentKind::Data)
            .map_err(|message| LoadProjectError::InvalidDocument {
                path: relative.to_path_buf(),
                range: None,
                message,
            })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    /// Record that the name at `span` in `document` points at `target`.
    pub(crate) fn link(&self, document: &DocumentId, span: TextSpan, target: LinkTarget) {
        if span != NO_SPAN {
            self.links.borrow_mut().push(Link {
                document: document.clone(),
                span,
                target,
            });
        }
    }

    /// Where a declared object's name is: a data declaration's name, or a
    /// script declaration by name.
    pub(crate) fn declared_target(&self, object: &ResolvedObject) -> Option<LinkTarget> {
        let identity = object.source_identity();
        match object {
            ResolvedObject::EffectDefinition(_) | ResolvedObject::OperatorDefinition(_) => {
                Some(LinkTarget::Script {
                    document: identity.document_id().clone(),
                    declaration: identity.object().to_string(),
                    member: ScriptMember::Declaration,
                })
            }
            _ => self
                .data
                .get(identity.document_id())
                .and_then(|data| data.names.get(identity.object()))
                .map(|span| LinkTarget::Data {
                    document: identity.document_id().clone(),
                    span: *span,
                }),
        }
    }

    /// The name of the owned collection member `identity` addresses.
    pub(crate) fn member_span(
        &self,
        identity: &donder_language::identity::ObjectIdentity,
    ) -> Option<LinkTarget> {
        let root = identity.root_source();
        let data = self.data.get(root.document_id())?;
        let declaration = &data.declarations.get(root.object())?.1;
        resolve::member_name(declaration, identity.owned_path()).map(|span| LinkTarget::Data {
            document: root.document_id().clone(),
            span,
        })
    }

    /// The declaration span of a declared object, or none for scripts.
    pub(crate) fn declaration_span(&self, identity: &SourceIdentity) -> TextSpan {
        self.data
            .get(identity.document_id())
            .and_then(|data| data.declarations.get(identity.object()))
            .map_or(NO_SPAN, |(span, _)| *span)
    }

    /// The data document holding a declared object.
    pub(crate) fn declaration(
        &self,
        identity: &SourceIdentity,
    ) -> Result<Arc<DataDocument>, LoadProjectError> {
        self.data
            .get(identity.document_id())
            .filter(|data| data.declarations.contains_key(identity.object()))
            .cloned()
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: identity.object().to_string(),
            })
    }

    fn resolve_project(
        &mut self,
        entrypoint: &DocumentId,
    ) -> Result<ProjectData, LoadProjectError> {
        let data = self.data.get(entrypoint).cloned().ok_or_else(|| {
            LoadProjectError::InvalidDocument {
                path: entrypoint.path().to_path_buf(),
                range: None,
                message: "the root document is not loaded".into(),
            }
        })?;
        let mut projects = data
            .declarations
            .iter()
            .filter_map(|(name, (span, declaration))| match declaration {
                Declaration::Project(project) => Some((name, *span, project)),
                _ => None,
            });
        let (name, _, root) = projects
            .next()
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: entrypoint.path().to_path_buf(),
                range: None,
                message: "the root document declares no `Project`".into(),
            })?;
        if let Some((_, span, _)) = projects.next() {
            return Err(self.invalid(entrypoint, span, "the root document declares one `Project`"));
        }
        let root_id = ProjectId(SourceIdentity::from_document(
            entrypoint.clone(),
            name.clone(),
        ));
        let mut project = ProjectData {
            root: ProjectRoot {
                id: root_id.clone(),
                description: root.description.clone(),
                setup: ValueSource::Reference(SetupId(root_id.0.clone().into())),
                sequences: Vec::new(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: self.definitions.clone(),
        };
        let owner = donder_language::identity::ObjectIdentity::from(root_id.0.clone());
        let mut resolver = DomainResolver {
            loader: self,
            project: &mut project,
        };
        let setup = resolver.setup_source(entrypoint, &owner, &root.setup)?;
        let sequences = root
            .sequences
            .iter()
            .map(|source| resolver.sequence_source(entrypoint, &owner, source))
            .collect::<Result<_, _>>()?;
        project.root.setup = setup;
        project.root.sequences = sequences;
        Ok(project)
    }
}
