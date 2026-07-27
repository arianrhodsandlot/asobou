use super::{AudioBackend, AudioSink};

pub struct NullBackend;
struct NullSink;

impl AudioBackend for NullBackend {
    fn preferred_sample_rate(&mut self) -> Result<Option<u32>, String> {
        Ok(None)
    }

    fn start(&mut self, _sample_rate: f64) -> Result<Box<dyn AudioSink + Send>, String> {
        Ok(Box::new(NullSink))
    }

    fn stop(&mut self) {}
}

impl AudioSink for NullSink {
    fn push(&mut self, _samples: &[i16]) {}
}
