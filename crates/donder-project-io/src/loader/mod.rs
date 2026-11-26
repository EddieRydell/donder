//! Project loading. Documents are read and indexed, imports followed, and
//! declarations then resolved into typed state, where every object gets a
//! fresh session identity.
mod locate;
mod resolve;

use donder_model::ObjectKind;
use std::cell::RefCell;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::{TextSpan, compile_script};
use donder_language::data::NO_SPAN;
use donder_language::{ImportAlias, ImportDeclaration, SourceReference};
use donder_model::LayoutId;
use donder_model::ObjectId;
use donder_model::{
    CurveDefinition, CurveId, GradientDefinition, GradientId, ScriptDefinition, ScriptId,
};
use donder_model::{DonderProject, ProjectData, ProjectId, ProjectRoot};
use indexmap::{IndexMap, IndexSet};

use crate::ProjectCheckCache;
use crate::address::{DeclaredObject, DocumentId, SourceIdentity};
use crate::check_cache::ReadDocument;
use crate::diagnostics::LoadError;
use crate::diagnostics::{byte_range, data_diagnostic, dsl_diagnostic};
use crate::document::Declaration;
use crate::imports::ParsedImport;
use crate::index::{Link, LinkTarget, ScriptMember};
use crate::source::{ProjectSession, ReferencedAsset, SourceDocument, SourceProject};
use crate::{IoDiagnosticCode, TextRange};
use resolve::DomainResolver;

/// Session ids by address. A reference and the object it names get the same
/// id in either order, so references resolve before the object is built.
#[derive(Default)]
pub(crate) struct Interner {
    ids: std::collections::HashMap<SourceIdentity, u32>,
    /// `None` for an id of [`Interner::of`]'s session that no object has.
    addresses: Vec<Option<SourceIdentity>>,
}

impl Interner {
    /// The ids `session` gives its objects, so each address interns to its
    /// object's id there.
    fn of(session: &ProjectSession) -> Self {
        let mut interner = Self::default();
        for (id, address) in session.addresses() {
            let index = id.raw() as usize;
            if interner.addresses.len() <= index {
                interner.addresses.resize(index + 1, None);
            }
            interner.ids.insert(address.clone(), id.raw());
            interner.addresses[index] = Some(address);
        }
        interner
    }

    fn intern(&mut self, address: SourceIdentity) -> u32 {
        if let Some(&id) = self.ids.get(&address) {
            return id;
        }
        let id = u32::try_from(self.addresses.len())
            .unwrap_or_else(|_| unreachable!("a project has fewer than 2^32 objects"));
        self.ids.insert(address.clone(), id);
        self.addresses.push(Some(address));
        id
    }
}

/// A data document's text and its declarations by name.
pub(crate) struct DataDocument {
    text: String,
    read: Arc<ReadDocument>,
    /// Each declaration's index in the reading.
    declarations: IndexMap<String, usize>,
    /// Each declaration's name span.
    names: IndexMap<String, TextSpan>,
}

impl DataDocument {
    /// The span and value of the declaration named `name`.
    fn declaration(&self, name: &str) -> Option<(TextSpan, &Declaration)> {
        let (_, span, declaration) =
            &self.read.document.declarations[*self.declarations.get(name)?];
        Some((*span, declaration))
    }

    /// A declaration [`Loader::declaration`] found in this document.
    fn declared(&self, name: &str) -> (TextSpan, &Declaration) {
        self.declaration(name)
            .unwrap_or_else(|| unreachable!("the document declares `{name}`"))
    }

    fn declarations(&self) -> impl Iterator<Item = (&str, TextSpan, &Declaration)> {
        self.declarations.iter().map(|(name, &index)| {
            let (_, span, declaration) = &self.read.document.declarations[index];
            (name.as_str(), *span, declaration)
        })
    }
}

/// The script definitions and library values every document may use, read
/// before the project resolves.
#[derive(Default)]
pub(crate) struct LoadedDefinitions {
    curves: IndexMap<CurveId, CurveDefinition>,
    gradients: IndexMap<GradientId, GradientDefinition>,
    scripts: IndexMap<ScriptId, ScriptDefinition>,
}

