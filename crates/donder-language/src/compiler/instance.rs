//! A definition bound to one invocation. Instantiation substitutes what
//! preparation knows: fixed parameter values decide control flow, while other
//! values stay parameter slots so that differently configured instances still
//! share one program. Fusion and constant signals rewrite instances in the
//! global signal graph before they are lowered.
use super::check::Definition;
use super::ir::{
    Binary, Context, CurveQuery, Domain, Graph, Node, Op, Param, Rebuild, Substitute, Ternary,
    TooManyLoops, Unary,
};
use donder_runtime_types::BoundParams;
use donder_runtime_types::Color;
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{
    AutomationMapping, AutomationSource, Curve, CurvePoint, PreparedAutomation, Shared,
};
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

/// The live store words a console invocation's live binding reads: its live
/// automation's first word (its value; its integral's base and the time
/// that base holds from follow), and the word holding that integral at the
/// invocation's start, or none for an operator, which starts with the
/// session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LiveWords {
    pub value: u32,
    pub snapshot: Option<u32>,
}

/// An invocation prepared for the global signal graph.
#[derive(Clone, Debug)]
pub struct Instance {
    pub(crate) graph: Graph,
    pub(crate) root: Node,
    /// The value of every parameter slot. A slot left to playback holds its
    /// bound value, which playback reads in place.
    pub(crate) values: Vec<Value>,
}

impl Instance {
    /// `definition` with `params`, each automated parameter computed from
    /// its binding: an envelope from the query's time, a live binding from
    /// the live store words `live` names, in the invocation's live order.
    pub(crate) fn new(
        definition: &Definition,
        params: &BoundParams,
        automation: &[PreparedAutomation],
        constants: ProgramConstants,
        live: &[LiveWords],
    ) -> Result<Self, TooManyLoops> {
        let playback = |index: usize| {
            automation.iter().any(|binding| {
                usize::from(binding.param_index) == index
                    && binding.mapping == AutomationMapping::LiveMarks
            })
        };
        Self::build(definition, params, automation, constants, live, playback)
    }

