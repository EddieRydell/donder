//! Where each element's colors go: from its channel through the output
//! filters patched to it to controller outputs. Vixen: `DataFlowManager`;
//! an element's channel, each filter and each controller output is a
//! component, and a patch feeds one component from one output of another.

use super::config::Config;
use super::contract;
use crate::report::ElementPatch;
use crate::setup::{Dimming, Slot};
use crate::xml::Element;
use donder_runtime_types::PixelEncoding;
use std::collections::{HashMap, HashSet};

pub(crate) const COLOR_BREAKDOWN: &str = "ab38a16f-0de1-4f6e-a8c0-ae5b20d347e0";
pub(crate) const DIMMING_CURVE: &str = "2e40d6b1-43d2-4668-b63a-c600fadd7dd5";

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
const WHITE: i32 = 164;

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

    /// Which of red, green, blue and white each output sends. Vixen:
    /// `RGBColorBreakdownMixingFilter.ConfigureRGBFilter`, which takes
    /// `Color.Red` as red, `Color.Lime` as green, and any other color as
    /// blue; a breakdown of four colors mixes RGBW, its `Color.White`
    /// output the white. Vixen's RGBW mix and Donder's both light white as
    /// the least of red, green and blue.
    fn channels(&self) -> Result<Vec<u8>, ElementPatch> {
        if self.sixteen_bit {
            return Err(ElementPatch::SixteenBit);
        }
        if !self.mix {
            return Err(ElementPatch::DiscreteColors);
        }
        let rgbw = self.colors.len() == 4;
        let channel = |color: &Color| {
            if *color == Color::known(RED) {
                0
            } else if *color == Color::known(LIME) {
                1
            } else if rgbw && *color == Color::known(WHITE) {
                3
            } else {
                2
            }
        };
        let order: Vec<u8> = self.colors.iter().map(channel).collect();
        let valid = match order[..] {
            [a, b, c] => (PixelEncoding::Rgb { order: [a, b, c] }).is_valid(),
            [a, b, c, d] => (PixelEncoding::Rgbw {
                order: [a, b, c, d],
            })
            .is_valid(),
            _ => false,
        };
        if valid {
            Ok(order)
        } else {
            Err(ElementPatch::Colors)
        }
    }
}

/// A dimming curve's points, input to output in percent. Vixen:
/// `DimmingCurveData.Curve`, a ZedGraph `PointPairList`.
pub(crate) struct Curve {
    points: Vec<(f64, f64)>,
}

impl Curve {
    pub(crate) fn read(data: &Element) -> Result<Self, contract::FieldError> {
        let points = contract::required(contract::required(data, "Curve")?, "Points")?
            .children("PointPair")
            .map(|pair| {
                Ok((
                    contract::double(pair, "X", 0.0)?,
                    contract::double(pair, "Y", 0.0)?,
                ))
            })
            .collect::<Result<_, contract::FieldError>>()?;
        Ok(Self { points })
    }

    /// The curve's output at `x`, both in percent, between its points.
    /// Vixen: `Curve.GetValue`.
    fn at(&self, x: f64) -> f64 {
        let mut points = self.points.iter().copied();
        let Some(mut previous) = points.next() else {
            return x;
        };
        if x <= previous.0 {
            return previous.1;
        }
        for point in points {
            if x <= point.0 {
                let span = point.0 - previous.0;
                let along = if span > 0.0 {
                    (x - previous.0) / span
                } else {
                    1.0
                };
                return previous.1 + (point.1 - previous.1) * along;
            }
            previous = point;
        }
        previous.1
    }

    /// The brightness and gamma closest to the curve: its output at full
    /// input, and the power that best fits its shape below that, by least
    /// squares on the logarithms. A curve that is exactly a brightness and a
    /// gamma fits exactly; any other comes close.
    pub(crate) fn fit(&self) -> Dimming {
        let brightness = (self.at(100.0) / 100.0).clamp(0.0, 1.0);
        let (mut product, mut square) = (0.0, 0.0);
        if brightness > 0.0 {
            for x in (1..100).map(f64::from) {
                let y = self.at(x) / (100.0 * brightness);
                if y > 0.0 {
                    let input = (x / 100.0).ln();
                    product += input * y.ln();
                    square += input * input;
                }
            }
        }
        let gamma = if square > 0.0 {
            (product / square).max(0.01)
        } else {
            1.0
        };
        Dimming {
            brightness: brightness as f32,
            gamma: gamma as f32,
        }
    }
}

/// A pixel's channels: where the first is, its color order, and the dimming
/// its curves come closest to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PatchedPixel {
    pub(crate) slot: Slot,
    pub(crate) encoding: PixelEncoding,
    pub(crate) dimming: Option<Dimming>,
}

/// `inner`'s output dimmed by `outer`: `b₂ (b₁ x^g₁)^g₂`.
fn compose(inner: Option<Dimming>, outer: Option<Dimming>) -> Option<Dimming> {
    match (inner, outer) {
        (None, dimming) | (dimming, None) => dimming,
        (Some(inner), Some(outer)) => Some(Dimming {
            brightness: outer.brightness * inner.brightness.powf(outer.gamma),
            gamma: inner.gamma * outer.gamma,
        }),
    }
}

