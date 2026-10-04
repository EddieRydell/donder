//! Show time follows a clock, never the number of completed render iterations.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Playing,
    Paused,
    Stopped,
    Ended,
}

#[derive(Clone, Copy)]
pub struct Clock {
    pub id: u32,
    pub local_anchor: u64,
    pub master_anchor: u64,
    pub rate_ppb: i32,
    pub uncertainty: u32,
    pub valid_until: u64,
}

impl Clock {
    pub const fn new() -> Self {
        Self {
            id: 0,
            local_anchor: 0,
            master_anchor: 0,
            rate_ppb: 0,
            uncertainty: u32::MAX,
            valid_until: 0,
        }
    }
    pub fn master_at(&self, local: u64) -> u64 {
        let elapsed = i128::from(local) - i128::from(self.local_anchor);
        (i128::from(self.master_anchor)
            + elapsed
            + elapsed * i128::from(self.rate_ppb) / 1_000_000_000)
            .clamp(0, i128::from(u64::MAX)) as u64
    }
    pub fn local_at(&self, master: u64) -> u64 {
        let elapsed = (i128::from(master) - i128::from(self.master_anchor)) * 1_000_000_000
            / (1_000_000_000 + i128::from(self.rate_ppb));
        (i128::from(self.local_anchor) + elapsed).clamp(0, i128::from(u64::MAX)) as u64
    }
    pub fn usable(&self, id: u32, local: u64) -> bool {
        id != 0 && self.id == id && local < self.valid_until && self.uncertainty <= 2_000
    }
}

#[derive(Clone, Copy)]
pub struct Scheduled {
    pub id: u32,
    pub at: u64,
    pub mode: Mode,
    pub position: u32,
    pub looping: bool,
}

#[derive(Clone, Copy)]
pub struct Transport {
    pub mode: Mode,
    anchor_time: u64,
    anchor_position: u32,
    looping: bool,
    pub pending: Option<Scheduled>,
    pub command_id: u32,
    pub generation: u32,
}

