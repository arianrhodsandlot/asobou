use crate::audio::AudioSink;
use crate::render::Frame;
use libc::{c_char, c_int, c_uint, c_void};
use libloading::Library;
use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU16;
use std::sync::atomic::{AtomicU32, Ordering};

// Environment command constants
const RETRO_ENV_GET_SYSTEM_DIRECTORY: c_uint = 9;
const RETRO_ENV_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENV_SET_INPUT_DESCRIPTORS: c_uint = 11;
const RETRO_ENV_SET_KEYBOARD_CALLBACK: c_uint = 12;
const RETRO_ENV_SET_HW_RENDER: c_uint = 14;
const RETRO_ENV_GET_VARIABLE: c_uint = 15;
const RETRO_ENV_SET_VARIABLES: c_uint = 16;
const RETRO_ENV_GET_LOG_INTERFACE: c_uint = 27;
const RETRO_ENV_GET_SAVE_DIRECTORY: c_uint = 31;
const RETRO_ENV_SET_CONTROLLER_INFO: c_uint = 35;
const RETRO_ENV_GET_CORE_OPTIONS_VERSION: c_uint = 52;
const RETRO_ENV_GET_MESSAGE_INTERFACE_VERSION: c_uint = 59;
const RETRO_ENV_SET_PERFORMANCE_LEVEL: c_uint = 8;
const RETRO_ENV_GET_VARIABLE_UPDATE: c_uint = 17;
const RETRO_ENV_SET_GEOMETRY: c_uint = 37;
const RETRO_ENV_GET_LANGUAGE: c_uint = 39;
const RETRO_ENV_SET_CORE_OPTIONS_INTL: c_uint = 54;
const RETRO_ENV_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
const RETRO_ENV_GET_INPUT_BITMASKS: c_uint = 51 | 0x10000;
const RETRO_ENV_GET_TARGET_SAMPLE_RATE: c_uint = 81 | 0x10000;
const RETRO_ENV_SET_SERIALIZATION_QUIRKS: c_uint = 87;

#[repr(C)]
pub struct RetroSystemInfo {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: c_int,
    pub block_extract: c_int,
}

#[repr(C)]
pub struct RetroGameGeometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
pub struct RetroSystemTiming {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
pub struct RetroSystemAvInfo {
    pub geometry: RetroGameGeometry,
    pub timing: RetroSystemTiming,
}

#[repr(C)]
pub(crate) struct RetroGameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

type RetroEnvironmentFn = unsafe extern "C" fn(c_uint, *mut c_void) -> bool;
type RetroVideoRefreshFn = unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize);
type RetroAudioSampleFn = unsafe extern "C" fn(i16, i16);
type RetroAudioSampleBatchFn = unsafe extern "C" fn(*const i16, usize) -> usize;
type RetroInputPollFn = unsafe extern "C" fn();
type RetroInputStateFn = unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16;

pub struct Core {
    _lib: Library,
    pub retro_init: unsafe extern "C" fn(),
    pub retro_deinit: unsafe extern "C" fn(),
    pub retro_api_version: unsafe extern "C" fn() -> c_uint,
    pub retro_get_system_info: unsafe extern "C" fn(*mut RetroSystemInfo),
    pub retro_get_system_av_info: unsafe extern "C" fn(*mut RetroSystemAvInfo),
    pub retro_set_environment: unsafe extern "C" fn(RetroEnvironmentFn),
    pub retro_set_video_refresh: unsafe extern "C" fn(RetroVideoRefreshFn),
    pub retro_set_audio_sample: unsafe extern "C" fn(RetroAudioSampleFn),
    pub retro_set_audio_sample_batch: unsafe extern "C" fn(RetroAudioSampleBatchFn),
    pub retro_set_input_poll: unsafe extern "C" fn(RetroInputPollFn),
    pub retro_set_input_state: unsafe extern "C" fn(RetroInputStateFn),
    pub retro_load_game: unsafe extern "C" fn(*const RetroGameInfo) -> bool,
    pub retro_run: unsafe extern "C" fn(),
    pub retro_unload_game: unsafe extern "C" fn(),
}

