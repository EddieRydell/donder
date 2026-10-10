//! Strip bytecode. The query and target stages become scalar blocks and the
//! pixel region tree becomes the body: a branch is a `Branch` with its arms
//! inline, a reduction a `Reduce` with its parts inline. A value that varies
//! by pixel or signal is a row; every other value is a scalar.
use super::schedule::{Plan, RegionId, Stage, is_leaf};
use super::{LowerError, slots};
use crate::compiler::ir::{
    self, Binary, Constant, Context, Domain, Graph, Node, Op, Ternary, Unary,
};
use donder_runtime_types::Shared as Arc;
use donder_runtime_types::bytecode::{
    Bank, BytecodeProgram, ColorBinary, ColorComponent, CompareOp, ContextRead, FloatBinary,
    FloatUnary, Input, Instruction, IntBinary, MAX_DEPTH, MAX_ROW_BYTES, MarkOp, NO_FRAME_CACHE,
    Reducer, Resource, SignalPixel, Slot, Span, param_bank,
};
use donder_runtime_types::{Color, Identifier, Type, Value};
use std::collections::{HashMap, HashSet};

pub(super) fn emit(graph: &Graph, root: Node, plan: &Plan) -> Result<BytecodeProgram, LowerError> {
    let mut emitter = Emitter::new(graph, plan);
    let nodes = super::reachable(graph, root);
    emitter.absorb(&nodes);
    // Every bound enum value, read or automated, has an option index.
    for param in graph.params() {
        emitter.enum_options(&param.ty);
    }
    for &node in &nodes {
        emitter.enum_options(graph.ty(node));
    }
    let mut query = Vec::new();
    for &node in &plan.query {
        if !emitter.absorbed.contains(&node) {
            emitter.node(node, &mut query);
        }
    }
    let mut target = Vec::new();
    for &node in &plan.target {
        if !emitter.absorbed.contains(&node) {
            emitter.node(node, &mut target);
        }
    }
    let mut body = Vec::new();
    emitter.region(0, &mut body);
    let result = emitter.operand(root);
    emitter.finish(query, target, body, result)
}

struct Emitter<'a> {
    graph: &'a Graph,
    plan: &'a Plan,
    values: HashMap<Node, Slot>,
    /// Requested destinations: a branch arm's value written in place.
    targets: HashMap<Node, Slot>,
    absorbed: HashSet<Node>,
    /// Constants and resource leaves, loaded once before the query stage.
    head: Vec<Instruction>,
    constants: HashMap<Constant, Slot>,
    leaves: HashMap<Node, Slot>,
    /// Virtual slots allocated so far, by bank and kind.
    next: HashMap<(Bank, bool), u16>,
    curves: Vec<Arc<donder_runtime_types::Curve>>,
    gradients: Vec<Arc<donder_runtime_types::Gradient>>,
    marks: Vec<Arc<donder_runtime_types::Marks>>,
    arrays: Vec<Arc<[Value]>>,
    enums: Vec<Identifier>,
    operands: Vec<Slot>,
    /// Whole-frame input caches, one per input and query-uniform time: samples
    /// of one input at one time share its frame.
    frame_caches: HashMap<(u32, Node), u16>,
    /// Frame caches so far: the inputs', then each scan's own.
    frame_cache_count: u16,
    /// Selections open at the current point, and the most at any point.
    depth: u16,
    deepest: u16,
    error: Option<LowerError>,
}

impl<'a> Emitter<'a> {
    fn new(graph: &'a Graph, plan: &'a Plan) -> Self {
        Self {
            graph,
            plan,
            values: HashMap::new(),
            targets: HashMap::new(),
            absorbed: HashSet::new(),
            head: Vec::new(),
            constants: HashMap::new(),
            leaves: HashMap::new(),
            next: HashMap::new(),
            curves: Vec::new(),
            gradients: Vec::new(),
            marks: Vec::new(),
            arrays: Vec::new(),
            enums: Vec::new(),
            operands: Vec::new(),
            frame_caches: HashMap::new(),
            frame_cache_count: 0,
            depth: 0,
            deepest: 0,
            error: None,
        }
    }