impl Transport {
    pub const fn new() -> Self {
        Self {
            mode: Mode::Stopped,
            anchor_time: 0,
            anchor_position: 0,
            looping: false,
            pending: None,
            command_id: 0,
            generation: 0,
        }
    }
    pub fn apply(&mut self, mode: Mode, position: u32, now: u64, looping: bool) {
        self.generation = self.generation.wrapping_add(1);
        self.mode = mode;
        self.anchor_time = now;
        self.anchor_position = position;
        self.looping = looping;
        self.pending = None;
    }
    pub fn schedule(&mut self, command: Scheduled) -> bool {
        if command.id <= self.command_id {
            return false;
        }
        self.command_id = command.id;
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(command);
        true
    }
    pub fn sample(&self, now: u64, duration: u32) -> (Mode, u32) {
        let mut current = *self;
        if let Some(pending) = current.pending
            && now >= pending.at
        {
            current.apply(pending.mode, pending.position, pending.at, pending.looping);
        }
        if current.mode != Mode::Playing {
            return (current.mode, current.anchor_position);
        }
        let position = u64::from(current.anchor_position) + now.saturating_sub(current.anchor_time);
        if current.looping {
            (Mode::Playing, (position % u64::from(duration)) as u32)
        } else if position >= u64::from(duration) {
            (Mode::Ended, duration)
        } else {
            (Mode::Playing, position as u32)
        }
    }
    pub fn refresh(&mut self, now: u64, duration: u32) {
        if let Some(pending) = self.pending
            && now >= pending.at
        {
            self.apply(pending.mode, pending.position, pending.at, pending.looping);
        }
        let (mode, position) = self.sample(now, duration);
        if mode == Mode::Ended && self.mode != Mode::Ended {
            self.apply(mode, position, now, false);
        }
    }
    pub fn cancel(&mut self, id: u32, now: u64) -> bool {
        if self
            .pending
            .is_some_and(|pending| pending.id == id && now < pending.at)
        {
            self.pending = None;
            self.generation = self.generation.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_invalidates_a_precomputed_future_frame() {
        let mut transport = Transport::new();
        transport.schedule(Scheduled {
            id: 1,
            at: 1_000_000,
            mode: Mode::Playing,
            position: 0,
            looping: false,
        });
        let generation = transport.generation;
        assert_eq!(
            transport.sample(1_010_000, 8_000_000),
            (Mode::Playing, 10_000)
        );
        assert!(!transport.cancel(2, 999_999));
        assert!(transport.cancel(1, 999_999));
        assert_ne!(transport.generation, generation);
        assert_eq!(transport.sample(1_010_000, 8_000_000), (Mode::Stopped, 0));
        transport.schedule(Scheduled {
            id: 2,
            at: 2_000_000,
            mode: Mode::Playing,
            position: 0,
            looping: false,
        });
        assert!(!transport.cancel(2, 2_000_000));
        assert_eq!(
            transport.sample(2_010_000, 8_000_000),
            (Mode::Playing, 10_000)
        );
    }
    #[test]
    fn missed_frames_follow_elapsed_time_and_scheduled_start() {
        let mut transport = Transport::new();
        assert!(transport.schedule(Scheduled {
            id: 1,
            at: 1_000_000,
            mode: Mode::Playing,
            position: 0,
            looping: false
        }));
        assert_eq!(transport.sample(999_999, 8_000_000), (Mode::Stopped, 0));
        assert_eq!(
            transport.sample(1_100_000, 8_000_000),
            (Mode::Playing, 100_000)
        );
        assert_eq!(
            transport.sample(9_000_000, 8_000_000),
            (Mode::Ended, 8_000_000)
        );
    }
    #[test]
    fn pause_seek_stop_and_replaced_schedules() {
        let mut transport = Transport::new();
        transport.apply(Mode::Playing, 0, 1_000_000, false);
        transport.schedule(Scheduled {
            id: 2,
            at: 1_100_000,
            mode: Mode::Paused,
            position: 100_000,
            looping: false,
        });
        assert_eq!(
            transport.sample(1_099_999, 8_000_000),
            (Mode::Playing, 99_999)
        );
        assert_eq!(
            transport.sample(2_000_000, 8_000_000),
            (Mode::Paused, 100_000)
        );
        assert!(!transport.schedule(Scheduled {
            id: 1,
            at: 3_000_000,
            mode: Mode::Playing,
            position: 0,
            looping: true
        }));
        transport.apply(Mode::Paused, 7_000_000, 2_000_000, false);
        assert_eq!(
            transport.sample(3_000_000, 8_000_000),
            (Mode::Paused, 7_000_000)
        );
        transport.apply(Mode::Stopped, 0, 3_000_000, false);
        assert_eq!(transport.sample(4_000_000, 8_000_000), (Mode::Stopped, 0));
        transport.apply(Mode::Playing, 0, 0, true);
        assert_eq!(
            transport.sample(9_000_000, 8_000_000),
            (Mode::Playing, 1_000_000)
        );
    }
    #[test]
    fn clock_offset_drift_and_freshness() {
        assert!(!Clock::new().usable(1, 0));
        let clock = Clock {
            id: 1,
            local_anchor: 100_000,
            master_anchor: 200_000,
            rate_ppb: 100_000,
            uncertainty: 500,
            valid_until: 10_100_000,
        };
        assert_eq!(clock.master_at(10_100_000), 10_201_000);
        assert_eq!(clock.local_at(10_201_000), 10_100_000);
        assert!(clock.usable(1, 100_000));
        assert!(!clock.usable(2, 100_000));
        assert!(!clock.usable(1, 10_100_000));
        let mut transport = Transport::new();
        transport.apply(Mode::Playing, 0, 0, false);
        transport.refresh(8_000_000, 8_000_000);
        assert_eq!(transport.mode, Mode::Ended);
    }
}