pub(super) struct Loader {
    workspace: crate::ProjectWorkspace,
    entrypoint: DocumentId,
    pub(crate) documents: IndexMap<DocumentId, SourceDocument>,
    data: IndexMap<DocumentId, Arc<DataDocument>>,
    pub(crate) visible_objects: IndexMap<DocumentId, IndexMap<SourceReference, ObjectId>>,
    pub(crate) import_locations: IndexMap<DocumentId, Vec<ParsedImport>>,
    loading_documents: IndexSet<DocumentId>,
    definitions: LoadedDefinitions,
    referenced_assets: RefCell<Vec<ReferencedAsset>>,
    pub(crate) checked_scripts: IndexSet<Utf8PathBuf>,
    pub(crate) texts: crate::project_loading::SourceTexts,
    cache: ProjectCheckCache,
    /// Where each resolved name points, for the language server.
    pub(crate) links: RefCell<IndexSet<Link>>,
    interner: RefCell<Interner>,
    /// The project's one layout, which fixture references name members of.
    pub(crate) layout: Option<SourceIdentity>,
}

impl Loader {
    pub(crate) fn new(
        workspace: crate::ProjectWorkspace,
        texts: crate::project_loading::SourceTexts,
        cache: ProjectCheckCache,
    ) -> Self {
        let entrypoint = DocumentId::new(crate::PROJECT_ROOT_FILE.into());
        Self {
            workspace,
            entrypoint,
            documents: IndexMap::new(),
            data: IndexMap::new(),
            visible_objects: IndexMap::new(),
            import_locations: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: LoadedDefinitions::default(),
            referenced_assets: RefCell::default(),
            checked_scripts: IndexSet::new(),
            texts,
            cache,
            links: RefCell::new(IndexSet::new()),
            interner: RefCell::default(),
            layout: None,
        }
    }

    /// The texts and readings this load used, for the rest of the check.
    pub(crate) fn into_texts_and_cache(
        self,
    ) -> (crate::project_loading::SourceTexts, ProjectCheckCache) {
        (self.texts, self.cache)
    }

    pub(crate) fn load(&mut self) -> Result<ProjectSession, LoadError> {
        let entrypoint = self.entrypoint.clone();
        self.check_format(&entrypoint)?;
        self.load_document(&entrypoint)?;
        self.build_document_scopes()?;
        let mut project = self.resolve_project(&entrypoint)?;
        self.resolve_loaded_objects(&mut project)?;
        project.next_id = u32::try_from(self.interner.borrow().addresses.len())
            .unwrap_or_else(|_| unreachable!("a project has fewer than 2^32 objects"));
        let project =
            DonderProject::try_new(project).map_err(|error| self.validation_error(&error))?;
        Ok(ProjectSession::loaded(
            project,
            SourceProject {
                workspace: self.workspace.clone(),
                entrypoint,
                documents: std::mem::take(&mut self.documents),
                referenced_assets: self.referenced_assets.take(),
            },
        ))
    }

    /// The one sequence `document` declares, read beside `session`'s
    /// documents: every object it names takes its id in `session`.
    pub(crate) fn detached_sequence(
        &mut self,
        session: &ProjectSession,
        document: &DocumentId,
    ) -> Result<donder_model::Sequence, LoadError> {
        *self.interner.borrow_mut() = Interner::of(session);
        let layout = session.address_of(session.project().root().layout);
        self.load_document(layout.document_id())?;
        self.layout = Some(layout);
        self.load_document(document)?;
        self.build_document_scopes()?;
        let Some(&ObjectId::Sequence(id)) = self.visible_objects[document].values().next() else {
            unreachable!("the caller read one sequence declaration")
        };
        let mut project = self.project_data(ProjectRoot {
            id: ProjectId(u32::MAX),
            description: None,
            layout: LayoutId(u32::MAX),
            routes: Vec::new(),
        });
        DomainResolver {
            loader: self,
            project: &mut project,
        }
        .resolve::<donder_model::Sequence>(id)?;
        let mut sequence = project
            .sequences
            .shift_remove(&id)
            .unwrap_or_else(|| unreachable!("the sequence was just resolved"));
        // The text numbers a layout's items in order; `session` may not.
        let fixture = |target: &mut donder_model::FixtureId| {
            let item = session
                .project
                .layout()
                .object()
                .fixtures
                .get(target.0 as usize - 1)
                .unwrap_or_else(|| unreachable!("the text printed the session's layout"));
            *target = item.id;
        };
        for clip in &mut sequence.composition.clips {
            std::sync::Arc::make_mut(clip)
                .targets
                .iter_mut()
                .for_each(fixture);
        }
        Ok(sequence)
    }