    /// Mark nodes emitted as part of their only user: curve samples feeding
    /// a clamp, gradient samples feeding a unit-clamped scale, and a color's
    /// saturation and intensity (and hue, when shifted) feeding `hsv`.
    fn absorb(&mut self, nodes: &[Node]) {
        let (graph, plan) = (self.graph, self.plan);
        let single = |node: Node, user: Node| {
            plan.uses(node) == 1
                && !is_leaf(graph, node)
                && (plan.stage(node), plan.region_of.get(&node))
                    == (plan.stage(user), plan.region_of.get(&user))
        };
        for &node in nodes.iter().rev() {
            if self.absorbed.contains(&node) {
                continue;
            }
            match *graph.op(node) {
                Op::Ternary(Ternary::Clamp, value, ..)
                    if matches!(graph.op(value), Op::Binary(Binary::CurveSample, ..))
                        && single(value, node) =>
                {
                    self.absorbed.insert(value);
                }
                Op::Binary(Binary::ColorScale, color, scale)
                    if matches!(graph.op(color), Op::Binary(Binary::GradientSample, ..))
                        && unit_clamp(graph, scale).is_some()
                        && single(color, node)
                        && single(scale, node) =>
                {
                    self.absorbed.insert(color);
                    self.absorbed.insert(scale);
                }
                Op::Ternary(Ternary::Hsv, hue, saturation, value) => {
                    let Some(color) = recolored(graph, saturation, value) else {
                        continue;
                    };
                    if !(single(saturation, node) && single(value, node)) {
                        continue;
                    }
                    self.absorbed.insert(saturation);
                    self.absorbed.insert(value);
                    if let Some((own, _)) = hue_shift(graph, hue, color)
                        && single(hue, node)
                        && single(own, hue)
                    {
                        self.absorbed.insert(hue);
                        self.absorbed.insert(own);
                    }
                }
                _ => {}
            }
        }
    }

    /// Every option of an enum type, so each bound value has an index.
    fn enum_options(&mut self, ty: &Type) {
        match ty {
            Type::Enum(options) => {
                for option in options {
                    self.enum_index(option);
                }
            }
            Type::Array(item) => self.enum_options(item),
            _ => {}
        }
    }

    fn enum_index(&mut self, name: &Identifier) -> u16 {
        let index = match self.enums.iter().position(|existing| existing == name) {
            Some(index) => index,
            None => {
                self.enums.push(name.clone());
                self.enums.len() - 1
            }
        };
        index as u16
    }

    fn fail(&mut self, error: LowerError) {
        self.error.get_or_insert(error);
    }

    fn fresh(&mut self, bank: Bank, row: bool) -> Slot {
        let next = self.next.entry((bank, row)).or_insert(0);
        let index = *next;
        *next = next.saturating_add(1);
        if index >= Slot::LIMIT {
            self.fail(LowerError::Slots);
        }
        if row {
            Slot::row(index)
        } else {
            Slot::scalar(index)
        }
    }

    fn is_row(&self, node: Node) -> bool {
        self.graph
            .domain(node)
            .intersects(Domain::PIXEL.union(Domain::SIGNAL))
    }

    fn bank(&self, node: Node) -> Bank {
        Bank::for_type(self.graph.ty(node))
    }

    /// The slot `node` writes: a requested target or a new one.
    fn destination(&mut self, node: Node) -> Slot {
        let slot = match self.targets.remove(&node) {
            Some(slot) => slot,
            None => self.fresh(self.bank(node), self.is_row(node)),
        };
        self.values.insert(node, slot);
        slot
    }

    /// The slot holding an operand's value.
    fn operand(&mut self, node: Node) -> Slot {
        if let Some(&slot) = self.values.get(&node) {
            return slot;
        }
        match self.graph.op(node) {
            Op::Constant(constant) => self.constant(constant.clone()),
            Op::Param(_) => self.leaf(node),
            op => unreachable!("operand {node:?} ({op:?}) is used before it is computed"),
        }
    }

    fn constant(&mut self, constant: Constant) -> Slot {
        if let Some(&slot) = self.constants.get(&constant) {
            return slot;
        }
        let bank = Bank::for_type(&constant.ty);
        let dst = self.fresh(bank, false);
        let load = match &constant.value {
            Value::Float(value) => Instruction::FloatConst {
                dst,
                bits: value.to_bits(),
            },
            Value::Int(value) => Instruction::IntConst { dst, value: *value },
            Value::Bool(value) => Instruction::BoolConst { dst, value: *value },
            Value::Color(value) => Instruction::ColorConst { dst, value: *value },
            Value::Enum(name) => Instruction::IntConst {
                dst,
                value: i32::from(self.enum_index(name)),
            },
            Value::Curve(curve) => {
                self.curves.push(Arc::clone(curve));
                resource(dst, Resource::Curve, self.curves.len())
            }
            Value::Gradient(gradient) => {
                self.gradients.push(Arc::clone(gradient));
                resource(dst, Resource::Gradient, self.gradients.len())
            }
            Value::Marks(marks) => {
                self.marks.push(Arc::clone(marks));
                resource(dst, Resource::Marks, self.marks.len())
            }
            Value::Array(items) => {
                self.arrays.push(Arc::clone(items));
                resource(dst, Resource::Array, self.arrays.len())
            }
            Value::Void => unreachable!("checked constants have values"),
        };
        self.head.push(load);
        self.constants.insert(constant, dst);
        dst
    }

