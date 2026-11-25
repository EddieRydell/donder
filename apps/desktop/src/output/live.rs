use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use donder_model::{Controller, ControllerId};
use donder_output::OutputTransports;

use crate::audio::AudioEngine;
use crate::playing::{Playing, PlayingOutput};
use donder_editor::dto::{
    LiveOutputControllerSnapshot, LiveOutputControllerState, LiveOutputSnapshot, LiveOutputState,
};
use donder_runtime_types::ConsoleEvent;

const INITIAL_TICK_INTERVAL: Duration = Duration::from_millis(20);
const HOLDING_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

enum LiveFeed {
    /// What plays, on the controllers its show was prepared on.
    Playing(Box<PlayingOutput>),
    /// A test pattern, until output stops.
    Test {
        test: donder_output::OutputTest,
        started: Instant,
    },
}

type ActiveOutput = (u32, OutputTransports, LiveOutputSnapshot, LiveFeed);

enum Command {
    Enable {
        generation: u32,
        /// The streamed controllers the feed's frames are for.
        controllers: Vec<(ControllerId, Controller)>,
        feed: LiveFeed,
    },
    /// A performance's inputs, in arrival order.
    Events {
        generation: u32,
        events: Vec<ConsoleEvent>,
    },
    Disable {
        generation: u32,
    },
    Shutdown {
        generation: u32,
    },
}

struct Update {
    generation: u32,
    snapshot: LiveOutputSnapshot,
}

pub(crate) struct LiveOutputService {
    sender: mpsc::Sender<Command>,
    receiver: mpsc::Receiver<Update>,
    generation: u32,
    snapshot: LiveOutputSnapshot,
    /// A start that waits for the output to finish stopping.
    pending_start: bool,
    /// Whether the active output plays a performance, whose inputs follow it.
    session: bool,
    worker: Option<JoinHandle<()>>,
}

impl LiveOutputService {
    pub(crate) fn new(audio: Arc<Mutex<AudioEngine>>) -> Self {
        let (sender, command_receiver) = mpsc::channel();
        let (update_sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || worker(command_receiver, update_sender, audio));
        Self {
            sender,
            receiver,
            generation: 0,
            snapshot: disabled_snapshot(0),
            pending_start: false,
            session: false,
            worker: Some(worker),
        }
    }

    /// Play `playing` to the controllers its show was prepared on.
    pub(crate) fn play(&mut self, playing: Playing) -> LiveOutputSnapshot {
        let controllers = playing.controllers.clone();
        self.start(
            &controllers,
            LiveFeed::Playing(Box::new(PlayingOutput::new(playing))),
        )
    }

    /// Send a performance's new inputs to the output that plays it.
    pub(crate) fn events(&mut self, events: &[ConsoleEvent]) {
        if !self.session {
            return;
        }
        if self
            .sender
            .send(Command::Events {
                generation: self.generation,
                events: events.to_vec(),
            })
            .is_err()
        {
            fail_snapshot(
                &mut self.snapshot,
                "Live output worker is unavailable.".into(),
            );
        }
    }

    /// Stream a test pattern in place of live output until output stops.
    pub(crate) fn test(&mut self, test: donder_output::OutputTest) -> LiveOutputSnapshot {
        let controllers = test
            .controllers()
            .iter()
            .map(|(id, controller)| (*id, controller.clone()))
            .collect::<Vec<_>>();
        self.start(
            &controllers,
            LiveFeed::Test {
                test,
                started: Instant::now(),
            },
        )
    }