    /// Refuse a project saved in another format before reading any of it, so
    /// one message says so instead of its documents failing to decode.
    fn check_format(&mut self, entrypoint: &DocumentId) -> Result<(), LoadError> {
        let absolute = self.absolute_document_path(entrypoint)?;
        let text = self.read_source(entrypoint, &absolute)?;
        let Some(format) = crate::document::project_format(&text) else {
            return Ok(());
        };
        let current = i64::from(crate::PROJECT_FORMAT_VERSION);
        if format.value == current {
            return Ok(());
        }
        let message = if format.value > current {
            "This project was saved by a newer version of Donder."
        } else {
            "This project was saved by an older version of Donder."
        };
        Err(crate::diagnostics::invalid_document(
            entrypoint.path().to_path_buf(),
            Some(byte_range(&text, format.span.start, format.span.end)),
            message.to_string(),
        ))
    }

    /// A broken project rule, at the text of the object or member it concerns.
    fn validation_error(&self, error: &donder_model::ProjectValidationError) -> LoadError {
        let root = self.address(error.object);
        let span = self.data.get(root.document_id()).and_then(|data| {
            let (_, declaration) = data.declaration(root.object())?;
            locate::site_span(declaration, &error.site())
                .or_else(|| data.names.get(root.object()).copied())
        });
        crate::diagnostics::invalid_document(
            root.document().to_path_buf(),
            span.and_then(|span| self.range(root.document_id(), span)),
            crate::address::object_problem(&root, &error.problem),
        )
    }

    /// The text range of `span` in a loaded data document.
    pub(crate) fn range(&self, document: &DocumentId, span: TextSpan) -> Option<TextRange> {
        (span != NO_SPAN)
            .then(|| self.data.get(document))
            .flatten()
            .map(|data| byte_range(&data.text, span.start, span.end))
    }

    /// The session id of the object at `address`.
    fn intern(&self, address: SourceIdentity) -> u32 {
        self.interner.borrow_mut().intern(address)
    }

    /// The address an id was interned under.
    pub(crate) fn address(&self, id: impl Into<ObjectId>) -> SourceIdentity {
        self.interner.borrow().addresses[id.into().raw() as usize]
            .clone()
            .unwrap_or_else(|| unreachable!("every id the loader hands out has an address"))
    }

    /// The declared object `identity` names, as a `kind`.
    fn declared(&self, kind: &ObjectKind, identity: SourceIdentity) -> ObjectId {
        kind.id(self.intern(identity))
    }

    pub(crate) fn invalid(
        &self,
        document: &DocumentId,
        span: TextSpan,
        message: impl Into<String>,
    ) -> LoadError {
        crate::diagnostics::invalid_document(
            document.path().to_path_buf(),
            self.range(document, span),
            message.into(),
        )
    }