    /// A resource or enum parameter, loaded once before the query stage.
    fn leaf(&mut self, node: Node) -> Slot {
        if let Some(&slot) = self.leaves.get(&node) {
            return slot;
        }
        let Op::Param(index) = *self.graph.op(node) else {
            unreachable!("only parameters are leaves besides constants")
        };
        let ty = self.graph.ty(node).clone();
        let dst = self.fresh(Bank::for_type(&ty), false);
        let bank = self.bank_index(index as usize);
        let load = match Resource::for_type(&ty) {
            Some(kind) => Instruction::ResourceParam { dst, kind, bank },
            None => {
                debug_assert!(
                    matches!(ty, Type::Enum(_)),
                    "primitive parameters are scheduled"
                );
                Instruction::IntParam { dst, bank }
            }
        };
        self.head.push(load);
        self.leaves.insert(node, dst);
        dst
    }

    /// A parameter's bank within its storage.
    fn bank_index(&self, param: usize) -> u16 {
        param_bank(&self.param_types(), param)
    }

    fn param_types(&self) -> Vec<Type> {
        self.graph
            .params()
            .iter()
            .map(|param| param.ty.clone())
            .collect()
    }

    /// Emit the nodes of a region in order.
    fn region(&mut self, region: RegionId, code: &mut Vec<Instruction>) {
        let nodes = self.plan.regions[region].nodes.clone();
        for node in nodes {
            if !self.absorbed.contains(&node) {
                self.node(node, code);
            }
        }
    }

    fn node(&mut self, node: Node, code: &mut Vec<Instruction>) {
        let graph = self.graph;
        let instruction = match graph.op(node).clone() {
            // Constants and resource parameters load on first use.
            Op::Constant(_) => return,
            Op::ParamIntegral(_) => unreachable!("instantiation replaces parameter integrals"),
            Op::Param(_) if is_leaf(graph, node) => return,
            Op::Param(index) => {
                let bank = self.bank_index(index as usize);
                let dst = self.destination(node);
                match self.bank(node) {
                    Bank::Float => Instruction::FloatParam { dst, bank },
                    Bank::Int => Instruction::IntParam { dst, bank },
                    Bank::Bool => Instruction::BoolParam { dst, bank },
                    Bank::Color => Instruction::ColorParam { dst, bank },
                    Bank::Resource => unreachable!("resource parameters are leaves"),
                }
            }
            Op::Context(context) => match pixel_input(context) {
                Some(input) => {
                    self.values.insert(node, Slot::input(input));
                    return;
                }
                None => Instruction::Context {
                    dst: self.destination(node),
                    read: context_read(context),
                },
            },
            // A reduction writes its own index.
            Op::LoopIndex(_) => return,
            Op::Unary(op, a) => self.unary(node, op, a),
            Op::Binary(op, a, b) => self.binary(node, op, a, b),
            Op::Ternary(op, a, b, c) => self.ternary(node, op, a, b, c),
            Op::Select(condition, yes, no) => {
                if self.plan.branches.contains(&node) {
                    self.branch(node, code);
                    return;
                }
                let condition = self.operand(condition);
                let (yes, no) = (self.operand(yes), self.operand(no));
                Instruction::Select {
                    bank: self.bank(node),
                    dst: self.destination(node),
                    condition,
                    yes,
                    no,
                }
            }
            Op::Reduce(_) => {
                self.reduce(node, code);
                return;
            }
            // A stencil emits its tap and its source.
            Op::Tap { .. } => return,
            Op::Source => unreachable!("a stencil places its source"),
            Op::ScanTap { .. } => {
                self.scan(node, code);
                return;
            }
            Op::Previous | Op::Scan { .. } => unreachable!("lowering turns scans into scan taps"),
            Op::Items(_) => unreachable!("array literals are folded into picks"),
            Op::Pick { index, items } => {
                let index = self.operand(index);
                let sources: Vec<Slot> = items.iter().map(|&item| self.operand(item)).collect();
                let items = Span {
                    start: self.operands.len() as u16,
                    len: sources.len() as u16,
                };
                if self.operands.len() + sources.len() > usize::from(u16::MAX) {
                    self.fail(LowerError::Code);
                }
                self.operands.extend(sources);
                Instruction::Pick {
                    bank: self.bank(node),
                    dst: self.destination(node),
                    index,
                    items,
                }
            }
            Op::Sample { input, time, pixel } => {
                let seconds = self.operand(time);
                let pixel = pixel.map(|index| self.operand(index));
                let frame_cache = self.frame_cache(input, time);
                Instruction::Sample {
                    dst: self.destination(node),
                    input: input as u16,
                    seconds,
                    pixel,
                    frame_cache,
                }
            }
        };
        code.push(instruction);
    }

