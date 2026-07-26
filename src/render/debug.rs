use super::Frame;
use super::Renderer;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INTERVAL: Duration = Duration::from_secs(1);

pub struct DebugRenderer {
    dir: PathBuf,
    base_name: String,
    last_save: SystemTime,
}

impl DebugRenderer {
    pub fn new(rom_stem: &str) -> Self {
        let dir = PathBuf::from("debug");
        std::fs::create_dir_all(&dir).ok();
        Self {
            dir,
            base_name: rom_stem.to_string(),
            last_save: UNIX_EPOCH,
        }
    }

    fn save_frame(&self, frame: &Frame) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let filename = format!("{}-{}.png", self.base_name, iso_format(now));
        let path = self.dir.join(&filename);

        if let Some(img) = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
            frame.width,
            frame.height,
            frame.data.clone(),
        ) {
            img.save(&path).ok();
        }
    }
}

impl Renderer for DebugRenderer {
    fn setup(&mut self, _src_width: u32, _src_height: u32) {}

    fn render(&mut self, frame: &Frame, _out: &mut dyn io::Write) -> io::Result<()> {
        let now = SystemTime::now();
        if now.duration_since(self.last_save).unwrap_or(Duration::ZERO) >= INTERVAL {
            self.last_save = now;
            self.save_frame(frame);
        }
        Ok(())
    }

    fn cleanup(&mut self) {}
}

fn iso_format(unix_secs: u64) -> String {
    let secs_per_day: u64 = 86400;
    let days = unix_secs / secs_per_day;
    let time_of_day = unix_secs % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01, compute year/month/day
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let month_days: [u64; 12] = [
        31, if is_leap(y) { 29 } else { 28 }, 31, 30, 31, 30,
        31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}",
        y,
        m + 1,
        remaining + 1,
        hours,
        minutes,
        seconds
    )
}

fn is_leap(year: u64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
