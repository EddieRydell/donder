//! Listening to music as it is played. A [`Listener`] hears notes and the
//! sustain pedal as they arrive and reports what a listener would notice: the
//! chord and its quality, the key and when it changes, a resolution home, the
//! beat at the player's tempo, fast runs, big chords and silence, and running
//! measures of the key, tension, loudness, density, register and tempo.
//!
//! Everything is plain arithmetic on what was played, with no model to load:
//! chords by matching chord templates against the sounding pitch classes, the
//! key by correlating a decaying pitch-class profile with Krumhansl-Kessler
//! key profiles, and the tempo from the intervals between onsets.

mod chord;
mod key;
mod rhythm;
mod tune;

use std::collections::VecDeque;

pub use chord::{Chord, ChordQuality};
pub use key::{Key, Mode};
pub use tune::{MAX_TUNE_NOTES, MIN_TUNE_NOTES, Melody};

use chord::recognise;
use key::{KeyChange, KeyFinder};
use rhythm::BeatTracker;
use tune::MelodyLine;

/// A musical moment a listener notices, which plays as a button.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Heard {
    /// The chord changed. Momentary.
    ChordChange,
    /// The key changed. Momentary.
    KeyChange,
    /// A dominant chord resolved to the chord a fifth below: V to I.
    /// Momentary.
    Resolution,
    /// A beat at the player's tempo. Momentary.
    Beat,
    /// A fast run up the keyboard. Momentary.
    RunUp,
    /// A fast run down the keyboard. Momentary.
    RunDown,
    /// Five or more notes struck hard together. Momentary.
    BigChord,
    /// Held while the chord is of this quality.
    Chord(ChordQuality),
    /// Held while the key is in this mode.
    Mode(Mode),
    /// Held once nothing has been played for this many seconds.
    Idle { seconds: u16 },
    /// The tune was played: its melody's steps, in any key. Momentary.
    Tune(Melody),
}

impl Heard {
    /// Whether it is pressed and released at once, as an event, rather than
    /// held while something is so.
    pub fn is_momentary(self) -> bool {
        !matches!(self, Self::Chord(_) | Self::Mode(_) | Self::Idle { .. })
    }
}

/// A running measure of the music, which plays as a dial from 0 to 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Measure {
    /// The key's tonic round the circle of fifths: C at 0, G at 1/12, and on
    /// to F at 11/12.
    KeyColor,
    /// The chord's root round the circle of fifths.
    ChordColor,
    /// How far the harmony is from rest: low on the home chord, high on
    /// dominants, diminished chords and notes outside the key.
    Tension,
    /// How hard the music is being played, falling away in silence.
    Loudness,
    /// How many notes are being played, from none to twelve a second.
    Density,
    /// How high the sounding notes are, from the bottom of a piano to its top.
    Register,
    /// The tempo, from 60 beats a minute at 0 to 200 at 1.
    Tempo,
}

/// What the listener hears.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Played {
    /// A note struck with a velocity in `0..=1`.
    NoteOn {
        note: u8,
        velocity: f32,
    },
    NoteOff {
        note: u8,
    },
    /// The sustain pedal went down or up.
    Sustain(bool),
}

/// What the listener reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Noticed {
    Pressed(Heard),
    Released(Heard),
    Measured(Measure, f32),
}

/// How often at most a changing measure is reported, in seconds.
const MEASURE_INTERVAL: f64 = 0.04;
/// The smallest change of a measure worth reporting.
const MEASURE_STEP: f32 = 1.0 / 128.0;
/// How long loudness takes to fall to about a third, in seconds.
const LOUDNESS_DECAY: f64 = 0.8;
/// The window density counts notes over, in seconds.
const DENSITY_WINDOW: f64 = 2.0;
/// The notes a second that measure as full density.
const FULL_DENSITY: f64 = 12.0;
/// The lowest and highest notes of a piano, for register.
const LOWEST_NOTE: f32 = 21.0;
const HIGHEST_NOTE: f32 = 108.0;
/// Notes struck this close together count as one chord.
const CHORD_SPREAD: f64 = 0.05;
/// The chord is named once no note has been struck for this long, so a
/// chord's notes all land before it is heard.
const SETTLE: f64 = 0.04;
/// Notes from this one up are mostly melody, and count less towards the
/// chord.
const MELODY: usize = 72;
const MELODY_WEIGHT: f32 = 0.4;
/// A big chord: this many notes within `CHORD_SPREAD`, this hard on average.
const BIG_CHORD_NOTES: usize = 5;
const BIG_CHORD_VELOCITY: f32 = 0.85;
/// A run: notes this close in time, stepping at most this far, for at least
/// this many notes spanning at least this many semitones.
const RUN_GAP: f64 = 0.14;
const RUN_STEP: u8 = 4;
const RUN_NOTES: usize = 6;
const RUN_SPAN: u8 = 7;