    /// The whole-frame cache of `input` at a query-uniform `time`.
    fn frame_cache(&mut self, input: u32, time: Node) -> u16 {
        let cached =
            self.graph.constant_value(time).is_some() || self.plan.stage(time) == Stage::Query;
        if !cached {
            return NO_FRAME_CACHE;
        }
        let next = &mut self.frame_cache_count;
        *self.frame_caches.entry((input, time)).or_insert_with(|| {
            *next += 1;
            *next - 1
        })
    }

    fn unary(&mut self, node: Node, op: Unary, a: Node) -> Instruction {
        let a = self.operand(a);
        let float = |op| (op, a);
        let float_op = match op {
            Unary::Negate => Some(float(FloatUnary::Negate)),
            Unary::Sin => Some(float(FloatUnary::Sin)),
            Unary::Cos => Some(float(FloatUnary::Cos)),
            Unary::Tan => Some(float(FloatUnary::Tan)),
            Unary::Exp => Some(float(FloatUnary::Exp)),
            Unary::Log => Some(float(FloatUnary::Log)),
            Unary::Abs => Some(float(FloatUnary::Abs)),
            Unary::Floor => Some(float(FloatUnary::Floor)),
            Unary::Ceil => Some(float(FloatUnary::Ceil)),
            Unary::Trunc => Some(float(FloatUnary::Trunc)),
            Unary::RoundEven => Some(float(FloatUnary::RoundEven)),
            Unary::Sqrt => Some(float(FloatUnary::Sqrt)),
            Unary::Smoothstep => Some(float(FloatUnary::Smoothstep)),
            Unary::Rand => Some(float(FloatUnary::Rand)),
            _ => None,
        };
        let dst = self.destination(node);
        if let Some((op, a)) = float_op {
            return Instruction::FloatUnary { op, dst, a };
        }
        match op {
            Unary::QuerySeconds | Unary::QueryProgress => Instruction::Clock {
                progress: op == Unary::QueryProgress,
                dst,
                seconds: a,
            },
            Unary::IntNegate => Instruction::IntNegate { dst, a },
            Unary::Not => Instruction::Not { dst, a },
            Unary::IntToFloat => Instruction::IntToFloat { dst, a },
            Unary::FloatToInt => Instruction::FloatToInt { dst, a },
            Unary::Hue
            | Unary::Saturation
            | Unary::Intensity
            | Unary::Red
            | Unary::Green
            | Unary::Blue => Instruction::ColorComponent {
                op: match op {
                    Unary::Hue => ColorComponent::Hue,
                    Unary::Saturation => ColorComponent::Saturation,
                    Unary::Red => ColorComponent::Red,
                    Unary::Green => ColorComponent::Green,
                    Unary::Blue => ColorComponent::Blue,
                    _ => ColorComponent::Intensity,
                },
                dst,
                color: a,
            },
            Unary::Invert => Instruction::Invert { dst, color: a },
            Unary::Len => Instruction::Len { dst, array: a },
            Unary::MarkCount => Instruction::Mark {
                op: MarkOp::Count,
                dst,
                marks: a,
                operand: Slot::NONE,
            },
            Unary::SectionCount => Instruction::SectionCount { dst, width: a },
            Unary::SectionIndex => Instruction::SectionIndex { dst, width: a },
            _ => unreachable!("float unaries are handled above"),
        }
    }

