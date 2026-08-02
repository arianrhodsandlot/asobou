use crate::emulation::state::{StateBackend, compress, decompress};
use std::collections::VecDeque;

pub struct Rewind {
    granularity: u64,
    budget: usize,
    state_size: usize,
    scratch: Vec<u8>,
    buffer: VecDeque<(u64, Vec<u8>)>,
    used: usize,
}

impl Rewind {
    pub fn new(state_size: usize, granularity: u64, budget: usize) -> Option<Self> {
        if state_size == 0 || granularity == 0 {
            return None;
        }
        Some(Self {
            granularity,
            budget,
            state_size,
            scratch: vec![0; state_size],
            buffer: VecDeque::new(),
            used: 0,
        })
    }

    pub fn capture(&mut self, backend: &dyn StateBackend, frame: u64) {
        if !frame.is_multiple_of(self.granularity) {
            return;
        }
        if !backend.serialize(&mut self.scratch) {
            return;
        }
        let compressed = compress(&self.scratch);
        self.used += compressed.len();
        self.buffer.push_back((frame, compressed));
        while self.used > self.budget && self.buffer.len() > 1 {
            if let Some((_, data)) = self.buffer.pop_front() {
                self.used -= data.len();
            }
        }
    }

    pub fn rewind(
        &mut self,
        backend: &dyn StateBackend,
        frame: u64,
        run: &mut dyn FnMut(),
    ) -> Option<u64> {
        let target = frame.checked_sub(self.granularity)?;
        let (restored, data) = self
            .buffer
            .iter()
            .rev()
            .find(|(f, _)| *f < target)
            .map(|(f, data)| (*f, data.clone()))?;
        if !backend.unserialize(&decompress(&data, self.state_size)) {
            return None;
        }
        while self.buffer.back().is_some_and(|(f, _)| *f >= target) {
            if let Some((_, data)) = self.buffer.pop_back() {
                self.used -= data.len();
            }
        }
        for current in restored..target {
            run();
            self.capture(backend, current + 1);
        }
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeCore {
        state: Cell<u64>,
    }

    impl StateBackend for FakeCore {
        fn serialize(&self, data: &mut [u8]) -> bool {
            data.copy_from_slice(&self.state.get().to_le_bytes());
            true
        }

        fn unserialize(&self, data: &[u8]) -> bool {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[..8]);
            self.state.set(u64::from_le_bytes(bytes));
            true
        }
    }

    fn step(core: &FakeCore) {
        core.state.set(core.state.get() + 1);
    }

    fn play_from(core: &FakeCore, rewind: &mut Rewind, start: u64, frames: u64) -> u64 {
        let mut frame = start;
        for _ in 0..frames {
            step(core);
            frame += 1;
            rewind.capture(core, frame);
        }
        frame
    }

    fn play(core: &FakeCore, rewind: &mut Rewind, frames: u64) -> u64 {
        rewind.capture(core, 0);
        play_from(core, rewind, 0, frames)
    }

    #[test]
    fn captures_every_granularity_frames() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 2, 1024).unwrap();
        let frames = play(&core, &mut rewind, 6);

        assert_eq!(frames, 6);
        assert_eq!(
            rewind.buffer.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
            [0, 2, 4, 6]
        );
    }

    #[test]
    fn rewind_restores_a_state_and_reruns_to_the_target() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 2, 1024).unwrap();
        play(&core, &mut rewind, 6);
        let mut runs = 0;

        let target = rewind
            .rewind(&core, 6, &mut || {
                runs += 1;
                step(&core);
            })
            .unwrap();

        assert_eq!(target, 4);
        assert_eq!(core.state.get(), 4);
        assert_eq!(runs, 2);
    }

    #[test]
    fn repeated_rewinds_step_back_until_the_buffer_starts() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 2, 1024).unwrap();
        play(&core, &mut rewind, 6);

        let first = rewind.rewind(&core, 6, &mut || step(&core)).unwrap();
        let second = rewind.rewind(&core, first, &mut || step(&core)).unwrap();

        assert_eq!((first, second), (4, 2));
        assert_eq!(core.state.get(), 2);
        assert!(rewind.rewind(&core, second, &mut || step(&core)).is_none());
    }

    #[test]
    fn rewinding_then_playing_forward_keeps_rewind_available() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 2, 1024).unwrap();
        let frame = play(&core, &mut rewind, 6);
        let target = rewind.rewind(&core, frame, &mut || step(&core)).unwrap();

        let resumed = play_from(&core, &mut rewind, target, 4);
        let mut runs = 0;
        let target2 = rewind
            .rewind(&core, resumed, &mut || {
                runs += 1;
                step(&core);
            })
            .unwrap();

        assert_eq!((target, resumed), (4, 8));
        assert_eq!(target2, 6);
        assert_eq!(core.state.get(), 6);
        assert_eq!(runs, 2);
    }

    #[test]
    fn rewind_is_unavailable_before_granularity_frames() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 2, 1024).unwrap();
        rewind.capture(&core, 0);
        step(&core);
        rewind.capture(&core, 1);

        assert!(rewind.rewind(&core, 1, &mut || step(&core)).is_none());
        assert!(rewind.rewind(&core, 0, &mut || step(&core)).is_none());
    }

    #[test]
    fn budget_evicts_the_oldest_snapshots() {
        let core = FakeCore {
            state: Cell::new(0),
        };
        let mut rewind = Rewind::new(8, 1, 100).unwrap();
        play(&core, &mut rewind, 20);

        assert!(rewind.used <= 100);
        assert!(rewind.buffer.len() < 20);
        assert!(rewind.buffer.front().is_some_and(|(f, _)| *f > 0));
    }

    #[test]
    fn zero_state_size_disables_rewind() {
        assert!(Rewind::new(0, 2, 1024).is_none());
        assert!(Rewind::new(8, 0, 1024).is_none());
    }
}