#[derive(Clone, Copy, Debug)]
struct Sounding {
    velocity: f32,
    onset: f64,
    /// The key is down; otherwise only the pedal holds it.
    held: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Reported {
    value: Option<f32>,
    at: f64,
}

#[derive(Clone, Debug, Default)]
struct Run {
    last: Option<(u8, f64)>,
    rising: Option<bool>,
    notes: usize,
    low: u8,
    high: u8,
    noticed: bool,
}

/// Listens to one performer. It reports only the moments and measures it
/// was asked for.
#[derive(Clone, Debug)]
pub struct Listener {
    heard: Vec<Heard>,
    measures: Vec<Measure>,
    sounding: [Option<Sounding>; 128],
    sustain: bool,
    /// Recent onsets: when, which note, how hard.
    onsets: VecDeque<(f64, u8, f32)>,
    last_onset: Option<f64>,
    idle: Vec<(u16, bool)>,
    chord: Option<Chord>,
    /// Notes were struck since the chord was last named; lifting notes never
    /// changes the chord.
    struck: bool,
    keys: KeyFinder,
    beats: BeatTracker,
    loudness: f32,
    loudness_at: f64,
    run: Run,
    big_chord_at: f64,
    line: MelodyLine,
    /// Each tune listened for, and when in the melody line it was last heard.
    tunes: Vec<(Melody, Option<u64>)>,
    reported: Vec<Reported>,
}

impl Listener {
    pub fn new(heard: &[Heard], measures: &[Measure]) -> Self {
        Self {
            idle: heard
                .iter()
                .filter_map(|heard| match heard {
                    Heard::Idle { seconds } => Some((*seconds, false)),
                    _ => None,
                })
                .collect(),
            heard: heard.to_vec(),
            measures: measures.to_vec(),
            sounding: [None; 128],
            sustain: false,
            onsets: VecDeque::new(),
            last_onset: None,
            chord: None,
            struck: false,
            keys: KeyFinder::default(),
            beats: BeatTracker::default(),
            loudness: 0.0,
            loudness_at: 0.0,
            run: Run::default(),
            big_chord_at: f64::NEG_INFINITY,
            line: MelodyLine::default(),
            tunes: heard
                .iter()
                .filter_map(|heard| match heard {
                    Heard::Tune(melody) => Some((*melody, None)),
                    _ => None,
                })
                .collect(),
            reported: vec![Reported::default(); measures.len()],
        }
    }