    fn binary(&mut self, node: Node, op: Binary, a: Node, b: Node) -> Instruction {
        use Binary::*;
        let graph = self.graph;
        if op == ColorScale && self.absorbed.contains(&a) {
            let Op::Binary(GradientSample, gradient, position) = *graph.op(a) else {
                unreachable!("an absorbed scale operand is a gradient sample")
            };
            let scale = unit_clamp(graph, b).unwrap_or_else(|| unreachable!("absorbed unit clamp"));
            let (gradient, position, scale) = (
                self.operand(gradient),
                self.operand(position),
                self.operand(scale),
            );
            return Instruction::GradientScaled {
                dst: self.destination(node),
                gradient,
                position,
                scale,
            };
        }
        let left_ty = graph.ty(a).clone();
        let (a, b) = (self.operand(a), self.operand(b));
        let dst = self.destination(node);
        let float = |op| Instruction::FloatBinary { op, dst, a, b };
        let int = |op| Instruction::IntBinary { op, dst, a, b };
        let float_compare = |op| Instruction::FloatCompare { op, dst, a, b };
        let int_compare = |op| Instruction::IntCompare { op, dst, a, b };
        let color = |op| Instruction::ColorBinary { op, dst, a, b };
        let mark = |op| Instruction::Mark {
            op,
            dst,
            marks: a,
            operand: b,
        };
        match op {
            Add => float(FloatBinary::Add),
            Subtract => float(FloatBinary::Subtract),
            Multiply => float(FloatBinary::Multiply),
            Divide => float(FloatBinary::Divide),
            Remainder => float(FloatBinary::Remainder),
            Min => float(FloatBinary::Min),
            Max => float(FloatBinary::Max),
            ValueOr => float(FloatBinary::ValueOr),
            Atan2 => float(FloatBinary::Atan2),
            PowerFloat => float(FloatBinary::Power),
            IntAdd => int(IntBinary::Add),
            IntSubtract => int(IntBinary::Subtract),
            IntMultiply => int(IntBinary::Multiply),
            IntRemainder => int(IntBinary::Remainder),
            IntFloorDivide => int(IntBinary::FloorDivide),
            IntMin => int(IntBinary::Min),
            IntMax => int(IntBinary::Max),
            Less => float_compare(CompareOp::Less),
            LessEqual => float_compare(CompareOp::LessEqual),
            Greater => float_compare(CompareOp::Greater),
            GreaterEqual => float_compare(CompareOp::GreaterEqual),
            IntLess => int_compare(CompareOp::Less),
            IntLessEqual => int_compare(CompareOp::LessEqual),
            IntGreater => int_compare(CompareOp::Greater),
            IntGreaterEqual => int_compare(CompareOp::GreaterEqual),
            Equal | NotEqual => Instruction::Equal {
                bank: Bank::for_type(&left_ty),
                negate: op == NotEqual,
                dst,
                a,
                b,
            },
            ColorAdd => color(ColorBinary::Add),
            ColorMultiply => color(ColorBinary::Multiply),
            ColorMax => color(ColorBinary::Max),
            ColorScale => Instruction::ColorScale {
                dst,
                color: a,
                scale: b,
            },
            CurveSample => Instruction::CurveSample {
                dst,
                curve: a,
                position: b,
            },
            CurveIntegral => Instruction::CurveIntegral {
                dst,
                curve: a,
                position: b,
            },
            GradientSample => Instruction::GradientSample {
                dst,
                gradient: a,
                position: b,
            },
            Index => {
                let ty = graph.ty(node).clone();
                let default = self.constant(Constant {
                    value: ty.default_value(),
                    ty,
                });
                Instruction::Index {
                    bank: self.bank(node),
                    dst,
                    array: a,
                    index: b,
                    default,
                }
            }
            MarkAt => mark(MarkOp::At),
            MarkLast => mark(MarkOp::Last),
            MarkLastIndex => mark(MarkOp::LastIndex),
            CurveFirstCrossing => Instruction::CurveCrossing {
                dst,
                curve: a,
                value: b,
                before: Slot::NONE,
            },
            SectionPosition => Instruction::SectionPosition {
                dst,
                width: a,
                inverse: b,
            },
            Power => Instruction::Power {
                dst,
                base: a,
                count: b,
            },
        }
    }

    fn ternary(&mut self, node: Node, op: Ternary, a: Node, b: Node, c: Node) -> Instruction {
        let graph = self.graph;
        if op == Ternary::Hsv && self.absorbed.contains(&b) {
            let color = recolored(graph, b, c).unwrap_or_else(|| unreachable!("absorbed recolor"));
            let (hue, shift) = match hue_shift(graph, a, color) {
                Some((_, turns)) if self.absorbed.contains(&a) => (turns, true),
                _ => (a, false),
            };
            let (color, hue) = (self.operand(color), self.operand(hue));
            return Instruction::Recolor {
                shift,
                dst: self.destination(node),
                color,
                hue,
            };
        }
        if op == Ternary::Clamp && self.absorbed.contains(&a) {
            let Op::Binary(Binary::CurveSample, curve, position) = *graph.op(a) else {
                unreachable!("an absorbed clamp operand is a curve sample")
            };
            let (curve, position) = (self.operand(curve), self.operand(position));
            let (min, max) = (self.operand(b), self.operand(c));
            return Instruction::CurveClamped {
                dst: self.destination(node),
                curve,
                position,
                min,
                max,
            };
        }
        let (a, b, c) = (self.operand(a), self.operand(b), self.operand(c));
        let dst = self.destination(node);
        match op {
            Ternary::Clamp => Instruction::Clamp {
                dst,
                value: a,
                min: b,
                max: c,
            },
            Ternary::Mix => Instruction::Mix {
                dst,
                a,
                b,
                amount: c,
            },
            Ternary::MixColor => Instruction::MixColor {
                dst,
                a,
                b,
                amount: c,
            },
            Ternary::Rgb => Instruction::Rgb {
                dst,
                red: a,
                green: b,
                blue: c,
            },
            Ternary::Hsv => Instruction::Hsv {
                dst,
                hue: a,
                saturation: b,
                value: c,
            },
            Ternary::CurveLastCrossing => Instruction::CurveCrossing {
                dst,
                curve: a,
                value: b,
                before: c,
            },
        }
    }

