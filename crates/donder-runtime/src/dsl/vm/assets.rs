//! The instructions that read fonts, images and mark labels. Few programs
//! use them and their sampling dominates their cost, so they run from flash,
//! outside the instruction RAM the rest of the interpreter fills.
use super::strip::{Handle, Machine, Sel, apply1, apply3, pick_color};
use crate::dsl::bytecode::{GlyphOp, ImageOp, Instruction};
use crate::sampling::{glyph_advance, glyph_coverage, image_color, mark_label_char};
use donder_runtime_types::{FontView, ImageView};

/// Run `instruction`, one of the asset instructions, on `machine`.
#[inline(never)]
pub(super) fn run(machine: &Machine<'_, '_>, instruction: &Instruction, sel: Sel<'_>) {
    use Instruction as I;
    match *instruction {
        I::MarkText {
            dst,
            marks,
            index,
            char,
        } => {
            let (dst, marks) = (machine.int_dst(dst), machine.handle(marks));
            let (index, char) = (machine.int(index), machine.int(char));
            apply3(sel, dst, marks, index, char, |marks, index, char| {
                mark_label_char(&machine.marks(marks), index, char)
            });
        }
        I::Glyph {
            op,
            dst,
            font,
            code,
            x,
            y,
        } => {
            let (dst, font) = (machine.float_dst(dst), machine.handle(font));
            match op {
                GlyphOp::Height => apply1(sel, dst, font, |font| font_view(machine, font).height),
                GlyphOp::Advance => {
                    let (font, code) = (font_view(machine, font.at(0)), machine.int(code));
                    apply1(sel, dst, code, |code| glyph_advance(&font, code));
                }
                GlyphOp::Coverage => {
                    let font = font_view(machine, font.at(0));
                    let code = machine.int(code);
                    let (x, y) = (machine.float(x), machine.float(y));
                    apply3(sel, dst, code, x, y, |code, x, y| {
                        glyph_coverage(&font, code, x, y)
                    });
                }
            }
        }
        I::Image {
            op,
            dst,
            image,
            frame,
            x,
            y,
        } => {
            let image = machine.handle(image);
            match op {
                ImageOp::Color => {
                    let image = image_view(machine, image.at(0));
                    let (frame, x, y) = (machine.int(frame), machine.float(x), machine.float(y));
                    pick_color(sel, machine.color_dst(dst), |i| {
                        image_color(&image, frame.at(i), x.at(i), y.at(i))
                    });
                }
                ImageOp::Frames => apply1(sel, machine.int_dst(dst), image, |image| {
                    image_view(machine, image).frames as i32
                }),
                measure => apply1(sel, machine.float_dst(dst), image, |image| {
                    let image = image_view(machine, image);
                    match measure {
                        ImageOp::Width => image.width as f32,
                        ImageOp::Height => image.height as f32,
                        _ => image.rate,
                    }
                }),
            }
        }
        _ => unreachable!("only asset instructions run here"),
    }
}

fn font_view<'m>(machine: &Machine<'m, '_>, handle: Handle) -> FontView<'m> {
    match handle.kind() {
        Ok(index) => machine.bound.show.font(index),
        Err(_) => unreachable!("show fonts resolve when bound"),
    }
}

fn image_view<'m>(machine: &Machine<'m, '_>, handle: Handle) -> ImageView<'m> {
    match handle.kind() {
        Ok(index) => machine.bound.show.image(index),
        Err(_) => unreachable!("show images resolve when bound"),
    }
}
