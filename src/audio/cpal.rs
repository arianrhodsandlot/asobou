use super::{AudioBackend, AudioSink};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use ringbuf::HeapRb;
use ringbuf::traits::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CpalBackend {
    device: Option<cpal::Device>,
    config: Option<cpal::SupportedStreamConfig>,
    stream: Option<cpal::Stream>,
    dropped_samples: Arc<AtomicUsize>,
}

struct CpalSink {
    producer: ringbuf::HeapProd<i16>,
    source_rate: f64,
    target_rate: f64,
    next_output_position: f64,
    input_position: u64,
    previous: [i16; 2],
    has_previous: bool,
    dropped_samples: Arc<AtomicUsize>,
}

impl CpalBackend {
    pub fn new() -> Self {
        Self {
            device: None,
            config: None,
            stream: None,
            dropped_samples: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn prepare(&mut self) -> Result<(), String> {
        if self.device.is_some() && self.config.is_some() {
            return Ok(());
        }

        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("failed to query default output configuration: {e}"))?;
        self.device = Some(device);
        self.config = Some(config);
        Ok(())
    }
}

impl AudioBackend for CpalBackend {
    fn preferred_sample_rate(&mut self) -> Result<Option<u32>, String> {
        self.prepare()?;
        Ok(self.config.as_ref().map(|config| config.sample_rate()))
    }

