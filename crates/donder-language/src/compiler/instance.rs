//! A definition bound to one invocation. Instantiation substitutes what
//! preparation knows: fixed parameter values decide control flow, while other
//! values stay parameter slots so that differently configured instances still
//! share one program. Fusion and constant signals rewrite instances in the
//! global signal graph before they are lowered.
use super::check::Definition;
use super::ir::{
    Binary, Context, Domain, Graph, Node, Op, Param, Rebuild, STATE, Substitute, TooManyLoops,
};
use donder_runtime_types::BoundParams;
use donder_runtime_types::Color;
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{AutomatedQuantity, PreparedAutomation};
use donder_runtime_types::{Type, Value};

/// Context values shared by every sample of a prepared invocation. An absent
/// value remains a context read (for example, differently sized fixtures).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProgramConstants {
    pub pixel_count: Option<i32>,
    pub duration_seconds: Option<f32>,
}

impl ProgramConstants {
    /// Bitwise equality.
    pub fn same(&self, other: &Self) -> bool {
        self.pixel_count == other.pixel_count
            && self.duration_seconds.map(f32::to_bits) == other.duration_seconds.map(f32::to_bits)
    }

    /// A hash consistent with [`Self::same`].
    pub fn hash_same<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        self.pixel_count.hash(state);
        self.duration_seconds.map(f32::to_bits).hash(state);
    }
}

/// An invocation prepared for the global signal graph.
#[derive(Clone, Debug)]
pub struct Instance {
    pub(crate) graph: Graph,
    pub(crate) root: Node,
    /// The value of every parameter slot. Automated slots hold their authored
    /// value; playback replaces it.
    pub(crate) values: Vec<Value>,
    pub(crate) automation: Vec<PreparedAutomation>,
}

impl Instance {
    pub(crate) fn new(
        definition: &Definition,
        params: &BoundParams,
        automation: &[PreparedAutomation],
        constants: ProgramConstants,
    ) -> Result<Self, TooManyLoops> {
        let automated = |index: usize| {
            automation
                .iter()
                .any(|binding| usize::from(binding.param_index) == index)
        };
        let mut slots: Vec<Param> = definition
            .graph
            .params()
            .iter()
            .enumerate()
            .map(|(index, param)| Param {
                ty: param.ty.clone(),
                domain: if automated(index) {
                    Domain::TIME
                } else {
                    Domain::PARAM
                },
            })
            .collect();
        let mut values = params.values().to_vec();
        // Context values fixed for this instance become slots too.
        let mut fixed = |slots: &mut Vec<Param>, ty, value| {
            slots.push(Param {
                ty,
                domain: Domain::PARAM,
            });
            values.push(value);
            slots.len() as u32 - 1
        };
        let duration = constants
            .duration_seconds
            .map(|seconds| fixed(&mut slots, Type::Float, Value::Float(seconds)));
        let pixel_count = constants
            .pixel_count
            .map(|count| fixed(&mut slots, Type::Int, Value::Int(count)));
        // The integral of an automated parameter is a slot that playback
        // writes from a copy of the parameter's binding.
        let mut automation = automation.to_vec();
        let mut integrals = Vec::new();
        for node in super::lower::reachable(&definition.graph, definition.root) {
            let Op::ParamIntegral(index) = *definition.graph.op(node) else {
                continue;
            };
            let Some(binding) = automation
                .iter()
                .find(|binding| u32::from(binding.param_index) == index)
            else {
                continue;
            };
            let slot = slots.len() as u32;
            let integral = PreparedAutomation {
                quantity: AutomatedQuantity::Integral,
                param_index: slot as u16,
                ..binding.clone()
            };
            slots.push(Param {
                ty: Type::Float,
                domain: Domain::TIME,
            });
            values.push(Value::Float(0.0));
            automation.push(integral);
            integrals.push((index, slot));
        }
        let mut graph = Graph::new(slots, definition.graph.inputs());
        let mut substitute = Bind {
            values: values
                .iter()
                .enumerate()
                .map(|(index, value)| (!automated(index)).then(|| value.clone()))
                .collect(),
            duration,
            pixel_count,
            integrals,
        };
        let root =
            Rebuild::new(&definition.graph).node(&mut graph, definition.root, &mut substitute)?;
        Ok(Self {
            graph,
            root,
            values,
            automation,
        })
    }

