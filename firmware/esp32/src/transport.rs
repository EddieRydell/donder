#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "loader", derive(serde::Serialize))]
#[cfg_attr(feature = "loader", serde(rename_all = "camelCase"))]
pub enum Mode {
    Playing,
    Paused,
    Stopped,
}

pub struct Transport {
    pub mode: Mode,
    frame: u32,
    next_frame: u32,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            mode: Mode::Playing,
            frame: 0,
            next_frame: 0,
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if mode == Mode::Paused && self.mode == Mode::Stopped {
            return;
        }
        self.mode = mode;
        if mode == Mode::Stopped {
            self.frame = 0;
            self.next_frame = 0;
        }
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn advance(&mut self, frame_count: u32) -> u32 {
        if self.mode == Mode::Playing {
            self.frame = self.next_frame;
            self.next_frame = if self.frame + 1 >= frame_count {
                0
            } else {
                self.frame + 1
            };
        }
        self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_holds_resume_continues_stop_rewinds_and_playback_loops() {
        let mut transport = Transport::new();
        assert_eq!(transport.advance(3), 0);
        assert_eq!(transport.advance(3), 1);
        transport.set_mode(Mode::Paused);
        assert_eq!(transport.advance(3), 1);
        assert_eq!(transport.advance(3), 1);
        transport.set_mode(Mode::Playing);
        assert_eq!(transport.advance(3), 2);
        assert_eq!(transport.advance(3), 0);
        transport.set_mode(Mode::Stopped);
        transport.set_mode(Mode::Paused);
        assert_eq!(transport.mode, Mode::Stopped);
        assert_eq!(transport.frame(), 0);
        assert_eq!(transport.advance(3), 0);
        transport.set_mode(Mode::Playing);
        assert_eq!(transport.advance(3), 0);
        assert_eq!(transport.advance(3), 1);
    }

    #[test]
    fn single_frame_and_new_upload_start_at_zero() {
        let mut transport = Transport::new();
        for _ in 0..5 {
            assert_eq!(transport.advance(1), 0);
        }
        transport.advance(100);
        transport.advance(100);
        assert_eq!(Transport::new().advance(100), 0);
    }
}
