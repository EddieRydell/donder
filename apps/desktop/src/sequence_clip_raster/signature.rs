use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RenderInputSignature {
    Valid(Box<RenderInputSignatureData>),
    Invalid { message: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RenderInputSignatureData {
    effect: EffectInst,
    automation_clips: Vec<AutomationInputSignature>,
    definition: Option<EffectDefinition>,
    generator_definitions: Vec<(dawn_language::effect::EffectDefinitionId, EffectDefinition)>,
    curve_references: Vec<(CurveId, Option<CurveDefinition>)>,
    gradient_references: Vec<(GradientId, Option<GradientDefinition>)>,
    mark_references: Vec<(MarkCollectionKey, Option<Vec<DawnTime>>)>,
    target_pixels: Vec<RenderedTargetPixelAddress>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AutomationInputSignature {
    clip_id: u32,
    start: DawnTime,
    duration: dawn_language::values::DawnDuration,
    curve: Curve,
    bindings: Vec<AutomationBinding>,
}

pub(super) fn render_signature(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence: &Sequence,
    effect: &EffectInst,
) -> Result<RenderInputSignature, String> {
    let target_pixels =
        resolve_effect_target_pixel_addresses(project, setup_id, &effect.target, &effect.scope)
            .map_err(|error| format!("{error:?}"))?;
    let definition = project
        .definitions
        .effects
        .resolve(&effect.definition)
        .cloned();
    let generator_definitions = if definition
        .as_ref()
        .is_some_and(|definition| definition.kind == EffectKind::Generator)
    {
        project
            .definitions
            .effects
            .definitions
            .iter()
            .map(|(id, definition)| (id.clone(), definition.clone()))
            .collect()
    } else {
        Vec::new()
    };
    let mut curve_references = Vec::new();
    let mut gradient_references = Vec::new();
    let mut mark_references = Vec::new();
    for value in effect.param_overrides.values() {
        collect_param_references(
            project,
            sequence,
            value,
            &mut curve_references,
            &mut gradient_references,
            &mut mark_references,
        );
    }
    let automation_clips = sequence
        .automation_clips
        .iter()
        .filter_map(|clip| {
            let bindings = clip
                .bindings
                .iter()
                .filter(|binding| {
                    binding
                        .effect_param()
                        .is_some_and(|(effect_id, _)| effect_id == &effect.id)
                })
                .cloned()
                .collect::<Vec<_>>();
            (!bindings.is_empty()).then(|| AutomationInputSignature {
                clip_id: clip.id.0,
                start: clip.start.clone(),
                duration: clip.duration.clone(),
                curve: clip.curve.clone(),
                bindings,
            })
        })
        .collect();
    Ok(RenderInputSignature::Valid(Box::new(
        RenderInputSignatureData {
            effect: effect.clone(),
            automation_clips,
            definition,
            generator_definitions,
            curve_references,
            gradient_references,
            mark_references,
            target_pixels,
        },
    )))
}

pub(super) fn collect_param_references(
    project: &DawnProject,
    sequence: &Sequence,
    value: &EffectParamValue,
    curve_references: &mut Vec<(CurveId, Option<CurveDefinition>)>,
    gradient_references: &mut Vec<(GradientId, Option<GradientDefinition>)>,
    mark_references: &mut Vec<(MarkCollectionKey, Option<Vec<DawnTime>>)>,
) {
    match value {
        EffectParamValue::Curve(CurveSource::Reference(id)) => {
            curve_references.push((id.clone(), project.definitions.curves.get(id).cloned()));
        }
        EffectParamValue::Gradient(GradientSource::Reference(id)) => {
            gradient_references.push((id.clone(), project.definitions.gradients.get(id).cloned()));
        }
        EffectParamValue::Marks(key) => {
            let marks = sequence
                .mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .map(|collection| collection.marks.clone());
            mark_references.push((key.clone(), marks));
        }
        EffectParamValue::Array(values) => {
            for value in values {
                collect_param_references(
                    project,
                    sequence,
                    value,
                    curve_references,
                    gradient_references,
                    mark_references,
                );
            }
        }
        EffectParamValue::Int(_)
        | EffectParamValue::Float(_)
        | EffectParamValue::Bool(_)
        | EffectParamValue::Color(_)
        | EffectParamValue::Enum(_)
        | EffectParamValue::Curve(CurveSource::Inline(_))
        | EffectParamValue::Gradient(GradientSource::Inline(_)) => {}
    }
}

pub(super) fn signature_key(signature: &RenderInputSignature) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_render_signature(signature, &mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn raster_token(cache_key: &RasterCacheKey, signature_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cache_key.hash(&mut hasher);
    signature_key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn hash_effect_raster_settings<H: Hasher>(
    settings: &EffectRasterSettings,
    state: &mut H,
) {
    settings.render_scale.to_bits().hash(state);
    settings.max_columns.hash(state);
    settings.max_rows.hash(state);
    settings.min_frame_stride.hash(state);
}

pub(super) fn hash_render_signature<H: Hasher>(signature: &RenderInputSignature, state: &mut H) {
    match signature {
        RenderInputSignature::Valid(data) => {
            0u8.hash(state);
            hash_effect_inst(&data.effect, state);
            data.automation_clips.len().hash(state);
            for clip in &data.automation_clips {
                hash_automation_input_signature(clip, state);
            }
            hash_optional_effect_definition(&data.definition, state);
            data.generator_definitions.len().hash(state);
            for (id, definition) in &data.generator_definitions {
                id.hash(state);
                hash_effect_definition(definition, state);
            }
            data.curve_references.len().hash(state);
            for (id, definition) in &data.curve_references {
                id.hash(state);
                hash_optional_curve_definition(definition, state);
            }
            data.gradient_references.len().hash(state);
            for (id, definition) in &data.gradient_references {
                id.hash(state);
                hash_optional_gradient_definition(definition, state);
            }
            data.mark_references.len().hash(state);
            for (key, marks) in &data.mark_references {
                key.hash(state);
                match marks {
                    Some(marks) => {
                        1u8.hash(state);
                        hash_marks(marks, state);
                    }
                    None => 0u8.hash(state),
                }
            }
            data.target_pixels.len().hash(state);
            for pixel in &data.target_pixels {
                pixel.fixture_id.hash(state);
                pixel.fixture_pixel_index.hash(state);
            }
        }
        RenderInputSignature::Invalid { message } => {
            1u8.hash(state);
            message.hash(state);
        }
    }
}

pub(super) fn hash_automation_input_signature<H: Hasher>(
    clip: &AutomationInputSignature,
    state: &mut H,
) {
    clip.clip_id.hash(state);
    clip.start.0.hash(state);
    clip.duration.0.hash(state);
    hash_curve(&clip.curve, state);
    clip.bindings.len().hash(state);
    for binding in &clip.bindings {
        binding.target.hash(state);
        hash_automation_mapping(&binding.mapping, state);
    }
}

pub(super) fn hash_automation_mapping<H: Hasher>(mapping: &AutomationMapping, state: &mut H) {
    match mapping {
        AutomationMapping::Float { min, max } => {
            0u8.hash(state);
            min.to_bits().hash(state);
            max.to_bits().hash(state);
        }
        AutomationMapping::Int { min, max } => {
            1u8.hash(state);
            min.hash(state);
            max.hash(state);
        }
        AutomationMapping::Bool => {
            2u8.hash(state);
        }
        AutomationMapping::Enum { values } => {
            3u8.hash(state);
            values.len().hash(state);
            for value in values {
                value.hash(state);
            }
        }
        AutomationMapping::Curve { min, max } => {
            4u8.hash(state);
            min.to_bits().hash(state);
            max.to_bits().hash(state);
        }
    }
}

pub(super) fn hash_effect_inst<H: Hasher>(effect: &EffectInst, state: &mut H) {
    effect.id.hash(state);
    effect.start.0.hash(state);
    effect.duration.0.hash(state);
    hash_effect_target(&effect.target, state);
    hash_effect_scope(&effect.scope, state);
    effect.definition.hash(state);
    effect.param_overrides.len().hash(state);
    for (key, value) in &effect.param_overrides {
        key.hash(state);
        hash_effect_param_value(value, state);
    }
}

pub(super) fn hash_effect_target<H: Hasher>(
    target: &dawn_language::layout::FixtureTarget,
    state: &mut H,
) {
    target.hash(state);
}

pub(super) fn hash_effect_scope<H: Hasher>(scope: &EffectScope, state: &mut H) {
    match scope {
        EffectScope::PerFixture => 0u8.hash(state),
        EffectScope::WholeTarget => 1u8.hash(state),
    }
}

pub(super) fn hash_effect_param_value<H: Hasher>(value: &EffectParamValue, state: &mut H) {
    match value {
        EffectParamValue::Int(value) => {
            0u8.hash(state);
            value.hash(state);
        }
        EffectParamValue::Float(value) => {
            1u8.hash(state);
            value.to_bits().hash(state);
        }
        EffectParamValue::Bool(value) => {
            2u8.hash(state);
            value.hash(state);
        }
        EffectParamValue::Color(value) => {
            3u8.hash(state);
            value.hash(state);
        }
        EffectParamValue::Enum(value) => {
            4u8.hash(state);
            value.hash(state);
        }
        EffectParamValue::Marks(key) => {
            5u8.hash(state);
            key.hash(state);
        }
        EffectParamValue::Curve(source) => {
            6u8.hash(state);
            hash_curve_source(source, state);
        }
        EffectParamValue::Gradient(source) => {
            7u8.hash(state);
            hash_gradient_source(source, state);
        }
        EffectParamValue::Array(values) => {
            8u8.hash(state);
            values.len().hash(state);
            for value in values {
                hash_effect_param_value(value, state);
            }
        }
    }
}

pub(super) fn hash_gradient_source<H: Hasher>(source: &GradientSource, state: &mut H) {
    match source {
        GradientSource::Inline(gradient) => {
            0u8.hash(state);
            hash_gradient(gradient, state);
        }
        GradientSource::Reference(id) => {
            1u8.hash(state);
            id.hash(state);
        }
    }
}

pub(super) fn hash_curve_source<H: Hasher>(source: &CurveSource, state: &mut H) {
    match source {
        CurveSource::Inline(curve) => {
            0u8.hash(state);
            hash_curve(curve, state);
        }
        CurveSource::Reference(id) => {
            1u8.hash(state);
            id.hash(state);
        }
    }
}

pub(super) fn hash_optional_effect_definition<H: Hasher>(
    definition: &Option<EffectDefinition>,
    state: &mut H,
) {
    match definition {
        Some(definition) => {
            1u8.hash(state);
            hash_effect_definition(definition, state);
        }
        None => 0u8.hash(state),
    }
}

pub(super) fn hash_effect_definition<H: Hasher>(definition: &EffectDefinition, state: &mut H) {
    definition.generated_effect_targets.len().hash(state);
    for target in &definition.generated_effect_targets {
        target.hash(state);
    }
    match &definition.implementation {
        dawn_language::effect::EffectImplementation::Native(builtin) => builtin.hash(state),
        dawn_language::effect::EffectImplementation::Dsl(compiled) => {
            hash_compiled_effect(compiled, state)
        }
    }
}

pub(super) fn hash_optional_curve_definition<H: Hasher>(
    definition: &Option<CurveDefinition>,
    state: &mut H,
) {
    match definition {
        Some(definition) => {
            1u8.hash(state);
            hash_curve(&definition.curve, state);
        }
        None => 0u8.hash(state),
    }
}

pub(super) fn hash_optional_gradient_definition<H: Hasher>(
    definition: &Option<GradientDefinition>,
    state: &mut H,
) {
    match definition {
        Some(definition) => {
            1u8.hash(state);
            hash_gradient(&definition.gradient, state);
        }
        None => 0u8.hash(state),
    }
}

pub(super) fn hash_gradient<H: Hasher>(gradient: &Gradient, state: &mut H) {
    gradient.stops.len().hash(state);
    for stop in &gradient.stops {
        stop.position.to_bits().hash(state);
        stop.color.hash(state);
    }
}

pub(super) fn hash_curve<H: Hasher>(curve: &Curve, state: &mut H) {
    curve.points.len().hash(state);
    for point in &curve.points {
        point.position.to_bits().hash(state);
        point.value.to_bits().hash(state);
    }
}

pub(super) fn hash_marks<H: Hasher>(marks: &[DawnTime], state: &mut H) {
    marks.len().hash(state);
    for mark in marks {
        mark.0.hash(state);
    }
}