    /// What `played`, at `time` seconds into the performance, makes the
    /// listener notice.
    pub fn hear(&mut self, played: Played, time: f64) -> Vec<Noticed> {
        let mut noticed = Vec::new();
        match played {
            Played::NoteOn { note, velocity } => {
                let Some(slot) = self.sounding.get_mut(usize::from(note)) else {
                    return noticed;
                };
                *slot = Some(Sounding {
                    velocity,
                    onset: time,
                    held: true,
                });
                self.onsets.push_back((time, note, velocity));
                self.last_onset = Some(time);
                self.struck = true;
                for (seconds, idle) in &mut self.idle {
                    if std::mem::take(idle) {
                        noticed.push(Noticed::Released(Heard::Idle { seconds: *seconds }));
                    }
                }
                self.decay_loudness(time);
                self.loudness = self.loudness.max(velocity);
                self.keys.hear(note, velocity, time);
                self.beats.hear(time, velocity);
                self.notice_run(note, time, &mut noticed);
                self.notice_tunes(note, time, &mut noticed);
                self.notice_big_chord(time, &mut noticed);
            }
            Played::NoteOff { note } => {
                if let Some(slot) = self.sounding.get_mut(usize::from(note)) {
                    if self.sustain {
                        if let Some(sounding) = slot {
                            sounding.held = false;
                        }
                    } else {
                        *slot = None;
                    }
                }
            }
            Played::Sustain(down) => {
                self.sustain = down;
                if !down {
                    for slot in &mut self.sounding {
                        if slot.is_some_and(|sounding| !sounding.held) {
                            *slot = None;
                        }
                    }
                }
            }
        }
        self.notice_harmony(time, &mut noticed);
        self.measure(time, &mut noticed);
        noticed
    }

    /// What the passing of time makes the listener notice: beats, silence,
    /// and measures that fall away. Call it every few milliseconds.
    pub fn tick(&mut self, time: f64) -> Vec<Noticed> {
        let mut noticed = Vec::new();
        if self.wants(Heard::Beat) && self.beats.due(time) {
            noticed.push(Noticed::Pressed(Heard::Beat));
            noticed.push(Noticed::Released(Heard::Beat));
        }
        let since = self.last_onset.unwrap_or(0.0);
        for (seconds, idle) in &mut self.idle {
            if !*idle && time - since >= f64::from(*seconds) {
                *idle = true;
                noticed.push(Noticed::Pressed(Heard::Idle { seconds: *seconds }));
            }
        }
        self.keys.settle(time);
        self.notice_harmony(time, &mut noticed);
        self.measure(time, &mut noticed);
        noticed
    }

    /// The chord the listener hears now.
    pub fn chord(&self) -> Option<Chord> {
        self.chord
    }

    /// The key the listener hears now.
    pub fn key(&self) -> Option<Key> {
        self.keys.key()
    }

    fn wants(&self, heard: Heard) -> bool {
        self.heard.contains(&heard)
    }

    fn decay_loudness(&mut self, time: f64) {
        let elapsed = (time - self.loudness_at).max(0.0);
        self.loudness *= (-elapsed / LOUDNESS_DECAY).exp() as f32;
        self.loudness_at = time;
    }

    fn notice_run(&mut self, note: u8, time: f64, noticed: &mut Vec<Noticed>) {
        let run = &mut self.run;
        // The note before, when this one steps on from it quickly.
        let stepped = run.last.filter(|(last, at)| {
            time - at <= RUN_GAP && (1..=RUN_STEP).contains(&note.abs_diff(*last))
        });
        match stepped {
            Some((last, _)) if run.rising.is_none_or(|rising| rising == (note > last)) => {
                run.rising = Some(note > last);
                run.notes += 1;
                run.low = run.low.min(note);
                run.high = run.high.max(note);
            }
            // A run that turns round starts again from the note it turned at.
            Some((last, _)) => {
                *run = Run {
                    rising: Some(note > last),
                    low: last.min(note),
                    high: last.max(note),
                    notes: 2,
                    ..Run::default()
                };
            }
            None => {
                *run = Run {
                    low: note,
                    high: note,
                    notes: 1,
                    ..Run::default()
                };
            }
        }
        run.last = Some((note, time));
        if !run.noticed && run.notes >= RUN_NOTES && run.high - run.low >= RUN_SPAN {
            run.noticed = true;
            let heard = if run.rising == Some(true) {
                Heard::RunUp
            } else {
                Heard::RunDown
            };
            if self.heard.contains(&heard) {
                noticed.push(Noticed::Pressed(heard));
                noticed.push(Noticed::Released(heard));
            }
        }
    }

