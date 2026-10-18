//! Compiled definitions and their invocations. A definition is checked IR; an
//! invocation binds it to validated values; an instance is an invocation at
//! one place in the global signal graph, where preparation may substitute
//! constant inputs and fuse sources before lowering it to a program.
use super::check::{self, Definition};
use super::declarations::{check_ranges, resolve_params};
use super::instance::{self, ProgramConstants};
use super::ir::Op;
use super::ir::interval::{Bounds, interval};
use super::lower::LowerError;
use super::syntax::ast::DeclarationKind;
use super::{Diagnostic, OperatorInputDecl, ParamDecl, ParamRange};
use donder_runtime_types::Color;
use donder_runtime_types::PreparedAutomation;
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{
    BindingError, BoundParams, Identifier, OperatorInvocation, OperatorProgram, SampleInvocation,
    SampleProgram, Type, Value,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CompiledEffect(Arc<Definition>);

#[derive(Clone, Debug)]
pub struct CompiledOperator(Arc<Definition>);

impl PartialEq for CompiledEffect {
    fn eq(&self, other: &Self) -> bool {
        same_definition(&self.0, &other.0)
    }
}

impl PartialEq for CompiledOperator {
    fn eq(&self, other: &Self) -> bool {
        same_definition(&self.0, &other.0)
    }
}

fn same_definition(a: &Arc<Definition>, b: &Arc<Definition>) -> bool {
    Arc::ptr_eq(a, b)
        || (a.name == b.name && a.params == b.params && a.fingerprint == b.fingerprint)
}

/// The definitions of one script document, each kind in declaration order.
#[derive(Clone, Debug, Default)]
pub struct CompiledScript {
    pub effects: Vec<CompiledEffect>,
    pub operators: Vec<CompiledOperator>,
}

/// Compile a script document, which may declare effects and operators.
pub fn compile_script(source: &str) -> Result<CompiledScript, Vec<Diagnostic>> {
    let mut script = CompiledScript::default();
    for definition in compile(source, None)? {
        match definition.kind {
            DeclarationKind::Effect => script.effects.push(CompiledEffect(definition)),
            DeclarationKind::Operator => script.operators.push(CompiledOperator(definition)),
        }
    }
    Ok(script)
}

/// Compile the effect declarations of one source.
pub fn compile_effects(source: &str) -> Result<Vec<CompiledEffect>, Vec<Diagnostic>> {
    Ok(compile(source, Some(DeclarationKind::Effect))?
        .into_iter()
        .map(CompiledEffect)
        .collect())
}

/// Compile the operator declarations of one source.
pub fn compile_operators(source: &str) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    Ok(compile(source, Some(DeclarationKind::Operator))?
        .into_iter()
        .map(CompiledOperator)
        .collect())
}

fn compile(
    source: &str,
    kind: Option<DeclarationKind>,
) -> Result<Vec<Arc<Definition>>, Vec<Diagnostic>> {
    let module = super::syntax::parse(source)?;
    if let Some(kind) = kind
        && let Some(declaration) = module
            .declarations
            .iter()
            .find(|declaration| declaration.kind != kind)
    {
        let message = match kind {
            DeclarationKind::Effect => "operator declarations belong in operator sources",
            DeclarationKind::Operator => "effect declarations belong in effect sources",
        };
        return Err(vec![Diagnostic::new(declaration.name.span, message)]);
    }
    let definitions = check::check(module)?;
    let mut diagnostics = Vec::new();
    for definition in &definitions {
        // The most general instance, with every parameter left to playback,
        // has the most code; specialization only removes work from it.
        let name = definition.name.as_str();
        let message = match instance::Instance::generic(definition).lower() {
            Err(LowerError::Rows(bytes)) => Some(format!(
                "`{name}` needs {bytes} bytes of per-pixel values; a program has at most {}",
                donder_runtime_types::bytecode::MAX_ROW_BYTES
            )),
            Err(LowerError::Depth(depth)) => Some(format!(
                "`{name}` nests {depth} per-pixel choices and reductions; a program has at most {}",
                donder_runtime_types::bytecode::MAX_DEPTH
            )),
            Err(LowerError::Slots | LowerError::Code) => Some(format!("`{name}` is too large")),
            Err(LowerError::Loops) | Ok(_) => None,
        };
        if let Some(message) = message {
            diagnostics.push(Diagnostic::new(definition.span, message));
        }
    }
    if diagnostics.is_empty() {
        Ok(definitions.into_iter().map(Arc::new).collect())
    } else {
        Err(diagnostics)
    }
}

