//! MIDI songs for performances: a MIDI file played as a pianist would play
//! it, its messages handed on at their times as a port's are, and heard on a
//! SoundFont piano. Every channel but the drums folds onto the one piano, and
//! a key struck again while another part holds it down is struck once.

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
use kira::info::Info;
use kira::sound::{Sound, SoundData};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Frame};
use midly::{MetaMessage, MidiMessage as FileMessage, Smf, Timing, TrackEventKind};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

use crate::midi::MidiMessage;

/// The General MIDI drum channel, 10 counted from 1.
const DRUM_CHANNEL: u8 = 9;
/// The control changes a song plays: the sustain and soft pedals.
const SONG_PEDALS: [u8; 2] = [64, 67];
/// How long the player sleeps at most before looking whether it was stopped.
const STOP_CHECK: Duration = Duration::from_millis(10);

/// A MIDI file's notes and pedals, in seconds from its first note.
pub(crate) struct MidiSong {
    pub(crate) name: String,
    events: Vec<(f64, MidiMessage)>,
    pub(crate) length_seconds: f64,
}

impl MidiSong {
    pub(crate) fn load(path: &str) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|error| format!("Could not read {path}: {error}"))?;
        let smf =
            Smf::parse(&bytes).map_err(|error| format!("{path} is not a MIDI file: {error}"))?;
        if smf.header.format == midly::Format::Sequential {
            return Err(format!(
                "{path} plays its tracks one after another; only songs whose tracks play together can be performed."
            ));
        }
        let name = std::path::Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(path)
            .to_string();
        // Each event's tick, with the tempo changes that time them.
        let mut ticks: Vec<(u64, TickEvent)> = Vec::new();
        for track in &smf.tracks {
            let mut tick = 0u64;
            for event in track {
                tick += u64::from(event.delta.as_int());
                match event.kind {
                    TrackEventKind::Meta(MetaMessage::Tempo(tempo)) => {
                        ticks.push((tick, TickEvent::Tempo(tempo.as_int())));
                    }
                    TrackEventKind::Midi { channel, message }
                        if channel.as_int() != DRUM_CHANNEL =>
                    {
                        let message = match message {
                            FileMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                                MidiMessage::NoteOn {
                                    note: key.as_int(),
                                    velocity: vel.as_int(),
                                }
                            }
                            FileMessage::NoteOn { key, .. } | FileMessage::NoteOff { key, .. } => {
                                MidiMessage::NoteOff { note: key.as_int() }
                            }
                            FileMessage::Controller { controller, value }
                                if SONG_PEDALS.contains(&controller.as_int()) =>
                            {
                                MidiMessage::ControlChange {
                                    number: controller.as_int(),
                                    value: value.as_int(),
                                }
                            }
                            _ => continue,
                        };
                        ticks.push((tick, TickEvent::Message(message)));
                    }
                    _ => {}
                }
            }
        }
        // At one tick, tempo first; each track keeps its own order, so a
        // note that starts and ends at once still ends.
        ticks.sort_by_key(|(tick, event)| (*tick, !matches!(event, TickEvent::Tempo(_))));
        let mut seconds_per_tick = match smf.header.timing {
            // 120 beats a minute until the song sets its tempo.
            Timing::Metrical(per_beat) => 0.5 / f64::from(per_beat.as_int()),
            Timing::Timecode(fps, subframes) => {
                1.0 / (f64::from(fps.as_f32()) * f64::from(subframes))
            }
        };
        let per_beat = match smf.header.timing {
            Timing::Metrical(per_beat) => Some(f64::from(per_beat.as_int())),
            Timing::Timecode(..) => None,
        };
        let (mut seconds, mut last) = (0.0, 0u64);
        // How many parts hold each key down: the piano plays the first
        // press and the last release.
        let mut down = [0u32; 128];
        let mut events = Vec::new();
        for (tick, event) in ticks {
            seconds += (tick - last) as f64 * seconds_per_tick;
            last = tick;
            let message = match event {
                TickEvent::Tempo(micros) => {
                    if let Some(per_beat) = per_beat {
                        seconds_per_tick = f64::from(micros) / 1_000_000.0 / per_beat;
                    }
                    continue;
                }
                TickEvent::Message(message) => message,
            };
            match message {
                MidiMessage::NoteOn { note, .. } => {
                    down[usize::from(note)] = down[usize::from(note)].saturating_add(1);
                    if down[usize::from(note)] > 1 {
                        continue;
                    }
                }
                MidiMessage::NoteOff { note } => {
                    let held = &mut down[usize::from(note)];
                    if *held == 0 {
                        continue;
                    }
                    *held -= 1;
                    if *held > 0 {
                        continue;
                    }
                }
                MidiMessage::ControlChange { .. } | MidiMessage::PitchBend { .. } => {}
            }
            events.push((seconds, message));
        }
        let first = events
            .iter()
            .find(|(_, message)| matches!(message, MidiMessage::NoteOn { .. }))
            .ok_or_else(|| format!("{path} has no notes to play."))?
            .0;
        for (time, _) in &mut events {
            *time = (*time - first).max(0.0);
        }
        let length_seconds = events.last().map_or(0.0, |(time, _)| *time);
        Ok(Self {
            name,
            events,
            length_seconds,
        })
    }
}