    /// Write `value` into `result`, unless it was computed there.
    fn assign(&mut self, result: Slot, value: Node, code: &mut Vec<Instruction>) {
        let src = self.operand(value);
        if src != result {
            code.push(Instruction::Move {
                bank: self.bank(value),
                dst: result,
                src,
            });
        }
    }

    /// Compute `value` directly into `result` when its arm computes it alone.
    fn target(&mut self, value: Node, result: Slot, region: RegionId) {
        if self.plan.uses(value) == 1
            && self.plan.region_of.get(&value) == Some(&region)
            && !is_leaf(self.graph, value)
            && !matches!(
                self.graph.op(value),
                Op::Reduce(_) | Op::LoopIndex(_) | Op::Context(_)
            )
            && self.is_row(value) == !result.is_scalar()
        {
            self.targets.insert(value, result);
        }
    }

    fn length(&mut self, code: &[Instruction]) -> u16 {
        u16::try_from(code.len()).unwrap_or_else(|_| {
            self.fail(LowerError::Code);
            0
        })
    }

    /// Open `count` selections for nested code.
    fn open(&mut self, count: u16) {
        self.depth += count;
        self.deepest = self.deepest.max(self.depth);
    }

    fn branch(&mut self, select: Node, code: &mut Vec<Instruction>) {
        let Op::Select(condition, yes, no) = *self.graph.op(select) else {
            unreachable!("branches are selects")
        };
        let children = self.plan.children[&select].clone();
        let (then_region, else_region) = (children[0], children[1]);
        let condition = self.operand(condition);
        let result = self.destination(select);
        self.target(yes, result, then_region);
        self.target(no, result, else_region);
        let opened = u16::from(!condition.is_scalar());
        self.open(opened);
        let mut then_code = Vec::new();
        self.region(then_region, &mut then_code);
        self.assign(result, yes, &mut then_code);
        let mut else_code = Vec::new();
        self.region(else_region, &mut else_code);
        self.assign(result, no, &mut else_code);
        self.depth -= opened;
        let (then_len, else_len) = (self.length(&then_code), self.length(&else_code));
        code.push(Instruction::Branch {
            condition,
            then_len,
            else_len,
        });
        code.append(&mut then_code);
        code.append(&mut else_code);
    }

    fn reduce(&mut self, node: Node, code: &mut Vec<Instruction>) {
        let Op::Reduce(id) = *self.graph.op(node) else {
            unreachable!("reductions are reduce nodes")
        };
        let data = self.graph.loop_(id).clone();
        if matches!(self.graph.op(data.body), Op::Tap { .. }) {
            self.stencil(node, code);
            return;
        }
        let children = self.plan.children[&node].clone();
        let bank = self.bank(node);
        let acc = self.destination(node);
        let initial = match data.reducer {
            ir::Reducer::First | ir::Reducer::Last => self.operand(
                data.default
                    .unwrap_or_else(|| unreachable!("checked default")),
            ),
            reducer => {
                let ty = self.graph.ty(node).clone();
                let value = ir::reduce_identity(reducer, &ty)
                    .unwrap_or_else(|| unreachable!("checked reduction type"));
                self.constant(Constant { value, ty })
            }
        };
        code.push(Instruction::Move {
            bank,
            dst: acc,
            src: initial,
        });
        let (start, end) = (self.operand(data.start), self.operand(data.end));
        let index = self.fresh(Bank::Int, self.is_row(data.index));
        self.values.insert(data.index, index);
        let opened = u16::from(!acc.is_scalar());
        self.open(opened);
        let mut loop_code = Vec::new();
        self.region(children[0], &mut loop_code);
        let filter = match data.filter {
            Some(filter) => self.operand(filter),
            None => Slot::NONE,
        };
        let mut contribute = Vec::new();
        if data.filter.is_some() {
            self.region(children[1], &mut contribute);
        }
        let value = self.operand(data.body);
        self.depth -= opened;
        let (loop_len, contribute_len) = (self.length(&loop_code), self.length(&contribute));
        code.push(Instruction::Reduce {
            reducer: match data.reducer {
                ir::Reducer::Max => Reducer::Max,
                ir::Reducer::Min => Reducer::Min,
                ir::Reducer::Sum => Reducer::Sum,
                ir::Reducer::Any => Reducer::Any,
                ir::Reducer::All => Reducer::All,
                ir::Reducer::First => Reducer::First,
                ir::Reducer::Last => Reducer::Last,
            },
            bank,
            acc,
            index,
            start,
            end,
            filter,
            value,
            loop_len,
            contribute_len,
        });
        code.append(&mut loop_code);
        code.append(&mut contribute);
    }