pub static FRAME: Mutex<Option<Arc<Frame>>> = Mutex::new(None);
pub static AUDIO: Mutex<Option<Box<dyn AudioSink + Send>>> = Mutex::new(None);
static PIXEL_FORMAT: AtomicU32 = AtomicU32::new(PixelFormat::ZeroRgb1555 as u32);
static TARGET_SAMPLE_RATE: AtomicU32 = AtomicU32::new(0);
static JOYPAD_BUTTONS: AtomicU16 = AtomicU16::new(0);

#[derive(Clone, Copy)]
#[repr(u32)]
enum PixelFormat {
    ZeroRgb1555 = 0,
    Xrgb8888 = 1,
    Rgb565 = 2,
}

impl PixelFormat {
    fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::ZeroRgb1555),
            1 => Some(Self::Xrgb8888),
            2 => Some(Self::Rgb565),
            _ => None,
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Xrgb8888 => 4,
            Self::ZeroRgb1555 | Self::Rgb565 => 2,
        }
    }
}

pub unsafe fn load_core(path: &Path) -> Result<Core, Box<dyn std::error::Error>> {
    let lib = unsafe { Library::new(path)? };

    Ok(Core {
        retro_init: *unsafe { lib.get(b"retro_init")? },
        retro_deinit: *unsafe { lib.get(b"retro_deinit")? },
        retro_api_version: *unsafe { lib.get(b"retro_api_version")? },
        retro_get_system_info: *unsafe { lib.get(b"retro_get_system_info")? },
        retro_get_system_av_info: *unsafe { lib.get(b"retro_get_system_av_info")? },
        retro_set_environment: *unsafe { lib.get(b"retro_set_environment")? },
        retro_set_video_refresh: *unsafe { lib.get(b"retro_set_video_refresh")? },
        retro_set_audio_sample: *unsafe { lib.get(b"retro_set_audio_sample")? },
        retro_set_audio_sample_batch: *unsafe { lib.get(b"retro_set_audio_sample_batch")? },
        retro_set_input_poll: *unsafe { lib.get(b"retro_set_input_poll")? },
        retro_set_input_state: *unsafe { lib.get(b"retro_set_input_state")? },
        retro_load_game: *unsafe {
            lib.get::<unsafe extern "C" fn(*const RetroGameInfo) -> bool>(b"retro_load_game")?
        },
        retro_run: *unsafe { lib.get(b"retro_run")? },
        retro_unload_game: *unsafe { lib.get(b"retro_unload_game")? },
        _lib: lib,
    })
}

unsafe extern "C" fn env_callback(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        RETRO_ENV_GET_CORE_OPTIONS_VERSION => {
            if !data.is_null() {
                unsafe { *(data as *mut c_uint) = 1 };
            }
            true
        }
        RETRO_ENV_SET_VARIABLES => true,
        RETRO_ENV_SET_CONTROLLER_INFO => true,
        RETRO_ENV_GET_LOG_INTERFACE => false,
        RETRO_ENV_SET_SERIALIZATION_QUIRKS => true,
        RETRO_ENV_GET_MESSAGE_INTERFACE_VERSION => {
            if !data.is_null() {
                unsafe { *(data as *mut c_uint) = 1 };
            }
            true
        }
        RETRO_ENV_SET_PERFORMANCE_LEVEL => true,
        RETRO_ENV_SET_INPUT_DESCRIPTORS => true,
        RETRO_ENV_SET_KEYBOARD_CALLBACK => true,
        RETRO_ENV_SET_HW_RENDER => false,
        RETRO_ENV_GET_VARIABLE => false,
        RETRO_ENV_GET_SYSTEM_DIRECTORY => {
            if !data.is_null() {
                let path = CString::new(".").unwrap();
                unsafe { *(data as *mut *const c_char) = path.into_raw() };
            }
            true
        }
        RETRO_ENV_GET_SAVE_DIRECTORY => {
            if !data.is_null() {
                let path = CString::new(".").unwrap();
                unsafe { *(data as *mut *const c_char) = path.into_raw() };
            }
            true
        }
        RETRO_ENV_SET_PIXEL_FORMAT => {
            if data.is_null() {
                return false;
            }
            let value = unsafe { *(data as *const c_uint) };
            if let Some(format) = PixelFormat::from_raw(value) {
                PIXEL_FORMAT.store(format as u32, Ordering::Relaxed);
                true
            } else {
                false
            }
        }
        RETRO_ENV_GET_VARIABLE_UPDATE => false,
        RETRO_ENV_SET_GEOMETRY => true,
        RETRO_ENV_SET_CORE_OPTIONS_DISPLAY => true,
        RETRO_ENV_GET_INPUT_BITMASKS => false,
        RETRO_ENV_GET_TARGET_SAMPLE_RATE => {
            let sample_rate = TARGET_SAMPLE_RATE.load(Ordering::Relaxed);
            if data.is_null() || sample_rate == 0 {
                false
            } else {
                unsafe { *(data as *mut c_uint) = sample_rate };
                true
            }
        }
        RETRO_ENV_GET_LANGUAGE => {
            if !data.is_null() {
                unsafe { *(data as *mut c_uint) = 0 };
            }
            true
        }
        RETRO_ENV_SET_CORE_OPTIONS_INTL => true,
        _ => false,
    }
}

unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    if data.is_null() {
        return;
    }

    let Some(format) = PixelFormat::from_raw(PIXEL_FORMAT.load(Ordering::Relaxed)) else {
        return;
    };
    let Some(pixels) = (unsafe {
        convert_frame(
            data as *const u8,
            width as usize,
            height as usize,
            pitch,
            format,
        )
    }) else {
        return;
    };

    if let Ok(mut frame) = FRAME.lock() {
        *frame = Some(Arc::new(Frame {
            data: pixels,
            width,
            height,
        }));
    }
}

unsafe fn convert_frame(
    data: *const u8,
    width: usize,
    height: usize,
    pitch: usize,
    format: PixelFormat,
) -> Option<Vec<u8>> {
    let row_len = width.checked_mul(format.bytes_per_pixel())?;
    if pitch < row_len {
        return None;
    }
    let capacity = width.checked_mul(height)?.checked_mul(3)?;
    let mut pixels = Vec::with_capacity(capacity);

    for y in 0..height {
        let offset = y.checked_mul(pitch)?;
        let row = unsafe { std::slice::from_raw_parts(data.add(offset), row_len) };
        convert_row(row, format, &mut pixels);
    }

    Some(pixels)
}

fn convert_row(row: &[u8], format: PixelFormat, output: &mut Vec<u8>) {
    match format {
        PixelFormat::ZeroRgb1555 => {
            for pixel in row.chunks_exact(2) {
                let value = u16::from_ne_bytes([pixel[0], pixel[1]]);
                output.extend_from_slice(&[
                    expand_5((value >> 10) as u8),
                    expand_5((value >> 5) as u8),
                    expand_5(value as u8),
                ]);
            }
        }
        PixelFormat::Xrgb8888 => {
            for pixel in row.chunks_exact(4) {
                let value = u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
                output.extend_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8]);
            }
        }
        PixelFormat::Rgb565 => {
            for pixel in row.chunks_exact(2) {
                let value = u16::from_ne_bytes([pixel[0], pixel[1]]);
                output.extend_from_slice(&[
                    expand_5((value >> 11) as u8),
                    expand_6((value >> 5) as u8),
                    expand_5(value as u8),
                ]);
            }
        }
    }
}

fn expand_5(value: u8) -> u8 {
    let value = value & 0x1f;
    (value << 3) | (value >> 2)
}

fn expand_6(value: u8) -> u8 {
    let value = value & 0x3f;
    (value << 2) | (value >> 4)
}

pub unsafe fn setup_callbacks(core: &Core) {
    unsafe {
        (core.retro_set_environment)(env_callback);
        (core.retro_set_video_refresh)(video_refresh);
        (core.retro_set_audio_sample)(audio_sample);
        (core.retro_set_audio_sample_batch)(audio_sample_batch);
        (core.retro_set_input_poll)(input_poll);
        (core.retro_set_input_state)(input_state);
    }
}

pub fn set_target_sample_rate(sample_rate: Option<u32>) {
    TARGET_SAMPLE_RATE.store(sample_rate.unwrap_or(0), Ordering::Relaxed);
}

pub fn set_joypad_buttons(buttons: u16) {
    JOYPAD_BUTTONS.store(buttons, Ordering::Release);
}