enum TickEvent {
    /// Microseconds a beat.
    Tempo(u32),
    Message(MidiMessage),
}

/// A SoundFont loaded to play songs on.
pub(crate) fn load_sound_font(path: &str) -> Result<Arc<SoundFont>, String> {
    let file = File::open(path)
        .map_err(|error| format!("Could not open the SoundFont {path}: {error}"))?;
    SoundFont::new(&mut BufReader::new(file))
        .map(Arc::new)
        .map_err(|error| format!("{path} is not a SoundFont: {error}"))
}

/// A song playing. Stopping or dropping it stops the song and lifts every key
/// and pedal it holds.
pub(crate) struct SongPlayer {
    pub(crate) name: String,
    pub(crate) length_seconds: f64,
    started: Instant,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    playing: Option<std::thread::JoinHandle<()>>,
    /// Keeps the song's sound playing.
    _audio: AudioManager<DefaultBackend>,
}

impl SongPlayer {
    /// Play `song` from now, calling `on_message` with each message at its
    /// time, and sounding it on `sound_font`.
    pub(crate) fn start(
        song: MidiSong,
        sound_font: &Arc<SoundFont>,
        on_message: impl Fn(MidiMessage) + Send + 'static,
    ) -> Result<Self, String> {
        let (sound, messages) = channel();
        let synthesizer =
            Synthesizer::new(sound_font, &SynthesizerSettings::new(output_sample_rate()?))
                .map_err(|error| format!("The SoundFont cannot play songs: {error}"))?;
        let mut audio = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|error| format!("Could not open the speakers for the song: {error}"))?;
        audio
            .play(SongSound {
                synthesizer,
                messages,
            })
            .map_err(|error| format!("Could not play the song's sound: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        Ok(Self {
            name: song.name.clone(),
            length_seconds: song.length_seconds,
            started,
            stop: stop.clone(),
            finished: finished.clone(),
            playing: Some(std::thread::spawn(move || {
                play(&song, started, &stop, &on_message, &sound);
                finished.store(true, Ordering::Relaxed);
            })),
            _audio: audio,
        })
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        self.started
            .elapsed()
            .as_secs_f64()
            .min(self.length_seconds)
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// Stop the song and wait until every key and pedal it held is lifted,
    /// so none of it lands after a song that follows. Its messages take the
    /// performance's lock, so never call this holding it.
    pub(crate) fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(playing) = self.playing.take() {
            let _ = playing.join();
        }
    }
}