    /// A neighborhood reduction: its header, the source block sampling the
    /// input and computing the source weight, and the offset block.
    fn stencil(&mut self, node: Node, code: &mut Vec<Instruction>) {
        let graph = self.graph;
        let Op::Reduce(id) = *graph.op(node) else {
            unreachable!("stencils are reduce nodes")
        };
        let data = graph.loop_(id).clone();
        let Op::Tap {
            input,
            time,
            edges,
            weight,
            scale,
            pixel,
            ..
        } = *graph.op(data.body)
        else {
            unreachable!("a stencil's body is its tap")
        };
        let children = self.plan.children[&node].clone();
        let acc = self.destination(node);
        let black = self.constant(Constant {
            value: Value::Color(Color::BLACK),
            ty: Type::Color,
        });
        code.push(Instruction::Move {
            bank: Bank::Color,
            dst: acc,
            src: black,
        });
        let (start, end) = (self.operand(data.start), self.operand(data.end));
        let one = self.constant(Constant {
            value: Value::Float(1.0),
            ty: Type::Float,
        });
        let pixel = pixel.map_or(one, |pixel| self.operand(pixel));
        let index = self.fresh(Bank::Int, false);
        self.values.insert(data.index, index);
        let (mut source, weight) = self.source_block(input, time, weight, one);
        let mut taps = Vec::new();
        self.region(children[0], &mut taps);
        let scale = scale.map_or(one, |scale| self.operand(scale));
        let (source_len, tap_len) = (self.length(&source), self.length(&taps));
        code.push(Instruction::Stencil {
            reducer: match data.reducer {
                ir::Reducer::Max => Reducer::Max,
                _ => Reducer::Sum,
            },
            edges,
            acc,
            index,
            start,
            end,
            weight,
            scale,
            pixel,
            source_len,
            tap_len,
        });
        code.append(&mut source);
        code.append(&mut taps);
    }

    /// A source block: a sample of `input` at the current pixel, then the
    /// code computing `weight` (or `one`) from it.
    fn source_block(
        &mut self,
        input: u32,
        time: Node,
        weight: Option<Node>,
        one: Slot,
    ) -> (Vec<Instruction>, Slot) {
        let sample = self.fresh(Bank::Color, true);
        let mut source = vec![Instruction::Sample {
            dst: sample,
            input: input as u16,
            seconds: self.operand(time),
            pixel: SignalPixel::Current,
            frame_cache: self.frame_cache(input, time),
        }];
        let Some(weight) = weight else {
            return (source, one);
        };
        // The source runs over other pixels than the strip's, so its values
        // are this block's alone.
        let cone = self.source_cone(weight);
        for &node in &cone {
            if matches!(self.graph.op(node), Op::Source) {
                self.values.insert(node, sample);
            } else if !self.absorbed.contains(&node) {
                self.node(node, &mut source);
            }
        }
        let slot = self.operand(weight);
        for node in cone {
            self.values.remove(&node);
        }
        (source, slot)
    }

    /// A scan: its instruction, then its source block.
    fn scan(&mut self, node: Node, code: &mut Vec<Instruction>) {
        let Op::ScanTap {
            direction,
            input,
            time,
            decay,
            weight,
        } = *self.graph.op(node)
        else {
            unreachable!("scans are scan taps")
        };
        let one = self.constant(Constant {
            value: Value::Float(1.0),
            ty: Type::Float,
        });
        let decay = self.operand(decay);
        let (mut source, weight) = self.source_block(input, time, weight, one);
        let cache = self.frame_cache_count;
        self.frame_cache_count += 1;
        let source_len = self.length(&source);
        let dst = self.destination(node);
        code.push(Instruction::Scan {
            direction,
            dst,
            decay,
            weight,
            cache,
            source_len,
        });
        code.append(&mut source);
    }

