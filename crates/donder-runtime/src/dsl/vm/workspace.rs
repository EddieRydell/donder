//! Strip workspaces: the slots, selections and buffers a strip interpreter
//! runs in, sized for every program it runs before playback. Sizing and
//! allocation run once, so they stay out of instruction RAM.
use super::strip::{Handle, Level, NO_COLOR, Pixels, STRIP, Store};
use crate::dsl::bytecode::{Banks, BytecodeProgram, Instruction};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use donder_runtime_types::Color;

/// The fixture index of a position beyond the target, which no pixel's
/// neighbor has.
pub(crate) const OUTSIDE: i32 = i32::MIN;

/// A stencil's neighborhood: up to a strip of pixels and a strip of offsets.
pub(super) const NEIGHBORHOOD: usize = 2 * STRIP;

/// The input colors, fixture indices and source weights of a stencil's
/// neighborhood.
pub(super) struct Neighborhood {
    pub(super) colors: Box<[Color]>,
    pub(super) locals: Box<[i32]>,
    pub(super) weights: Box<[f32]>,
}

// Inlined into the code below: per-frame code alone belongs in instruction RAM.
impl<T: Copy> Store<T> {
    #[inline(always)]
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            scalars: 0,
            rows: 0,
        }
    }

    /// Fit `scalars` scalars and `rows` rows, holding exactly the slots.
    #[inline(always)]
    pub(super) fn reserve(&mut self, scalars: u16, rows: u16, fill: T) {
        self.scalars = self.scalars.max(usize::from(scalars));
        self.rows = self.rows.max(usize::from(rows));
        let len = Self::len(self.scalars, self.rows);
        if self.values.len() < len {
            self.values.reserve_exact(len - self.values.len());
            self.values.resize_with(len, || Cell::new(fill));
        }
    }

    #[inline(always)]
    pub(super) fn len(scalars: usize, rows: usize) -> usize {
        let rows = if scalars == 0 { rows } else { rows.max(1) };
        scalars + rows * STRIP
    }

    #[inline(always)]
    pub(super) fn bytes(scalars: u16, rows: u16) -> Option<usize> {
        Self::len(usize::from(scalars), usize::from(rows)).checked_mul(size_of::<T>())
    }
}

/// Slots of every program a workspace runs, reserved before playback.
pub(crate) struct StripWorkspace {
    pub(super) pixels: Pixels,
    pub(super) floats: Store<f32>,
    pub(super) ints: Store<i32>,
    pub(super) bools: Store<bool>,
    pub(super) colors: Store<Color>,
    pub(super) handles: Store<Handle>,
    pub(super) levels: Vec<Level>,
    /// Signal samples land here before they are written to the selection.
    pub(super) samples: RefCell<[Color; STRIP]>,
    /// Held by workspaces whose programs have stencils.
    pub(super) neighborhood: Option<Box<RefCell<Neighborhood>>>,
}

impl Default for StripWorkspace {
    fn default() -> Self {
        Self {
            pixels: Pixels::default(),
            floats: Store::new(),
            ints: Store::new(),
            bools: Store::new(),
            colors: Store::new(),
            handles: Store::new(),
            levels: Vec::new(),
            samples: RefCell::new([NO_COLOR; STRIP]),
            neighborhood: None,
        }
    }
}

impl core::fmt::Debug for StripWorkspace {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("StripWorkspace")
            .field("floats", &self.floats.values.len())
            .field("ints", &self.ints.values.len())
            .field("bools", &self.bools.values.len())
            .field("colors", &self.colors.values.len())
            .field("handles", &self.handles.values.len())
            .finish_non_exhaustive()
    }
}

impl StripWorkspace {
    /// Grow every bank to fit `program`.
    pub(crate) fn reserve(&mut self, program: &BytecodeProgram) {
        self.reserve_slots(&StripSlots::of(program));
    }

    /// Grow every bank to fit `slots`, holding exactly the slots.
    pub(crate) fn reserve_slots(&mut self, slots: &StripSlots) {
        let StripSlots {
            scalars,
            rows,
            depth,
            stencils,
        } = *slots;
        if stencils && self.neighborhood.is_none() {
            self.neighborhood = Some(Box::new(RefCell::new(Neighborhood {
                colors: vec![NO_COLOR; NEIGHBORHOOD].into(),
                locals: vec![OUTSIDE; NEIGHBORHOOD].into(),
                weights: vec![0.0; NEIGHBORHOOD].into(),
            })));
        }
        self.floats.reserve(scalars.floats, rows.floats, 0.0);
        self.ints.reserve(scalars.ints, rows.ints, 0);
        self.bools.reserve(scalars.bools, rows.bools, false);
        self.colors.reserve(scalars.colors, rows.colors, NO_COLOR);
        self.handles
            .reserve(scalars.resources, rows.resources, Handle::Empty);
        let depth = usize::from(depth);
        if self.levels.len() < depth {
            self.levels.reserve_exact(depth - self.levels.len());
            self.levels.resize_with(depth, Level::default);
        }
    }
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
    pub(crate) fn of(program: &BytecodeProgram) -> Self {
        Self {
            scalars: program.scalars,
            rows: program.rows,
            depth: program.depth,
            stencils: has_stencil(program),
        }
    }

    pub(crate) fn include(&mut self, program: &BytecodeProgram) {
        self.scalars = self.scalars.max(program.scalars);
        self.rows = self.rows.max(program.rows);
        self.depth = self.depth.max(program.depth);
        self.stencils |= has_stencil(program);
    }

    /// Bytes a workspace holding these slots reserves.
    pub(crate) fn storage_estimate(&self) -> Option<usize> {
        let Self {
            scalars,
            rows,
            depth,
            stencils,
        } = *self;
        let mut bytes = size_of::<StripWorkspace>()
            .checked_add(usize::from(depth).checked_mul(size_of::<Level>())?)?
            .checked_add(if stencils {
                size_of::<RefCell<Neighborhood>>()
                    + NEIGHBORHOOD * (size_of::<Color>() + size_of::<i32>() + size_of::<f32>())
            } else {
                0
            })?;
        for bytes_of_bank in [
            Store::<f32>::bytes(scalars.floats, rows.floats)?,
            Store::<i32>::bytes(scalars.ints, rows.ints)?,
            Store::<bool>::bytes(scalars.bools, rows.bools)?,
            Store::<Color>::bytes(scalars.colors, rows.colors)?,
            Store::<Handle>::bytes(scalars.resources, rows.resources)?,
        ] {
            bytes = bytes.checked_add(bytes_of_bank)?;
        }
        Some(bytes)
    }
}

fn has_stencil(program: &BytecodeProgram) -> bool {
    program
        .body()
        .iter()
        .any(|instruction| matches!(instruction, Instruction::Stencil { .. }))
}
