//! An XML document read into elements: whole, as xLights writes its show
//! files, or only the elements a reader keeps, for files as large as a Vixen
//! profile's module store. Element names are local names, without their
//! namespace prefix; attribute names keep theirs.

use crate::setup::ImportError;
use camino::Utf8Path;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

fn read_text(path: &Utf8Path) -> Result<String, ImportError> {
    let bytes = std::fs::read(path).map_err(|error| ImportError::Read {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let mut text = String::from_utf8(bytes).map_err(|error| ImportError::Xml {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    // .NET writes its files with a byte order mark.
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(text)
}

pub(crate) fn read_file(path: &Utf8Path) -> Result<Element, ImportError> {
    let text = read_text(path)?;
    parse(&text).map_err(|message| ImportError::Xml {
        path: path.to_owned(),
        message,
    })
}

/// The outermost elements of the file at `path` that `keep` keeps, each
/// with everything inside it; `keep` sees each element before its children.
pub(crate) fn read_kept(
    path: &Utf8Path,
    keep: impl FnMut(&Element) -> bool,
) -> Result<Vec<Element>, ImportError> {
    let text = read_text(path)?;
    parse_kept(&text, keep).map_err(|message| ImportError::Xml {
        path: path.to_owned(),
        message,
    })
}

pub(crate) struct Element {
    pub(crate) name: String,
    attributes: Vec<(String, String)>,
    pub(crate) children: Vec<Element>,
    /// The text directly inside the element, its references resolved.
    pub(crate) text: String,
}

impl Element {
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn attribute_names(&self) -> impl Iterator<Item = &str> {
        self.attributes.iter().map(|(key, _)| key.as_str())
    }

    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    pub(crate) fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |child| child.name == name)
    }
}

pub(crate) fn parse(text: &str) -> Result<Element, String> {
    let mut root = None;
    let kept = parse_kept(text, |_| true)?;
    for element in kept {
        if root.replace(element).is_some() {
            return Err("the file has two root elements".to_string());
        }
    }
    root.ok_or_else(|| "the file has no elements".to_string())
}

fn parse_kept(text: &str, mut keep: impl FnMut(&Element) -> bool) -> Result<Vec<Element>, String> {
    let mut reader = Reader::from_str(text);
    // The elements being built, outermost first, inside a kept element.
    let mut open: Vec<Element> = Vec::new();
    // How deep the reader is in elements that are not kept.
    let mut skipped = 0_usize;
    let mut kept = Vec::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("{error} at byte {}", reader.buffer_position()))?;
        match event {
            Event::Start(start) => {
                let element = element(&start)?;
                if open.is_empty() && !keep(&element) {
                    skipped += 1;
                } else {
                    open.push(element);
                }
            }
            Event::Empty(start) => {
                let element = element(&start)?;
                match open.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None if keep(&element) => kept.push(element),
                    None => {}
                }
            }
            Event::End(_) => match open.pop() {
                Some(element) => match open.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None => kept.push(element),
                },
                None => {
                    skipped = skipped.checked_sub(1).ok_or("an end tag closes nothing")?;
                }
            },
            Event::Text(text) => {
                // Indentation between elements is not text of theirs.
                let text = text.xml_content(XmlVersion::Implicit1_0);
                if let Some(element) = open.last_mut()
                    && !text.trim().is_empty()
                {
                    element.text.push_str(&text);
                }
            }
            Event::CData(text) => {
                if let Some(element) = open.last_mut() {
                    element
                        .text
                        .push_str(&text.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(element) = open.last_mut() {
                    match reference
                        .resolve_char_ref()
                        .map_err(|error| error.to_string())?
                    {
                        Some(character) => element.text.push(character),
                        None => {
                            let name = reference.xml_content(XmlVersion::Implicit1_0);
                            let resolved = resolve_predefined_entity(&name)
                                .ok_or_else(|| format!("the entity `&{name};` is not defined"))?;
                            element.text.push_str(resolved);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !open.is_empty() || skipped != 0 {
        return Err("the file ends inside an element".to_string());
    }
    Ok(kept)
}

fn element(start: &BytesStart) -> Result<Element, String> {
    let name = start.local_name().as_ref().to_string();
    let attributes = start
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|error| error.to_string())?;
            let key = attribute.key.as_ref().to_string();
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|error| error.to_string())?
                .into_owned();
            Ok((key, value))
        })
        .collect::<Result<_, String>>()?;
    Ok(Element {
        name,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}
