use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use donder_model::Controller;
use donder_output::OutputTransports;

use crate::audio::AudioEngine;
use crate::rendering::{ConsoleOutput, SequenceRenderService};
use donder_editor::dto::{
    AudioTransportState, LiveOutputControllerSnapshot, LiveOutputControllerState,
    LiveOutputSnapshot, LiveOutputState,
};
use donder_runtime_types::ConsoleEvent;

const INITIAL_TICK_INTERVAL: Duration = Duration::from_millis(20);
const HOLDING_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

enum LiveFeed {
    /// The prepared sequence of render session `session`, whose
    /// controllers the transports opened; frames of a later session wait for
    /// output to start again on its controllers.
    Sequence { session: u64 },
    /// A performance, rendered on its own session clock.
    Console(Box<ConsoleOutput>),
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
        controllers: Vec<Controller>,
        feed: LiveFeed,
    },
    /// Commands for the performance a console output plays, in arrival order.
    ConsoleEvents {
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
    /// Whether the active output plays a performance.
    console: bool,
    worker: Option<JoinHandle<()>>,
}

impl LiveOutputService {
    pub(crate) fn new(
        audio: Arc<Mutex<AudioEngine>>,
        render: Arc<Mutex<SequenceRenderService>>,
    ) -> Self {
        let (sender, command_receiver) = mpsc::channel();
        let (update_sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || worker(command_receiver, update_sender, audio, render));
        Self {
            sender,
            receiver,
            generation: 0,
            snapshot: disabled_snapshot(0),
            pending_start: false,
            console: false,
            worker: Some(worker),
        }
    }

    /// Play render session `session`'s sequence to `controllers`, the
    /// controllers it was prepared on.
    pub(crate) fn enable(
        &mut self,
        session: u64,
        controllers: &[Controller],
    ) -> LiveOutputSnapshot {
        self.start(controllers, LiveFeed::Sequence { session })
    }

    /// Play a performance to the controllers it was prepared on.
    pub(crate) fn perform(&mut self, output: ConsoleOutput) -> LiveOutputSnapshot {
        let controllers = output.controllers().to_vec();
        self.start(&controllers, LiveFeed::Console(Box::new(output)))
    }

    /// Send a performance's new commands to the output that plays it.
    pub(crate) fn console_events(&mut self, events: &[ConsoleEvent]) {
        if !self.console {
            return;
        }
        if self
            .sender
            .send(Command::ConsoleEvents {
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
        let controllers = test.controllers().values().cloned().collect::<Vec<_>>();
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
    fn start(&mut self, controllers: &[Controller], feed: LiveFeed) -> LiveOutputSnapshot {
        let controllers = controllers
            .iter()
            .filter(|controller| controller.universes().is_some())
            .cloned()
            .collect::<Vec<_>>();
        self.pending_start = false;
        self.console = matches!(feed, LiveFeed::Console(_));
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
        self.console = false;
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
    render: Arc<Mutex<SequenceRenderService>>,
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
                match OutputTransports::open(&controllers) {
                    Ok(transports) => {
                        snapshot.state = match feed {
                            LiveFeed::Sequence { .. } => LiveOutputState::Holding,
                            LiveFeed::Console(_) => LiveOutputState::Streaming,
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
            Ok(Command::ConsoleEvents { generation, events }) => {
                if let Some((active_generation, _, _, LiveFeed::Console(output))) = active.as_mut()
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
        if let LiveFeed::Console(output) = feed {
            tick_interval = output.frame_interval();
            if let Err(error) = transports.send(output.render()) {
                let generation = *generation;
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
            continue;
        }
        let LiveFeed::Sequence { session } = feed else {
            unreachable!("test and console feeds continued above")
        };
        let audio = audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot();
        let rendered = {
            let mut render = render
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // A later session's frames are for its own controllers: hold
            // until output starts again on them.
            if render.session_generation() != *session {
                tick_interval = HOLDING_REFRESH_INTERVAL;
                continue;
            }
            render.render_current_sequence_frame(&audio)
        };
        match rendered {
            Ok(rendered) => {
                tick_interval = if matches!(audio.state, AudioTransportState::Playing) {
                    rendered.frame.frame_interval
                } else {
                    HOLDING_REFRESH_INTERVAL
                };
                if let Err(error) = transports.send(&rendered.frame.controller_frames) {
                    let generation = *generation;
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
                } else {
                    let state = if matches!(audio.state, AudioTransportState::Playing) {
                        LiveOutputState::Streaming
                    } else {
                        LiveOutputState::Holding
                    };
                    if snapshot.state != state {
                        snapshot.state = state;
                        let _ = updates.send(Update {
                            generation: *generation,
                            snapshot: snapshot.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                let generation = *generation;
                let failed = stop_output(
                    &mut active,
                    &mut stop_error,
                    generation,
                    Some(format!("Output rendering failed: {error}")),
                );
                let _ = updates.send(Update {
                    generation,
                    snapshot: failed,
                });
            }
        }
    }
}

fn stop_output(
    active: &mut Option<ActiveOutput>,
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

fn preparing_snapshot(generation: u32, controllers: &[Controller]) -> LiveOutputSnapshot {
    let active_universe_count = controllers
        .iter()
        .map(|controller| {
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
            .map(|controller| LiveOutputControllerSnapshot {
                id: format!("{:?}", controller.id.0),
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
            id,
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
        let controllers = [controller];
        let transports = OutputTransports::open(&controllers).unwrap();
        let snapshot = preparing_snapshot(3, &controllers);
        let mut active = Some((3, transports, snapshot, LiveFeed::Sequence { session: 0 }));
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