    /// The instance with `automation`, and every parameter `playback` names
    /// left to playback.
    fn build(
        definition: &Definition,
        params: &BoundParams,
        automation: &[PreparedAutomation],
        constants: ProgramConstants,
        live: &[LiveWords],
        playback: impl Fn(usize) -> bool,
    ) -> Result<Self, TooManyLoops> {
        let mut slots: Vec<Param> = definition
            .graph
            .params()
            .iter()
            .enumerate()
            .map(|(index, param)| Param {
                ty: param.ty.clone(),
                domain: if playback(index) {
                    Domain::TIME
                } else {
                    Domain::PARAM
                },
                range: param.range,
            })
            .collect();
        let mut values = params.values().to_vec();
        // Context values fixed for this instance become slots too.
        let mut fixed = |slots: &mut Vec<Param>, ty, value| {
            slots.push(Param {
                ty,
                domain: Domain::PARAM,
                range: None,
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
        // A binding's timing, range, curve and live words are slots, so
        // instances that differ only in them share a program.
        let mut driven = Vec::new();
        for binding in automation {
            if binding.mapping == AutomationMapping::LiveMarks {
                continue;
            }
            let (min, max) = match binding.mapping {
                AutomationMapping::Float { min, max } | AutomationMapping::Curve { min, max } => {
                    (min, max)
                }
                AutomationMapping::Int { min, max } => (min as f32, max as f32),
                _ => (0.0, 1.0),
            };
            let mut float =
                |slots: &mut Vec<Param>, value: f32| fixed(slots, Type::Float, Value::Float(value));
            let (min, max) = (float(&mut slots, min), float(&mut slots, max));
            let source = match &binding.source {
                AutomationSource::Envelope {
                    start,
                    duration,
                    curve,
                } => {
                    let seconds = |ticks: u32| (f64::from(ticks) / 1_000_000.0) as f32;
                    let span = seconds(duration.as_ticks());
                    let curve = match binding.mapping {
                        // A window of a curve reads it already in its range.
                        AutomationMapping::Curve { min, max } => Shared::new(Curve {
                            points: curve
                                .points
                                .iter()
                                .map(|point| CurvePoint {
                                    position: point.position,
                                    value: min + (max - min) * point.value,
                                })
                                .collect(),
                        }),
                        _ => Shared::clone(curve),
                    };
                    Source::Envelope {
                        start: float(&mut slots, seconds(start.as_ticks())),
                        inverse: float(&mut slots, 1.0 / span),
                        duration: float(&mut slots, span),
                        curve: fixed(&mut slots, Type::Curve, Value::Curve(curve)),
                    }
                }
                AutomationSource::Live(index) => {
                    let words = live[usize::from(*index)];
                    let mut word = |slots: &mut Vec<Param>, word: u32| {
                        fixed(slots, Type::Int, Value::Int(word as i32))
                    };
                    Source::Live {
                        value: word(&mut slots, words.value),
                        base: word(&mut slots, words.value + 1),
                        since: word(&mut slots, words.value + 2),
                        snapshot: words.snapshot.map(|snapshot| word(&mut slots, snapshot)),
                    }
                }
            };
            driven.push(Driven {
                param: u32::from(binding.param_index),
                mapping: binding.mapping.clone(),
                min,
                max,
                source,
            });
        }
        let mut graph = Graph::new(slots, definition.graph.inputs());
        let driven_param =
            |index: usize| driven.iter().any(|driven| driven.param as usize == index);
        let mut substitute = Bind {
            values: values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    (!(playback(index) || driven_param(index))).then(|| value.clone())
                })
                .collect(),
            duration,
            pixel_count,
            driven,
        };
        let root =
            Rebuild::new(&definition.graph).node(&mut graph, definition.root, &mut substitute)?;
        Ok(Self {
            graph,
            root,
            values,
        })
    }

    /// `definition` with `params`, each parameter of `playback` left to
    /// playback.
    pub(crate) fn left_to_playback(
        definition: &Definition,
        params: &BoundParams,
        playback: &[usize],
    ) -> Self {
        Self::build(
            definition,
            params,
            &[],
            ProgramConstants::default(),
            &[],
            |index| playback.contains(&index),
        )
        .unwrap_or_else(|_| unreachable!("a definition's reductions fit its instance"))
    }

    /// The most general instance: every parameter is left to playback.
    pub(crate) fn generic(definition: &Definition) -> Self {
        let values: Vec<Value> = definition
            .graph
            .params()
            .iter()
            .map(|param| param.ty.default_value())
            .collect();
        let params = BoundParams::from_values(
            definition
                .graph
                .params()
                .iter()
                .map(|param| &param.ty)
                .zip(values),
        );
        Self::build(
            definition,
            &params,
            &[],
            ProgramConstants::default(),
            &[],
            |_| true,
        )
        .unwrap_or_else(|_| unreachable!("a definition's reductions fit its instance"))
    }

    pub fn inputs(&self) -> usize {
        self.graph.inputs() as usize
    }

    /// The color this instance produces at every time and pixel while every
    /// input is black, if the folds prove it is one color.
    pub fn black_input_color(&self) -> Option<Color> {
        (0..self.inputs())
            .fold(self.clone(), |instance, _| instance.with_black_input(0))
            .constant_color()
    }

    /// The color this instance always produces, if it is constant.
    pub fn constant_color(&self) -> Option<Color> {
        match self.graph.constant_value(self.root) {
            Some(Value::Color(color)) => Some(*color),
            _ => None,
        }
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
        Some(Self {
            graph,
            root,
            values,
        })
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
/// become their slots; automated parameters become their bindings' values.
struct Bind {
    values: Vec<Option<Value>>,
    duration: Option<u32>,
    pixel_count: Option<u32>,
    driven: Vec<Driven>,
}

/// An automated parameter, the slots of its range and what drives it.
struct Driven {
    param: u32,
    mapping: AutomationMapping,
    min: u32,
    max: u32,
    source: Source,
}

/// The slots of a binding's source.
enum Source {
    /// Its start and duration in show seconds, the inverse of the duration,
    /// and its curve, which a curve parameter's window holds already in its
    /// range.
    Envelope {
        start: u32,
        inverse: u32,
        duration: u32,
        curve: u32,
    },
    /// The int slots naming its live store words.
    Live {
        value: u32,
        base: u32,
        since: u32,
        snapshot: Option<u32>,
    },
}

impl Driven {
    fn range(&self, target: &mut Graph) -> (Node, Node) {
        (
            target.add(Op::Param(self.min)),
            target.add(Op::Param(self.max)),
        )
    }

    /// The slot a query of the curve parameter reads: an envelope's curve,
    /// or for a live value the parameter itself.
    fn curve_slot(&self) -> u32 {
        match self.source {
            Source::Envelope { curve, .. } => curve,
            Source::Live { .. } => self.param,
        }
    }

    /// The live store's word that slot `slot` names.
    fn live(target: &mut Graph, slot: u32) -> Node {
        let word = target.add(Op::Param(slot));
        target.unary(Unary::Live, word)
    }

    /// Where an envelope is at `time` after the invocation's origin, and
    /// where it was at the origin.
    fn positions(target: &mut Graph, start: u32, inverse: u32) -> (Node, Node) {
        let origin = target.add(Op::Context(Context::Origin));
        let start = target.add(Op::Param(start));
        let inverse = target.add(Op::Param(inverse));
        let offset = target.binary(Binary::Subtract, origin, start);
        let time = target.add(Op::Context(Context::Time));
        let elapsed = target.binary(Binary::Add, time, offset);
        (
            target.binary(Binary::Multiply, elapsed, inverse),
            target.binary(Binary::Multiply, offset, inverse),
        )
    }

    /// The binding's amount now, in 0..1.
    fn amount(&self, target: &mut Graph) -> Node {
        match self.source {
            Source::Envelope {
                start,
                inverse,
                curve,
                ..
            } => {
                let (position, _) = Self::positions(target, start, inverse);
                let curve = target.add(Op::Param(curve));
                target.binary(Binary::CurveSample, curve, position)
            }
            Source::Live { value, .. } => Self::live(target, value),
        }
    }

    /// The parameter's value now.
    fn value(&self, target: &mut Graph, ty: &Type) -> Node {
        let amount = self.amount(target);
        let (min, max) = self.range(target);
        match &self.mapping {
            AutomationMapping::Int { .. } => {
                let value = target.ternary(Ternary::Mix, min, max, amount);
                let rounded = round(target, value);
                target.unary(Unary::FloatToInt, rounded)
            }
            AutomationMapping::Bool => {
                let half = target.float(0.5);
                target.binary(Binary::GreaterEqual, amount, half)
            }
            AutomationMapping::Enum { values } => {
                let count = target.float(values.len() as f32);
                let scaled = target.binary(Binary::Multiply, amount, count);
                let floor = target.unary(Unary::Floor, scaled);
                let index = target.unary(Unary::FloatToInt, floor);
                let last = target.int(values.len() as i32 - 1);
                let index = target.binary(Binary::Min, index, last);
                let items = values
                    .iter()
                    .map(|value| target.typed_constant(Value::Enum(value.clone()), ty.clone()))
                    .collect();
                target.add(Op::Pick { index, items })
            }
            _ => target.ternary(Ternary::Mix, min, max, amount),
        }
    }

    /// The parameter's integral from the origin to now: its least value
    /// for the time since, plus its range times the amount's integral.
    fn integral(&self, target: &mut Graph) -> Node {
        let time = target.add(Op::Context(Context::Time));
        let area = match self.source {
            Source::Envelope {
                start,
                inverse,
                duration,
                curve,
            } => {
                let (position, origin) = Self::positions(target, start, inverse);
                let curve = target.add(Op::Param(curve));
                let now = target.binary(Binary::CurveIntegral, curve, position);
                let before = target.binary(Binary::CurveIntegral, curve, origin);
                let area = target.binary(Binary::Subtract, now, before);
                let duration = target.add(Op::Param(duration));
                target.binary(Binary::Multiply, duration, area)
            }
            // The amount's integral over the session is its base plus the
            // amount times the time since that base.
            Source::Live {
                value,
                base,
                since,
                snapshot,
            } => {
                let (value, base, since) = (
                    Self::live(target, value),
                    Self::live(target, base),
                    Self::live(target, since),
                );
                let origin = target.add(Op::Context(Context::Origin));
                let offset = target.binary(Binary::Subtract, origin, since);
                let held = target.binary(Binary::Add, offset, time);
                let rest = target.binary(Binary::Multiply, value, held);
                let now = target.binary(Binary::Add, base, rest);
                match snapshot {
                    Some(snapshot) => {
                        let start = Self::live(target, snapshot);
                        target.binary(Binary::Subtract, now, start)
                    }
                    None => now,
                }
            }
        };
        let (min, max) = self.range(target);
        let span = target.binary(Binary::Subtract, max, min);
        let varying = target.binary(Binary::Multiply, span, area);
        let least = target.binary(Binary::Multiply, min, time);
        target.binary(Binary::Add, least, varying)
    }

    /// `query` of its curve parameter: an envelope's window, its curve from
    /// where it is now on, or a live value's constant.
    fn query(&self, target: &mut Graph, query: CurveQuery) -> Node {
        let Source::Envelope {
            start,
            inverse,
            curve,
            ..
        } = self.source
        else {
            let value = self.value(target, &Type::Float);
            let (zero, missing) = (target.float(0.0), target.float(f32::NAN));
            return match query {
                CurveQuery::Sample(_) => value,
                CurveQuery::Integral(at) => target.binary(Binary::Multiply, value, at),
                CurveQuery::FirstCrossing(reached, after) => {
                    let after = target.binary(Binary::Max, after, zero);
                    let equal = target.binary(Binary::Equal, reached, value);
                    target.select(equal, after, missing)
                }
                CurveQuery::LastCrossing(reached, before) => {
                    let equal = target.binary(Binary::Equal, reached, value);
                    let started = target.binary(Binary::GreaterEqual, before, zero);
                    let at = target.select(started, zero, missing);
                    target.select(equal, at, missing)
                }
            };
        };
        let (position, _) = Self::positions(target, start, inverse);
        let curve = target.add(Op::Param(curve));
        let shift = |target: &mut Graph, at| target.binary(Binary::Add, at, position);
        match query {
            CurveQuery::Sample(at) => {
                let at = shift(target, at);
                CurveQuery::Sample(at).of(target, curve)
            }
            CurveQuery::Integral(at) => {
                let at = shift(target, at);
                let now = target.binary(Binary::CurveIntegral, curve, at);
                let start = target.binary(Binary::CurveIntegral, curve, position);
                target.binary(Binary::Subtract, now, start)
            }
            CurveQuery::FirstCrossing(value, after) => {
                // The window starts at its position.
                let zero = target.float(0.0);
                let after = target.binary(Binary::Max, after, zero);
                let after = shift(target, after);
                let at = CurveQuery::FirstCrossing(value, after).of(target, curve);
                target.binary(Binary::Subtract, at, position)
            }
            CurveQuery::LastCrossing(value, before) => {
                let before = shift(target, before);
                let at = CurveQuery::LastCrossing(value, before).of(target, curve);
                target.binary(Binary::Subtract, at, position)
            }
        }
    }
}

/// `value` rounded to the nearest whole number, halves away from zero.
fn round(target: &mut Graph, value: Node) -> Node {
    let whole = target.unary(Unary::Trunc, value);
    let fraction = target.binary(Binary::Subtract, value, whole);
    let fraction = target.unary(Unary::Abs, fraction);
    let half = target.float(0.5);
    let away = target.binary(Binary::GreaterEqual, fraction, half);
    let zero = target.float(0.0);
    let negative = target.binary(Binary::Less, value, zero);
    let (down, up) = (target.float(-1.0), target.float(1.0));
    let sign = target.select(negative, down, up);
    let next = target.binary(Binary::Add, whole, sign);
    target.select(away, next, whole)
}

impl Bind {
    /// What drives parameter `param`; the last binding of a parameter wins.
    fn driven(&self, param: u32) -> Option<&Driven> {
        self.driven
            .iter()
            .rev()
            .find(|driven| driven.param == param)
    }

    /// The automated curve parameter a query of slot `slot` reads.
    fn curve(&self, slot: u32) -> Option<&Driven> {
        self.driven.iter().find(|driven| {
            driven.curve_slot() == slot && matches!(driven.mapping, AutomationMapping::Curve { .. })
        })
    }

    /// Whether `curve` is an automated curve, or a choice of curves that may
    /// be one.
    fn reaches_curve(&self, target: &Graph, curve: Node) -> bool {
        match target.op(curve) {
            Op::Param(slot) => self.curve(*slot).is_some(),
            Op::Select(_, yes, no) => {
                self.reaches_curve(target, *yes) || self.reaches_curve(target, *no)
            }
            Op::Pick { items, .. } => items.iter().any(|&item| self.reaches_curve(target, item)),
            _ => false,
        }
    }

    /// `query` of `curve`, pushed through choices into each automated curve.
    fn query(&self, target: &mut Graph, curve: Node, query: CurveQuery) -> Node {
        match target.op(curve).clone() {
            Op::Param(slot) => match self.curve(slot) {
                Some(driven) => driven.query(target, query),
                None => query.of(target, curve),
            },
            Op::Select(condition, yes, no) => {
                let yes = self.query(target, yes, query);
                let no = self.query(target, no, query);
                target.select(condition, yes, no)
            }
            Op::Pick { index, items } => {
                let items = items
                    .iter()
                    .map(|&item| self.query(target, item, query))
                    .collect();
                target.add(Op::Pick { index, items })
            }
            _ => query.of(target, curve),
        }
    }
}

impl Substitute for Bind {
    fn param(&mut self, target: &mut Graph, index: u32) -> Node {
        match self.driven(index) {
            Some(driven) if matches!(driven.mapping, AutomationMapping::Curve { .. }) => {
                target.add(Op::Param(driven.curve_slot()))
            }
            Some(driven) => {
                let ty = target.params()[index as usize].ty.clone();
                driven.value(target, &ty)
            }
            None => target.add(Op::Param(index)),
        }
    }

    /// A parameter without automation integrates to `parameter * time`.
    fn param_integral(&mut self, target: &mut Graph, index: u32) -> Node {
        if let Some(driven) = self.driven(index) {
            return driven.integral(target);
        }
        let value = target.add(Op::Param(index));
        let time = target.add(Op::Context(Context::Time));
        target.binary(Binary::Multiply, value, time)
    }

    fn curve_query(&mut self, target: &mut Graph, curve: Node, query: CurveQuery) -> Option<Node> {
        self.reaches_curve(target, curve)
            .then(|| self.query(target, curve, query))
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
            core::cmp::Ordering::Equal => target.color(Color::BLACK),
            core::cmp::Ordering::Less => target.add(Op::Read { input, pixel }),
            core::cmp::Ordering::Greater => target.add(Op::Read {
                input: input - 1,
                pixel,
            }),
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
