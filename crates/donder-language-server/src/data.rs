//! Data document features: hover, completion, outline and formatting, read
//! from the syntax tree, the schema and the project index.
use donder_language::analysis::{
    Completion, CompletionKind, DataToken, DataTokenKind, SymbolKind, analyze_script,
    data_token_stream,
};
use donder_language::data::schema::{Definition, Fields, Schema, Shape};
use donder_language::data::tree::{DataDocument, DataField, DataValue, Spanned, is_pascal_case};
use donder_language::dsl::TextSpan;
use donder_project_io::{DECLARATION_TYPE_NAMES, LinkTarget, ScriptMember, document_schema};

use crate::navigation::script_member;
use crate::workspace::{Kind, Workspace, kind};

fn within(span: TextSpan, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

fn parse(text: &str) -> DataDocument {
    donder_language::data::parse(text).0
}

/// The canonical text, when the document parses without errors.
pub fn format(text: &str) -> Option<String> {
    let (document, diagnostics) = donder_language::data::parse(text);
    diagnostics
        .is_empty()
        .then(|| donder_language::data::print(&document))
}

fn description(fields: &[DataField]) -> Option<&str> {
    fields.iter().find_map(|field| match &field.value.value {
        DataValue::String(text) if field.name.value.as_str() == "description" => {
            Some(text.as_str())
        }
        _ => None,
    })
}

fn named(ty: &str, name: &str, description: Option<&str>) -> String {
    let mut text = format!("```donder\n{ty} {name}\n```");
    if let Some(description) = description {
        text.push_str("\n\n");
        text.push_str(description);
    }
    text
}

/// Markdown for the object whose name is at `span`.
pub fn describe(text: &str, span: TextSpan) -> Option<String> {
    fn search(value: &Spanned<DataValue>, span: TextSpan) -> Option<String> {
        match &value.value {
            DataValue::Record(ty, fields) => {
                let own = fields
                    .value
                    .iter()
                    .find_map(|field| match &field.value.value {
                        DataValue::Reference(segments)
                            if field.name.value.as_str() == "name"
                                && segments.len() == 1
                                && segments[0].span == span =>
                        {
                            Some(named(
                                ty.value.as_str(),
                                segments[0].value.as_str(),
                                description(&fields.value),
                            ))
                        }
                        _ => None,
                    });
                own.or_else(|| {
                    fields
                        .value
                        .iter()
                        .find_map(|field| search(&field.value, span))
                })
            }
            DataValue::Named(ty, name, fields) => {
                if name.span == span {
                    Some(named(
                        ty.value.as_str(),
                        name.value.as_str(),
                        description(&fields.value),
                    ))
                } else {
                    fields
                        .value
                        .iter()
                        .find_map(|field| search(&field.value, span))
                }
            }
            DataValue::List(items) | DataValue::Tuple(items) => {
                items.iter().find_map(|item| search(item, span))
            }
            DataValue::Map(fields) => fields.iter().find_map(|field| search(&field.value, span)),
            _ => None,
        }
    }
    let document = parse(text);
    if let Some(import) = document
        .imports
        .iter()
        .find(|import| import.alias.span == span)
    {
        return Some(format!(
            "```donder\nimport {} from {};\n```",
            import.alias.value.as_str(),
            import
                .paths
                .iter()
                .map(|path| format!("<{}>", path.value))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    document.declarations.iter().find_map(|declaration| {
        if declaration.name.span == span {
            Some(named(
                declaration.ty.value.as_str(),
                declaration.name.value.as_str(),
                description(&declaration.fields.value),
            ))
        } else {
            declaration
                .fields
                .value
                .iter()
                .find_map(|field| search(&field.value, span))
        }
    })
}

pub fn shape_text(shape: &Shape) -> String {
    match shape {
        Shape::Integer => "integer".into(),
        Shape::Float => "float, like `1.0`".into(),
        Shape::Duration => "duration, like `1.5s`".into(),
        Shape::Distance => "distance, like `0.35m`".into(),
        Shape::Color => "color, like `#ff8800`".into(),
        Shape::String => "string".into(),
        Shape::Path => "path, like `<audio/song.mp3>`".into(),
        Shape::Bool => "`true` or `false`".into(),
        Shape::Name => "name".into(),
        Shape::Reference => "reference".into(),
        Shape::Params => "parameter values, `{ name: value }`".into(),
        Shape::Option(inner) => format!("{} or `none`", shape_text(inner)),
        Shape::List(item) => format!("list of {}", shape_text(item)),
        Shape::Tuple(items) => format!(
            "({})",
            items.iter().map(shape_text).collect::<Vec<_>>().join(", ")
        ),
        Shape::Named(name) => format!("`{name}`"),
        Shape::Source(name) => format!("a `{name}` reference, or `{name} {{ ... }}` owned here"),
        Shape::NamedSource(name) => {
            format!("a `{name}` reference, or `{name} name {{ ... }}` owned here")
        }
    }
}

/// The fields of a record type or a variant.
fn fields<'a>(schema: &'a Schema, ty: &str) -> Option<&'a Fields> {
    if let Some(Some(Definition::Record(fields))) = schema.types.get(ty) {
        return Some(fields);
    }
    schema
        .types
        .values()
        .flatten()
        .find_map(|definition| match definition {
            Definition::Variants(variants) => variants
                .iter()
                .find_map(|(name, fields)| (*name == ty).then_some(fields.as_ref()).flatten()),
            Definition::Record(_) => None,
        })
}

fn definition_text(schema: &Schema, ty: &str) -> Option<String> {
    let field_lines = |fields: &Fields| {
        fields
            .iter()
            .map(|(name, shape)| format!("  {name}: {}", shape_text(shape)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if let Some(fields) = fields(schema, ty) {
        return Some(format!(
            "```donder\n{ty} {{\n{}\n}}\n```",
            field_lines(fields)
        ));
    }
    if let Some(Some(Definition::Variants(variants))) = schema.types.get(ty) {
        let variants = variants
            .iter()
            .map(|(name, fields)| match fields {
                Some(fields) if !fields.is_empty() => format!("{name} {{ ... }}"),
                _ => name.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" | ");
        return Some(format!("```donder\n{ty} = {variants}\n```"));
    }
    None
}

/// Hover on a field name or a type name.
pub fn schema_hover(text: &str, offset: usize) -> Option<String> {
    let schema = document_schema();
    let tokens = data_token_stream(text);
    let index = tokens.iter().position(|token| within(token.span, offset))?;
    let token = tokens[index];
    let word = &text[token.span.start..token.span.end];
    if token.kind != DataTokenKind::Name {
        return None;
    }
    if is_pascal_case(word) {
        // A type, or a variant of one.
        return definition_text(&schema, word).or_else(|| {
            schema
                .types
                .iter()
                .find_map(|(ty, definition)| match definition {
                    Some(Definition::Variants(variants))
                        if variants.iter().any(|(name, _)| *name == word) =>
                    {
                        definition_text(&schema, ty)
                    }
                    _ => None,
                })
        });
    }
    if tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == DataTokenKind::Colon)
    {
        let frames = frames(text, &tokens[..index]);
        if let Some(Frame::Record { ty, .. }) = frames.last()
            && let Some((_, shape)) = fields(&schema, ty)?.iter().find(|(name, _)| *name == word)
        {
            return Some(format!(
                "```donder\n{ty}.{word}\n```\n\n{}",
                shape_text(shape)
            ));
        }
    }
    None
}

/// An open container the cursor is in, from the tokens before it.
#[derive(Clone, Debug)]
enum Frame {
    Record {
        ty: String,
        seen: Vec<String>,
        field: Option<String>,
        /// The last segment of this record's `effect` or `operator` field.
        definition: Option<TextSpan>,
    },
    /// A list, and the record field holding it.
    List {
        owner: Option<(String, String)>,
    },
    /// Parameter values, and the record field holding them.
    Map {
        definition: Option<TextSpan>,
        seen: Vec<String>,
        field: Option<String>,
    },
    Other,
}

fn frames(text: &str, tokens: &[DataToken]) -> Vec<Frame> {
    let word = |token: &DataToken| &text[token.span.start..token.span.end];
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1);
        match token.kind {
            DataTokenKind::Open('{') => {
                // `Type {` or `Type name {`.
                let ty = tokens[..index]
                    .iter()
                    .rev()
                    .take(2)
                    .find(|previous| {
                        previous.kind == DataTokenKind::Name && is_pascal_case(word(previous))
                    })
                    .filter(|previous| {
                        tokens[..index].last().is_some_and(|last| {
                            last.span == previous.span
                                || last.kind == DataTokenKind::Name && !is_pascal_case(word(last))
                        })
                    })
                    .map(|previous| word(previous).to_string());
                let frame = match (ty, stack.last()) {
                    (Some(ty), _) => Frame::Record {
                        ty,
                        seen: Vec::new(),
                        field: None,
                        definition: None,
                    },
                    (
                        None,
                        Some(Frame::Record {
                            field: Some(field),
                            definition,
                            ..
                        }),
                    ) if field == "params" => Frame::Map {
                        definition: *definition,
                        seen: Vec::new(),
                        field: None,
                    },
                    _ => Frame::Other,
                };
                stack.push(frame);
            }
            DataTokenKind::Open('[') => {
                let owner = match stack.last() {
                    Some(Frame::Record {
                        ty,
                        field: Some(field),
                        ..
                    }) => Some((ty.clone(), field.clone())),
                    Some(Frame::List { owner }) => owner.clone(),
                    _ => None,
                };
                stack.push(Frame::List { owner });
            }
            DataTokenKind::Open(_) => stack.push(Frame::Other),
            DataTokenKind::Close(_) => {
                stack.pop();
            }
            DataTokenKind::Name | DataTokenKind::Keyword
                if next.is_some_and(|next| next.kind == DataTokenKind::Colon) =>
            {
                match stack.last_mut() {
                    Some(Frame::Record { seen, field, .. }) => {
                        seen.push(word(token).to_string());
                        *field = Some(word(token).to_string());
                    }
                    Some(Frame::Map { field, seen, .. }) => {
                        seen.push(word(token).to_string());
                        *field = Some(word(token).to_string());
                    }
                    _ => {}
                }
            }
            DataTokenKind::Name => {
                if let Some(Frame::Record {
                    field: Some(field),
                    definition,
                    ..
                }) = stack.last_mut()
                    && (field == "effect" || field == "operator")
                {
                    *definition = Some(token.span);
                }
            }
            DataTokenKind::Comma => match stack.last_mut() {
                Some(Frame::Record { field, .. }) | Some(Frame::Map { field, .. }) => *field = None,
                _ => {}
            },
            _ => {}
        }
    }
    stack
}

/// What a reference field names, for completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Wanted {
    Declaration(&'static str),
    Effect,
    Operator,
    /// `layout.fixture`.
    Fixture,
    Layer,
    Clip,
    Node,
    Marks,
}

fn wanted(ty: &str, field: &str) -> Option<Wanted> {
    Some(match (ty, field) {
        ("Project", "setup") => Wanted::Declaration("Setup"),
        ("Project", "sequences") => Wanted::Declaration("Sequence"),
        ("Setup", "layout") => Wanted::Declaration("Layout"),
        ("Setup", "patch") => Wanted::Declaration("Patch"),
        ("Setup", "controllers") | ("Route", "controller") => Wanted::Declaration("Controller"),
        ("Fixture", "definition") => Wanted::Declaration("FixtureDefinition"),
        ("Route", "target") | ("Clip", "target") | ("AutomationClip", "row") => Wanted::Fixture,
        ("Clip", "effect") => Wanted::Effect,
        ("OperatorNode", "operator") => Wanted::Operator,
        ("Clip", "layer") | ("LayerNode", "layer") => Wanted::Layer,
        ("Edge", "from") | ("Edge", "to") | ("NodeParam", "node") => Wanted::Node,
        ("ClipParam", "clip") => Wanted::Clip,
        _ => return None,
    })
}

/// The declarations a document can name, as `name` or `alias.name`, with
/// their type: `Layout`, `Effect` or `Operator` for scripts.
fn visible(workspace: &Workspace, text: &str) -> Vec<(String, String)> {
    let mut names = Vec::new();
    let local = parse(text);
    for declaration in &local.declarations {
        names.push((
            declaration.name.value.as_str().to_string(),
            declaration.ty.value.as_str().to_string(),
        ));
    }
    for import in &local.imports {
        for path in &import.paths {
            let Some(path_uri) = workspace.uri(camino::Utf8Path::new(&path.value)) else {
                continue;
            };
            let Some(source) = workspace.text(&path_uri) else {
                continue;
            };
            let alias = import.alias.value.as_str();
            match kind(&path_uri) {
                Kind::Data => {
                    for declaration in parse(&source).declarations {
                        names.push((
                            format!("{alias}.{}", declaration.name.value.as_str()),
                            declaration.ty.value.as_str().to_string(),
                        ));
                    }
                }
                Kind::Script => {
                    for symbol in analyze_script(&source).symbols {
                        let ty = match symbol.kind {
                            SymbolKind::Effect => "Effect",
                            SymbolKind::Operator => "Operator",
                            _ => continue,
                        };
                        names.push((format!("{alias}.{}", symbol.name), ty.to_string()));
                    }
                }
                Kind::Other => {}
            }
        }
    }
    names
}

/// Names declared in this document by `Type { name: X` or `LayerNode { layer: X`.
fn local_names(text: &str, ty: &str, field: &str) -> Vec<String> {
    let tokens = data_token_stream(text);
    let word = |token: &DataToken| &text[token.span.start..token.span.end];
    let mut names = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if word(token) == ty
            && let [open, name, colon, value, ..] = &tokens[index + 1..]
            && open.kind == DataTokenKind::Open('{')
            && word(name) == field
            && colon.kind == DataTokenKind::Colon
            && value.kind == DataTokenKind::Name
        {
            names.push(word(value).to_string());
        }
    }
    names
}

/// The fixture and group names of the layout a reference names.
fn layout_items(workspace: &Workspace, text: &str, prefix: &[&str]) -> Vec<String> {
    fn collect(value: &Spanned<DataValue>, names: &mut Vec<String>) {
        match &value.value {
            DataValue::Record(ty, fields) if matches!(ty.value.as_str(), "Group" | "Fixture") => {
                for field in &fields.value {
                    if let (DataValue::Reference(segments), "name") =
                        (&field.value.value, field.name.value.as_str())
                        && segments.len() == 1
                    {
                        names.push(segments[0].value.as_str().to_string());
                    }
                }
            }
            DataValue::List(items) => items.iter().for_each(|item| collect(item, names)),
            _ => {}
        }
    }
    let layout = |source: &str, name: &str| {
        let mut names = Vec::new();
        for declaration in parse(source).declarations {
            if declaration.name.value.as_str() == name {
                for field in &declaration.fields.value {
                    if field.name.value.as_str() == "items" {
                        collect(&field.value, &mut names);
                    }
                }
            }
        }
        names
    };
    match prefix {
        [name] => layout(text, name),
        [alias, name] => {
            let document = parse(text);
            let Some(import) = document
                .imports
                .iter()
                .find(|import| import.alias.value.as_str() == *alias)
            else {
                return Vec::new();
            };
            import
                .paths
                .iter()
                .filter_map(|path| workspace.uri(camino::Utf8Path::new(&path.value)))
                .filter_map(|path| workspace.text(&path))
                .flat_map(|source| layout(&source, name))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn snippet_fields(fields: &Fields) -> String {
    fields
        .iter()
        .enumerate()
        .map(|(index, (name, shape))| {
            let placeholder = match shape {
                Shape::Option(_) => "none".to_string(),
                Shape::List(_) => "[]".to_string(),
                Shape::Params => "{}".to_string(),
                _ => String::new(),
            };
            format!("{name}: ${{{}:{placeholder}}}", index + 1)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn record_completions(schema: &Schema, ty: &str, named: bool) -> Vec<Completion> {
    let Some(fields) = fields(schema, ty) else {
        return Vec::new();
    };
    let name = if named { " ${0:name}" } else { "" };
    vec![
        Completion::new(ty, CompletionKind::Snippet)
            .detail(format!("{ty} {{ ... }}"))
            .insert(format!("{ty}{name} {{ {} }}", snippet_fields(fields))),
    ]
}

fn shape_completions(schema: &Schema, shape: &Shape, references: &[String]) -> Vec<Completion> {
    let reference_items = || {
        references
            .iter()
            .map(|name| Completion::new(name.as_str(), CompletionKind::Reference))
            .collect::<Vec<_>>()
    };
    match shape {
        Shape::Bool => ["true", "false"]
            .iter()
            .map(|word| Completion::new(*word, CompletionKind::Keyword))
            .collect(),
        Shape::Option(inner) => std::iter::once(Completion::new("none", CompletionKind::Keyword))
            .chain(shape_completions(schema, inner, references))
            .collect(),
        Shape::List(item) => shape_completions(schema, item, references),
        Shape::Named(ty) => match schema.types.get(ty) {
            Some(Some(Definition::Variants(variants))) => variants
                .iter()
                .map(|(name, fields)| match fields {
                    Some(fields) if !fields.is_empty() => {
                        Completion::new(*name, CompletionKind::EnumMember)
                            .detail(format!("{name} {{ ... }}"))
                            .insert(format!("{name} {{ {} }}", snippet_fields(fields)))
                    }
                    _ => Completion::new(*name, CompletionKind::EnumMember),
                })
                .collect(),
            _ => record_completions(schema, ty, false),
        },
        Shape::Source(ty) => reference_items()
            .into_iter()
            .chain(record_completions(schema, ty, false))
            .collect(),
        Shape::NamedSource(ty) => reference_items()
            .into_iter()
            .chain(record_completions(schema, ty, true))
            .collect(),
        Shape::Reference | Shape::Name => reference_items(),
        _ => Vec::new(),
    }
}

/// The script declaration a record's `effect` or `operator` names, by its
/// link in the project index.
fn definition_params(
    workspace: &Workspace,
    uri: &str,
    span: Option<TextSpan>,
) -> Option<(donder_language::analysis::ScriptAnalysis, usize)> {
    let document = workspace.document_id(uri)?;
    let check = workspace.check.as_ref()?;
    let link = check.report.index.link_at(&document, span?.start)?;
    let LinkTarget::Script {
        document,
        declaration,
        ..
    } = &link.target
    else {
        return None;
    };
    let analysis = analyze_script(&workspace.document_text(document)?);
    let symbol = script_member(&analysis, declaration, &ScriptMember::Declaration)?;
    Some((analysis, symbol))
}

pub fn completions(workspace: &Workspace, uri: &str, text: &str, offset: usize) -> Vec<Completion> {
    let schema = document_schema();
    let tokens = data_token_stream(text);
    // Tokens before the cursor, without a partly typed name.
    let before = tokens
        .iter()
        .position(|token| {
            token.span.start >= offset
                || token.span.end >= offset && token.kind == DataTokenKind::Name
        })
        .unwrap_or(tokens.len());
    let tokens = &tokens[..before];
    let word = |token: &DataToken| &text[token.span.start..token.span.end];
    // A dotted prefix being completed: `layouts.main.`.
    let mut prefix = Vec::new();
    let mut index = tokens.len();
    while index >= 2
        && tokens[index - 1].kind == DataTokenKind::Dot
        && tokens[index - 2].kind == DataTokenKind::Name
    {
        prefix.insert(0, word(&tokens[index - 2]));
        index -= 2;
    }
    let stack = frames(text, &tokens[..index]);
    let names_for = |wanted: Wanted| -> Vec<String> {
        match wanted {
            Wanted::Declaration(ty) => visible(workspace, text)
                .into_iter()
                .filter(|(_, kind)| kind == ty)
                .map(|(name, _)| name)
                .collect(),
            Wanted::Effect | Wanted::Operator => {
                let ty = if wanted == Wanted::Effect {
                    "Effect"
                } else {
                    "Operator"
                };
                visible(workspace, text)
                    .into_iter()
                    .filter(|(_, kind)| kind == ty)
                    .map(|(name, _)| name)
                    .collect()
            }
            Wanted::Fixture => visible(workspace, text)
                .into_iter()
                .filter(|(_, kind)| kind == "Layout")
                .map(|(name, _)| format!("{name}."))
                .collect(),
            Wanted::Layer => local_names(text, "Layer", "name"),
            Wanted::Clip => local_names(text, "Clip", "name"),
            Wanted::Marks => local_names(text, "MarkCollection", "name"),
            Wanted::Node => {
                let mut names = local_names(text, "OperatorNode", "name");
                names.extend(local_names(text, "LayerNode", "layer"));
                names.push("output".into());
                names
            }
        }
    };
    let field_owner = match stack.last() {
        Some(Frame::Record {
            ty,
            field: Some(field),
            ..
        }) => Some((ty.clone(), field.clone())),
        Some(Frame::List { owner }) => owner.clone(),
        _ => None,
    };
    if !prefix.is_empty() {
        // After a dot: a layout's fixtures, or an alias's declarations.
        if let Some((ty, field)) = &field_owner
            && wanted(ty, field) == Some(Wanted::Fixture)
            && !(prefix.len() == 1
                && parse(text)
                    .imports
                    .iter()
                    .any(|import| import.alias.value.as_str() == prefix[0]))
        {
            return layout_items(workspace, text, &prefix)
                .into_iter()
                .map(|name| Completion::new(name, CompletionKind::Reference))
                .collect();
        }
        let qualified = format!("{}.", prefix.join("."));
        let mut names = visible(workspace, text);
        if let Some((ty, field)) = &field_owner
            && let Some(wanted) = wanted(ty, field)
        {
            let wanted_names = names_for(wanted);
            names.retain(|(name, _)| {
                wanted_names
                    .iter()
                    .any(|wanted| wanted.trim_end_matches('.') == name)
            });
        }
        return names
            .into_iter()
            .filter_map(|(name, ty)| {
                name.strip_prefix(&qualified)
                    .map(|rest| Completion::new(rest, CompletionKind::Reference).detail(ty))
            })
            .collect();
    }
    match stack.last() {
        None => {
            let mut completions = vec![
                Completion::new("import", CompletionKind::Keyword)
                    .insert("import ${1:alias} from <${2:path}>;"),
            ];
            for ty in DECLARATION_TYPE_NAMES {
                completions.extend(record_completions(&schema, ty, true));
            }
            completions
        }
        Some(Frame::Record {
            ty,
            field: None,
            seen,
            ..
        }) => fields(&schema, ty)
            .map(|fields| {
                fields
                    .iter()
                    .filter(|(name, _)| !seen.iter().any(|seen| seen == name))
                    .enumerate()
                    .map(|(order, (name, shape))| {
                        let mut completion = Completion::new(*name, CompletionKind::Field)
                            .detail(shape_text(shape))
                            .insert(format!("{name}: "));
                        // The next field in schema order first.
                        completion.documentation =
                            (order == 0).then(|| format!("The next field of `{ty}`."));
                        completion
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some(Frame::Record {
            ty,
            field: Some(field),
            ..
        }) => {
            let Some((_, shape)) = fields(&schema, ty)
                .and_then(|fields| fields.iter().find(|(name, _)| name == field))
            else {
                return Vec::new();
            };
            let references = wanted(ty, field).map(names_for).unwrap_or_default();
            shape_completions(&schema, shape, &references)
        }
        Some(Frame::List { owner }) => {
            let Some((ty, field)) = owner else {
                return Vec::new();
            };
            let Some((_, shape)) = fields(&schema, ty)
                .and_then(|fields| fields.iter().find(|(name, _)| name == field))
            else {
                return Vec::new();
            };
            let references = wanted(ty, field).map(names_for).unwrap_or_default();
            shape_completions(&schema, shape, &references)
        }
        Some(Frame::Map {
            definition,
            field,
            seen,
        }) => {
            let Some((analysis, declaration)) = definition_params(workspace, uri, *definition)
            else {
                return Vec::new();
            };
            let params = analysis.symbols.iter().enumerate().filter(|(_, symbol)| {
                symbol.kind == SymbolKind::Param && symbol.parent == Some(declaration)
            });
            match field {
                None => params
                    .filter(|(_, symbol)| !seen.contains(&symbol.name))
                    .map(|(_, symbol)| {
                        let mut completion = Completion::new(&symbol.name, CompletionKind::Field)
                            .detail(symbol.detail.clone())
                            .insert(format!("{}: ", symbol.name));
                        completion.documentation = symbol.description.clone();
                        completion
                    })
                    .collect(),
                Some(param) => {
                    let Some((index, symbol)) =
                        params.into_iter().find(|(_, symbol)| &symbol.name == param)
                    else {
                        return Vec::new();
                    };
                    let options = analysis
                        .symbols
                        .iter()
                        .filter(|option| {
                            option.kind == SymbolKind::EnumOption && option.parent == Some(index)
                        })
                        .map(|option| Completion::new(&option.name, CompletionKind::EnumMember))
                        .collect::<Vec<_>>();
                    if !options.is_empty() {
                        return options;
                    }
                    let ty = symbol
                        .detail
                        .split_once(':')
                        .map_or("", |(_, ty)| ty.trim());
                    if ty.starts_with("bool") {
                        ["true", "false"]
                            .iter()
                            .map(|word| Completion::new(*word, CompletionKind::Keyword))
                            .collect()
                    } else if ty.starts_with("marks") {
                        names_for(Wanted::Marks)
                            .into_iter()
                            .map(|name| Completion::new(name, CompletionKind::Reference))
                            .collect()
                    } else if ty.starts_with("curve") || ty.starts_with("gradient") {
                        let wanted = if ty.starts_with("curve") {
                            "Curve"
                        } else {
                            "Gradient"
                        };
                        visible(workspace, text)
                            .into_iter()
                            .filter(|(_, kind)| kind == wanted)
                            .map(|(name, _)| Completion::new(name, CompletionKind::Reference))
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
            }
        }
        Some(Frame::Other) => Vec::new(),
    }
}

/// An outline entry: a name, its type, its name span, its whole span and
/// its children.
pub struct Outline {
    pub name: String,
    pub ty: String,
    pub span: TextSpan,
    pub extent: TextSpan,
    pub children: Vec<Outline>,
}

pub fn outline(text: &str) -> Vec<Outline> {
    fn items(value: &Spanned<DataValue>) -> Vec<Outline> {
        match &value.value {
            DataValue::Record(ty, fields) => {
                let children = fields
                    .value
                    .iter()
                    .flat_map(|field| items(&field.value))
                    .collect();
                match fields
                    .value
                    .iter()
                    .find_map(|field| match &field.value.value {
                        DataValue::Reference(segments)
                            if field.name.value.as_str() == "name" && segments.len() == 1 =>
                        {
                            Some(&segments[0])
                        }
                        _ => None,
                    }) {
                    Some(name) => vec![Outline {
                        name: name.value.as_str().to_string(),
                        ty: ty.value.as_str().to_string(),
                        span: name.span,
                        extent: ty.span.to(fields.span),
                        children,
                    }],
                    None => children,
                }
            }
            DataValue::Named(ty, name, fields) => vec![Outline {
                name: name.value.as_str().to_string(),
                ty: ty.value.as_str().to_string(),
                span: name.span,
                extent: ty.span.to(fields.span),
                children: fields
                    .value
                    .iter()
                    .flat_map(|field| items(&field.value))
                    .collect(),
            }],
            DataValue::List(values) | DataValue::Tuple(values) => {
                values.iter().flat_map(items).collect()
            }
            _ => Vec::new(),
        }
    }
    parse(text)
        .declarations
        .iter()
        .map(|declaration| Outline {
            name: declaration.name.value.as_str().to_string(),
            ty: declaration.ty.value.as_str().to_string(),
            span: declaration.name.span,
            extent: declaration.ty.span.to(declaration.fields.span),
            children: declaration
                .fields
                .value
                .iter()
                .flat_map(|field| items(&field.value))
                .collect(),
        })
        .collect()
}
