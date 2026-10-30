//! Playback memory: one caller-provided buffer, split front to back into the
//! typed regions a show's workspace needs. Nothing is allocated. [`Size`]
//! counts the regions [`Carve`] hands out, in the same order, so a caller can
//! size the buffer first.
use core::cell::Cell;

/// Every region starts on this alignment.
const ALIGN: usize = 4;

pub(crate) type Row<T> = [Cell<T>; crate::dsl::STRIP];

/// Hands out regions of a buffer, front to back. Every region is filled.
pub(crate) struct Carve<'w> {
    rest: &'w mut [u8],
    /// Whether regions are filled; a re-borrowed buffer keeps its contents.
    fill: bool,
}

impl<'w> Carve<'w> {
    /// Regions of a fresh buffer, each filled with its value.
    pub(crate) fn new(buffer: &'w mut [u8]) -> Self {
        Self {
            rest: buffer,
            fill: true,
        }
    }

    /// The same regions of a buffer carved before, as they are.
    pub(crate) fn again(buffer: &'w mut [u8]) -> Self {
        Self {
            rest: buffer,
            fill: false,
        }
    }

    fn bytes(&mut self, len: usize) -> Option<&'w mut [u8]> {
        let rest = core::mem::take(&mut self.rest);
        let padding = rest.as_ptr().align_offset(ALIGN);
        if padding > rest.len() {
            return None;
        }
        let (_, rest) = rest.split_at_mut(padding);
        if rest.len() < len {
            return None;
        }
        let (taken, rest) = rest.split_at_mut(len);
        self.rest = rest;
        Some(taken)
    }

    pub(crate) fn values<T: bytemuck::Pod>(
        &mut self,
        count: usize,
        fill: T,
    ) -> Option<&'w mut [T]> {
        let bytes = self.bytes(count.checked_mul(size_of::<T>())?)?;
        let values: &mut [T] = bytemuck::try_cast_slice_mut(bytes).ok()?;
        if self.fill {
            values.fill(fill);
        }
        Some(values)
    }

    pub(crate) fn cells<T: bytemuck::Pod>(
        &mut self,
        count: usize,
        fill: T,
    ) -> Option<&'w [Cell<T>]> {
        Some(Cell::from_mut(self.values(count, fill)?).as_slice_of_cells())
    }

    pub(crate) fn row<T: bytemuck::Pod>(&mut self, fill: T) -> Option<&'w Row<T>> {
        <&Row<T>>::try_from(self.cells(crate::dsl::STRIP, fill)?).ok()
    }

    /// `count` cleared flags.
    pub(crate) fn flags(&mut self, count: usize) -> Option<&'w [Cell<bool>]> {
        let bytes = self.bytes(count)?;
        if self.fill {
            bytes.fill(0);
        }
        let flags: &mut [bool] = bytemuck::checked::try_cast_slice_mut(bytes).ok()?;
        Some(Cell::from_mut(flags).as_slice_of_cells())
    }
}

/// Counts the bytes of [`Carve`]'s regions in a 4-byte-aligned buffer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Size {
    pub(crate) bytes: usize,
}

impl Size {
    fn add(&mut self, bytes: usize) {
        self.bytes = self.bytes.div_ceil(ALIGN) * ALIGN + bytes;
    }

    pub(crate) fn values<T>(&mut self, count: usize) {
        self.add(count * size_of::<T>());
    }

    pub(crate) fn row<T>(&mut self) {
        self.values::<T>(crate::dsl::STRIP);
    }

    pub(crate) fn flags(&mut self, count: usize) {
        self.add(count);
    }
}
