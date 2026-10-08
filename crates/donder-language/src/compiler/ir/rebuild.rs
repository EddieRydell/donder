//! Copy a graph into another with its leaves replaced. Instantiation, fusion
//! and constant signals are rebuilds. Building in the target refolds every
//! node, so a substituted constant simplifies everything that depends on it,
//! and a decided condition copies only the arm it selects.
use super::{Binary, Context, Graph, Node, Op, TooManyLoops};
use donder_runtime_types::Value;
use donder_runtime_types::bytecode::SignalPixel;
use std::collections::HashMap;

pub(crate) trait Substitute {
    /// A replacement for a whole source node, before its operands are copied.
    fn replace(&mut self, _target: &mut Graph, _node: Node) -> Option<Node> {
        None
    }
    fn param(&mut self, target: &mut Graph, index: u32) -> Node {
        target.add(Op::Param(index))
    }
    fn param_integral(&mut self, target: &mut Graph, index: u32) -> Node {
        target.add(Op::ParamIntegral(index))
    }
    fn context(&mut self, target: &mut Graph, context: Context) -> Node {
        target.add(Op::Context(context))
    }
    fn sample(
        &mut self,
        target: &mut Graph,
        input: u32,
        time: Node,
        pixel: SignalPixel<Node>,
    ) -> Result<Node, TooManyLoops> {
        Ok(target.add(Op::Sample { input, time, pixel }))
    }
    /// A node to multiply by instead of dividing by `divisor`.
    fn reciprocal(&mut self, _target: &mut Graph, _divisor: Node) -> Option<Node> {
        None
    }
    /// A value known before playback, for a condition or reduction bound.
    fn decide(&mut self, _target: &Graph, _node: Node) -> Option<Value> {
        None
    }
}

pub(crate) struct Rebuild<'a> {
    source: &'a Graph,
    memo: HashMap<Node, Node>,
}

impl<'a> Rebuild<'a> {
    pub(crate) fn new(source: &'a Graph) -> Self {
        Self {
            source,
            memo: HashMap::new(),
        }
    }

    pub(crate) fn node(
        &mut self,
        target: &mut Graph,
        node: Node,
        substitute: &mut impl Substitute,
    ) -> Result<Node, TooManyLoops> {
        if let Some(&built) = self.memo.get(&node) {
            return Ok(built);
        }
        if let Some(built) = substitute.replace(target, node) {
            self.memo.insert(node, built);
            return Ok(built);
        }
        let built = match self.source.op(node).clone() {
            Op::Constant(constant) => target.add(Op::Constant(constant)),
            Op::Param(index) => substitute.param(target, index),
            Op::ParamIntegral(index) => substitute.param_integral(target, index),
            Op::Context(context) => substitute.context(target, context),
            Op::LoopIndex(_) => unreachable!("a loop index is mapped with its reduction"),
            Op::Unary(op, a) => {
                let a = self.node(target, a, substitute)?;
                target.unary(op, a)
            }
            Op::Binary(op, a, b) => {
                let a = self.node(target, a, substitute)?;
                let inverse = match op {
                    Binary::Divide => substitute.reciprocal(target, b),
                    _ => None,
                };
                match inverse {
                    Some(inverse) => target.binary(Binary::Multiply, a, inverse),
                    None => {
                        let b = self.node(target, b, substitute)?;
                        target.binary(op, a, b)
                    }
                }
            }
            Op::Ternary(op, a, b, c) => {
                let a = self.node(target, a, substitute)?;
                let b = self.node(target, b, substitute)?;
                let c = self.node(target, c, substitute)?;
                target.ternary(op, a, b, c)
            }
            Op::Select(condition, yes, no) => {
                let condition = self.node(target, condition, substitute)?;
                match target.constant_value(condition) {
                    Some(Value::Bool(true)) => self.node(target, yes, substitute)?,
                    Some(Value::Bool(false)) => self.node(target, no, substitute)?,
                    _ => {
                        let yes = self.node(target, yes, substitute)?;
                        let no = self.node(target, no, substitute)?;
                        // A choice that is fixed as a whole is evaluated before
                        // playback; deciding it would only multiply programs.
                        let fixed = target
                            .domain(condition)
                            .union(target.domain(yes))
                            .union(target.domain(no))
                            .is_instance_fixed();
                        match (!fixed)
                            .then(|| substitute.decide(target, condition))
                            .flatten()
                        {
                            Some(Value::Bool(true)) => yes,
                            Some(Value::Bool(false)) => no,
                            _ => target.select(condition, yes, no),
                        }
                    }
                }
            }
            Op::Reduce(id) => {
                let data = self.source.loop_(id).clone();
                let mut bound = |rebuild: &mut Self, target: &mut Graph, node| {
                    let node = rebuild.node(target, node, substitute)?;
                    Ok(match decided(target, node, substitute) {
                        Some(value) => target.constant(value),
                        None => node,
                    })
                };
                let start = bound(self, target, data.start)?;
                let end = bound(self, target, data.end)?;
                let (id, index) = target.begin_loop(start, end)?;
                self.memo.insert(data.index, index);
                let body = self.node(target, data.body, substitute)?;
                let filter = data
                    .filter
                    .map(|filter| self.node(target, filter, substitute))
                    .transpose()?;
                let default = data
                    .default
                    .map(|default| self.node(target, default, substitute))
                    .transpose()?;
                target.finish_loop(id, data.reducer, body, filter, default)
            }
            Op::Items(items) => {
                let items = items
                    .iter()
                    .map(|&item| self.node(target, item, substitute))
                    .collect::<Result<_, _>>()?;
                target.add(Op::Items(items))
            }
            Op::Pick { index, items } => {
                let index = self.node(target, index, substitute)?;
                let items = items
                    .iter()
                    .map(|&item| self.node(target, item, substitute))
                    .collect::<Result<_, _>>()?;
                target.add(Op::Pick { index, items })
            }
            Op::Sample { input, time, pixel } => {
                let time = self.node(target, time, substitute)?;
                let pixel = match pixel {
                    SignalPixel::Current => SignalPixel::Current,
                    SignalPixel::Local(index) => {
                        SignalPixel::Local(self.node(target, index, substitute)?)
                    }
                    SignalPixel::Global(index) => {
                        SignalPixel::Global(self.node(target, index, substitute)?)
                    }
                };
                substitute.sample(target, input, time, pixel)?
            }
        };
        self.memo.insert(node, built);
        Ok(built)
    }
}

fn decided(target: &Graph, node: Node, substitute: &mut impl Substitute) -> Option<Value> {
    match target.constant_value(node) {
        Some(value) => Some(value.clone()),
        None => substitute.decide(target, node),
    }
}
