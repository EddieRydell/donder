use crate::PackageError;
use camino::Utf8Path;

pub(crate) fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}

pub(crate) fn validate_relative_path(value: &str, label: &str) -> Result<(), PackageError> {
    let path = Utf8Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component.ends_with('.')
                || component.ends_with(' ')
                || is_windows_reserved_component(component)
        })
        || path.components().any(|component| {
            matches!(
                component,
                camino::Utf8Component::ParentDir
                    | camino::Utf8Component::RootDir
                    | camino::Utf8Component::Prefix(_)
            )
        })
    {
        return Err(PackageError::Invalid(format!(
            "{label} must be a safe module-relative path"
        )));
    }
    Ok(())
}

pub fn validate_module_relative_donder_path(value: &str) -> Result<(), PackageError> {
    validate_relative_path(value, "Donder document")?;
    require_donder_document(value, "Donder document")
}

pub fn validate_package_reference_name(value: &str) -> Result<(), PackageError> {
    if !valid_alias(value) {
        return Err(PackageError::Invalid(format!(
            "invalid package reference name `{value}`"
        )));
    }
    Ok(())
}

pub(crate) fn is_windows_reserved_component(component: &str) -> bool {
    let basename = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || basename.strip_prefix("COM").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || basename.strip_prefix("LPT").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
}

pub(crate) fn require_donder_document(path: &str, label: &str) -> Result<(), PackageError> {
    if !path.ends_with(".donder") {
        return Err(PackageError::Invalid(format!(
            "{label} `{path}` must be a Donder document"
        )));
    }
    Ok(())
}
