//! Strip workspaces: the slots, selections and buffers a strip interpreter
//! runs in, carved from the playback buffer for every program they run.
//! Carving runs once, so it stays out of instruction RAM.
use super::strip::{Handle, Levels, Pixels, Range, STRIP, Store};
use crate::arena::{Carve, Size};
use crate::format::{self, SectionPixel};
use crate::view::ProgramView;
use core::cell::{Cell, RefCell};
use donder_runtime_types::Color;
use donder_runtime_types::bytecode::Banks;

/// The fixture index of a position beyond the target, which no pixel's
/// neighbor has.
pub(crate) const OUTSIDE: i32 = i32::MIN;
/// A stencil's neighborhood: up to a strip of pixels and a strip of offsets.
pub(super) const NEIGHBORHOOD: usize = 2 * STRIP;

/// A run of positions whose fixture index less their position is `key`:
/// consecutive pixels of one fixture.
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(super) struct Segment {
    pub(super) start: i16,
    pub(super) end: i16,
    pub(super) key: i32,
}

/// The input colors, fixture indices and neighbor weights of a stencil's
/// neighborhood.
pub(super) struct Neighborhood<'w> {
    pub(super) colors: &'w mut [Color],
    pub(super) locals: &'w mut [i32],
    pub(super) weights: &'w mut [f32],
    /// The neighborhood's segments, and the selected pixels', keyed alike: a
    /// pixel's neighbor is in its fixture where their keys agree.
    pub(super) segments: &'w mut [Segment],
    pub(super) runs: &'w mut [Segment],
}

/// The slots of every program a workspace runs.
pub(crate) struct StripWorkspace<'w> {
    pub(super) pixels: Pixels<'w>,
    pub(super) floats: Store<'w, f32>,
    pub(super) ints: Store<'w, i32>,
    pub(super) bools: Store<'w, bool>,
    pub(super) handles: Store<'w, Handle>,
    pub(super) levels: Levels<'w>,
    /// Signal samples land here before they are written to the selection.
    pub(super) samples: RefCell<&'w mut [Color; STRIP]>,
    /// Held by workspaces whose programs have stencils.
    pub(super) neighborhood: Option<RefCell<Neighborhood<'w>>>,
}

/// The slots a strip workspace holds: the bankwise most of every program it
/// runs.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StripSlots {
    scalars: Banks,
    rows: Banks,
    depth: u16,
    stencils: bool,
}

impl StripSlots {
    pub(crate) fn include(&mut self, program: ProgramView<'_>) {
        let (scalars, rows, depth) = program.slots();
        self.scalars = self.scalars.max(scalars);
        self.rows = self.rows.max(rows);
        self.depth = self.depth.max(depth);
        self.stencils |= program.flag(format::PROGRAM_HAS_STENCIL);
    }

    fn banks(&self) -> [(usize, usize); 4] {
        let bank = |scalars: u16, rows: u16| (usize::from(scalars), usize::from(rows));
        [
            bank(self.scalars.floats, self.rows.floats),
            bank(self.scalars.ints, self.rows.ints),
            bank(self.scalars.bools, self.rows.bools),
            bank(self.scalars.resources, self.rows.resources),
        ]
    }

    /// Adds the regions [`Self::carve`] takes.
    pub(crate) fn size(&self, size: &mut Size) {
        size.row::<i32>();
        size.row::<f32>();
        size.row::<f32>();
        size.row::<f32>();
        size.row::<SectionPixel>();
        let [floats, ints, bools, handles] = self.banks();
        size.values::<f32>(Store::<f32>::len(floats.0, floats.1));
        size.values::<i32>(Store::<i32>::len(ints.0, ints.1));
        size.flags(Store::<bool>::len(bools.0, bools.1));
        size.values::<Handle>(Store::<Handle>::len(handles.0, handles.1));
        let depth = usize::from(self.depth);
        size.values::<Range>(2 * STRIP * depth);
        size.flags(STRIP * depth);
        size.row::<Color>();
        if self.stencils {
            size.values::<Color>(NEIGHBORHOOD);
            size.values::<i32>(NEIGHBORHOOD);
            size.values::<f32>(NEIGHBORHOOD);
            size.values::<Segment>(NEIGHBORHOOD);
            size.values::<Segment>(STRIP);
        }
    }

    pub(crate) fn carve<'w>(&self, arena: &mut Carve<'w>) -> Option<StripWorkspace<'w>> {
        let pixels = Pixels {
            index: arena.row(0)?,
            fraction: arena.row(0.0)?,
            x: arena.row(0.0)?,
            y: arena.row(0.0)?,
            sections: arena.row(SectionPixel::default())?,
        };
        let [floats, ints, bools, handles] = self.banks();
        let floats = Store::new(
            arena.cells(Store::<f32>::len(floats.0, floats.1), 0.0)?,
            floats,
        );
        let ints = Store::new(arena.cells(Store::<i32>::len(ints.0, ints.1), 0)?, ints);
        let bools = Store::new(arena.flags(Store::<bool>::len(bools.0, bools.1))?, bools);
        let handles = Store::new(
            arena.cells(Store::<Handle>::len(handles.0, handles.1), Handle::EMPTY)?,
            handles,
        );
        let depth = usize::from(self.depth);
        let levels = Levels::new(
            arena.cells(2 * STRIP * depth, Range::EMPTY)?,
            arena.flags(STRIP * depth)?,
        );
        let samples = <&mut [Color; STRIP]>::try_from(arena.values(STRIP, Color::BLACK)?).ok()?;
        let neighborhood = if self.stencils {
            Some(RefCell::new(Neighborhood {
                colors: arena.values(NEIGHBORHOOD, Color::BLACK)?,
                locals: arena.values(NEIGHBORHOOD, OUTSIDE)?,
                weights: arena.values(NEIGHBORHOOD, 0.0)?,
                segments: arena.values(NEIGHBORHOOD, Segment::default())?,
                runs: arena.values(STRIP, Segment::default())?,
            }))
        } else {
            None
        };
        Some(StripWorkspace {
            pixels,
            floats,
            ints,
            bools,
            handles,
            levels,
            samples: RefCell::new(samples),
            neighborhood,
        })
    }
}

impl<T: Copy> Store<'_, T> {
    /// Values a bank of `scalars` and `rows` holds. A bank with scalars but no
    /// rows keeps a row of padding, so every scalar has a row-sized window.
    pub(super) fn len(scalars: usize, rows: usize) -> usize {
        let rows = if scalars == 0 { rows } else { rows.max(1) };
        scalars + rows * STRIP
    }
}

impl<'w, T: Copy> Store<'w, T> {
    fn new(values: &'w [Cell<T>], (scalars, _): (usize, usize)) -> Self {
        Self { values, scalars }
    }
}
