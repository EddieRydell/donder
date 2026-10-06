//! Authenticated clock exchanges and scheduled transport commands.
use super::*;
use picoserve::extract::FromRequest;

#[derive(Clone, Copy)]
pub enum Endpoint {
    Clock,
    Command,
}
pub struct Control(pub Endpoint);

use crate::control_protocol::Command;

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
        let received = local_micros();
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        if matches!(self.0, Endpoint::Clock) {
            let connection = request.body_connection.finalize().await?;
            return picoserve::response::Response::empty(StatusCode::NO_CONTENT)
                .with_header("X-Donder-Boot-Id", state.boot_id)
                .with_header("X-Donder-Received-Micros", received)
                .with_header("X-Donder-Sent-Micros", local_micros())
                .with_header("Cache-Control", "no-store")
                .write_to(connection, response_writer)
                .await;
        }
        if request.body_connection.content_length() > 512 {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Control body exceeds 512 bytes\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
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
                if boot_id != state.boot_id
                    || clock_id == 0
                    || rate_ppb.unsigned_abs() > 200_000
                    || valid_for_micros > 15_000_000
                    || valid_for_micros == 0
                    || now.abs_diff(local_micros) > 2_000_000
                {
                    Err("Invalid, stale, or imprecise clock estimate")
                } else {
                    let mut clock = state.clock.lock().await;
                    let active = state.playback.lock().await;
                    if clock.id != 0
                        && clock.id != clock_id
                        && active.as_ref().is_some_and(|p| {
                            p.transport
                                .sample(
                                    clock.master_at(now),
                                    p.show.sequence().duration().as_ticks(),
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
                looping,
                archive_crc,
                archive_bytes,
            } => {
                let clock = *state.clock.lock().await;
                let master_now = clock.master_at(now);
                let mut active = state.playback.lock().await;
                match active.as_mut() {
                    None => Err("No sequence loaded"),
                    Some(_) if boot_id != state.boot_id || !clock.usable(clock_id, now) => {
                        Err("Clock is not synchronized")
                    }
                    Some(p) if p.archive_crc != archive_crc || p.archive_bytes != archive_bytes => {
                        Err("Sequence changed before scheduling")
                    }
                    Some(p) if position_micros > p.show.sequence().duration().as_ticks() => {
                        Err("Position exceeds sequence duration")
                    }
                    Some(_)
                        if at_micros < master_now + 20_000
                            || at_micros > master_now + 2_000_000 =>
                    {
                        Err("Start deadline is late or too far ahead")
                    }
                    Some(p) => {
                        if p.transport.schedule(transport::Scheduled {
                            id: command_id,
                            at: at_micros,
                            mode,
                            position: position_micros,
                            looping,
                        }) {
                            Ok(())
                        } else {
                            Err("Stale transport command")
                        }
                    }
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
            Ok(()) => "OK\n".write_to(connection, response_writer).await,
            Err(message) => {
                (StatusCode::CONFLICT, message)
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}