    fn resolve_loaded_objects(&mut self, project: &mut ProjectData) -> Result<(), LoadError> {
        // Every declared object gets typed state, including objects nothing
        // uses, so saving can always print every declaration.
        let objects: Vec<_> = self
            .visible_objects
            .iter()
            .flat_map(|(document, visible)| {
                visible
                    .iter()
                    .filter(|(key, _)| matches!(key, SourceReference::Local(_)))
                    .map(|(_, object)| (document.clone(), *object))
            })
            .collect();
        let entrypoint = self.entrypoint.clone();
        let mut resolver = DomainResolver {
            loader: self,
            project,
        };
        let order = objects.clone();
        for (document, object) in objects {
            match object {
                ObjectId::Project(id) => {
                    if entrypoint != document || resolver.project.root.id != id {
                        let span = resolver
                            .loader
                            .declaration_span(&resolver.loader.address(id));
                        return Err(resolver.loader.invalid(
                            &document,
                            span,
                            format!("a project is declared only in {}", crate::PROJECT_ROOT_FILE),
                        ));
                    }
                }
                ObjectId::Controller(id) => resolver.resolve::<donder_model::Controller>(id)?,
                ObjectId::InputDevice(id) => resolver.resolve::<donder_model::InputDevice>(id)?,
                ObjectId::Layout(id) => {
                    if resolver.loader.layout.as_ref() != Some(&resolver.loader.address(id)) {
                        let span = resolver
                            .loader
                            .declaration_span(&resolver.loader.address(id));
                        return Err(resolver.loader.invalid(
                            &document,
                            span,
                            "a project has one layout, the one its root names",
                        ));
                    }
                    resolver.resolve::<donder_model::Layout>(id)?
                }
                ObjectId::FixtureDefinition(id) => {
                    resolver.resolve::<donder_model::FixtureDefinition>(id)?
                }
                ObjectId::Sequence(id) => resolver.resolve::<donder_model::Sequence>(id)?,
                ObjectId::Console(id) => resolver.resolve::<donder_model::Console>(id)?,
                ObjectId::Curve(_) | ObjectId::Gradient(_) | ObjectId::Script(_) => {}
            }
        }
        // Objects resolve as references reach them; the project lists them in
        // the order the documents declare them, the root first.
        let order = order
            .into_iter()
            .enumerate()
            .map(|(index, (_, object))| (object, index))
            .collect::<std::collections::HashMap<_, _>>();
        let project = &mut *resolver.project;
        in_order(&mut project.layouts, &order);
        in_order(&mut project.controllers, &order);
        in_order(&mut project.input_devices, &order);
        in_order(&mut project.sequences, &order);
        in_order(&mut project.consoles, &order);
        in_order(&mut project.fixtures, &order);
        in_order(&mut project.curves, &order);
        in_order(&mut project.gradients, &order);
        in_order(&mut project.scripts, &order);
        Ok(())
    }

