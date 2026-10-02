//! Link typed child calls once. A linked invocation owns its argument pairing;
//! preparation never reconstructs that pairing from independent program/value lists.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorTarget {
    Sample {
        program: Arc<super::super::SampleProgram>,
        params: Box<[ParamDecl]>,
    },
    Generator(Arc<LinkedGenerator>),
}

impl GeneratorTarget {
    fn params(&self) -> &[ParamDecl] {
        match self {
            Self::Sample { params, .. } => params,
            Self::Generator(generator) => generator.program.params(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Argument {
    Emitted(usize),
    Default(Value),
}

#[derive(Clone, Debug, PartialEq)]
struct ChildCall {
    target: GeneratorTarget,
    arguments: Box<[Argument]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkedGenerator {
    program: Arc<GeneratorProgram>,
    children: Box<[ChildCall]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratorInvocation {
    generator: Arc<LinkedGenerator>,
    inputs: Box<[GeneratorInput]>,
}

#[derive(Debug)]
pub struct GeneratorEmission {
    child: SpecializedChild,
    inputs: Box<[GeneratorBinding]>,
    execution: EmissionExecution,
    links: Box<[Option<super::super::ParameterLink>]>,
}

#[derive(Debug)]
pub enum EmissionExecution {
    Sample(Arc<super::super::SampleProgram>),
    Generator(GeneratorInvocation),
}

pub struct LinkedSpecialization {
    calculations: Vec<GeneratorCalculation>,
    children: Vec<GeneratorEmission>,
}

impl LinkedGenerator {
    /// Portable construction may only select pixels inherited from its builder
    /// context. Reject embedded pixel records anywhere in the linked program;
    /// ordinary target selection instructions only preserve existing records.
    pub(crate) fn has_builder_target_provenance(&self) -> bool {
        provenance::program(&self.program)
            && self.children.iter().all(|child| {
                child.arguments.iter().all(|argument| match argument {
                    Argument::Emitted(_) => true,
                    Argument::Default(value) => provenance::value(value),
                }) && match &child.target {
                    GeneratorTarget::Sample { program, params } => {
                        provenance::bytecode(program.bytecode()) && provenance::declarations(params)
                    }
                    GeneratorTarget::Generator(generator) => {
                        generator.has_builder_target_provenance()
                    }
                }
            })
    }

    pub fn link(
        program: Arc<GeneratorProgram>,
        targets: Box<[GeneratorTarget]>,
    ) -> Option<Arc<Self>> {
        if program.emissions.len() != targets.len() {
            return None;
        }
        let mut children = Vec::with_capacity(targets.len());
        for (emission, target) in program.emissions.iter().zip(targets) {
            let params = target.params();
            if let GeneratorTarget::Sample { program, .. } = &target
                && !program
                    .input_types()
                    .iter()
                    .eq(params.iter().map(|param| &param.ty))
            {
                return None;
            }
            if params.len() > u16::MAX as usize
                || emission.iter().enumerate().any(|(index, argument)| {
                    emission[..index]
                        .iter()
                        .any(|previous| previous.name == argument.name)
                        || !params.iter().any(|param| param.name == argument.name)
                })
                || params.iter().enumerate().any(|(index, param)| {
                    params[..index]
                        .iter()
                        .any(|previous| previous.name == param.name)
                })
            {
                return None;
            }
            let mut arguments = Vec::with_capacity(params.len());
            for param in params {
                let argument = match emission
                    .iter()
                    .enumerate()
                    .find(|(_, value)| value.name == param.name)
                {
                    Some((index, argument)) => {
                        if !param.ty.accepts(&argument.ty) || (param.fixed && !argument.fixed) {
                            return None;
                        }
                        Argument::Emitted(index)
                    }
                    None => {
                        let value = param.default.as_ref()?;
                        param.ty.accepts_value(value).then_some(())?;
                        Argument::Default(value.clone())
                    }
                };
                arguments.push(argument);
            }
            children.push(ChildCall {
                target,
                arguments: arguments.into(),
            });
        }
        Some(Arc::new(Self {
            program,
            children: children.into(),
        }))
    }

    pub fn params(&self) -> &[ParamDecl] {
        self.program.params()
    }

    /// External values enter once, when a project instance is accepted. Child
    /// invocations are formed only by the linked call plan below.
    pub fn bind(
        self: &Arc<Self>,
        inputs: Box<[GeneratorInput]>,
    ) -> Result<GeneratorInvocation, RuntimeError> {
        self.program.bind(&inputs)?;
        Ok(GeneratorInvocation {
            generator: Arc::clone(self),
            inputs,
        })
    }
}

impl GeneratorInvocation {
    pub(crate) fn params(&self) -> &[ParamDecl] {
        self.generator.program.params()
    }
    pub fn specialize(&self, context: &GeneratorContext) -> LinkedSpecialization {
        let result = BoundGenerator {
            program: &self.generator.program,
            inputs: &self.inputs,
        }
        .specialize(context);
        let children = result
            .children
            .into_iter()
            .map(|child| {
                let call = &self.generator.children[child.definition.0 as usize];
                let inputs: Box<[_]> = call
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        Argument::Emitted(index) => child.params[*index].1.clone(),
                        Argument::Default(value) => GeneratorBinding::Constant(value.clone()),
                    })
                    .collect();
                let execution = match &call.target {
                    GeneratorTarget::Sample { program, .. } => {
                        EmissionExecution::Sample(Arc::clone(program))
                    }
                    GeneratorTarget::Generator(generator) => {
                        EmissionExecution::Generator(GeneratorInvocation {
                            generator: Arc::clone(generator),
                            inputs: inputs
                                .iter()
                                .map(|input| match input {
                                    GeneratorBinding::Constant(value) => {
                                        GeneratorInput::Fixed(value.clone())
                                    }
                                    GeneratorBinding::Parameter(_)
                                    | GeneratorBinding::Calculation { .. } => GeneratorInput::Live,
                                })
                                .collect(),
                        })
                    }
                };
                let links = inputs
                    .iter()
                    .zip(call.target.params())
                    .map(|(input, destination)| {
                        binding_link(
                            input,
                            self.generator.program.params(),
                            &result.calculations,
                            &destination.ty,
                        )
                    })
                    .collect();
                GeneratorEmission {
                    child,
                    inputs,
                    execution,
                    links,
                }
            })
            .collect();
        LinkedSpecialization {
            calculations: result.calculations,
            children,
        }
    }
}

impl LinkedSpecialization {
    pub fn into_parts(self) -> (Vec<GeneratorCalculation>, Vec<GeneratorEmission>) {
        (self.calculations, self.children)
    }
}

impl GeneratorEmission {
    pub(crate) fn links(&self) -> &[Option<super::super::ParameterLink>] {
        &self.links
    }

    pub fn child(&self) -> &SpecializedChild {
        &self.child
    }

    pub fn inputs(&self) -> &[GeneratorBinding] {
        &self.inputs
    }

    pub fn execution(&self) -> &EmissionExecution {
        &self.execution
    }
}
