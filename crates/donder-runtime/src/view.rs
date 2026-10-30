//! A show's bytes borrowed for playback: the records preparation wrote, read in
//! place. The show is trusted: only its producer writes it, so playback
//! checks nothing but its format version.
use crate::format::{self, NONE, Sections};
use donder_runtime_types::bytecode::Banks;
use donder_runtime_types::{InstructionWord, SampleDuration, SampleTime};

/// Current show format accepted by this runtime.
pub const FORMAT_VERSION: u32 = 75;

/// Why a show is not played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Not a show of this format: a show prepared by another Donder
    /// release, or bytes too short to hold its header.
    Version,
}

/// A show's playback bytes, borrowed in place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Show<'a> {
    pub(crate) s: Sections<'a>,
}

impl<'a> Show<'a> {
    /// Borrow a show of this format. `bytes` must be 4-byte aligned.
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self, LoadError> {
        format::open(bytes, FORMAT_VERSION)
            .map(|s| Self { s })
            .ok_or(LoadError::Version)
    }

    /// A show this build wrote.
    pub(crate) fn trusted(bytes: &'a [u8]) -> Self {
        match Self::open(bytes) {
            Ok(show) => show,
            Err(_) => unreachable!("the host builds shows of its format"),
        }
    }

    pub(crate) fn meta(&self) -> &'a format::Meta {
        &self.s.meta[0]
    }

    pub(crate) fn frame_rate(&self) -> u32 {
        self.meta().frame_rate
    }

    pub(crate) fn frame_count(&self) -> u32 {
        self.meta().frame_count
    }

    pub(crate) fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.meta().duration)
    }

    pub(crate) fn pixel_count(&self) -> usize {
        self.meta().pixel_count as usize
    }

    pub(crate) fn fixtures(&self) -> &'a [format::Fixture] {
        self.s.fixtures
    }

    pub(crate) fn outputs(&self) -> &'a [format::Output] {
        self.s.outputs
    }

    pub(crate) fn clip_count(&self) -> usize {
        self.s.clips.len()
    }

    /// Clips of the show's timeline active at `time`, for diagnostics.
    pub(crate) fn active_clip_count(&self, time: SampleTime) -> usize {
        let ticks = time.as_ticks();
        self.s
            .clips
            .iter()
            .filter(|clip| ticks >= clip.start && ticks - clip.start < clip.duration)
            .count()
    }

    pub(crate) fn program(&self, index: u32) -> ProgramView<'a> {
        let record = &self.s.programs[index as usize];
        ProgramView {
            record,
            code: record.code.get(self.s.code),
            resources: record.resources.get(self.s.program_resources),
        }
    }

    /// The node's input nodes.
    pub(crate) fn inputs(&self, node: &format::Node) -> &'a [u32] {
        node.inputs.get(self.s.node_inputs)
    }

    /// Whether the show is a console's: it has voices commands start.
    pub(crate) fn is_console(&self) -> bool {
        !self.s.voices.is_empty() || !self.s.live_automations.is_empty()
    }

    /// The root node, or `None` for a black show.
    pub(crate) fn root(&self) -> Option<&'a format::Node> {
        let root = self.meta().root;
        (root != NONE).then(|| &self.s.nodes[root as usize])
    }
}

/// A program of a show.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramView<'a> {
    pub(crate) record: &'a format::Program,
    pub(crate) code: &'a [InstructionWord],
    /// Constant resources, by index into their kinds' sections.
    pub(crate) resources: &'a [u32],
}

fn banks(counts: format::BankCounts) -> Banks {
    let [floats, ints, bools, resources] = counts.counts;
    Banks {
        floats,
        ints,
        bools,
        resources,
    }
}

impl ProgramView<'_> {
    pub(crate) fn flag(&self, flag: u32) -> bool {
        self.record.flags & flag != 0
    }

    pub(crate) fn scans(&self) -> usize {
        self.record.scans as usize
    }

    pub(crate) fn slots(&self) -> (Banks, Banks, u16) {
        (
            banks(self.record.scalars),
            banks(self.record.rows),
            self.record.depth as u16,
        )
    }
}