    fn notice_tunes(&mut self, note: u8, time: f64, noticed: &mut Vec<Noticed>) {
        if !self.line.hear(note, time) {
            return;
        }
        let added = self.line.added();
        for (melody, heard_at) in &mut self.tunes {
            // A tune is heard once each time it is played through.
            let again = heard_at.is_none_or(|at| added >= at + melody.notes().len() as u64 / 2);
            if again && self.line.ends_with(melody) {
                *heard_at = Some(added);
                noticed.push(Noticed::Pressed(Heard::Tune(*melody)));
                noticed.push(Noticed::Released(Heard::Tune(*melody)));
            }
        }
    }

    fn notice_big_chord(&mut self, time: f64, noticed: &mut Vec<Noticed>) {
        // Each note once, however often it was struck again.
        let mut struck: Vec<(u8, f32)> = Vec::new();
        for &(_, note, velocity) in self
            .onsets
            .iter()
            .rev()
            .take_while(|(at, _, _)| time - at <= CHORD_SPREAD)
        {
            match struck.iter_mut().find(|(other, _)| *other == note) {
                Some((_, hardest)) => *hardest = hardest.max(velocity),
                None => struck.push((note, velocity)),
            }
        }
        let struck = struck
            .into_iter()
            .map(|(_, velocity)| velocity)
            .collect::<Vec<_>>();
        let hard = struck.iter().sum::<f32>() / struck.len().max(1) as f32;
        if struck.len() >= BIG_CHORD_NOTES
            && hard >= BIG_CHORD_VELOCITY
            && time - self.big_chord_at > CHORD_SPREAD * 4.0
        {
            self.big_chord_at = time;
            if self.wants(Heard::BigChord) {
                noticed.push(Noticed::Pressed(Heard::BigChord));
                noticed.push(Noticed::Released(Heard::BigChord));
            }
        }
    }

    /// The sounding notes, weighted by how hard and how recently they were
    /// struck, with notes only the pedal holds counting less.
    fn sounding_weights(&self, time: f64) -> ([f32; 12], Option<u8>) {
        let mut weights = [0.0; 12];
        let mut bass = None;
        for (note, sounding) in self.sounding.iter().enumerate() {
            let Some(sounding) = sounding else {
                continue;
            };
            let age = (time - sounding.onset).max(0.0);
            let held = if sounding.held { 1.0 } else { 0.6 };
            let register = if note >= MELODY { MELODY_WEIGHT } else { 1.0 };
            let weight =
                (0.3 + 0.7 * sounding.velocity) * held * register * (-age / 4.0).exp() as f32;
            weights[note % 12] += weight;
            bass.get_or_insert(note as u8);
        }
        (weights, bass)
    }

    fn notice_harmony(&mut self, time: f64, noticed: &mut Vec<Noticed>) {
        let (weights, bass) = self.sounding_weights(time);
        if let Some(KeyChange {
            from,
            to,
            established,
        }) = self.keys.take_change()
        {
            // Finding the first key is not a change of key.
            if established {
                self.moment(Heard::KeyChange, noticed);
            }
            for mode in [Mode::Major, Mode::Minor] {
                let heard = Heard::Mode(mode);
                if !self.wants(heard) {
                    continue;
                }
                let was = from.is_some_and(|key| key.mode == mode);
                let is = to.mode == mode;
                if was && !is {
                    noticed.push(Noticed::Released(heard));
                } else if is && !was {
                    noticed.push(Noticed::Pressed(heard));
                }
            }
        }
        if !self.struck || self.last_onset.is_some_and(|onset| time - onset < SETTLE) {
            return;
        }
        self.struck = false;
        let Some(chord) = recognise(&weights, bass, self.chord) else {
            return;
        };
        if Some(chord) == self.chord {
            return;
        }
        let previous = self.chord.replace(chord);
        self.moment(Heard::ChordChange, noticed);
        if previous.is_some_and(|previous| chord.resolves(previous, self.keys.settled(time))) {
            self.moment(Heard::Resolution, noticed);
        }
        let was = previous.map(|previous| previous.quality);
        if was != Some(chord.quality) {
            if let Some(was) = was
                && self.wants(Heard::Chord(was))
            {
                noticed.push(Noticed::Released(Heard::Chord(was)));
            }
            if self.wants(Heard::Chord(chord.quality)) {
                noticed.push(Noticed::Pressed(Heard::Chord(chord.quality)));
            }
        }
    }