    pub(super) fn load_document(&mut self, document_id: &DocumentId) -> Result<(), LoadError> {
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
            crate::SourceDocumentFormat::Other => Err(crate::diagnostics::invalid_document(
                document_id.path().to_path_buf(),
                None,
                "not a Donder document",
            )),
        };
        self.loading_documents.shift_remove(document_id);
        result
    }

    pub(super) fn absolute_document_path(
        &self,
        document_id: &DocumentId,
    ) -> Result<Utf8PathBuf, LoadError> {
        crate::validate_document_path(document_id.path().as_str()).map_err(|message| {
            crate::diagnostics::invalid_document(document_id.path().to_owned(), None, message)
        })?;
        let path = self.workspace.root.join(document_id.path());
        if path.exists() {
            let absolute = crate::canonicalize(&path)
                .map_err(|source| crate::diagnostics::read_error(path.clone(), &source))?;
            if !absolute.starts_with(&self.workspace.root) {
                return Err(crate::diagnostics::invalid_document(
                    document_id.path().to_owned(),
                    None,
                    "Document escapes the project root",
                ));
            }
        }
        Ok(path)
    }

    fn read_source(
        &mut self,
        document_id: &DocumentId,
        absolute: &Utf8Path,
    ) -> Result<String, LoadError> {
        self.texts
            .text(document_id.path())
            .map(str::to_owned)
            .map_err(|source| crate::diagnostics::read_error(absolute, source))
    }

    fn load_script(
        &mut self,
        document_id: &DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
        self.checked_scripts.insert(relative.to_path_buf());
        let script = compile_script(&source).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| {
                    dsl_diagnostic(
                        relative,
                        &source,
                        diagnostic,
                        IoDiagnosticCode::ScriptCompile,
                    )
                })
                .collect::<Vec<_>>()
        })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        let declared = script.into_iter().map(|definition| {
            let name = definition.name().clone();
            let identity = self.intern(SourceIdentity::from_document(
                document_id.clone(),
                name.as_str().to_string(),
            ));
            let id = ScriptId(identity);
            self.definitions
                .scripts
                .insert(id, ScriptDefinition::new(definition));
            (name, ObjectId::from(id))
        });
        for (name, object) in declared.collect::<Vec<_>>() {
            objects.push(DeclaredObject::new(object, name.clone()));
            if visible
                .insert(SourceReference::Local(name.clone()), object)
                .is_some()
            {
                return Err(crate::diagnostics::invalid_document(
                    relative.to_path_buf(),
                    None,
                    format!("`{}` is declared twice", name.as_str()),
                ));
            }
        }
        self.visible_objects.insert(document_id.clone(), visible);
        let document = SourceDocument::script(objects, source).map_err(|message| {
            crate::diagnostics::invalid_document(relative.to_path_buf(), None, message)
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
    ) -> Result<(), LoadError> {
        let relative = document_id.path();
        let text = self.read_source(document_id, absolute)?;
        let read = self.cache.read(relative, &text);
        let parsed = &read.document;
        if !read.diagnostics.is_empty() {
            return Err(read
                .diagnostics
                .iter()
                .map(|diagnostic| data_diagnostic(relative, &text, diagnostic.clone()))
                .collect());
        }
        let imports = parsed
            .imports
            .iter()
            .map(|import| {
                let range = |span: TextSpan| Some(byte_range(&text, span.start, span.end));
                Ok(ParsedImport {
                    declaration: ImportDeclaration {
                        alias: ImportAlias::new(import.alias.value.as_str()).map_err(
                            |message| {
                                crate::diagnostics::invalid_document(
                                    relative.to_path_buf(),
                                    range(import.alias.span),
                                    message,
                                )
                            },
                        )?,
                        documents: import
                            .paths
                            .iter()
                            .map(|path| Utf8PathBuf::from(&path.value))
                            .collect(),
                    },
                    range: range(import.alias.span),
                    alias_span: import.alias.span,
                    source_ranges: import.paths.iter().map(|path| range(path.span)).collect(),
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        let mut declarations = IndexMap::new();
        let mut names = IndexMap::new();
        for (index, (name, _, declaration)) in parsed.declarations.iter().enumerate() {
            names.insert(name.value.as_str().to_string(), name.span);
            let identity =
                SourceIdentity::from_document(document_id.clone(), name.value.as_str().to_string());
            let kind = declaration.kind();
            match &declaration {
                Declaration::Curve(curve) => {
                    self.definitions.curves.insert(
                        CurveId(self.intern(identity.clone())),
                        CurveDefinition {
                            description: curve.description.clone(),
                            curve: resolve::curve(&curve.points),
                        },
                    );
                }
                Declaration::Gradient(gradient) => {
                    self.definitions.gradients.insert(
                        GradientId(self.intern(identity.clone())),
                        GradientDefinition {
                            description: gradient.description.clone(),
                            gradient: resolve::gradient(&gradient.space, &gradient.stops),
                        },
                    );
                }
                _ => {}
            }
            let object = self.declared(&kind, identity);
            objects.push(DeclaredObject::new(object, name.value.clone()));
            visible.insert(SourceReference::Local(name.value.clone()), object);
            declarations.insert(name.value.as_str().to_string(), index);
        }
        self.visible_objects.insert(document_id.clone(), visible);
        self.data.insert(
            document_id.clone(),
            Arc::new(DataDocument {
                text,
                read: Arc::clone(&read),
                declarations,
                names,
            }),
        );
        let import_edges = self.load_imports(document_id, &imports)?;
        let document = SourceDocument::data(import_edges, objects).map_err(|message| {
            crate::diagnostics::invalid_document(relative.to_path_buf(), None, message)
        })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    /// Record that the name at `span` in `document` points at `target`.
    pub(crate) fn link(&self, document: &DocumentId, span: TextSpan, target: LinkTarget) {
        if span != NO_SPAN {
            self.links.borrow_mut().insert(Link {
                document: document.clone(),
                span,
                target,
            });
        }
    }

    /// Where a declared object's name is: a data declaration's name, or a
    /// script declaration by name.
    pub(crate) fn declared_target(&self, object: ObjectId) -> Option<LinkTarget> {
        let address = self.address(object);
        let identity = &address;
        match object {
            ObjectId::Script(_) => Some(LinkTarget::Script {
                document: identity.document_id().clone(),
                declaration: identity.object().to_string(),
                member: ScriptMember::Declaration,
            }),
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

    /// The declaration span of a declared object, or none for scripts.
    fn declaration_span(&self, identity: &SourceIdentity) -> TextSpan {
        self.data
            .get(identity.document_id())
            .and_then(|data| data.declaration(identity.object()))
            .map_or(NO_SPAN, |(span, _)| span)
    }

    /// The data document holding a declared object.
    fn declaration(&self, identity: &SourceIdentity) -> Result<Arc<DataDocument>, LoadError> {
        self.data
            .get(identity.document_id())
            .filter(|data| data.declaration(identity.object()).is_some())
            .cloned()
            .ok_or_else(|| {
                crate::diagnostics::invalid_reference(
                    identity.document().to_path_buf(),
                    None,
                    identity.object(),
                )
            })
    }

    /// A project of `root` and the definitions read so far, before any other
    /// object resolves.
    fn project_data(&self, root: ProjectRoot) -> ProjectData {
        ProjectData {
            root,
            layouts: IndexMap::new(),
            controllers: IndexMap::new(),
            input_devices: IndexMap::new(),
            sequences: IndexMap::new(),
            consoles: IndexMap::new(),
            fixtures: IndexMap::new(),
            curves: self.definitions.curves.clone(),
            gradients: self.definitions.gradients.clone(),
            scripts: self.definitions.scripts.clone(),
            next_id: 0,
        }
    }

    fn resolve_project(&mut self, entrypoint: &DocumentId) -> Result<ProjectData, LoadError> {
        let data = self.data.get(entrypoint).cloned().ok_or_else(|| {
            crate::diagnostics::invalid_document(
                entrypoint.path().to_path_buf(),
                None,
                "the root document is not loaded",
            )
        })?;
        let mut projects =
            data.declarations()
                .filter_map(|(name, span, declaration)| match declaration {
                    Declaration::Project(project) => Some((name, span, project)),
                    _ => None,
                });
        let (name, _, root) = projects.next().ok_or_else(|| {
            crate::diagnostics::invalid_document(
                entrypoint.path().to_path_buf(),
                None,
                "the root document declares no `Project`",
            )
        })?;
        if let Some((_, span, _)) = projects.next() {
            return Err(self.invalid(entrypoint, span, "the root document declares one `Project`"));
        }
        let root_identity = SourceIdentity::from_document(entrypoint.clone(), name.to_string());
        let root_id = ProjectId(self.intern(root_identity));
        let mut project = self.project_data(ProjectRoot {
            id: root_id,
            description: root.description.clone(),
            layout: LayoutId(u32::MAX),
            routes: Vec::new(),
        });
        let layout = self.resolve_reference::<LayoutId>(entrypoint, &root.layout)?;
        self.layout = Some(self.address(layout));
        let mut resolver = DomainResolver {
            loader: self,
            project: &mut project,
        };
        let routes = resolver.routes(entrypoint, &root.routes)?;
        project.root.layout = layout;
        project.root.routes = routes;
        Ok(project)
    }
}

/// Sort `store` by each object's place in `order`.
fn in_order<I: Copy + Into<ObjectId>, T>(
    store: &mut IndexMap<I, T>,
    order: &std::collections::HashMap<ObjectId, usize>,
) {
    store.sort_by_cached_key(|id, _| order[&(*id).into()]);
}