    /// The most general instance: every parameter is left to playback.
    pub(crate) fn generic(definition: &Definition) -> Self {
        let values: Vec<Value> = definition
            .graph
            .params()
            .iter()
            .map(|param| param.ty.default_value())
            .collect();
        let automation = (0..values.len())
            .map(|index| PreparedAutomation {
                start: donder_runtime_types::SampleTime::from_ticks(0),
                duration: donder_runtime_types::SampleDuration::from_ticks(1),
                curve: donder_runtime_types::Shared::new(donder_runtime_types::Curve {
                    points: Vec::new(),
                }),
                mapping: donder_runtime_types::AutomationMapping::Bool,
                quantity: AutomatedQuantity::Value,
                param_index: index as u16,
            })
            .collect::<Vec<_>>();
        let params = BoundParams::from_values(
            definition
                .graph
                .params()
                .iter()
                .map(|param| &param.ty)
                .zip(values),
        );
        Self::new(
            definition,
            &params,
            &automation,
            ProgramConstants::default(),
        )
        .unwrap_or_else(|_| unreachable!("a definition's reductions fit its instance"))
    }

    pub fn inputs(&self) -> usize {
        self.graph.inputs() as usize
    }

    /// The color this instance always produces, if it is constant.
    pub fn constant_color(&self) -> Option<Color> {
        match self.graph.constant_value(self.root) {
            Some(Value::Color(color)) => Some(*color),
            _ => None,
        }
    }

    /// Whether this instance, once its inputs are all black, is black at
    /// every frame: black while its previous frame is, which starts black.
    pub fn stays_black(&self) -> bool {
        let mut graph = Graph::new(self.graph.params().to_vec(), self.graph.inputs());
        let Ok(root) = Rebuild::new(&self.graph).node(&mut graph, self.root, &mut BlackState)
        else {
            return false;
        };
        matches!(graph.constant_value(root), Some(Value::Color(color)) if *color == Color::BLACK)
    }

    /// Replace every read of `input` with black, for a signal that is black
    /// everywhere. The input is removed and later inputs shift down.
    pub fn with_black_input(&self, input: usize) -> Self {
        let mut graph = Graph::new(self.graph.params().to_vec(), self.graph.inputs() - 1);
        let mut substitute = BlackInput {
            input: input as u32,
        };
        let root = Rebuild::new(&self.graph)
            .node(&mut graph, self.root, &mut substitute)
            .unwrap_or_else(|_| unreachable!("substitution keeps the reductions"));
        Self {
            graph,
            root,
            values: self.values.clone(),
            automation: self.automation.clone(),
        }
    }

    /// Substitute `upstream` into this instance's only read of `input`, on
    /// the current pixel; the upstream instance runs at the consumer's query
    /// and its inputs follow the consumer's remaining inputs. `None` keeps the
    /// boundary: several read sites, addressed pixels, upstream automation at
    /// another time, or a combined program beyond the row or nesting limits.
    pub fn fuse_input(&self, input: usize, upstream: &Self) -> Option<Self> {
        let fused = self.fuse(input as u32, upstream)?;
        fused.try_lower().ok()?;
        Some(fused)
    }

    fn fuse(&self, input: u32, upstream: &Self) -> Option<Self> {
        let sites: Vec<Node> = self
            .graph
            .nodes()
            .filter(|&node| self.reaches(node))
            .filter(|&node| self.graph.op(node).input() == Some(input))
            .collect();
        // One sample at the current pixel; a scan or a stencil reads its input
        // at other pixels of the fixture too.
        let [site] = sites.as_slice() else {
            return None;
        };
        let Op::Read {
            pixel: SignalPixel::Current,
            ..
        } = *self.graph.op(*site)
        else {
            return None;
        };
        // A node's previous frame is its own; fused, it would be another's.
        if self.reads_state() || upstream.reads_state() {
            return None;
        }
        let offset = self.graph.params().len() as u32;
        let mut params = self.graph.params().to_vec();
        params.extend(upstream.graph.params().iter().cloned());
        let inputs = self.graph.inputs() - 1 + upstream.graph.inputs();
        let mut graph = Graph::new(params, inputs);
        let mut substitute = Fuse {
            input,
            remaining: self.graph.inputs() - 1,
            offset,
            upstream,
        };
        let root = Rebuild::new(&self.graph)
            .node(&mut graph, self.root, &mut substitute)
            .ok()?;
        let mut values = self.values.clone();
        values.extend(upstream.values.iter().cloned());
        let mut automation = self.automation.clone();
        automation.extend(upstream.automation.iter().cloned().map(|mut binding| {
            binding.param_index += offset as u16;
            binding
        }));
        Some(Self {
            graph,
            root,
            values,
            automation,
        })
    }

