//! Where each element's colors go: from its channel through the output
//! filters patched to it to controller outputs. Vixen: `DataFlowManager`;
//! an element's channel, each filter and each controller output is a
//! component, and a patch feeds one component from one output of another.

use super::config::Config;
use super::contract;
use crate::report::ElementPatch;
use crate::setup::Slot;
use crate::xml::Element;
use donder_runtime_types::PixelEncoding;
use std::collections::{HashMap, HashSet};

const COLOR_BREAKDOWN: &str = "ab38a16f-0de1-4f6e-a8c0-ae5b20d347e0";
const DIMMING_CURVE: &str = "2e40d6b1-43d2-4668-b63a-c600fadd7dd5";

/// A `System.Drawing.Color` as written: `Color.Equals` compares all four.
#[derive(Debug, PartialEq)]
struct Color {
    known: i32,
    name: Option<String>,
    state: i32,
    value: i64,
}

impl Color {
    /// A named color, as `Color.Red` and the like are.
    fn known(known: i32) -> Self {
        Self {
            known,
            name: None,
            state: 1,
            value: 0,
        }
    }

    fn read(element: &Element) -> Result<Self, contract::FieldError> {
        Ok(Self {
            known: contract::int(element, "knownColor", 0)?,
            name: contract::text(element, "name").map(str::to_string),
            state: contract::int(element, "state", 0)?,
            value: contract::text(element, "value")
                .unwrap_or("0")
                .trim()
                .parse()
                .map_err(|_| contract::FieldError {
                    field: "value".to_string(),
                    value: contract::text(element, "value")
                        .unwrap_or_default()
                        .to_string(),
                })?,
        })
    }
}

/// `KnownColor` values of the colors a mixing breakdown tells apart.
const RED: i32 = 141;
const LIME: i32 = 104;

/// A color breakdown's settings. Vixen: `ColorBreakdownData`.
pub(crate) struct Breakdown {
    colors: Vec<Color>,
    mix: bool,
    sixteen_bit: bool,
}

impl Breakdown {
    pub(crate) fn read(data: &Element) -> Result<Self, contract::FieldError> {
        let colors = contract::field(data, "BreakdownItems")
            .into_iter()
            .flat_map(|items| items.children("ColorBreakdownItem"))
            .map(|item| Color::read(contract::required(item, "Color")?))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            colors,
            mix: contract::boolean(data, "MixColors", false)?,
            sixteen_bit: contract::boolean(data, "_16Bit", false)?,
        })
    }

    /// Which of red, green and blue each output sends. Vixen:
    /// `RGBColorBreakdownMixingFilter.ConfigureRGBFilter`, which takes
    /// `Color.Red` as red, `Color.Lime` as green, and any other color as
    /// blue.
    fn channels(&self) -> Result<[u8; 3], ElementPatch> {
        if self.sixteen_bit {
            return Err(ElementPatch::SixteenBit);
        }
        if !self.mix {
            return Err(ElementPatch::DiscreteColors);
        }
        // Vixen: ColorBreakdownModule._CreateOutputs mixes four colors to
        // RGBW with its own conversion.
        if self.colors.len() == 4 {
            return Err(ElementPatch::Rgbw);
        }
        let [a, b, c] = &self.colors[..] else {
            return Err(ElementPatch::Colors);
        };
        let channel = |color: &Color| {
            if *color == Color::known(RED) {
                0
            } else if *color == Color::known(LIME) {
                1
            } else {
                2
            }
        };
        let order = [channel(a), channel(b), channel(c)];
        if (PixelEncoding::Rgb { order }).is_valid() {
            Ok(order)
        } else {
            Err(ElementPatch::Colors)
        }
    }
}

/// Each element channel's pixel: where its first color goes and the order
/// of its colors.
pub(crate) struct Patching<'a> {
    /// What each component's outputs feed, by component and output.
    feeds: HashMap<(&'a str, u32), Vec<&'a str>>,
    config: &'a Config,
    breakdowns: &'a HashMap<String, Result<Breakdown, contract::FieldError>>,
    /// Where each controller output's channel is, by output id.
    slots: &'a HashMap<String, Slot>,
    /// Controller output ids.
    outputs: HashSet<&'a str>,
}

impl<'a> Patching<'a> {
    pub(crate) fn new(
        config: &'a Config,
        breakdowns: &'a HashMap<String, Result<Breakdown, contract::FieldError>>,
        slots: &'a HashMap<String, Slot>,
    ) -> Self {
        let mut feeds: HashMap<(&str, u32), Vec<&str>> = HashMap::new();
        for patch in &config.patches {
            feeds
                .entry((patch.source.as_str(), patch.output))
                .or_default()
                .push(patch.component.as_str());
        }
        let outputs = config
            .controllers
            .iter()
            .flat_map(|controller| &controller.outputs)
            .map(String::as_str)
            .collect();
        Self {
            feeds,
            config,
            breakdowns,
            slots,
            outputs,
        }
    }

    /// The one component an output feeds.
    fn fed(&self, source: &str, output: u32) -> Result<&'a str, ElementPatch> {
        match self
            .feeds
            .get(&(source, output))
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            [] => Err(ElementPatch::Unpatched),
            [component] => Ok(component),
            _ => Err(ElementPatch::FanOut),
        }
    }

    /// The pixel of the element whose channel is `channel`: the slot of its
    /// first channel and its color order.
    pub(crate) fn pixel(&self, channel: &str) -> Result<(Slot, PixelEncoding), ElementPatch> {
        let component = self.fed(channel, 0)?;
        if self.outputs.contains(component) {
            return Err(ElementPatch::SingleChannel);
        }
        let kind = self
            .config
            .filters
            .get(component)
            .map(String::as_str)
            .unwrap_or_default();
        match kind {
            COLOR_BREAKDOWN => {}
            DIMMING_CURVE => return Err(ElementPatch::DimmingCurve),
            other => return Err(ElementPatch::Filter(other.to_string())),
        }
        let breakdown = match self.breakdowns.get(component) {
            None => return Err(ElementPatch::NoBreakdownSettings),
            Some(Err(error)) => return Err(ElementPatch::BreakdownField(error.clone())),
            Some(Ok(breakdown)) => breakdown,
        };
        let colors = breakdown.channels()?;
        let mut slots = Vec::new();
        for (output, color) in colors.iter().enumerate() {
            let fed = self
                .fed(component, output as u32)
                .map_err(|problem| match problem {
                    ElementPatch::Unpatched => ElementPatch::Unrouted,
                    other => other,
                })?;
            if !self.outputs.contains(fed) {
                return Err(match self.config.filters.get(fed) {
                    Some(kind) if kind == DIMMING_CURVE => ElementPatch::DimmingCurve,
                    Some(kind) => ElementPatch::Filter(kind.clone()),
                    None => ElementPatch::Unrouted,
                });
            }
            let slot = *self.slots.get(fed).ok_or(ElementPatch::OutsideUniverses)?;
            slots.push((slot, *color));
        }
        let first = slots
            .iter()
            .map(|(slot, _)| *slot)
            .min_by_key(|slot| slot.offset)
            .ok_or(ElementPatch::Colors)?;
        let mut order = [0_u8; 3];
        let mut taken = [false; 3];
        for (slot, color) in &slots {
            let position = slot.offset.wrapping_sub(first.offset) as usize;
            if slot.controller != first.controller
                || slot.output != first.output
                || position >= 3
                || taken[position]
            {
                return Err(ElementPatch::Scattered);
            }
            taken[position] = true;
            order[position] = *color;
        }
        Ok((first, PixelEncoding::Rgb { order }))
    }
}
