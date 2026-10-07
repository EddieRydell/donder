//! Byte offsets to LSP positions, which count UTF-16 code units, and file
//! URIs to paths.
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::dsl::TextSpan;
use lsp_types::{Position, Range};

/// The byte offset of each line's start.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
        Self { starts }
    }

    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let start = self.starts[line];
        let character = text
            .get(start..offset)
            .map_or(0, |prefix| prefix.encode_utf16().count());
        Position::new(line as u32, character as u32)
    }

    pub fn offset(&self, text: &str, position: Position) -> usize {
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

    pub fn range(&self, text: &str, span: TextSpan) -> Range {
        Range::new(
            self.position(text, span.start),
            self.position(text, span.end),
        )
    }

    /// A position counted in characters, as project diagnostics count them.
    pub fn character_position(&self, text: &str, line: u32, characters: u32) -> Position {
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

/// `file:///C:/show/project.data.donder` for a path.
pub fn file_uri(path: &Utf8Path) -> String {
    let text = path.as_str().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The path of a `file:` URI.
pub fn uri_path(uri: &str) -> Option<Utf8PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut iter = rest.bytes();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let high = (iter.next()? as char).to_digit(16)?;
            let low = (iter.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    // `/C:/show` is a Windows drive path.
    let path = match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    };
    Some(Utf8PathBuf::from(path))
}