    /// The source-stage nodes `weight` is computed from, operands first.
    fn source_cone(&self, weight: Node) -> Vec<Node> {
        let mut cone = HashSet::new();
        let mut pending = vec![weight];
        while let Some(node) = pending.pop() {
            if self.plan.stage(node) == Stage::Source && cone.insert(node) {
                pending.extend(self.graph.op(node).operands());
            }
        }
        let mut cone: Vec<Node> = cone.into_iter().collect();
        cone.sort();
        cone
    }

    fn finish(
        mut self,
        query: Vec<Instruction>,
        target: Vec<Instruction>,
        body: Vec<Instruction>,
        mut result: Slot,
    ) -> Result<BytecodeProgram, LowerError> {
        let mut code = core::mem::take(&mut self.head);
        code.extend(query);
        let query_end = code.len();
        code.extend(target);
        let target_end = code.len();
        code.extend(body);
        if let Some(error) = self.error {
            return Err(error);
        }
        let (Ok(query_end), Ok(target_end), Ok(_)) = (
            u16::try_from(query_end),
            u16::try_from(target_end),
            u16::try_from(code.len()),
        ) else {
            return Err(LowerError::Code);
        };
        let params = self.param_types().into();
        let mut operands = self.operands;
        let (scalars, rows) = slots::allocate(
            &mut code,
            &mut operands,
            &mut result,
            usize::from(target_end),
        );
        if rows.row_bytes() > MAX_ROW_BYTES {
            return Err(LowerError::Rows(rows.row_bytes()));
        }
        if self.deepest > MAX_DEPTH {
            return Err(LowerError::Depth(self.deepest));
        }
        Ok(BytecodeProgram {
            params,
            code: code.into(),
            query_end,
            target_end,
            result,
            scalars,
            rows,
            depth: self.deepest,
            curves: self.curves.into(),
            gradients: self.gradients.into(),
            marks: self.marks.into(),
            arrays: self.arrays.into(),
            enums: self.enums.into(),
            operands: operands.into(),
            frame_caches: self.frame_cache_count,
        })
    }
}

fn resource(dst: Slot, kind: Resource, pool_len: usize) -> Instruction {
    Instruction::ResourceConst {
        dst,
        kind,
        index: (pool_len - 1) as u16,
    }
}

fn pixel_input(context: Context) -> Option<Input> {
    match context {
        Context::PixelIndex => Some(Input::PixelIndex),
        Context::PixelFraction => Some(Input::PixelFraction),
        Context::PixelX => Some(Input::PixelX),
        Context::PixelY => Some(Input::PixelY),
        _ => None,
    }
}

fn context_read(context: Context) -> ContextRead {
    match context {
        Context::Time => ContextRead::Seconds,
        Context::Progress => ContextRead::Progress,
        Context::Duration => ContextRead::Duration,
        Context::TargetCount => ContextRead::PixelCount,
        Context::TargetMinX => ContextRead::TargetMinX,
        Context::TargetMinY => ContextRead::TargetMinY,
        Context::TargetMaxX => ContextRead::TargetMaxX,
        Context::TargetMaxY => ContextRead::TargetMaxY,
        _ => unreachable!("pixel context is a strip input"),
    }
}

/// The color whose `saturation` and `intensity` these are.
fn recolored(graph: &Graph, saturation: Node, value: Node) -> Option<Node> {
    match (graph.op(saturation), graph.op(value)) {
        (Op::Unary(Unary::Saturation, a), Op::Unary(Unary::Intensity, b)) if a == b => Some(*a),
        _ => None,
    }
}

/// `hue(color) + turns`: the hue node and the turns.
fn hue_shift(graph: &Graph, hue: Node, color: Node) -> Option<(Node, Node)> {
    let Op::Binary(Binary::Add, a, b) = *graph.op(hue) else {
        return None;
    };
    let own = |node: Node| matches!(graph.op(node), Op::Unary(Unary::Hue, c) if *c == color);
    if own(a) {
        Some((a, b))
    } else if own(b) {
        Some((b, a))
    } else {
        None
    }
}

/// The value of `clamp(value, 0.0, 1.0)`.
fn unit_clamp(graph: &Graph, node: Node) -> Option<Node> {
    match *graph.op(node) {
        Op::Ternary(Ternary::Clamp, value, min, max)
            if graph.constant_value(min) == Some(&Value::Float(0.0))
                && graph.constant_value(max) == Some(&Value::Float(1.0)) =>
        {
            Some(value)
        }
        _ => None,
    }
}
