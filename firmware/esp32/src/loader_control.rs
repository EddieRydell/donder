//! Authenticated control commands: clock estimates and scheduled transport.
use super::*;
use picoserve::extract::FromRequest;

pub(crate) struct Control;

use donder_device_protocol::Command;

impl RequestHandlerService<LoaderState> for Control {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let body = picoserve::extract::Json::<Command>::from_request(
            state,
            request.parts,
            request.body_connection.body(),
        )
        .await;
        let command = match body {
            Ok(picoserve::extract::Json(command)) => command,
            Err(error) => {
                return error
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await;
            }
        };
        let connection = request.body_connection.finalize().await?;
        let now = local_micros();
        let result = match command {
            Command::SyncClock {
                boot_id,
                clock_id,
                local_micros,
                master_micros,
                rate_ppb,
                valid_for_micros,
            } => {
                if boot_id != state.boot_id {
                    Err("The controller restarted since this clock estimate")
                } else {
                    let mut clock = state.clock.lock().await;
                    let active = state.playback.lock().await;
                    if clock.id != 0
                        && clock.id != clock_id
                        && active.as_ref().is_some_and(|p| {
                            p.transport
                                .sample(
                                    clock.master_at(now),
                                    p.show.prepared().duration().as_ticks(),
                                )
                                .0
                                == transport::Mode::Playing
                                || p.transport.pending.is_some()
                        })
                    {
                        Err("Stop playback before changing clock master")
                    } else {
                        *clock = transport::Clock {
                            id: clock_id,
                            local_anchor: local_micros,
                            master_anchor: master_micros,
                            rate_ppb,
                            valid_until: now + u64::from(valid_for_micros),
                        };
                        Ok(())
                    }
                }
            }
            Command::Schedule {
                boot_id,
                clock_id,
                command_id,
                at_micros,
                mode,
                position_micros,
                speed,
                looping,
                show_id,
                show_bytes,
            } => {
                let clock = *state.clock.lock().await;
                let master_now = clock.master_at(now);
                let mut active = state.playback.lock().await;
                match active.as_mut() {
                    None => Err("No show is loaded"),
                    Some(_) if boot_id != state.boot_id || !clock.usable(clock_id, now) => {
                        Err("Clock is not synchronized")
                    }
                    Some(p) if p.show_id != show_id || p.show_bytes != show_bytes => {
                        Err("The show changed before scheduling")
                    }
                    Some(_) if mode == transport::Mode::Ended => {
                        Err("Ended is a playback status, not a command")
                    }
                    Some(_) if at_micros < master_now + 20_000 => {
                        Err("The start deadline is too late")
                    }
                    Some(p) => match speed.rate() {
                        None => Err("Playback speed must be positive"),
                        Some(rate) => {
                            if p.transport.schedule(transport::Scheduled {
                                id: command_id,
                                at: at_micros,
                                mode,
                                position: position_micros,
                                looping,
                                rate,
                            }) {
                                Ok(())
                            } else {
                                Err("Stale transport command")
                            }
                        }
                    },
                }
            }
            Command::Cancel { command_id } => {
                let master_now = state.clock.lock().await.master_at(now);
                // A future frame may already be in the peripheral's DMA queue.
                let cancellation_cutoff = master_now
                    + (DATA_SAMPLES as u64 * 1_000_000).div_ceil(u64::from(I2S_SAMPLE_RATE))
                    + 1_000;
                let mut active = state.playback.lock().await;
                match active.as_mut() {
                    Some(p) => {
                        if p.transport.cancel(command_id, cancellation_cutoff) {
                            Ok(())
                        } else {
                            Err(
                                "Command is due or may already be transmitting; refresh playback status",
                            )
                        }
                    }
                    _ => Err("Scheduled command is no longer pending; refresh playback status"),
                }
            }
        };
        match result {
            Ok(()) => {
                donder_device_protocol::OK
                    .write_to(connection, response_writer)
                    .await
            }
            Err(message) => {
                (StatusCode::CONFLICT, message)
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}
