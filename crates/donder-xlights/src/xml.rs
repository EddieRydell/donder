//! An XML document read whole into elements, as xLights writes its show files.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

pub(crate) struct Element {
    pub(crate) name: String,
    attributes: Vec<(String, String)>,
    pub(crate) children: Vec<Element>,
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
    let mut reader = Reader::from_str(text);
    let mut open: Vec<Element> = Vec::new();
    let mut root = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("{error} at byte {}", reader.buffer_position()))?;
        match event {
            Event::Start(start) => open.push(element(&start)?),
            Event::Empty(start) => close(&mut open, &mut root, element(&start)?)?,
            Event::End(_) => {
                let element = open.pop().ok_or("an end tag closes nothing")?;
                close(&mut open, &mut root, element)?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !open.is_empty() {
        return Err("the file ends inside an element".to_string());
    }
    root.ok_or_else(|| "the file has no elements".to_string())
}

fn close(open: &mut [Element], root: &mut Option<Element>, element: Element) -> Result<(), String> {
    match open.last_mut() {
        Some(parent) => parent.children.push(element),
        None if root.is_some() => return Err("the file has two root elements".to_string()),
        None => *root = Some(element),
    }
    Ok(())
}

fn element(start: &BytesStart) -> Result<Element, String> {
    let name = start.name().as_ref().to_string();
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
    })
}
