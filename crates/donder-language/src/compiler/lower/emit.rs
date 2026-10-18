//! Strip bytecode. The query and target stages become scalar blocks and the
//! pixel region tree becomes the body: a branch is a `Branch` with its arms
//! inline, a reduction a `Reduce` with its parts inline. A value that varies
//! by pixel or signal is a row; every other value is a scalar. A color is
//! three float slots, and color arithmetic is float arithmetic on them.
use super::schedule::{Plan, RegionId, Stage, is_leaf};
use super::{LowerError, slots};
use crate::compiler::ir::{
    self, Binary, Constant, Context, Domain, Graph, Node, Op, Ternary, Unary,
};
use donder_runtime_types::bytecode::{
    Bank, Banks, BytecodeProgram, ColorComponent, Colors, CompareOp, ContextRead, CurveOp,
    FloatBinary, FloatUnary, Input, Instruction, IntBinary, IntUnary, MAX_DEPTH, MAX_ROW_BYTES,
    MarkOp, Reducer, Resource, SignalPixel, Slot, param_bank,
};
use donder_runtime_types::{Color, Identifier, InstructionWord, Type, Value};
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
    emitter.tables(Stage::Query, &mut query);
    let mut target = Vec::new();
    for &node in &plan.target {
        if !emitter.absorbed.contains(&node) {
            emitter.node(node, &mut target);
        }
    }
    emitter.tables(Stage::Target, &mut target);
    let mut body = Vec::new();
    emitter.region(0, &mut body);
    let result = emitter.operand(root);
    emitter.finish(query, target, body, result)
}

