//! Compiled definitions and their invocations. A definition is checked IR; an
//! invocation binds it to validated values; an instance is an invocation at
//! one place in the global signal graph, where preparation may substitute
//! constant inputs and fuse sources before lowering it to a program.
use super::check::{self, Definition};
use super::declarations::check_ranges;
use super::instance::{Instance, ProgramConstants};
use super::ir::Op;
use super::ir::interval::{Bounds, interval};
use super::lower::LowerError;
use super::syntax::ast::DeclarationKind;
use super::{Diagnostic, OperatorInputDecl, ParamDecl, ParamRange};
use donder_runtime_types::PreparedAutomation;
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{BindingError, BoundParams, Identifier, Type, Value};
use std::sync::Arc;

/// A checked effect or operator declaration. An effect is an operator with
/// no signal inputs.
#[derive(Clone, Debug)]
pub struct CompiledDefinition(Arc<Definition>);

impl PartialEq for CompiledDefinition {
    fn eq(&self, other: &Self) -> bool {
        same_definition(&self.0, &other.0)
    }
}

fn same_definition(a: &Arc<Definition>, b: &Arc<Definition>) -> bool {
    Arc::ptr_eq(a, b)
        || (a.name == b.name && a.params == b.params && a.fingerprint == b.fingerprint)
}

/// Compile a script document's effect and operator declarations, in
/// declaration order.
pub fn compile_script(source: &str) -> Result<Vec<CompiledDefinition>, Vec<Diagnostic>> {
    let module = super::syntax::parse(source)?;
    let definitions = check::check(module, source)?;
    let diagnostics = lowering_diagnostics(&definitions);
    if diagnostics.is_empty() {
        Ok(definitions
            .into_iter()
            .map(|definition| CompiledDefinition(Arc::new(definition)))
            .collect())
    } else {
        Err(diagnostics)
    }
}

/// The checked definitions too large to lower.
pub(crate) fn lowering_diagnostics(definitions: &[Definition]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for definition in definitions {
        // The most general instance, with every parameter left to playback,
        // has the most code; specialization only removes work from it.
        let name = definition.name.as_str();
        let message = match Instance::generic(definition).try_lower() {
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
    diagnostics
}

impl CompiledDefinition {
    pub fn kind(&self) -> DeclarationKind {
        self.0.kind
    }

    pub fn name(&self) -> &Identifier {
        &self.0.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.0.params
    }

    /// The signal inputs an operator declares; an effect has none.
    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.0.inputs
    }

    /// The declaration's description string, if it has one.
    pub fn description(&self) -> Option<&str> {
        self.0.description.as_deref()
    }

    /// Positional `values` respect every declared range and the
    /// iteration limit of reductions bounded by parameter lengths.
    pub fn check_values(&self, values: &[Value]) -> Result<(), BindingError> {
        check_values(&self.0, values, &[])
    }

    /// Bind validated positional values and automation. An automated
    /// parameter's length is unknown, so no reduction may be bounded by a
    /// live marks parameter's.
    pub fn invoke(
        &self,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Invocation, BindingError> {
        invoke(&self.0, values, automation)
    }
}

fn check_values(
    definition: &Definition,
    values: &[Value],
    automation: &[PreparedAutomation],
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
            _ if automation
                .iter()
                .any(|binding| usize::from(binding.param_index) == index) =>
            {
                None
            }
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
) -> Result<Invocation, BindingError> {
    check_values(definition, &values, &automation)?;
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
    Ok(Invocation {
        definition: Arc::clone(definition),
        params,
        automation,
    })
}

/// A definition bound to validated values and automation.
#[derive(Clone, Debug)]
pub struct Invocation {
    definition: Arc<Definition>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
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
                Op::Scan { .. } | Op::Tap { .. } => true,
                Op::Read { pixel, .. } => !matches!(pixel, SignalPixel::Current),
                _ => false,
            })
    }

    /// This invocation at one place in a prepared sequence.
    pub fn instance(&self, constants: ProgramConstants) -> Instance {
        Instance::new(&self.definition, &self.params, &self.automation, constants)
            .unwrap_or_else(|_| unreachable!("a definition's reductions fit its instance"))
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
    }
}