    /// Stream `feed` to the E1.31 and Art-Net controllers among
    /// `controllers`, the controllers its frames are for.
    fn start(
        &mut self,
        controllers: &[(ControllerId, Controller)],
        feed: LiveFeed,
    ) -> LiveOutputSnapshot {
        let controllers = controllers
            .iter()
            .filter(|(_, controller)| controller.universes().is_some())
            .cloned()
            .collect::<Vec<_>>();
        self.pending_start = false;
        self.session = matches!(&feed, LiveFeed::Playing(output) if output.is_session());
        self.generation = self.generation.saturating_add(1);
        self.snapshot = preparing_snapshot(self.generation, &controllers);
        if matches!(feed, LiveFeed::Test { .. }) {
            self.snapshot.state = LiveOutputState::Testing;
        }
        if self
            .sender
            .send(Command::Enable {
                generation: self.generation,
                controllers,
                feed,
            })
            .is_err()
        {
            self.snapshot.state = LiveOutputState::Error;
            self.snapshot.last_error = Some("Live output worker is unavailable.".to_string());
        }
        self.snapshot.clone()
    }

    pub(crate) fn disable(&mut self) -> LiveOutputSnapshot {
        self.pending_start = false;
        self.session = false;
        self.snapshot();
        if matches!(
            self.snapshot.state,
            LiveOutputState::Disabled | LiveOutputState::Error
        ) {
            return self.snapshot.clone();
        }
        self.generation = self.generation.saturating_add(1);
        self.snapshot.generation = self.generation;
        self.snapshot.state = LiveOutputState::Stopping;
        if self
            .sender
            .send(Command::Disable {
                generation: self.generation,
            })
            .is_err()
        {
            fail_snapshot(
                &mut self.snapshot,
                "Cannot confirm output stopped: the output worker is unavailable.".into(),
            );
        }
        self.snapshot.clone()
    }

    /// Whether the output is stopping; a start then waits until it stopped.
    pub(crate) fn stopping(&mut self) -> bool {
        self.snapshot().state == LiveOutputState::Stopping
    }

    /// Start once the output finished stopping.
    pub(crate) fn defer_start(&mut self) {
        self.pending_start = true;
    }

    /// Whether a deferred start can go ahead now.
    pub(crate) fn take_pending_start(&mut self) -> bool {
        if self.pending_start && !self.stopping() {
            self.pending_start = false;
            return true;
        }
        false
    }

    pub(crate) fn snapshot(&mut self) -> LiveOutputSnapshot {
        for update in self.receiver.try_iter() {
            if update.generation == self.generation {
                self.snapshot = update.snapshot;
            }
        }
        self.snapshot.clone()
    }

    pub(crate) fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        self.generation = self.generation.saturating_add(1);
        if self
            .sender
            .send(Command::Shutdown {
                generation: self.generation,
            })
            .is_err()
        {
            fail_snapshot(
                &mut self.snapshot,
                "Output worker was unavailable during shutdown.".into(),
            );
        }
        if worker.join().is_err() {
            fail_snapshot(
                &mut self.snapshot,
                "Output worker failed during shutdown.".into(),
            );
        }
        self.snapshot();
    }
}