pub unsafe fn load_rom(core: &Core, rom_path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let rom_data;
    let path_c;
    let mut _temp_file = None;

    if rom_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let file = std::fs::File::open(rom_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut first = archive.by_index(0)?;
        let name = first.name().to_string();
        let ext = std::path::Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("rom");
        let mut data = Vec::with_capacity(first.size() as usize);
        std::io::Read::read_to_end(&mut first, &mut data)?;

        let mut tmp = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()?;
        std::io::Write::write_all(&mut tmp, &data)?;
        rom_data = data;
        path_c = CString::new(tmp.path().to_string_lossy().as_bytes())?.into_raw();
        _temp_file = Some(tmp);
    } else {
        rom_data = std::fs::read(rom_path)?;
        path_c = CString::new(rom_path.to_string_lossy().as_bytes())?.into_raw();
    }

    let game_info = RetroGameInfo {
        path: path_c,
        data: rom_data.as_ptr() as *const c_void,
        size: rom_data.len(),
        meta: std::ptr::null(),
    };

    let ok = unsafe { (core.retro_load_game)(&game_info) };
    if !path_c.is_null() {
        unsafe { drop(CString::from_raw(path_c as *mut c_char)); }
    }
    drop(_temp_file);
    Ok(ok)
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    if let Ok(mut guard) = AUDIO.lock() {
        if let Some(ref mut sink) = *guard {
            sink.push(&[left, right]);
        }
    }
}

unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    if data.is_null() {
        return 0;
    }
    if let Ok(mut guard) = AUDIO.lock() {
        if let Some(ref mut backend) = *guard {
            let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
            backend.push(samples);
        }
    }
    frames
}

unsafe extern "C" fn input_poll() {}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
    joypad_button_value(
        JOYPAD_BUTTONS.load(Ordering::Acquire),
        port,
        device,
        index,
        id,
    )
}

fn joypad_button_value(buttons: u16, port: u32, device: u32, index: u32, id: u32) -> i16 {
    if port != 0
        || device != crate::input::RETRO_DEVICE_JOYPAD
        || index != 0
        || id >= crate::input::JOYPAD_BUTTON_COUNT as u32
    {
        return 0;
    }
    i16::from(buttons & (1 << id) != 0)
}

#[cfg(test)]
mod tests {
    use super::{PixelFormat, convert_frame, convert_row, joypad_button_value};

    #[test]
    fn converts_xrgb8888_and_skips_row_padding() {
        let mut input = Vec::new();
        for row in [
            [0x0012_3456_u32, 0x0078_9abc],
            [0x00de_adbe_u32, 0x00ef_1020],
        ] {
            for pixel in row {
                input.extend_from_slice(&pixel.to_ne_bytes());
            }
            input.extend_from_slice(&[0xaa; 4]);
        }

        let output =
            unsafe { convert_frame(input.as_ptr(), 2, 2, 12, PixelFormat::Xrgb8888).unwrap() };

        assert_eq!(
            output,
            [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xad, 0xbe, 0xef, 0x10, 0x20
            ]
        );
    }

    #[test]
    fn converts_rgb565() {
        let input = [0xf800_u16, 0x07e0, 0x001f]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect::<Vec<_>>();
        let mut output = Vec::new();

        convert_row(&input, PixelFormat::Rgb565, &mut output);

        assert_eq!(output, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn converts_zero_rgb1555() {
        let input = [0x7c00_u16, 0x03e0, 0x001f]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect::<Vec<_>>();
        let mut output = Vec::new();

        convert_row(&input, PixelFormat::ZeroRgb1555, &mut output);

        assert_eq!(output, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn input_state_validates_libretro_query_fields() {
        let buttons = 1 << crate::input::BUTTON_A;

        assert_eq!(
            joypad_button_value(
                buttons,
                0,
                crate::input::RETRO_DEVICE_JOYPAD,
                0,
                crate::input::BUTTON_A as u32
            ),
            1
        );
        assert_eq!(
            joypad_button_value(
                buttons,
                0,
                crate::input::RETRO_DEVICE_JOYPAD,
                0,
                crate::input::JOYPAD_BUTTON_COUNT as u32
            ),
            0
        );
        assert_eq!(
            joypad_button_value(
                buttons,
                1,
                crate::input::RETRO_DEVICE_JOYPAD,
                0,
                crate::input::BUTTON_A as u32
            ),
            0
        );
        assert_eq!(
            joypad_button_value(buttons, 0, 0, 0, crate::input::BUTTON_A as u32),
            0
        );
        assert_eq!(
            joypad_button_value(
                buttons,
                0,
                crate::input::RETRO_DEVICE_JOYPAD,
                1,
                crate::input::BUTTON_A as u32
            ),
            0
        );
    }
}