    /// Whether the program reads its node's previous frame.
    fn reads_state(&self) -> bool {
        self.graph
            .nodes()
            .any(|node| self.graph.op(node).input() == Some(STATE) && self.reaches(node))
    }

    /// Whether `node` is reachable from the root, through reductions.
    fn reaches(&self, target: Node) -> bool {
        let mut pending = vec![self.root];
        let mut seen = std::collections::HashSet::new();
        while let Some(node) = pending.pop() {
            if node == target {
                return true;
            }
            if !seen.insert(node) {
                continue;
            }
            match self.graph.op(node) {
                Op::Reduce(id) => {
                    let data = self.graph.loop_(*id);
                    pending.extend([data.start, data.end]);
                    pending.extend(data.parts());
                }
                op => pending.extend(op.operands()),
            }
        }
        false
    }
}

/// Fixed values decide control flow; context values known to the instance
/// become their slots.
struct Bind {
    values: Vec<Option<Value>>,
    duration: Option<u32>,
    pixel_count: Option<u32>,
    /// The slot holding each automated parameter's integral.
    integrals: Vec<(u32, u32)>,
}

impl Substitute for Bind {
    /// A parameter without automation integrates to `parameter * time`.
    fn param_integral(&mut self, target: &mut Graph, index: u32) -> Node {
        match self.integrals.iter().find(|(param, _)| *param == index) {
            Some(&(_, slot)) => target.add(Op::Param(slot)),
            None => {
                let value = target.add(Op::Param(index));
                let time = target.add(Op::Context(Context::Time));
                target.binary(Binary::Multiply, value, time)
            }
        }
    }

    fn context(&mut self, target: &mut Graph, context: Context) -> Node {
        match (context, self.duration, self.pixel_count) {
            (Context::Duration, Some(slot), _) | (Context::TargetCount, _, Some(slot)) => {
                target.add(Op::Param(slot))
            }
            _ => target.add(Op::Context(context)),
        }
    }

    fn decide(&mut self, target: &Graph, node: Node) -> Option<Value> {
        if !target.domain(node).is_instance_fixed() || !target.loops_of(node).is_empty() {
            return None;
        }
        super::ir::evaluate(target, node, &self.values)
    }
}

struct BlackInput {
    input: u32,
}

impl Substitute for BlackInput {
    fn read(
        &mut self,
        target: &mut Graph,
        input: u32,
        pixel: SignalPixel<Node>,
    ) -> Result<Node, TooManyLoops> {
        Ok(match input.cmp(&self.input) {
            _ if input == STATE => target.add(Op::Read { input, pixel }),
            core::cmp::Ordering::Equal => target.color(Color::BLACK),
            core::cmp::Ordering::Less => target.add(Op::Read { input, pixel }),
            core::cmp::Ordering::Greater => target.add(Op::Read {
                input: input - 1,
                pixel,
            }),
        })
    }
}

/// Reads of the previous frame as black.
struct BlackState;

impl Substitute for BlackState {
    fn read(
        &mut self,
        target: &mut Graph,
        input: u32,
        pixel: SignalPixel<Node>,
    ) -> Result<Node, TooManyLoops> {
        Ok(if input == STATE {
            target.color(Color::BLACK)
        } else {
            target.add(Op::Read { input, pixel })
        })
    }
}

struct Fuse<'a> {
    input: u32,
    /// The consumer's inputs other than the fused one.
    remaining: u32,
    offset: u32,
    upstream: &'a Instance,
}

impl Substitute for Fuse<'_> {
    fn read(
        &mut self,
        target: &mut Graph,
        input: u32,
        pixel: SignalPixel<Node>,
    ) -> Result<Node, TooManyLoops> {
        if input != self.input {
            let input = if input > self.input { input - 1 } else { input };
            return Ok(target.add(Op::Read { input, pixel }));
        }
        let mut leaves = Leaves {
            offset: self.offset,
            first_input: self.remaining,
        };
        Rebuild::new(&self.upstream.graph).node(target, self.upstream.root, &mut leaves)
    }
}

/// A fused upstream instance's leaves: its parameters after the consumer's, and its
/// inputs after the consumer's remaining inputs.
struct Leaves {
    offset: u32,
    first_input: u32,
}

impl Substitute for Leaves {
    fn param(&mut self, target: &mut Graph, index: u32) -> Node {
        target.add(Op::Param(index + self.offset))
    }

    fn read(
        &mut self,
        target: &mut Graph,
        input: u32,
        pixel: SignalPixel<Node>,
    ) -> Result<Node, TooManyLoops> {
        Ok(target.add(Op::Read {
            input: input + self.first_input,
            pixel,
        }))
    }
}