impl Drop for LiveOutputService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker(
    receiver: mpsc::Receiver<Command>,
    updates: mpsc::Sender<Update>,
    audio: Arc<Mutex<AudioEngine>>,
) {
    let mut active: Option<ActiveOutput> = None;
    let mut stop_error: Option<LiveOutputSnapshot> = None;
    let mut tick_interval = INITIAL_TICK_INTERVAL;
    let mut tick_started = Instant::now();
    loop {
        let wait = active.as_ref().map_or(Duration::from_secs(60), |_| {
            tick_interval.saturating_sub(tick_started.elapsed())
        });
        match receiver.recv_timeout(wait) {
            Ok(Command::Enable {
                generation,
                controllers,
                feed,
            }) => {
                if active.is_none() {
                    stop_error = None;
                }
                let stopped = stop_output(&mut active, &mut stop_error, generation, None);
                if stopped.state == LiveOutputState::Error {
                    let _ = updates.send(Update {
                        generation,
                        snapshot: stopped,
                    });
                    continue;
                }
                tick_interval = INITIAL_TICK_INTERVAL;
                let mut snapshot = preparing_snapshot(generation, &controllers);
                match OutputTransports::open(
                    controllers.iter().map(|(id, controller)| (*id, controller)),
                ) {
                    Ok(transports) => {
                        snapshot.state = match &feed {
                            LiveFeed::Playing(output) if output.is_session() => {
                                LiveOutputState::Streaming
                            }
                            LiveFeed::Playing(_) => LiveOutputState::Holding,
                            LiveFeed::Test { .. } => LiveOutputState::Testing,
                        };
                        for controller in &mut snapshot.controllers {
                            controller.state = LiveOutputControllerState::Active;
                        }
                        let _ = updates.send(Update {
                            generation,
                            snapshot: snapshot.clone(),
                        });
                        active = Some((generation, transports, snapshot, feed));
                    }
                    Err(error) => {
                        fail_snapshot(&mut snapshot, format!("Output could not start: {error}"));
                        let _ = updates.send(Update {
                            generation,
                            snapshot,
                        });
                    }
                }
            }
            Ok(Command::Events { generation, events }) => {
                if let Some((active_generation, _, _, LiveFeed::Playing(output))) = active.as_mut()
                    && *active_generation == generation
                {
                    output.apply(&events);
                }
            }
            Ok(Command::Disable { generation }) => {
                let snapshot = stop_output(&mut active, &mut stop_error, generation, None);
                let _ = updates.send(Update {
                    generation,
                    snapshot,
                });
            }
            Ok(Command::Shutdown { generation }) => {
                let snapshot = stop_output(&mut active, &mut stop_error, generation, None);
                let _ = updates.send(Update {
                    generation,
                    snapshot,
                });
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let generation = active.as_ref().map_or(0, |output| output.0);
                let snapshot = stop_output(&mut active, &mut stop_error, generation, None);
                let _ = updates.send(Update {
                    generation,
                    snapshot,
                });
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let Some((generation, transports, snapshot, feed)) = active.as_mut() else {
            continue;
        };
        tick_started = Instant::now();
        if let LiveFeed::Test { test, started } = feed {
            let generation = *generation;
            if let Err(error) = transports.send(&test.frames(started.elapsed())) {
                let failed = stop_output(
                    &mut active,
                    &mut stop_error,
                    generation,
                    Some(format!("Output failed: {error}")),
                );
                let _ = updates.send(Update {
                    generation,
                    snapshot: failed,
                });
            }
            tick_interval = INITIAL_TICK_INTERVAL;
            continue;
        }
        let LiveFeed::Playing(output) = feed else {
            unreachable!("the test feed continued above")
        };
        let audio = audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot();
        let generation = *generation;
        let failure = match output.render(&audio) {
            Ok(tick) => {
                tick_interval = if tick.moving {
                    tick.interval
                } else {
                    HOLDING_REFRESH_INTERVAL
                };
                let state = if tick.moving {
                    LiveOutputState::Streaming
                } else {
                    LiveOutputState::Holding
                };
                match transports.send(tick.frames) {
                    Err(error) => Some(format!("Output failed: {error}")),
                    Ok(()) => {
                        if snapshot.state != state {
                            snapshot.state = state;
                            let _ = updates.send(Update {
                                generation,
                                snapshot: snapshot.clone(),
                            });
                        }
                        None
                    }
                }
            }
            Err(error) => Some(format!("Output rendering failed: {error}")),
        };
        if let Some(failure) = failure {
            let failed = stop_output(&mut active, &mut stop_error, generation, Some(failure));
            let _ = updates.send(Update {
                generation,
                snapshot: failed,
            });
        }
    }
}

fn stop_output<Feed>(
    active: &mut Option<(u32, OutputTransports, LiveOutputSnapshot, Feed)>,
    stop_error: &mut Option<LiveOutputSnapshot>,
    generation: u32,
    failure: Option<String>,
) -> LiveOutputSnapshot {
    let Some((_, mut transports, mut snapshot, _)) = active.take() else {
        return stop_error.as_ref().map_or_else(
            || disabled_snapshot(generation),
            |error| {
                let mut error = error.clone();
                error.generation = generation;
                error
            },
        );
    };
    let termination = transports.blackout_and_terminate().err().map(|error| {
        format!("Could not send blackout or terminate output: {error}. Check the controller connection; lights may retain their last values.")
    });
    let message = match (failure, termination) {
        (Some(failure), Some(termination)) => Some(format!("{failure} {termination}")),
        (failure, termination) => failure.or(termination),
    };
    if let Some(message) = message {
        snapshot.generation = generation;
        snapshot.active_controller_count = 0;
        snapshot.active_universe_count = 0;
        fail_snapshot(&mut snapshot, message);
        *stop_error = Some(snapshot.clone());
        snapshot
    } else {
        disabled_snapshot(generation)
    }
}

fn fail_snapshot(snapshot: &mut LiveOutputSnapshot, message: String) {
    snapshot.state = LiveOutputState::Error;
    snapshot.last_error = Some(message.clone());
    for controller in &mut snapshot.controllers {
        controller.state = LiveOutputControllerState::Error;
        controller.last_error = Some(message.clone());
    }
}

fn preparing_snapshot(
    generation: u32,
    controllers: &[(ControllerId, Controller)],
) -> LiveOutputSnapshot {
    let active_universe_count = controllers
        .iter()
        .map(|(_, controller)| {
            controller
                .universes()
                .map_or(0, |universes| u32::from(universes.count))
        })
        .sum();
    LiveOutputSnapshot {
        enabled: false,
        state: LiveOutputState::Preparing,
        generation,
        active_controller_count: controllers.len() as u32,
        active_universe_count,
        controllers: controllers
            .iter()
            .map(|(id, _)| LiveOutputControllerSnapshot {
                id: format!("{:?}", id.0),
                state: LiveOutputControllerState::Opening,
                last_error: None,
            })
            .collect(),
        last_error: None,
    }
}

pub(crate) fn disabled_snapshot(generation: u32) -> LiveOutputSnapshot {
    LiveOutputSnapshot {
        enabled: false,
        state: LiveOutputState::Disabled,
        generation,
        active_controller_count: 0,
        active_universe_count: 0,
        controllers: Vec::new(),
        last_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_blackout_retains_original_failure_and_drops_transports() {
        use donder_model::{ArtNetConfig, ArtNetMode, ControllerId, ControllerProtocol, Universes};
        let id = ControllerId(0);
        // Invalid protocol address forces a codec failure before any network send.
        let controller = Controller {
            description: None,
            protocol: ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: "127.0.0.1:0".parse().unwrap(),
                destination: "127.0.0.1:6454".parse().unwrap(),
                mode: ArtNetMode::Unicast,
                universes: Universes {
                    start: u16::MAX,
                    count: 1,
                    channels: 6,
                },
                fseq_channel: None,
                ports: Vec::new(),
            }),
        };
        let controllers = [(id, controller)];
        let transports =
            OutputTransports::open(controllers.iter().map(|(id, controller)| (*id, controller)))
                .unwrap();
        let snapshot = preparing_snapshot(3, &controllers);
        let mut active = Some((3, transports, snapshot, ()));
        let mut stop_error = None;
        let stopped = stop_output(
            &mut active,
            &mut stop_error,
            4,
            Some("Original render failure.".into()),
        );
        assert!(active.is_none());
        assert_eq!(stopped.generation, 4);
        assert_eq!(stopped.state, LiveOutputState::Error);
        assert_eq!(stopped.active_universe_count, 0);
        let message = stopped.last_error.unwrap();
        assert!(message.contains("Original render failure."));
        assert!(message.contains("Could not send blackout or terminate output"));
        assert!(message.contains("127.0.0.1:6454"));
        let repeated = stop_output(&mut active, &mut stop_error, 5, None);
        assert_eq!(repeated.state, LiveOutputState::Error);
        assert_eq!(repeated.generation, 5);
        assert_eq!(repeated.last_error.as_deref(), Some(message.as_str()));
    }
}