impl Drop for SongPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn play(
    song: &MidiSong,
    started: Instant,
    stop: &AtomicBool,
    on_message: &dyn Fn(MidiMessage),
    sound: &Sender<MidiMessage>,
) {
    let send = |message: MidiMessage| {
        on_message(message);
        // The sound has gone only when the player has, so nothing is lost.
        let _ = sound.send(message);
    };
    let mut down = [false; 128];
    let mut pedals = [false; SONG_PEDALS.len()];
    'events: for &(time, message) in &song.events {
        let due = started + Duration::from_secs_f64(time);
        loop {
            if stop.load(Ordering::Relaxed) {
                break 'events;
            }
            let left = due.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(STOP_CHECK));
        }
        match message {
            MidiMessage::NoteOn { note, .. } => down[usize::from(note)] = true,
            MidiMessage::NoteOff { note } => down[usize::from(note)] = false,
            MidiMessage::ControlChange { number, value } => {
                if let Some(pedal) = SONG_PEDALS.iter().position(|pedal| *pedal == number) {
                    pedals[pedal] = value >= 64;
                }
            }
            MidiMessage::PitchBend { .. } => {}
        }
        send(message);
    }
    // Nothing the song held stays down when it ends or is stopped.
    for note in (0..128u8).filter(|note| down[usize::from(*note)]) {
        send(MidiMessage::NoteOff { note });
    }
    for (pedal, number) in SONG_PEDALS.iter().enumerate() {
        if pedals[pedal] {
            send(MidiMessage::ControlChange {
                number: *number,
                value: 0,
            });
        }
    }
}

/// The sample rate of the output kira plays to: the default device's.
fn output_sample_rate() -> Result<i32, String> {
    let device = kira::backend::cpal::cpal::default_host()
        .default_output_device()
        .ok_or("There are no speakers to play the song on.")?;
    let config = device
        .default_output_config()
        .map_err(|error| format!("Could not read the speakers' sample rate: {error}"))?;
    i32::try_from(config.sample_rate())
        .map_err(|_| "The speakers' sample rate is too high.".to_string())
}

/// How long the piano rings on after the song's last message, in seconds.
const RING_OUT: f64 = 3.0;

/// The song heard on the SoundFont's first piano.
struct SongSound {
    synthesizer: Synthesizer,
    messages: Receiver<MidiMessage>,
}

impl SoundData for SongSound {
    type Error = String;
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        Ok((
            Box::new(SongVoice {
                sound: self,
                left: Vec::new(),
                right: Vec::new(),
                ringing: None,
            }),
            (),
        ))
    }
}

struct SongVoice {
    sound: SongSound,
    left: Vec<f32>,
    right: Vec<f32>,
    /// How long it rings on once the song has ended, in seconds.
    ringing: Option<f64>,
}

impl Sound for SongVoice {
    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        let synthesizer = &mut self.sound.synthesizer;
        loop {
            let message = match self.sound.messages.try_recv() {
                Ok(message) => message,
                Err(TryRecvError::Empty) => break,
                // The song has ended: it rings out, then the sound finishes.
                Err(TryRecvError::Disconnected) => {
                    self.ringing.get_or_insert(RING_OUT);
                    break;
                }
            };
            let (command, data1, data2) = match message {
                MidiMessage::NoteOn { note, velocity } => (0x90, note, velocity),
                MidiMessage::NoteOff { note } => (0x80, note, 0),
                MidiMessage::ControlChange { number, value } => (0xB0, number, value),
                MidiMessage::PitchBend { .. } => continue,
            };
            synthesizer.process_midi_message(0, command, i32::from(data1), i32::from(data2));
        }
        self.left.resize(out.len(), 0.0);
        self.right.resize(out.len(), 0.0);
        synthesizer.render(&mut self.left, &mut self.right);
        for ((frame, left), right) in out.iter_mut().zip(&self.left).zip(&self.right) {
            *frame = Frame {
                left: *left,
                right: *right,
            };
        }
        if let Some(ringing) = &mut self.ringing {
            *ringing -= dt * out.len() as f64;
        }
    }

    fn finished(&self) -> bool {
        self.ringing.is_some_and(|ringing| ringing <= 0.0)
    }
}
