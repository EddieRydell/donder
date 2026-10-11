use specta_typescript::semantic;
use tauri_specta::Builder;

pub fn builder() -> Builder<tauri::Wry> {
    crate::commands::register(
        Builder::<tauri::Wry>::new()
            .semantic_types(semantic::Configuration::default().enable_lossless_floats()),
    )
}

#[cfg(test)]
mod tests {
    /// Writes the committed frontend bindings; `pnpm generate:bindings` runs it.
    #[test]
    fn generated_bindings() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("frontend")
            .join("src")
            .join("generated")
            .join("bindings.ts");
        super::builder()
            .export(specta_typescript::Typescript::default(), path)
            .unwrap();
    }
}