    fn start(&mut self, source_rate: f64) -> Result<Box<dyn AudioSink + Send>, String> {
        self.prepare()?;
        let device = self.device.as_ref().unwrap();
        let supported_config = self.config.as_ref().unwrap();
        let sample_format = supported_config.sample_format();
        let config: cpal::StreamConfig = supported_config.clone().into();
        let channels = config.channels as usize;
        let target_rate = config.sample_rate;
        let ring = HeapRb::<i16>::new(target_rate as usize * 2 / 10);
        let (prod, cons) = ring.split();
        let error_callback = |err| eprintln!("audio stream error: {err}");
        let stream = match sample_format {
            SampleFormat::I8 => build_stream::<i8>(device, config, channels, cons, error_callback),
            SampleFormat::I16 => {
                build_stream::<i16>(device, config, channels, cons, error_callback)
            }
            SampleFormat::I32 => {
                build_stream::<i32>(device, config, channels, cons, error_callback)
            }
            SampleFormat::I64 => {
                build_stream::<i64>(device, config, channels, cons, error_callback)
            }
            SampleFormat::U8 => build_stream::<u8>(device, config, channels, cons, error_callback),
            SampleFormat::U16 => {
                build_stream::<u16>(device, config, channels, cons, error_callback)
            }
            SampleFormat::U32 => {
                build_stream::<u32>(device, config, channels, cons, error_callback)
            }
            SampleFormat::U64 => {
                build_stream::<u64>(device, config, channels, cons, error_callback)
            }
            SampleFormat::F32 => {
                build_stream::<f32>(device, config, channels, cons, error_callback)
            }
            SampleFormat::F64 => {
                build_stream::<f64>(device, config, channels, cons, error_callback)
            }
            format => return Err(format!("unsupported output sample format: {format}")),
        }
        .map_err(|e| format!("failed to open audio stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("failed to start stream: {e}"))?;
        self.stream = Some(stream);
        Ok(Box::new(CpalSink {
            producer: prod,
            source_rate,
            target_rate: target_rate as f64,
            next_output_position: 0.0,
            input_position: 0,
            previous: [0, 0],
            has_previous: false,
            dropped_samples: Arc::clone(&self.dropped_samples),
        }))
    }

    fn stop(&mut self) {
        self.stream.take();
        let dropped = self.dropped_samples.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            eprintln!("Audio dropped {dropped} samples because the output buffer was full");
        }
    }
}

impl AudioSink for CpalSink {
    fn push(&mut self, samples: &[i16]) {
        for frame in samples.chunks_exact(2) {
            let current = [frame[0], frame[1]];
            if !self.has_previous {
                self.previous = current;
                self.has_previous = true;
            }

            let position = self.input_position as f64;
            while self.next_output_position <= position {
                let fraction = if self.input_position == 0 {
                    1.0
                } else {
                    self.next_output_position - (position - 1.0)
                }
                .clamp(0.0, 1.0);
                let output = [
                    interpolate(self.previous[0], current[0], fraction),
                    interpolate(self.previous[1], current[1], fraction),
                ];
                let written = self.producer.push_slice(&output);
                if written < output.len() {
                    self.dropped_samples
                        .fetch_add(output.len() - written, Ordering::Relaxed);
                }
                self.next_output_position += self.source_rate / self.target_rate;
            }

            self.previous = current;
            self.input_position += 1;
        }
    }
}

fn interpolate(start: i16, end: i16, fraction: f64) -> i16 {
    (start as f64 + (end as f64 - start as f64) * fraction).round() as i16
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut consumer: ringbuf::HeapCons<i16>,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + Sample + FromSample<i16>,
{
    device.build_output_stream(
        config,
        move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
            for frame in output.chunks_mut(channels) {
                let left = consumer.try_pop().unwrap_or(0);
                let right = consumer.try_pop().unwrap_or(0);
                if channels == 1 {
                    frame[0] = T::from_sample(
                        ((left as i32 + right as i32) / 2).clamp(i16::MIN as i32, i16::MAX as i32)
                            as i16,
                    );
                } else {
                    frame[0] = T::from_sample(left);
                    frame[1] = T::from_sample(right);
                    for sample in &mut frame[2..] {
                        *sample = T::EQUILIBRIUM;
                    }
                }
            }
        },
        error_callback,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::AudioSink;
    use super::{CpalSink, interpolate};
    use ringbuf::HeapCons;
    use ringbuf::HeapRb;
    use ringbuf::traits::Consumer;
    use ringbuf::traits::Split;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn interpolate_endpoints() {
        assert_eq!(interpolate(100, 200, 0.0), 100);
        assert_eq!(interpolate(100, 200, 1.0), 200);
    }

    #[test]
    fn interpolate_midpoints_and_rounding() {
        assert_eq!(interpolate(0, 100, 0.5), 50);
        assert_eq!(interpolate(0, 10, 0.25), 3);
        assert_eq!(interpolate(100, 0, 0.25), 75);
        assert_eq!(interpolate(-100, 100, 0.5), 0);
    }

    fn sink_with(source_rate: f64, target_rate: f64) -> (CpalSink, HeapCons<i16>) {
        let rb = HeapRb::<i16>::new(1024);
        let (prod, cons) = rb.split();
        let sink = CpalSink {
            producer: prod,
            source_rate,
            target_rate,
            next_output_position: 0.0,
            input_position: 0,
            previous: [0, 0],
            has_previous: false,
            dropped_samples: Arc::new(AtomicUsize::new(0)),
        };
        (sink, cons)
    }

    fn drain(cons: &mut HeapCons<i16>) -> Vec<i16> {
        let mut out = Vec::new();
        while let Some(s) = cons.try_pop() {
            out.push(s);
        }
        out
    }

    #[test]
    fn equal_rates_pass_samples_through_unchanged() {
        let (mut sink, mut cons) = sink_with(44100.0, 44100.0);
        sink.push(&[100, -100, 200, -200]);
        assert_eq!(drain(&mut cons), vec![100, -100, 200, -200]);
    }

    #[test]
    fn upsample_interpolates_between_input_frames() {
        let (mut sink, mut cons) = sink_with(44100.0, 48000.0);
        sink.push(&[0, 0, 1000, 1000]);
        assert_eq!(drain(&mut cons), vec![0, 0, 919, 919]);
    }

    #[test]
    fn downsample_skips_output_positions() {
        let (mut sink, mut cons) = sink_with(48000.0, 44100.0);
        sink.push(&[0, 0, 1000, 1000, 2000, 2000]);
        assert_eq!(drain(&mut cons), vec![0, 0, 1088, 1088]);
    }
}
