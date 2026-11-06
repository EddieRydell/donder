//! Byte offsets to LSP positions, which count UTF-16 code units, and file
//! URIs to paths.
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::TextSpan;
use lsp_types::{Position, Range};
use url::Url;

/// The byte offset of each line's start.
pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
        Self { starts }
    }

    pub(crate) fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let start = self.starts[line];
        let character = text
            .get(start..offset)
            .map_or(0, |prefix| prefix.encode_utf16().count());
        Position::new(line as u32, character as u32)
    }

    pub(crate) fn offset(&self, text: &str, position: Position) -> usize {
        let Some(&start) = self.starts.get(position.line as usize) else {
            return text.len();
        };
        let end = self
            .starts
            .get(position.line as usize + 1)
            .map_or(text.len(), |next| next - 1);
        let line = &text[start..end.max(start)];
        let mut units = 0;
        for (index, character) in line.char_indices() {
            if units >= position.character as usize {
                return start + index;
            }
            units += character.len_utf16();
        }
        start + line.len()
    }

    pub(crate) fn range(&self, text: &str, span: TextSpan) -> Range {
        Range::new(
            self.position(text, span.start),
            self.position(text, span.end),
        )
    }

    /// A position counted in characters, as project diagnostics count them.
    pub(crate) fn character_position(&self, text: &str, line: u32, characters: u32) -> Position {
        let Some(&start) = self.starts.get(line as usize) else {
            return self.position(text, text.len());
        };
        let offset = text[start..]
            .char_indices()
            .take_while(|(_, character)| *character != '\n')
            .nth(characters as usize)
            .map_or_else(
                || start + text[start..].find('\n').unwrap_or(text.len() - start),
                |(index, _)| start + index,
            );
        self.position(text, offset)
    }
}

/// The `file:` URI of an absolute path.
pub fn file_uri(path: &Utf8Path) -> Option<String> {
    Url::from_file_path(path).ok().map(String::from)
}

/// The path of a `file:` URI.
pub fn uri_path(uri: &str) -> Option<Utf8PathBuf> {
    let path = Url::parse(uri).ok()?.to_file_path().ok()?;
    Utf8PathBuf::from_path_buf(path).ok()
}
