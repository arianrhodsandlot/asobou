pub mod cpal;
pub mod null;

pub trait AudioSink: Send {
    fn push(&mut self, samples: &[i16]);
}

pub trait AudioBackend {
    fn preferred_sample_rate(&mut self) -> Result<Option<u32>, String>;
    fn start(&mut self, sample_rate: f64) -> Result<Box<dyn AudioSink + Send>, String>;
    fn stop(&mut self);
}

pub fn create(name: &str) -> Box<dyn AudioBackend> {
    match name {
        "cpal" => Box::new(cpal::CpalBackend::new()),
        _ => Box::new(null::NullBackend),
    }
}