macro_rules! definition_api {
    ($type:ident) => {
        impl $type {
            pub fn name(&self) -> &Identifier {
                &self.0.name
            }

            pub fn params(&self) -> &[ParamDecl] {
                &self.0.params
            }

            /// The declaration's description string, if it has one.
            pub fn description(&self) -> Option<&str> {
                self.0.description.as_deref()
            }

            /// A digest of the compiled behavior, for caches of rendered output.
            pub fn fingerprint(&self) -> u64 {
                self.0.fingerprint
            }

            /// Positional `values` respect every declared range and the
            /// iteration limit of reductions bounded by parameter lengths.
            pub fn check_values(&self, values: &[Value]) -> Result<(), BindingError> {
                check_values(&self.0, values, &[])
            }

            /// Bind validated positional values and automation.
            pub fn invoke(
                &self,
                values: Vec<Value>,
                automation: Box<[PreparedAutomation]>,
            ) -> Result<Invocation, BindingError> {
                invoke(&self.0, values, automation, Box::new([]))
            }

            /// Bind values and automation, with `live` marks parameters that
            /// playback rebinds as marks arrive. Their length is unknown, so
            /// no reduction may be bounded by it.
            pub fn invoke_live(
                &self,
                values: Vec<Value>,
                automation: Box<[PreparedAutomation]>,
                live: Box<[u16]>,
            ) -> Result<Invocation, BindingError> {
                invoke(&self.0, values, automation, live)
            }

            /// Bind values by name, with declared defaults for the rest.
            pub fn bind<'p, P>(&self, params: P) -> Result<Invocation, BindingError>
            where
                P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
            {
                let values = resolve_params(&self.0.params, params)?;
                self.invoke(values, Box::new([]))
            }
        }
    };
}

definition_api!(CompiledEffect);
definition_api!(CompiledOperator);

impl CompiledOperator {
    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.0.inputs
    }
}

