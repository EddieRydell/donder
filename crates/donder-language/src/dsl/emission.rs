use super::{Diagnostic, EmittedReference, ParamDecl, Type};

/// Validate each authored emission against its linked declaration, including
/// emissions in branches that will not execute for the current instance.
pub fn validate_emission(
    emission: &EmittedReference,
    declarations: &[ParamDecl],
) -> Result<(), Diagnostic> {
    for (index, argument) in emission.arguments.iter().enumerate() {
        if emission.arguments[..index]
            .iter()
            .any(|previous| previous.name == argument.name)
        {
            return Err(Diagnostic::new(
                argument.span,
                format!("duplicate emitted field `{}`", argument.name.as_str()),
            ));
        }
        let expected = match argument.name.as_str() {
            "start" | "duration" => &Type::Float,
            "target" => {
                if matches!(
                    argument.ty,
                    Type::Target | Type::TargetItem | Type::TargetItems
                ) {
                    continue;
                }
                return Err(Diagnostic::new(
                    argument.span,
                    "emitted target must be a target selection",
                ));
            }
            _ => {
                let declaration = declarations
                    .iter()
                    .find(|param| param.name == argument.name)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            argument.span,
                            format!("unknown generated param `{}`", argument.name.as_str()),
                        )
                    })?;
                if declaration.fixed
                    && let Some(origin) = &argument.live_dependency
                {
                    return Err(Diagnostic::new(
                        argument.span,
                        format!(
                            "live dependency `{}` reaches fixed child parameter `{}`",
                            origin.as_str(),
                            argument.name.as_str()
                        ),
                    ));
                }
                &declaration.ty
            }
        };
        if !expected.accepts(&argument.ty) {
            return Err(Diagnostic::new(
                argument.span,
                format!(
                    "generated param `{}` expects {expected:?}, got {:?}",
                    argument.name.as_str(),
                    argument.ty
                ),
            ));
        }
    }
    for name in ["start", "duration", "target"] {
        if !emission
            .arguments
            .iter()
            .any(|argument| argument.name.as_str() == name)
        {
            return Err(Diagnostic::new(
                emission.span,
                format!("emit missing {name}"),
            ));
        }
    }
    for declaration in declarations {
        if declaration.default.is_none()
            && !emission
                .arguments
                .iter()
                .any(|argument| argument.name == declaration.name)
        {
            return Err(Diagnostic::new(
                emission.span,
                format!("missing generated param `{}`", declaration.name.as_str()),
            ));
        }
    }
    Ok(())
}