    fn moment(&self, heard: Heard, noticed: &mut Vec<Noticed>) {
        if self.wants(heard) {
            noticed.push(Noticed::Pressed(heard));
            noticed.push(Noticed::Released(heard));
        }
    }

    fn measure(&mut self, time: f64, noticed: &mut Vec<Noticed>) {
        while self
            .onsets
            .front()
            .is_some_and(|(at, _, _)| time - at > rhythm::WINDOW.max(DENSITY_WINDOW))
        {
            self.onsets.pop_front();
        }
        self.decay_loudness(time);
        for index in 0..self.measures.len() {
            let measure = self.measures[index];
            let Some(value) = self.value(measure, time) else {
                continue;
            };
            let reported = &mut self.reported[index];
            let discrete = matches!(measure, Measure::KeyColor | Measure::ChordColor);
            let changed = reported
                .value
                .is_none_or(|last| (last - value).abs() >= MEASURE_STEP);
            if changed && (discrete || time - reported.at >= MEASURE_INTERVAL) {
                *reported = Reported {
                    value: Some(value),
                    at: time,
                };
                noticed.push(Noticed::Measured(measure, value));
            }
        }
    }

    fn value(&self, measure: Measure, time: f64) -> Option<f32> {
        Some(match measure {
            Measure::KeyColor => self.keys.key()?.tonic.fifths(),
            Measure::ChordColor => self.chord?.root.fifths(),
            Measure::Tension => self.tension(time),
            Measure::Loudness => self.loudness,
            Measure::Density => {
                let recent = self
                    .onsets
                    .iter()
                    .filter(|(at, _, _)| time - at <= DENSITY_WINDOW)
                    .count();
                (recent as f64 / DENSITY_WINDOW / FULL_DENSITY).min(1.0) as f32
            }
            Measure::Register => {
                let (sum, count) = self
                    .sounding
                    .iter()
                    .enumerate()
                    .filter(|(_, sounding)| sounding.is_some())
                    .fold((0.0, 0), |(sum, count), (note, _)| {
                        (sum + note as f32, count + 1)
                    });
                if count == 0 {
                    return None;
                }
                ((sum / count as f32 - LOWEST_NOTE) / (HIGHEST_NOTE - LOWEST_NOTE)).clamp(0.0, 1.0)
            }
            Measure::Tempo => {
                let bpm = 60.0 / self.beats.period()?;
                ((bpm - 60.0) / 140.0).clamp(0.0, 1.0) as f32
            }
        })
    }

    /// How far the harmony is from rest.
    fn tension(&self, time: f64) -> f32 {
        let (weights, _) = self.sounding_weights(time);
        let total = weights.iter().sum::<f32>();
        if total <= f32::EPSILON {
            return 0.0;
        }
        let chord = self.chord.map_or(0.2, |chord| chord.quality.tension());
        let key = self.keys.key();
        let outside = key.map_or(0.0, |key| {
            weights
                .iter()
                .enumerate()
                .filter(|(class, _)| !key.contains(PitchClass(*class as u8)))
                .map(|(_, weight)| *weight)
                .sum::<f32>()
                / total
        });
        // Neighbouring semitones both sounding clash.
        let present = |class: usize| weights[class % 12] > total * 0.08;
        let clash = (0..12).any(|class| present(class) && present(class + 1));
        let home = match (self.chord, key) {
            (Some(chord), Some(key)) if chord.root == key.tonic => -0.1,
            _ => 0.0,
        };
        (chord + 0.4 * outside + if clash { 0.15 } else { 0.0 } + home).clamp(0.0, 1.0)
    }
}

/// A note's name, C at 0 up the chromatic scale to B at 11.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PitchClass(pub u8);

impl PitchClass {
    /// Its place round the circle of fifths: C at 0, G at 1/12, D at 2/12.
    pub fn fifths(self) -> f32 {
        f32::from(self.0 * 7 % 12) / 12.0
    }

    fn above(self, semitones: u8) -> Self {
        Self((self.0 + semitones) % 12)
    }
}