fn check_values(
    definition: &Definition,
    values: &[Value],
    live: &[u16],
) -> Result<(), BindingError> {
    check_ranges(&definition.params, values)?;
    if definition.length_bounds.is_empty() {
        return Ok(());
    }
    let ranges: Vec<_> = definition
        .params
        .iter()
        .map(|param| match (&param.ty, param.range) {
            (Type::Int, Some(ParamRange::Int { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            (Type::Float, Some(ParamRange::Float { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            _ => None,
        })
        .collect();
    let lengths: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            _ if live.iter().any(|&param| usize::from(param) == index) => None,
            Value::Array(items) => Some(items.len()),
            Value::Marks(marks) => Some(marks.len()),
            _ => None,
        })
        .collect();
    let bounds = Bounds {
        ranges: &ranges,
        lengths: &lengths,
    };
    for &count in &definition.length_bounds {
        let range = interval(&definition.graph, count, &bounds);
        if range.max.is_nan() || range.max > donder_runtime_types::bytecode::MAX_ITERATIONS as f64 {
            return Err(BindingError {
                message: format!(
                    "a reduction can run {} times with these parameter values; the limit is {}",
                    if range.max.is_finite() {
                        range.max.to_string()
                    } else {
                        "unboundedly many".into()
                    },
                    donder_runtime_types::bytecode::MAX_ITERATIONS
                ),
            });
        }
    }
    Ok(())
}

fn invoke(
    definition: &Arc<Definition>,
    values: Vec<Value>,
    automation: Box<[PreparedAutomation]>,
    live: Box<[u16]>,
) -> Result<Invocation, BindingError> {
    check_values(definition, &values, &live)?;
    let types: Vec<_> = definition
        .params
        .iter()
        .map(|param| param.ty.clone())
        .collect();
    let params = BoundParams::bind_values(&types, values)?;
    if !params.accepts_automation(&automation) {
        return Err(BindingError {
            message: "automation does not match its declaration".into(),
        });
    }
    let marks = |&param: &u16| {
        definition
            .params
            .get(usize::from(param))
            .is_some_and(|declared| declared.ty == Type::Marks)
    };
    if !live.iter().all(marks) {
        return Err(BindingError {
            message: "live mark collections bind marks parameters only".into(),
        });
    }
    Ok(Invocation {
        definition: Arc::clone(definition),
        params,
        automation,
        live,
    })
}

/// A definition bound to validated values and automation.
#[derive(Clone, Debug)]
pub struct Invocation {
    definition: Arc<Definition>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
    live: Box<[u16]>,
}

impl Invocation {
    pub fn params(&self) -> &BoundParams {
        &self.params
    }

    pub fn automation(&self) -> &[PreparedAutomation] {
        &self.automation
    }

    /// Whether it samples pixels of the current fixture other than the
    /// current one: by index, by offset, or along it in a scan.
    pub fn reads_fixture_pixels(&self) -> bool {
        let graph = &self.definition.graph;
        super::lower::reachable(graph, self.definition.root)
            .into_iter()
            .any(|node| match graph.op(node) {
                Op::Scan { .. } => true,
                Op::Sample { pixel, .. } => !matches!(pixel, SignalPixel::Current),
                _ => false,
            })
    }

    /// This invocation at one place in a prepared sequence.
    pub fn instance(&self, constants: ProgramConstants) -> Instance {
        Instance(
            instance::Instance::new(
                &self.definition,
                &self.params,
                &self.automation,
                &self.live,
                constants,
            )
            .unwrap_or_else(|_| unreachable!("a definition's reductions fit its instance")),
        )
    }

    /// Whether `other` prepares identically: the same definition, bitwise
    /// equal values and the same automation.
    pub fn same(&self, other: &Self) -> bool {
        same_definition(&self.definition, &other.definition)
            && self.params.types() == other.params.types()
            && self
                .params
                .values()
                .iter()
                .zip(other.params.values())
                .all(|(a, b)| a.same(b))
            && self.automation.len() == other.automation.len()
            && self
                .automation
                .iter()
                .zip(other.automation.iter())
                .all(|(a, b)| a.same(b))
            && self.live == other.live
    }

    /// A hash consistent with [`Self::same`].
    pub fn hash_same<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        self.definition.name.hash(state);
        self.definition.fingerprint.hash(state);
        self.params.types().hash(state);
        for value in self.params.values() {
            value.hash_same(state);
        }
        self.automation.len().hash(state);
        for automation in self.automation.iter() {
            automation.hash_same(state);
        }
        self.live.hash(state);
    }
}

/// An invocation prepared for the global signal graph.
#[derive(Clone, Debug)]
pub struct Instance(instance::Instance);

impl Instance {
    pub fn inputs(&self) -> usize {
        self.0.inputs() as usize
    }

    /// The color this instance always produces, if it is constant.
    pub fn constant_color(&self) -> Option<Color> {
        self.0.constant_color()
    }

    /// Whether this instance without inputs is black at every frame.
    pub fn stays_black(&self) -> bool {
        self.0.stays_black()
    }

    /// Replace an input that is black everywhere; later inputs shift down.
    pub fn with_black_input(&self, input: usize) -> Self {
        Self(
            self.0
                .with_black_input(input as u32)
                .unwrap_or_else(|_| unreachable!("substitution keeps the reductions")),
        )
    }

    /// Substitute `upstream` into this instance's only sample of `input`, on
    /// the current pixel. `None` keeps the boundary: several sample sites,
    /// addressed pixels, upstream automation at another time, or a combined
    /// program beyond the row or nesting limits.
    pub fn fuse_input(&self, input: usize, upstream: &Self) -> Option<Self> {
        let fused = self.0.fuse_input(input as u32, &upstream.0)?;
        fused.lower().ok()?;
        Some(Self(fused))
    }

    /// The prepared dataflow graph, its execution plan and its bytecode.
    pub fn explain(&self) -> String {
        self.0
            .explain()
            .unwrap_or_else(|error| format!("lowering failed: {error:?}"))
    }

    pub fn sample(&self) -> SampleInvocation {
        self.sample_live().0
    }

    /// The program, and its parameter indices of the live marks parameters
    /// in invocation order.
    pub fn sample_live(&self) -> (SampleInvocation, Box<[u16]>) {
        let lowered = self
            .0
            .lower()
            .unwrap_or_else(|error| unreachable!("checked definitions fit their banks: {error:?}"));
        let program = SampleProgram::new(lowered.bytecode);
        let invocation = SampleInvocation::bind(program, lowered.values)
            .and_then(|invocation| invocation.with_automation(lowered.automation.into()))
            .unwrap_or_else(|_| unreachable!("lowered values match their program"));
        (invocation, lowered.live.into())
    }

    pub fn operator(&self) -> OperatorInvocation {
        let inputs = self.inputs();
        let lowered = self
            .0
            .lower()
            .unwrap_or_else(|error| unreachable!("fused operators fit their banks: {error:?}"));
        let program = OperatorProgram::new(lowered.bytecode, inputs);
        OperatorInvocation::bind(program, lowered.values)
            .and_then(|invocation| invocation.with_automation(lowered.automation.into()))
            .unwrap_or_else(|_| unreachable!("lowered values match their program"))
    }
}
