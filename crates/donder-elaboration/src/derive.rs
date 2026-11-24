//! Deriving the data effects read from asset files: audio analyses, font
//! rasters and images.
use donder_model::{AudioFeature, Derivation, Derived, FontRaster, ImageSource};
use donder_runtime_types::{Font, Glyph, Image, Track};
use std::path::Path;
use std::sync::Arc;

/// What each of `derivations`, all of the asset file at `path`, derives. The
/// file is read once, and audio and images decoded once.
pub fn derive(path: &Path, derivations: &[Derivation]) -> Vec<Derived> {
    let mut audio = None;
    let mut bytes = None;
    let mut frames = None;
    derivations
        .iter()
        .map(|derivation| match derivation {
            Derivation::Audio(analysis) => {
                let audio = audio.get_or_insert_with(|| {
                    donder_audio_analysis::decode_audio(path).map_err(|error| error.to_string())
                });
                match audio {
                    Ok(audio) => {
                        match donder_audio_analysis::analyze_audio(
                            audio,
                            &feature(&analysis.feature),
                        ) {
                            Ok(track) => Derived::Track(Arc::new(Track::new(
                                track.samples,
                                track.channels,
                                track.signed,
                                track.rate,
                            ))),
                            Err(error) => Derived::Failed(error.to_string()),
                        }
                    }
                    Err(error) => Derived::Failed(error.clone()),
                }
            }
            Derivation::Font(raster) => {
                let bytes = bytes
                    .get_or_insert_with(|| fs_err::read(path).map_err(|error| error.to_string()));
                match bytes {
                    Ok(bytes) => rasterize(bytes, raster)
                        .map_or_else(Derived::Failed, |font| Derived::Font(Arc::new(font))),
                    Err(error) => Derived::Failed(error.clone()),
                }
            }
            Derivation::Image(source) => {
                let frames = frames.get_or_insert_with(|| decode_frames(path));
                match frames {
                    Ok(frames) => Derived::Image(Arc::new(image(frames, source))),
                    Err(error) => Derived::Failed(error.clone()),
                }
            }
        })
        .collect()
}

/// An image file's frames, each with how long it shows in milliseconds; a
/// still picture is one frame.
fn decode_frames(path: &Path) -> Result<Vec<(image::RgbaImage, u32)>, String> {
    use image::AnimationDecoder;
    let error = |error: image::ImageError| error.to_string();
    let reader = image::ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let animation = |frames: image::Frames<'_>| -> Result<Vec<_>, String> {
        frames
            .map(|frame| {
                let frame = frame.map_err(error)?;
                let (numerator, denominator) = frame.delay().numer_denom_ms();
                let millis = numerator / denominator.max(1);
                Ok((frame.into_buffer(), millis))
            })
            .collect()
    };
    let file = || {
        fs_err::File::open(path)
            .map(std::io::BufReader::new)
            .map_err(|error| error.to_string())
    };
    match reader.format() {
        Some(image::ImageFormat::Gif) => animation(
            image::codecs::gif::GifDecoder::new(file()?)
                .map_err(error)?
                .into_frames(),
        ),
        Some(image::ImageFormat::Png) => {
            let decoder = image::codecs::png::PngDecoder::new(file()?).map_err(error)?;
            if decoder.is_apng().map_err(error)? {
                animation(decoder.apng().map_err(error)?.into_frames())
            } else {
                Ok(vec![(reader.decode().map_err(error)?.to_rgba8(), 0)])
            }
        }
        Some(image::ImageFormat::WebP) => {
            let decoder = image::codecs::webp::WebPDecoder::new(file()?).map_err(error)?;
            if decoder.has_animation() {
                animation(decoder.into_frames())
            } else {
                Ok(vec![(reader.decode().map_err(error)?.to_rgba8(), 0)])
            }
        }
        _ => Ok(vec![(reader.decode().map_err(error)?.to_rgba8(), 0)]),
    }
}

/// The fastest an animation plays, in frames a second.
const MAX_IMAGE_RATE: u32 = 50;

