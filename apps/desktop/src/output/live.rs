use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use donder_language::controller::{Controller, ControllerId};
use donder_output::OutputTransports;
use indexmap::IndexMap;

use crate::audio::AudioEngine;
use crate::dto::{
    AudioTransportState, LiveOutputControllerSnapshot, LiveOutputControllerState,
    LiveOutputSnapshot, LiveOutputState,
};
use crate::rendering::SequenceRenderService;

const INITIAL_TICK_INTERVAL: Duration = Duration::from_millis(20);
const HOLDING_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const OUTPUT_TEST_DURATION: Duration = Duration::from_secs(10);

enum OutputSource {
    Sequence,
    Test {
        frames: Vec<donder_elaboration::ControllerPortFrame>,
        started: Instant,
    },
}

type ActiveOutput = (u32, OutputTransports, LiveOutputSnapshot, OutputSource);

enum Command {
    Enable {
        generation: u32,
        controllers: IndexMap<ControllerId, Controller>,
        active: Vec<ControllerId>,
        source: OutputSource,
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
    resume_after_prepare: bool,
    resume_ready: bool,
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
            resume_after_prepare: false,
            resume_ready: false,
            worker: Some(worker),
        }
    }

    pub(crate) fn enable(
        &mut self,
        controllers: IndexMap<ControllerId, Controller>,
        active: Vec<ControllerId>,
    ) -> LiveOutputSnapshot {
        self.start(controllers, active, OutputSource::Sequence)
    }

    pub(crate) fn test(
        &mut self,
        id: ControllerId,
        controller: Controller,
        frame: donder_elaboration::ControllerPortFrame,
    ) -> LiveOutputSnapshot {
        self.start(
            IndexMap::from([(id.clone(), controller)]),
            vec![id],
            OutputSource::Test {
                frames: vec![frame],
                started: Instant::now(),
            },
        )
    }

    fn start(
        &mut self,
        controllers: IndexMap<ControllerId, Controller>,
        active: Vec<ControllerId>,
        source: OutputSource,
    ) -> LiveOutputSnapshot {
        self.resume_after_prepare = false;
        self.resume_ready = false;
        self.generation = self.generation.saturating_add(1);
        self.snapshot = preparing_snapshot(self.generation, &controllers, &active);
        if matches!(source, OutputSource::Test { .. }) {
            self.snapshot.state = LiveOutputState::Testing;
        }
        if self
            .sender
            .send(Command::Enable {
                generation: self.generation,
                controllers,
                active,
                source,
            })
            .is_err()
        {
            self.snapshot.state = LiveOutputState::Error;
            self.snapshot.last_error = Some("Live output worker is unavailable.".to_string());
        }
        self.snapshot.clone()
    }

    pub(crate) fn disable(&mut self) -> LiveOutputSnapshot {
        self.resume_after_prepare = false;
        self.resume_ready = false;
        self.disable_preserving_resume()
    }

    fn disable_preserving_resume(&mut self) -> LiveOutputSnapshot {
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

    pub(crate) fn suspend(&mut self) -> LiveOutputSnapshot {
        self.snapshot();
        self.resume_ready = false;
        self.resume_after_prepare |= matches!(
            self.snapshot.state,
            LiveOutputState::Preparing | LiveOutputState::Holding | LiveOutputState::Streaming
        );
        self.disable_preserving_resume()
    }

    pub(crate) fn mark_prepared(&mut self) {
        self.resume_ready = true;
    }

    pub(crate) fn take_resume_after_prepare(&mut self) -> bool {
        self.snapshot();
        if self.snapshot.state == LiveOutputState::Error {
            self.resume_after_prepare = false;
        }
        if self.resume_ready && self.snapshot.state != LiveOutputState::Stopping {
            self.resume_ready = false;
            std::mem::take(&mut self.resume_after_prepare)
        } else {
            false
        }
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
                active: ids,
                source,
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
                let mut snapshot = preparing_snapshot(generation, &controllers, &ids);
                match OutputTransports::open(&controllers, &ids) {
                    Ok(transports) => {
                        snapshot.state = match source {
                            OutputSource::Sequence => LiveOutputState::Holding,
                            OutputSource::Test { .. } => LiveOutputState::Testing,
                        };
                        for controller in &mut snapshot.controllers {
                            controller.state = LiveOutputControllerState::Active;
                        }
                        let _ = updates.send(Update {
                            generation,
                            snapshot: snapshot.clone(),
                        });
                        active = Some((generation, transports, snapshot, source));
                    }
                    Err(error) => {
                        fail_snapshot(&mut snapshot, format!("{error:?}"));
                        let _ = updates.send(Update {
                            generation,
                            snapshot,
                        });
                    }
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

        let Some((generation, transports, snapshot, source)) = active.as_mut() else {
            continue;
        };
        tick_started = Instant::now();
        if let OutputSource::Test { frames, started } = source {
            let generation = *generation;
            if started.elapsed() >= OUTPUT_TEST_DURATION {
                let snapshot = stop_output(&mut active, &mut stop_error, generation, None);
                let _ = updates.send(Update {
                    generation,
                    snapshot,
                });
            } else if let Err(error) = transports.send(frames) {
                let failed = stop_output(
                    &mut active,
                    &mut stop_error,
                    generation,
                    Some(format!("Output failed: {error:?}")),
                );
                let _ = updates.send(Update {
                    generation,
                    snapshot: failed,
                });
            }
            tick_interval = INITIAL_TICK_INTERVAL;
            continue;
        }
        let audio = audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot();
        if matches!(
            audio.state,
            AudioTransportState::Stopped | AudioTransportState::Ended
        ) {
            let generation = *generation;
            let snapshot = stop_output(&mut active, &mut stop_error, generation, None);
            let _ = updates.send(Update {
                generation,
                snapshot,
            });
            continue;
        }
        let rendered = render
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .render_current_sequence_frame(&audio);
        match rendered {
            Ok(rendered) => {
                tick_interval = if matches!(audio.state, AudioTransportState::Playing) {
                    Duration::from_secs_f32(1.0 / rendered.frame.frame_rate.max(1) as f32)
                } else {
                    HOLDING_REFRESH_INTERVAL
                };
                if let Err(error) = transports.send(&rendered.frame.controller_frames) {
                    let generation = *generation;
                    let failed = stop_output(
                        &mut active,
                        &mut stop_error,
                        generation,
                        Some(format!("Output failed: {error:?}")),
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
                    Some(format!("Output rendering failed: {error:?}")),
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
        format!("Could not send blackout or terminate output: {error:?}. Check the controller connection; lights may retain their last values.")
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
    controllers: &IndexMap<ControllerId, Controller>,
    active: &[ControllerId],
) -> LiveOutputSnapshot {
    let active_universe_count = active
        .iter()
        .filter_map(|id| controllers.get(id))
        .map(|controller| controller.ports.len() as u32)
        .sum();
    LiveOutputSnapshot {
        state: LiveOutputState::Preparing,
        generation,
        active_controller_count: active.len() as u32,
        active_universe_count,
        controllers: active
            .iter()
            .map(|id| LiveOutputControllerSnapshot {
                id: format!("{}:{}", id.0.document(), id.0.object()),
                state: LiveOutputControllerState::Opening,
                last_error: None,
            })
            .collect(),
        last_error: None,
    }
}

pub(crate) fn disabled_snapshot(generation: u32) -> LiveOutputSnapshot {
    LiveOutputSnapshot {
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
        use donder_language::controller::{
            ArtNetConfig, ArtNetMode, ControllerPort, ControllerPortAddress, ControllerPortId,
            ControllerProtocol,
        };
        use donder_language::identity::{DocumentId, SourceIdentity};
        let id = ControllerId(SourceIdentity::from_document(
            DocumentId::new(uuid::Uuid::new_v4(), "controller.donder".into()),
            "broken".into(),
        ));
        // Invalid protocol address forces a codec failure before any network send.
        let controller = Controller {
            protocol: ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: "127.0.0.1:0".parse().unwrap(),
                destination: "127.0.0.1:6454".parse().unwrap(),
                mode: ArtNetMode::Unicast,
            }),
            ports: vec![ControllerPort {
                id: ControllerPortId(1),
                address: ControllerPortAddress::ArtNetPort(u16::MAX),
                slot_count: 6,
            }],
        };
        let controllers = IndexMap::from([(id.clone(), controller)]);
        let ids = vec![id];
        let transports = OutputTransports::open(&controllers, &ids).unwrap();
        let snapshot = preparing_snapshot(3, &controllers, &ids);
        let mut active = Some((3, transports, snapshot, OutputSource::Sequence));
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
        assert!(message.contains("broken"));
        let repeated = stop_output(&mut active, &mut stop_error, 5, None);
        assert_eq!(repeated.state, LiveOutputState::Error);
        assert_eq!(repeated.generation, 5);
        assert_eq!(repeated.last_error.as_deref(), Some(message.as_str()));
    }

    #[test]
    fn suspend_uses_pending_worker_state_before_deciding_to_resume() {
        for (state, stop_state, resume) in [
            (LiveOutputState::Error, LiveOutputState::Disabled, false),
            (LiveOutputState::Disabled, LiveOutputState::Disabled, false),
            (LiveOutputState::Holding, LiveOutputState::Disabled, true),
            (LiveOutputState::Holding, LiveOutputState::Error, false),
        ] {
            let (sender, _commands) = mpsc::channel();
            let (updates, receiver) = mpsc::channel();
            let mut snapshot = disabled_snapshot(9);
            snapshot.state = LiveOutputState::Preparing;
            let mut service = LiveOutputService {
                sender,
                receiver,
                generation: 9,
                snapshot,
                resume_after_prepare: false,
                resume_ready: false,
                worker: None,
            };
            let mut completed = disabled_snapshot(9);
            completed.state = state.clone();
            updates
                .send(Update {
                    generation: 9,
                    snapshot: completed,
                })
                .unwrap();
            service.suspend();
            service.mark_prepared();
            assert!(!service.take_resume_after_prepare());
            let mut stopped = disabled_snapshot(10);
            stopped.state = stop_state;
            updates
                .send(Update {
                    generation: 10,
                    snapshot: stopped,
                })
                .unwrap();
            assert_eq!(service.take_resume_after_prepare(), resume);
        }
    }
}