/// Words of code `code` takes.
fn words(code: &[Instruction]) -> usize {
    code.iter().map(Instruction::width).sum()
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
    /// Virtual slots allocated so far, by storage bank and kind.
    next: HashMap<(Bank, bool), u16>,
    resources: Vec<Value>,
    enums: Vec<Identifier>,
    /// Each table output's table row and its index's start.
    tables: HashMap<Node, (Slot, Slot)>,
    /// Frame caches so far, one per scan.
    scan_count: u16,
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
            resources: Vec::new(),
            enums: Vec::new(),
            tables: HashMap::new(),
            scan_count: 0,
            depth: 0,
            deepest: 0,
            error: None,
        }
    }

    /// Mark nodes emitted as part of their only user: gradient samples
    /// feeding a unit-clamped scale, and a color's saturation and intensity
    /// (and hue, when shifted) feeding `hsv`.
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

    /// A new virtual slot; a color's is the first of three floats.
    fn fresh(&mut self, bank: Bank, row: bool) -> Slot {
        let next = self.next.entry((bank.storage(), row)).or_insert(0);
        let index = *next;
        *next = next.saturating_add(bank.width());
        if u32::from(index) + u32::from(bank.width()) > u32::from(Slot::VIRTUAL) {
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

    fn float_constant(&mut self, value: f32) -> Slot {
        self.constant(Constant {
            value: Value::Float(value),
            ty: Type::Float,
        })
    }

    fn constant(&mut self, constant: Constant) -> Slot {
        if let Some(&slot) = self.constants.get(&constant) {
            return slot;
        }
        let bank = Bank::for_type(&constant.ty);
        let dst = self.fresh(bank, false);
        let scalar = |bank, bits| Instruction::Const { bank, dst, bits };
        match &constant.value {
            Value::Float(value) => self.head.push(scalar(Bank::Float, value.to_bits())),
            Value::Int(value) => self.head.push(scalar(Bank::Int, *value as u32)),
            Value::Bool(value) => self.head.push(scalar(Bank::Bool, u32::from(*value))),
            Value::Enum(name) => {
                let index = self.enum_index(name);
                self.head.push(scalar(Bank::Int, u32::from(index)));
            }
            Value::Color(color) => {
                for (channel, value) in [color.red, color.green, color.blue].into_iter().enumerate()
                {
                    self.head.push(Instruction::Const {
                        bank: Bank::Float,
                        dst: dst.channel(channel as u16),
                        bits: value.to_bits(),
                    });
                }
            }
            Value::Curve(_) | Value::Gradient(_) | Value::Marks(_) | Value::Array(_) => {
                self.resources.push(constant.value.clone());
                let index = self.resources.len() as u32 - 1;
                self.head.push(scalar(Bank::Resource, index));
            }
            Value::Void => unreachable!("checked constants have values"),
        }
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
        let bank = Bank::for_type(&ty);
        debug_assert!(
            bank == Bank::Resource || matches!(ty, Type::Enum(_)),
            "primitive parameters are scheduled"
        );
        let dst = self.fresh(bank, false);
        let index = self.bank_index(index as usize);
        self.head.push(Instruction::Param { bank, dst, index });
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
        match graph.op(node).clone() {
            // Constants and resource parameters load on first use.
            Op::Constant(_) => {}
            Op::ParamIntegral(_) => unreachable!("instantiation replaces parameter integrals"),
            Op::Param(_) if is_leaf(graph, node) => {}
            Op::Param(index) => {
                let index = self.bank_index(index as usize);
                let bank = self.bank(node);
                let dst = self.destination(node);
                code.push(Instruction::Param { bank, dst, index });
            }
            Op::Context(context) => match pixel_input(context) {
                Some(input) => {
                    self.values.insert(node, Slot::input(input));
                }
                None => code.push(Instruction::Context {
                    dst: self.destination(node),
                    read: context_read(context),
                }),
            },
            // A reduction writes its own index.
            Op::LoopIndex(_) => {}
            Op::Unary(op, a) => self.unary(node, op, a, code),
            Op::Binary(op, a, b) => self.binary(node, op, a, b, code),
            Op::Ternary(op, a, b, c) => self.ternary(node, op, a, b, c, code),
            Op::Select(condition, yes, no) => {
                if self.plan.branches.contains(&node) {
                    self.branch(node, code);
                    return;
                }
                let condition = self.operand(condition);
                let (yes, no) = (self.operand(yes), self.operand(no));
                let bank = self.bank(node);
                let dst = self.destination(node);
                select(bank, dst, condition, yes, no, code);
            }
            Op::Reduce(_) => self.reduce(node, code),
            // A stencil emits its tap and its source.
            Op::Tap { .. } => {}
            Op::Source => unreachable!("a stencil places its source"),
            Op::ScanTap { .. } => self.scan(node, code),
            Op::Previous | Op::Scan { .. } => unreachable!("lowering turns scans into scan taps"),
            Op::Items(_) => unreachable!("array literals are folded into picks"),
            Op::Pick { index, items } => self.pick(node, index, &items, code),
            Op::Sample { input, pixel } => {
                let pixel = pixel.map(|index| self.operand(index));
                code.push(Instruction::Sample {
                    dst: self.destination(node),
                    input: input as u16,
                    pixel,
                });
            }
        }
    }

    /// One of `items` at a clamped index: the last, unless the index is
    /// below an earlier one's successor.
    fn pick(&mut self, node: Node, index: Node, items: &[Node], code: &mut Vec<Instruction>) {
        let index = self.operand(index);
        let sources: Vec<Slot> = items.iter().map(|&item| self.operand(item)).collect();
        let (bank, row) = (self.bank(node), self.is_row(node));
        let Some((&last, rest)) = sources.split_last() else {
            unreachable!("checked picks have items")
        };
        let mut chosen = last;
        for (at, &item) in rest.iter().enumerate().rev() {
            let bound = self.constant(Constant {
                value: Value::Int(at as i32 + 1),
                ty: Type::Int,
            });
            let below = self.fresh(Bank::Bool, row);
            code.push(Instruction::Compare {
                op: CompareOp::Less,
                bank: Bank::Int,
                dst: below,
                a: index,
                b: bound,
            });
            let dst = if at == 0 {
                self.destination(node)
            } else {
                self.fresh(bank, row)
            };
            select(bank, dst, below, item, chosen, code);
            chosen = dst;
        }
        if rest.is_empty() {
            self.values.insert(node, last);
        }
    }

    fn unary(&mut self, node: Node, op: Unary, a: Node, code: &mut Vec<Instruction>) {
        let color = self.graph.ty(a) == &Type::Color;
        let a = self.operand(a);
        // A channel is the color's own slot.
        let channel = match op {
            Unary::Red => Some(0),
            Unary::Green => Some(1),
            Unary::Blue => Some(2),
            _ => None,
        };
        if let Some(channel) = channel {
            debug_assert!(color, "channels are of colors");
            self.values.insert(node, a.channel(channel));
            return;
        }
        let float = |op| Some(op);
        let float_op = match op {
            Unary::Negate => float(FloatUnary::Negate),
            Unary::Sin => float(FloatUnary::Sin),
            Unary::Cos => float(FloatUnary::Cos),
            Unary::Tan => float(FloatUnary::Tan),
            Unary::Exp => float(FloatUnary::Exp),
            Unary::Log => float(FloatUnary::Log),
            Unary::Abs => float(FloatUnary::Abs),
            Unary::Floor => float(FloatUnary::Floor),
            Unary::Ceil => float(FloatUnary::Ceil),
            Unary::Trunc => float(FloatUnary::Trunc),
            Unary::Sqrt => float(FloatUnary::Sqrt),
            Unary::Smoothstep => float(FloatUnary::Smoothstep),
            Unary::Rand => float(FloatUnary::Rand),
            _ => None,
        };
        if let Some(op) = float_op {
            let dst = self.destination(node);
            code.push(Instruction::FloatUnary { op, dst, a });
            return;
        }
        if op == Unary::Invert {
            let one = self.float_constant(1.0);
            let dst = self.destination(node);
            code.push(Instruction::FloatBinary {
                op: FloatBinary::Subtract,
                colors: Colors::NONE.with(1),
                dst,
                a: one,
                b: a,
            });
            return;
        }
        if op == Unary::Not {
            let no = self.constant(Constant {
                value: Value::Bool(false),
                ty: Type::Bool,
            });
            let dst = self.destination(node);
            code.push(Instruction::Compare {
                op: CompareOp::Equal,
                bank: Bank::Bool,
                dst,
                a,
                b: no,
            });
            return;
        }
        let dst = self.destination(node);
        code.push(match op {
            Unary::IntNegate => Instruction::IntUnary {
                op: IntUnary::Negate,
                dst,
                a,
            },
            Unary::IntAbs => Instruction::IntUnary {
                op: IntUnary::Abs,
                dst,
                a,
            },
            Unary::IntToFloat => Instruction::IntToFloat { dst, a },
            Unary::FloatToInt => Instruction::FloatToInt { dst, a },
            Unary::Hue | Unary::Saturation | Unary::Intensity => Instruction::ColorComponent {
                op: match op {
                    Unary::Hue => ColorComponent::Hue,
                    Unary::Saturation => ColorComponent::Saturation,
                    _ => ColorComponent::Intensity,
                },
                dst,
                color: a,
            },
            Unary::Len => Instruction::Length {
                kind: Resource::Array,
                dst,
                resource: a,
            },
            Unary::MarkCount => Instruction::Length {
                kind: Resource::Marks,
                dst,
                resource: a,
            },
            Unary::SectionCount | Unary::SectionIndex => Instruction::Section {
                index: op == Unary::SectionIndex,
                dst,
                width: a,
            },
            _ => unreachable!("other unaries are handled above"),
        });
    }

    fn binary(&mut self, node: Node, op: Binary, a: Node, b: Node, code: &mut Vec<Instruction>) {
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
            code.push(Instruction::Gradient {
                dst: self.destination(node),
                gradient,
                position,
                scale,
            });
            return;
        }
        if op == SectionPosition {
            self.section_position(node, a, b, code);
            return;
        }
        let left = Bank::for_type(graph.ty(a));
        let (a, b) = (self.operand(a), self.operand(b));
        if matches!(op, Equal | NotEqual) && left == Bank::Color {
            self.color_equal(node, op == NotEqual, a, b, code);
            return;
        }
        let dst = self.destination(node);
        let float = |op| Instruction::FloatBinary {
            op,
            colors: Colors::NONE,
            dst,
            a,
            b,
        };
        let int = |op| Instruction::IntBinary { op, dst, a, b };
        let compare = |op, bank| Instruction::Compare {
            op,
            bank,
            dst,
            a,
            b,
        };
        let curve = |op| Instruction::Curve {
            op,
            dst,
            curve: a,
            value: b,
            before: Slot::NONE,
        };
        let mark = |op| Instruction::Mark {
            op,
            dst,
            marks: a,
            operand: b,
        };
        // Color arithmetic: a color, and a color or a float.
        let color = |op, b_is_color: bool| Instruction::FloatBinary {
            op,
            colors: if b_is_color {
                Colors::NONE.with(0).with(1)
            } else {
                Colors::NONE.with(0)
            },
            dst,
            a,
            b,
        };
        let instruction = match op {
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
            Less => compare(CompareOp::Less, Bank::Float),
            LessEqual => compare(CompareOp::LessEqual, Bank::Float),
            Greater => compare(CompareOp::Greater, Bank::Float),
            GreaterEqual => compare(CompareOp::GreaterEqual, Bank::Float),
            IntLess => compare(CompareOp::Less, Bank::Int),
            IntLessEqual => compare(CompareOp::LessEqual, Bank::Int),
            IntGreater => compare(CompareOp::Greater, Bank::Int),
            IntGreaterEqual => compare(CompareOp::GreaterEqual, Bank::Int),
            Equal => compare(CompareOp::Equal, left),
            NotEqual => compare(CompareOp::NotEqual, left),
            ColorAdd => color(FloatBinary::Add, true),
            ColorMultiply => color(FloatBinary::Multiply, true),
            ColorMax => color(FloatBinary::Max, true),
            ColorScale => color(FloatBinary::Multiply, false),
            CurveSample => curve(CurveOp::Sample),
            CurveIntegral => curve(CurveOp::Integral),
            CurveFirstCrossing => curve(CurveOp::Crossing),
            GradientSample => Instruction::Gradient {
                dst,
                gradient: a,
                position: b,
                scale: Slot::NONE,
            },
            Index => {
                let ty = graph.ty(node).clone();
                let bank = Bank::for_type(&ty);
                let default = self.constant(Constant {
                    value: ty.default_value(),
                    ty,
                });
                Instruction::Index {
                    bank,
                    dst,
                    array: a,
                    index: b,
                    default,
                }
            }
            MarkAt => mark(MarkOp::At),
            MarkLast => mark(MarkOp::Last),
            MarkLastIndex => mark(MarkOp::LastIndex),
            // A float multiplied by itself an int number of times.
            Power => {
                let exponent = self.fresh(Bank::Float, !b.is_scalar());
                code.push(Instruction::IntToFloat {
                    dst: exponent,
                    a: b,
                });
                Instruction::FloatBinary {
                    op: FloatBinary::Power,
                    colors: Colors::NONE,
                    dst,
                    a,
                    b: exponent,
                }
            }
            SectionPosition => unreachable!("lowered above"),
        };
        code.push(instruction);
    }

    /// Bool: whether two colors are equal in every channel, or with `negate`
    /// differ in one.
    fn color_equal(
        &mut self,
        node: Node,
        negate: bool,
        a: Slot,
        b: Slot,
        code: &mut Vec<Instruction>,
    ) {
        let row = self.is_row(node);
        let op = if negate {
            CompareOp::NotEqual
        } else {
            CompareOp::Equal
        };
        // Equal: each channel equal, else false; not equal: any differs.
        let decided = self.constant(Constant {
            value: Value::Bool(negate),
            ty: Type::Bool,
        });
        let mut combined: Option<Slot> = None;
        for channel in 0..3 {
            let compared = self.fresh(Bank::Bool, row);
            code.push(Instruction::Compare {
                op,
                bank: Bank::Float,
                dst: compared,
                a: a.channel(channel),
                b: b.channel(channel),
            });
            combined = Some(match combined {
                None => compared,
                Some(previous) => {
                    let joined = if channel == 2 {
                        self.destination(node)
                    } else {
                        self.fresh(Bank::Bool, row)
                    };
                    let (yes, no) = if negate {
                        (decided, compared)
                    } else {
                        (compared, decided)
                    };
                    code.push(Instruction::Select {
                        bank: Bank::Bool,
                        dst: joined,
                        condition: previous,
                        yes,
                        no,
                    });
                    joined
                }
            });
        }
    }

    /// The position within the pixel's section, from a float width and its
    /// reciprocal: `(i - floor(i * inverse) * width) * inverse`.
    fn section_position(
        &mut self,
        node: Node,
        width: Node,
        inverse: Node,
        code: &mut Vec<Instruction>,
    ) {
        let (width, inverse) = (self.operand(width), self.operand(inverse));
        let mut row = || self.fresh(Bank::Float, true);
        let (index, scaled, floored, start, within) = (row(), row(), row(), row(), row());
        let binary = |op, dst, a, b| Instruction::FloatBinary {
            op,
            colors: Colors::NONE,
            dst,
            a,
            b,
        };
        code.push(Instruction::IntToFloat {
            dst: index,
            a: Slot::input(Input::PixelIndex),
        });
        code.push(binary(FloatBinary::Multiply, scaled, index, inverse));
        code.push(Instruction::FloatUnary {
            op: FloatUnary::Floor,
            dst: floored,
            a: scaled,
        });
        code.push(binary(FloatBinary::Multiply, start, floored, width));
        code.push(binary(FloatBinary::Subtract, within, index, start));
        let dst = self.destination(node);
        code.push(binary(FloatBinary::Multiply, dst, within, inverse));
    }

    fn ternary(
        &mut self,
        node: Node,
        op: Ternary,
        a: Node,
        b: Node,
        c: Node,
        code: &mut Vec<Instruction>,
    ) {
        let graph = self.graph;
        if op == Ternary::Hsv && self.absorbed.contains(&b) {
            let color = recolored(graph, b, c).unwrap_or_else(|| unreachable!("absorbed recolor"));
            let (hue, shift) = match hue_shift(graph, a, color) {
                Some((_, turns)) if self.absorbed.contains(&a) => (turns, true),
                _ => (a, false),
            };
            let (color, hue) = (self.operand(color), self.operand(hue));
            code.push(Instruction::Recolor {
                shift,
                dst: self.destination(node),
                color,
                hue,
            });
            return;
        }
        let (a, b, c) = (self.operand(a), self.operand(b), self.operand(c));
        let dst = self.destination(node);
        match op {
            Ternary::Clamp => code.push(Instruction::Clamp {
                dst,
                value: a,
                min: b,
                max: c,
            }),
            Ternary::Mix => code.push(Instruction::Mix {
                colors: Colors::NONE,
                dst,
                a,
                b,
                amount: c,
            }),
            Ternary::MixColor => code.push(Instruction::Mix {
                colors: Colors::NONE.with(0).with(1),
                dst,
                a,
                b,
                amount: c,
            }),
            Ternary::Rgb => {
                for (channel, src) in [a, b, c].into_iter().enumerate() {
                    code.push(Instruction::Move {
                        bank: Bank::Float,
                        dst: dst.channel(channel as u16),
                        src,
                    });
                }
            }
            Ternary::Hsv => code.push(Instruction::Hsv {
                dst,
                hue: a,
                saturation: b,
                value: c,
            }),
            Ternary::CurveLastCrossing => code.push(Instruction::Curve {
                op: CurveOp::LastCrossing,
                dst,
                curve: a,
                value: b,
                before: c,
            }),
        }
    }

    /// Write `value` into `result`, unless it was computed there.
    fn assign(&mut self, result: Slot, value: Node, code: &mut Vec<Instruction>) {
        let src = self.operand(value);
        if src != result {
            copy(self.bank(value), result, src, code);
        }
    }

    /// Compute `value` directly into `result` when its arm computes it alone.
    fn target(&mut self, value: Node, result: Slot, region: RegionId) {
        if self.plan.uses(value) == 1
            && self.plan.region_of.get(&value) == Some(&region)
            && !is_leaf(self.graph, value)
            && !matches!(
                self.graph.op(value),
                Op::Reduce(_)
                    | Op::LoopIndex(_)
                    | Op::Context(_)
                    | Op::Unary(Unary::Red | Unary::Green | Unary::Blue, _)
            )
            && self.is_row(value) == !result.is_scalar()
        {
            self.targets.insert(value, result);
        }
    }

    fn length(&mut self, code: &[Instruction]) -> u16 {
        u16::try_from(words(code)).unwrap_or_else(|_| {
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
        copy(bank, acc, initial, code);
        let (start, end) = (self.operand(data.start), self.operand(data.end));
        let index = self.fresh(Bank::Int, self.is_row(data.index));
        self.values.insert(data.index, index);
        let opened = u16::from(!acc.is_scalar());
        self.open(opened);
        let mut loop_code = Vec::new();
        self.lookups(node, index, None, &mut loop_code);
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
        copy(Bank::Color, acc, black, code);
        let (start, end) = (self.operand(data.start), self.operand(data.end));
        let one = self.float_constant(1.0);
        let pixel = pixel.map_or(one, |pixel| self.operand(pixel));
        let index = self.fresh(Bank::Int, false);
        self.values.insert(data.index, index);
        let (mut source, weight) = self.source_block(input, weight, one);
        // A tabulated scale is read by offset from its table.
        let table =
            scale.and_then(|scale| self.tables.get(&scale).map(|&(table, _)| (scale, table)));
        let skip = table
            .filter(|&(scale, _)| self.plan.uses(scale) == 1)
            .map(|(scale, _)| scale);
        let mut taps = Vec::new();
        self.lookups(node, index, skip, &mut taps);
        self.region(children[0], &mut taps);
        let scale = match table {
            Some((_, table)) => table,
            None => scale.map_or(one, |scale| self.operand(scale)),
        };
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
        weight: Option<Node>,
        one: Slot,
    ) -> (Vec<Instruction>, Slot) {
        let sample = self.fresh(Bank::Color, true);
        let mut source = vec![Instruction::Sample {
            dst: sample,
            input: input as u16,
            pixel: SignalPixel::Current,
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
            decay,
            weight,
        } = *self.graph.op(node)
        else {
            unreachable!("scans are scan taps")
        };
        let one = self.float_constant(1.0);
        let decay = self.operand(decay);
        let (mut source, weight) = self.source_block(input, weight, one);
        let cache = u8::try_from(self.scan_count).unwrap_or_else(|_| {
            self.fail(LowerError::Code);
            0
        });
        self.scan_count += 1;
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

    /// The tables `stage` fills, at the end of its block: for each output, a
    /// `Tabulate` and the code computing it from the reduction's index.
    fn tables(&mut self, stage: Stage, code: &mut Vec<Instruction>) {
        for table in self.plan.tables.clone() {
            if table.stage != stage {
                continue;
            }
            let Op::Reduce(id) = *self.graph.op(table.reduction) else {
                unreachable!("tables are of reductions")
            };
            let data = self.graph.loop_(id).clone();
            let (start, end) = (self.operand(data.start), self.operand(data.end));
            for (output, cone) in &table.outputs {
                let index = self.fresh(Bank::Int, false);
                self.values.insert(data.index, index);
                let mut body = Vec::new();
                for &node in cone {
                    if !self.absorbed.contains(&node) {
                        self.node(node, &mut body);
                    }
                }
                let value = self.operand(*output);
                for node in cone.iter().chain([&data.index]) {
                    self.values.remove(node);
                }
                let bank = self.bank(*output);
                let dst = self.fresh(bank, true);
                let len = u8::try_from(words(&body)).unwrap_or_else(|_| {
                    self.fail(LowerError::Code);
                    0
                });
                code.push(Instruction::Tabulate {
                    bank,
                    dst,
                    index,
                    start,
                    end,
                    value,
                    len,
                });
                code.append(&mut body);
                self.tables.insert(*output, (dst, start));
            }
        }
    }

    /// Read `reduction`'s table outputs at `index`, but `skip`.
    fn lookups(
        &mut self,
        reduction: Node,
        index: Slot,
        skip: Option<Node>,
        code: &mut Vec<Instruction>,
    ) {
        for output in self.plan.table_outputs(reduction) {
            if Some(output) == skip {
                continue;
            }
            let (table, start) = self.tables[&output];
            let bank = self.bank(output);
            let dst = self.fresh(bank, !index.is_scalar());
            code.push(Instruction::Lookup {
                bank,
                dst,
                table,
                index,
                start,
            });
            self.values.insert(output, dst);
        }
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
        let query_end = words(&code);
        code.extend(target);
        let target_end = words(&code);
        code.extend(body);
        if let Some(error) = self.error {
            return Err(error);
        }
        let (Ok(query_end), Ok(target_end), Ok(_)) = (
            u16::try_from(query_end),
            u16::try_from(target_end),
            u16::try_from(words(&code)),
        ) else {
            return Err(LowerError::Code);
        };
        let params = self.param_types().into();
        let (scalars, rows) = slots::allocate(
            &mut code,
            &mut result,
            [usize::from(query_end), usize::from(target_end)],
        );
        if !fits(scalars, Slot::SCALARS) || !fits(rows, Slot::ROWS) {
            return Err(LowerError::Slots);
        }
        if rows.row_bytes() > MAX_ROW_BYTES {
            return Err(LowerError::Rows(rows.row_bytes()));
        }
        if self.deepest > MAX_DEPTH {
            return Err(LowerError::Depth(self.deepest));
        }
        let mut words = Vec::with_capacity(code.len());
        for instruction in &code {
            InstructionWord::encode(instruction, &mut words);
        }
        Ok(BytecodeProgram {
            params,
            code: words.into(),
            query_end,
            target_end,
            result,
            scalars,
            rows,
            depth: self.deepest,
            resources: self.resources.into(),
            enums: self.enums.into(),
            scans: self.scan_count,
        })
    }
}

/// Whether every bank's count is below `limit`.
fn fits(banks: Banks, limit: u16) -> bool {
    [banks.floats, banks.ints, banks.bools, banks.resources]
        .iter()
        .all(|&count| count <= limit)
}

/// `dst = src` of `bank`.
fn copy(bank: Bank, dst: Slot, src: Slot, code: &mut Vec<Instruction>) {
    code.push(Instruction::Move { bank, dst, src });
}

/// A choice of `bank`.
fn select(
    bank: Bank,
    dst: Slot,
    condition: Slot,
    yes: Slot,
    no: Slot,
    code: &mut Vec<Instruction>,
) {
    code.push(Instruction::Select {
        bank,
        dst,
        condition,
        yes,
        no,
    });
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
        Context::FrameRate => ContextRead::FrameRate,
        Context::FramesElapsed => ContextRead::FramesElapsed,
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