/// Each element channel's pixel: where its first color goes and the order
/// of its colors.
pub(crate) struct Patching<'a> {
    /// What each component's outputs feed, by component and output.
    feeds: HashMap<(&'a str, u32), Vec<&'a str>>,
    config: &'a Config,
    breakdowns: &'a HashMap<String, Result<Breakdown, contract::FieldError>>,
    curves: &'a HashMap<String, Result<Curve, contract::FieldError>>,
    /// Where each controller output's channel is, by output id.
    slots: &'a HashMap<String, Slot>,
    /// Controller output ids.
    outputs: HashSet<&'a str>,
}

impl<'a> Patching<'a> {
    pub(crate) fn new(
        config: &'a Config,
        breakdowns: &'a HashMap<String, Result<Breakdown, contract::FieldError>>,
        curves: &'a HashMap<String, Result<Curve, contract::FieldError>>,
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
            curves,
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

    /// `component` past any dimming curves it is, and the dimming they come
    /// closest to.
    fn undimmed(&self, mut component: &'a str) -> Result<(&'a str, Option<Dimming>), ElementPatch> {
        let mut dimming = None;
        while self.config.filters.get(component).map(String::as_str) == Some(DIMMING_CURVE) {
            let curve = match self.curves.get(component) {
                None => return Err(ElementPatch::NoCurveSettings),
                Some(Err(error)) => return Err(ElementPatch::CurveField(error.clone())),
                Some(Ok(curve)) => curve,
            };
            dimming = compose(dimming, Some(curve.fit()));
            component = self.fed(component, 0)?;
        }
        Ok((component, dimming))
    }

    /// The pixel of the element whose channel is `channel`: the slot of its
    /// first channel, its color order, and the dimming its curves come
    /// closest to, before its breakdown and after it. Colors dimmed
    /// differently take their average dimming.
    pub(crate) fn pixel(&self, channel: &str) -> Result<PatchedPixel, ElementPatch> {
        let (component, before) = self.undimmed(self.fed(channel, 0)?)?;
        if self.outputs.contains(component) {
            return Err(ElementPatch::SingleChannel);
        }
        let kind = self
            .config
            .filters
            .get(component)
            .map(String::as_str)
            .unwrap_or_default();
        if kind != COLOR_BREAKDOWN {
            return Err(ElementPatch::Filter(kind.to_string()));
        }
        let breakdown = match self.breakdowns.get(component) {
            None => return Err(ElementPatch::NoBreakdownSettings),
            Some(Err(error)) => return Err(ElementPatch::BreakdownField(error.clone())),
            Some(Ok(breakdown)) => breakdown,
        };
        let colors = breakdown.channels()?;
        let mut slots = Vec::new();
        let mut after = Vec::new();
        for (output, color) in colors.iter().enumerate() {
            let fed = self
                .fed(component, output as u32)
                .map_err(|problem| match problem {
                    ElementPatch::Unpatched => ElementPatch::Unrouted,
                    other => other,
                })?;
            let (fed, dimming) = self.undimmed(fed)?;
            if !self.outputs.contains(fed) {
                return Err(match self.config.filters.get(fed) {
                    Some(kind) => ElementPatch::Filter(kind.clone()),
                    None => ElementPatch::Unrouted,
                });
            }
            let slot = *self.slots.get(fed).ok_or(ElementPatch::OutsideUniverses)?;
            slots.push((slot, *color));
            after.push(dimming);
        }
        let after = after.iter().any(Option::is_some).then(|| {
            let full = Dimming {
                brightness: 1.0,
                gamma: 1.0,
            };
            let count = after.len() as f32;
            let each = || after.iter().map(|dimming| dimming.unwrap_or(full));
            Dimming {
                brightness: each().map(|dimming| dimming.brightness).sum::<f32>() / count,
                gamma: each().map(|dimming| dimming.gamma).sum::<f32>() / count,
            }
        });
        let first = slots
            .iter()
            .map(|(slot, _)| *slot)
            .min_by_key(|slot| slot.offset)
            .ok_or(ElementPatch::Colors)?;
        let width = slots.len();
        let mut order = [0_u8; 4];
        let mut taken = [false; 4];
        for (slot, color) in &slots {
            let position = slot.offset.wrapping_sub(first.offset) as usize;
            if slot.controller != first.controller
                || slot.output != first.output
                || position >= width
                || taken[position]
            {
                return Err(ElementPatch::Scattered);
            }
            taken[position] = true;
            order[position] = *color;
        }
        let encoding = match width {
            3 => PixelEncoding::Rgb {
                order: [order[0], order[1], order[2]],
            },
            _ => PixelEncoding::Rgbw { order },
        };
        Ok(PatchedPixel {
            slot: first,
            encoding,
            dimming: compose(before, after),
        })
    }
}