/// `frames` scaled so the longer side is at most the source's size, over
/// black where they are transparent, shown at one rate: each frame repeats
/// for as many of the shortest frame's lengths as it lasts. As browsers do,
/// a frame of 10 ms or less lasts 100 ms.
fn image(frames: &[(image::RgbaImage, u32)], source: &ImageSource) -> Image {
    let Some((first, _)) = frames.first() else {
        return Image::empty();
    };
    let (width, height) = first.dimensions();
    let scale = (source.size as f32 / width.max(height) as f32).min(1.0);
    let scaled = |side: u32| ((side as f32 * scale).round() as u32).max(1);
    let (width, height) = (scaled(width), scaled(height));
    let lengths: Vec<u32> = frames
        .iter()
        .map(|(_, millis)| if *millis <= 10 { 100 } else { *millis })
        .collect();
    let animated = frames.len() > 1;
    let tick = lengths
        .iter()
        .copied()
        .min()
        .unwrap_or(100)
        .max(1000 / MAX_IMAGE_RATE);
    let mut pixels = Vec::new();
    let mut count = 0;
    for ((frame, _), length) in frames.iter().zip(&lengths) {
        let resized =
            image::imageops::resize(frame, width, height, image::imageops::FilterType::Triangle);
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for pixel in resized.pixels() {
            let [red, green, blue, alpha] = pixel.0;
            let over_black =
                |channel: u8| ((u32::from(channel) * u32::from(alpha) + 127) / 255) as u8;
            rgb.extend([over_black(red), over_black(green), over_black(blue)]);
        }
        let repeats = if animated {
            ((*length as f32 / tick as f32).round() as u32).max(1)
        } else {
            1
        };
        for _ in 0..repeats {
            pixels.extend_from_slice(&rgb);
        }
        count += repeats;
    }
    Image {
        width,
        height,
        frames: count,
        rate: if animated { 1000.0 / tick as f32 } else { 0.0 },
        pixels: pixels.into(),
    }
}

/// The font file `bytes` drawn at the raster's height: each of its
/// characters the font has, antialiased, and the line from the top of its
/// ascent to the bottom of its descent.
fn rasterize(bytes: &[u8], raster: &FontRaster) -> Result<Font, String> {
    use ab_glyph::{Font as _, ScaleFont as _};
    let font = ab_glyph::FontRef::try_from_slice(bytes)
        .map_err(|_| "the file is not a TrueType or OpenType font".to_string())?;
    let scaled = font.as_scaled(raster.height as f32);
    let ascent = scaled.ascent();
    let mut glyphs = Vec::new();
    let mut coverage = Vec::new();
    for &code in raster.characters.iter() {
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        let id = font.glyph_id(character);
        if id.0 == 0 {
            continue;
        }
        let mut glyph = Glyph {
            code,
            advance: scaled.h_advance(id),
            ..Glyph::default()
        };
        let placed = id.with_scale_and_position(scaled.scale(), ab_glyph::point(0.0, ascent));
        if let Some(outline) = font.outline_glyph(placed) {
            let bounds = outline.px_bounds();
            glyph.left = bounds.min.x as i32;
            glyph.top = bounds.min.y as i32;
            glyph.width = bounds.width() as u32;
            glyph.height = bounds.height() as u32;
            glyph.offset = coverage.len() as u32;
            let start = coverage.len();
            coverage.resize(start + (glyph.width * glyph.height) as usize, 0);
            outline.draw(|x, y, amount| {
                coverage[start + (y * glyph.width + x) as usize] =
                    (amount.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
        }
        glyphs.push(glyph);
    }
    Ok(Font {
        height: scaled.height(),
        glyphs: glyphs.into(),
        coverage: coverage.into(),
    })
}

fn feature(feature: &AudioFeature) -> donder_audio_analysis::AudioFeature {
    let number = |number: &donder_model::Number| f64::from(number.0);
    match feature {
        AudioFeature::Level {
            low,
            high,
            attack,
            decay,
        } => donder_audio_analysis::AudioFeature::Level {
            low: number(low),
            high: number(high),
            attack: number(attack),
            decay: number(decay),
        },
        AudioFeature::Spectrum {
            bands,
            attack,
            decay,
        } => donder_audio_analysis::AudioFeature::Spectrum {
            bands: number(bands),
            attack: number(attack),
            decay: number(decay),
        },
        AudioFeature::Wave => donder_audio_analysis::AudioFeature::Wave,
    }
}
