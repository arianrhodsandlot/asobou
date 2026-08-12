use crate::audio::AudioSink;
use crate::renderer::Frame;
use libc::{c_char, c_int, c_uint, c_void};
use libloading::Library;
use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::{fmt, mem};

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
const RETRO_ENV_SET_FRAME_TIME_CALLBACK: c_uint = 21;
const RETRO_ENV_SET_GEOMETRY: c_uint = 37;
const RETRO_ENV_GET_LANGUAGE: c_uint = 39;
const RETRO_ENV_SET_CORE_OPTIONS_INTL: c_uint = 54;
const RETRO_ENV_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
const RETRO_ENV_GET_INPUT_BITMASKS: c_uint = 51 | 0x10000;
const RETRO_ENV_GET_CAN_DUPE: c_uint = 3;
const RETRO_ENV_GET_TARGET_SAMPLE_RATE: c_uint = 81 | 0x10000;
const RETRO_ENV_SET_SERIALIZATION_QUIRKS: c_uint = 87;

pub const RETRO_SERIALIZATION_QUIRK_INCOMPLETE: usize = 1;

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
type RetroFrameTimeFn = unsafe extern "C" fn(i64);

#[derive(Clone, Copy)]
#[repr(C)]
struct RetroFrameTimeCallback {
    callback: Option<RetroFrameTimeFn>,
    reference: i64,
}

struct CoreFunctions {
    retro_init: unsafe extern "C" fn(),
    retro_deinit: unsafe extern "C" fn(),
    retro_api_version: unsafe extern "C" fn() -> c_uint,
    retro_get_system_info: unsafe extern "C" fn(*mut RetroSystemInfo),
    retro_get_system_av_info: unsafe extern "C" fn(*mut RetroSystemAvInfo),
    retro_set_environment: unsafe extern "C" fn(RetroEnvironmentFn),
    retro_set_video_refresh: unsafe extern "C" fn(RetroVideoRefreshFn),
    retro_set_audio_sample: unsafe extern "C" fn(RetroAudioSampleFn),
    retro_set_audio_sample_batch: unsafe extern "C" fn(RetroAudioSampleBatchFn),
    retro_set_input_poll: unsafe extern "C" fn(RetroInputPollFn),
    retro_set_input_state: unsafe extern "C" fn(RetroInputStateFn),
    retro_load_game: unsafe extern "C" fn(*const RetroGameInfo) -> bool,
    retro_run: unsafe extern "C" fn(),
    retro_unload_game: unsafe extern "C" fn(),
    retro_serialize_size: Option<unsafe extern "C" fn() -> usize>,
    retro_serialize: Option<unsafe extern "C" fn(*mut c_void, usize) -> bool>,
    retro_unserialize: Option<unsafe extern "C" fn(*const c_void, usize) -> bool>,
}

pub struct CoreLibrary {
    #[cfg(not(test))]
    _lib: Library,
    #[cfg(test)]
    _lib: Option<Library>,
    functions: CoreFunctions,
}

pub struct LoadedGame {
    core: CoreLibrary,
}

#[derive(Debug)]
pub enum LoadGameError {
    AlreadyActive,
    Prepare(Box<dyn std::error::Error>),
    Rejected,
}

impl fmt::Display for LoadGameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyActive => write!(formatter, "another libretro core is already active"),
            Self::Prepare(error) => error.fmt(formatter),
            Self::Rejected => write!(formatter, "core rejected the ROM"),
        }
    }
}

impl std::error::Error for LoadGameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Prepare(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

static CORE_ACTIVE: AtomicBool = AtomicBool::new(false);

impl LoadedGame {
    pub fn supports_complete_serialization(&self) -> bool {
        serialization_quirks() & RETRO_SERIALIZATION_QUIRK_INCOMPLETE == 0
            && self.core.functions.retro_serialize.is_some()
            && self.core.functions.retro_unserialize.is_some()
            && self.state_size().is_some_and(|size| size > 0)
    }

    pub fn state_size(&self) -> Option<usize> {
        self.core
            .functions
            .retro_serialize_size
            .map(|size| unsafe { size() })
    }

    pub fn serialize_state(&self, data: &mut [u8]) -> bool {
        let Some(serialize) = self.core.functions.retro_serialize else {
            return false;
        };
        unsafe { serialize(data.as_mut_ptr() as *mut c_void, data.len()) }
    }

    pub fn unserialize_state(&self, data: &[u8]) -> bool {
        let Some(unserialize) = self.core.functions.retro_unserialize else {
            return false;
        };
        unsafe { unserialize(data.as_ptr() as *const c_void, data.len()) }
    }

    pub fn run_frame(&self) {
        invoke_frame_time_callback();
        unsafe { (self.core.functions.retro_run)() };
    }

    pub fn av_info(&self) -> RetroSystemAvInfo {
        let mut info = unsafe { mem::zeroed() };
        unsafe { (self.core.functions.retro_get_system_av_info)(&mut info) };
        info
    }

    pub fn set_joypad_buttons(&self, buttons: u16) {
        JOYPAD_BUTTONS.store(buttons, Ordering::Release);
    }

    pub fn set_video_capture_enabled(&self, enabled: bool) {
        VIDEO_CAPTURE_ENABLED.store(enabled, Ordering::Release);
    }

    pub fn set_audio_muted(&self, muted: bool) {
        AUDIO_MUTED.store(muted, Ordering::Release);
    }

    pub fn latest_frame(&self) -> Option<Arc<Frame>> {
        FRAME.lock().ok().and_then(|frame| frame.as_ref().cloned())
    }

    pub fn install_audio_sink(&self, sink: Box<dyn AudioSink + Send>) {
        if let Ok(mut audio) = AUDIO.lock() {
            *audio = Some(sink);
        }
    }

    pub fn clear_audio_sink(&self) {
        if let Ok(mut audio) = AUDIO.lock() {
            audio.take();
        }
    }
}

impl Drop for LoadedGame {
    fn drop(&mut self) {
        unsafe {
            (self.core.functions.retro_unload_game)();
            (self.core.functions.retro_deinit)();
        }
        reset_callback_state(None);
        CORE_ACTIVE.store(false, Ordering::Release);
    }
}

static FRAME: Mutex<Option<Arc<Frame>>> = Mutex::new(None);
static AUDIO: Mutex<Option<Box<dyn AudioSink + Send>>> = Mutex::new(None);
static PIXEL_FORMAT: AtomicU32 = AtomicU32::new(PixelFormat::ZeroRgb1555 as u32);
static TARGET_SAMPLE_RATE: AtomicU32 = AtomicU32::new(0);
static JOYPAD_BUTTONS: AtomicU16 = AtomicU16::new(0);
static VIDEO_CAPTURE_ENABLED: AtomicBool = AtomicBool::new(true);
static AUDIO_MUTED: AtomicBool = AtomicBool::new(false);
static SERIALIZATION_QUIRKS: AtomicUsize = AtomicUsize::new(0);
static FRAME_TIME_CALLBACK: Mutex<Option<RetroFrameTimeCallback>> = Mutex::new(None);

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

pub fn load_core(path: &Path) -> Result<CoreLibrary, Box<dyn std::error::Error>> {
    let lib = unsafe { Library::new(path)? };

    let functions = CoreFunctions {
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
        retro_serialize_size: unsafe { lib.get(b"retro_serialize_size").ok() }
            .map(|symbol| *symbol),
        retro_serialize: unsafe { lib.get(b"retro_serialize").ok() }.map(|symbol| *symbol),
        retro_unserialize: unsafe { lib.get(b"retro_unserialize").ok() }.map(|symbol| *symbol),
    };
    Ok(CoreLibrary {
        #[cfg(not(test))]
        _lib: lib,
        #[cfg(test)]
        _lib: Some(lib),
        functions,
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
        RETRO_ENV_SET_FRAME_TIME_CALLBACK => {
            if data.is_null() {
                return false;
            }
            let callback = unsafe { *(data as *const RetroFrameTimeCallback) };
            if callback.callback.is_none() || callback.reference <= 0 {
                return false;
            }
            if let Ok(mut stored) = FRAME_TIME_CALLBACK.lock() {
                *stored = Some(callback);
                true
            } else {
                false
            }
        }
        RETRO_ENV_SET_CONTROLLER_INFO => true,
        RETRO_ENV_GET_LOG_INTERFACE => false,
        RETRO_ENV_SET_SERIALIZATION_QUIRKS => {
            if !data.is_null() {
                SERIALIZATION_QUIRKS.store(unsafe { *(data as *const usize) }, Ordering::Relaxed);
            }
            true
        }
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
        RETRO_ENV_GET_CAN_DUPE => {
            if !data.is_null() {
                unsafe { *(data as *mut bool) = true };
            }
            true
        }
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
    if data.is_null() || !VIDEO_CAPTURE_ENABLED.load(Ordering::Acquire) {
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

fn reset_callback_state(target_sample_rate: Option<u32>) {
    PIXEL_FORMAT.store(PixelFormat::ZeroRgb1555 as u32, Ordering::Relaxed);
    TARGET_SAMPLE_RATE.store(target_sample_rate.unwrap_or(0), Ordering::Relaxed);
    JOYPAD_BUTTONS.store(0, Ordering::Release);
    VIDEO_CAPTURE_ENABLED.store(true, Ordering::Release);
    AUDIO_MUTED.store(false, Ordering::Release);
    SERIALIZATION_QUIRKS.store(0, Ordering::Relaxed);
    if let Ok(mut frame) = FRAME.lock() {
        frame.take();
    }
    if let Ok(mut audio) = AUDIO.lock() {
        audio.take();
    }
    if let Ok(mut callback) = FRAME_TIME_CALLBACK.lock() {
        callback.take();
    }
}

fn setup_callbacks(functions: &CoreFunctions) {
    unsafe {
        (functions.retro_set_environment)(env_callback);
        (functions.retro_set_video_refresh)(video_refresh);
        (functions.retro_set_audio_sample)(audio_sample);
        (functions.retro_set_audio_sample_batch)(audio_sample_batch);
        (functions.retro_set_input_poll)(input_poll);
        (functions.retro_set_input_state)(input_state);
    }
}

struct InitializationGuard<'a> {
    functions: &'a CoreFunctions,
    armed: bool,
}

impl Drop for InitializationGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            unsafe { (self.functions.retro_deinit)() };
            reset_callback_state(None);
            CORE_ACTIVE.store(false, Ordering::Release);
        }
    }
}

impl CoreLibrary {
    pub fn load_game(
        self,
        rom_path: &Path,
        target_sample_rate: Option<u32>,
    ) -> Result<LoadedGame, LoadGameError> {
        if CORE_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(LoadGameError::AlreadyActive);
        }
        reset_callback_state(target_sample_rate);
        setup_callbacks(&self.functions);
        unsafe {
            let _ = (self.functions.retro_api_version)();
            (self.functions.retro_init)();
        }
        let mut guard = InitializationGuard {
            functions: &self.functions,
            armed: true,
        };
        let mut system_info = unsafe { mem::zeroed() };
        unsafe { (self.functions.retro_get_system_info)(&mut system_info) };
        let loaded = load_rom(&self.functions, rom_path).map_err(LoadGameError::Prepare)?;
        if !loaded {
            return Err(LoadGameError::Rejected);
        }
        guard.armed = false;
        drop(guard);
        Ok(LoadedGame { core: self })
    }
}

fn invoke_frame_time_callback() {
    let callback = FRAME_TIME_CALLBACK.lock().ok().and_then(|stored| *stored);
    if let Some(RetroFrameTimeCallback {
        callback: Some(callback),
        reference,
    }) = callback
    {
        unsafe { callback(reference) };
    }
}

fn serialization_quirks() -> usize {
    SERIALIZATION_QUIRKS.load(Ordering::Relaxed)
}

fn load_rom(
    functions: &CoreFunctions,
    rom_path: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
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

    let ok = unsafe { (functions.retro_load_game)(&game_info) };
    if !path_c.is_null() {
        unsafe {
            drop(CString::from_raw(path_c as *mut c_char));
        }
    }
    drop(_temp_file);
    Ok(ok)
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    if AUDIO_MUTED.load(Ordering::Acquire) {
        return;
    }
    if let Ok(mut guard) = AUDIO.lock()
        && let Some(ref mut sink) = *guard
    {
        sink.push(&[left, right]);
    }
}

unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    if data.is_null() {
        return 0;
    }
    if AUDIO_MUTED.load(Ordering::Acquire) {
        return frames;
    }
    if let Ok(mut guard) = AUDIO.lock()
        && let Some(ref mut backend) = *guard
    {
        let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
        backend.push(samples);
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
    use super::{
        FRAME, FRAME_TIME_CALLBACK, PIXEL_FORMAT, PixelFormat, RETRO_ENV_SET_FRAME_TIME_CALLBACK,
        RetroFrameTimeCallback, VIDEO_CAPTURE_ENABLED, convert_frame, convert_row, env_callback,
        invoke_frame_time_callback, joypad_button_value, video_refresh,
    };
    use crate::renderer::Frame;
    use libc::c_void;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    static LIFECYCLE_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static CALLS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    static LOAD_SUCCEEDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

    unsafe extern "C" fn fake_init() {
        CALLS.lock().unwrap().push("init");
    }

    unsafe extern "C" fn fake_deinit() {
        CALLS.lock().unwrap().push("deinit");
    }

    unsafe extern "C" fn fake_api_version() -> libc::c_uint {
        1
    }

    unsafe extern "C" fn fake_get_system_info(_: *mut super::RetroSystemInfo) {}

    unsafe extern "C" fn fake_get_av_info(_: *mut super::RetroSystemAvInfo) {}

    unsafe extern "C" fn fake_set_environment(_: super::RetroEnvironmentFn) {}

    unsafe extern "C" fn fake_set_video(_: super::RetroVideoRefreshFn) {}

    unsafe extern "C" fn fake_set_audio(_: super::RetroAudioSampleFn) {}

    unsafe extern "C" fn fake_set_audio_batch(_: super::RetroAudioSampleBatchFn) {}

    unsafe extern "C" fn fake_set_input_poll(_: super::RetroInputPollFn) {}

    unsafe extern "C" fn fake_set_input_state(_: super::RetroInputStateFn) {}

    unsafe extern "C" fn fake_load(_: *const super::RetroGameInfo) -> bool {
        CALLS.lock().unwrap().push("load");
        LOAD_SUCCEEDS.load(Ordering::Relaxed)
    }

    unsafe extern "C" fn fake_run() {
        CALLS.lock().unwrap().push("run");
    }

    unsafe extern "C" fn fake_unload() {
        CALLS.lock().unwrap().push("unload");
    }

    fn fake_library() -> super::CoreLibrary {
        super::CoreLibrary {
            _lib: None,
            functions: super::CoreFunctions {
                retro_init: fake_init,
                retro_deinit: fake_deinit,
                retro_api_version: fake_api_version,
                retro_get_system_info: fake_get_system_info,
                retro_get_system_av_info: fake_get_av_info,
                retro_set_environment: fake_set_environment,
                retro_set_video_refresh: fake_set_video,
                retro_set_audio_sample: fake_set_audio,
                retro_set_audio_sample_batch: fake_set_audio_batch,
                retro_set_input_poll: fake_set_input_poll,
                retro_set_input_state: fake_set_input_state,
                retro_load_game: fake_load,
                retro_run: fake_run,
                retro_unload_game: fake_unload,
                retro_serialize_size: None,
                retro_serialize: None,
                retro_unserialize: None,
            },
        }
    }

    fn prepare_lifecycle_test(load_succeeds: bool) -> std::sync::MutexGuard<'static, ()> {
        let guard = LIFECYCLE_TEST.lock().unwrap();
        super::CORE_ACTIVE.store(false, Ordering::Release);
        CALLS.lock().unwrap().clear();
        LOAD_SUCCEEDS.store(load_succeeds, Ordering::Relaxed);
        guard
    }

    static FRAME_TIME_RECEIVED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

    unsafe extern "C" fn receive_frame_time(usec: i64) {
        FRAME_TIME_RECEIVED.store(usec, Ordering::Relaxed);
    }

    #[test]
    fn stores_and_invokes_frame_time_callback() {
        let _test = LIFECYCLE_TEST.lock().unwrap();
        let callback = RetroFrameTimeCallback {
            callback: Some(receive_frame_time),
            reference: 16_667,
        };

        let accepted = unsafe {
            env_callback(
                RETRO_ENV_SET_FRAME_TIME_CALLBACK,
                (&raw const callback).cast_mut().cast(),
            )
        };
        let stored = FRAME_TIME_CALLBACK.lock().unwrap().unwrap();

        assert!(accepted);
        assert_eq!(stored.reference, 16_667);
        assert!(stored.callback.is_some());

        FRAME_TIME_RECEIVED.store(0, Ordering::Relaxed);
        invoke_frame_time_callback();

        assert_eq!(FRAME_TIME_RECEIVED.load(Ordering::Relaxed), 16_667);
    }

    #[test]
    fn converts_xrgb8888_and_skips_row_padding() {
        let _test = LIFECYCLE_TEST.lock().unwrap();
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
        let _test = LIFECYCLE_TEST.lock().unwrap();
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
        let _test = LIFECYCLE_TEST.lock().unwrap();
        let input = [0x7c00_u16, 0x03e0, 0x001f]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect::<Vec<_>>();
        let mut output = Vec::new();

        convert_row(&input, PixelFormat::ZeroRgb1555, &mut output);

        assert_eq!(output, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn video_refresh_skips_conversion_when_capture_is_disabled() {
        let _test = LIFECYCLE_TEST.lock().unwrap();
        let previous_format = PIXEL_FORMAT.swap(PixelFormat::Xrgb8888 as u32, Ordering::Relaxed);
        let previous_frame = FRAME.lock().unwrap().replace(Arc::new(Frame {
            data: vec![1, 2, 3],
            width: 1,
            height: 1,
        }));
        let pixel = 0x0012_3456_u32;

        VIDEO_CAPTURE_ENABLED.store(false, Ordering::Release);
        unsafe {
            video_refresh((&pixel as *const u32).cast::<c_void>(), 1, 1, 4);
        }

        assert_eq!(FRAME.lock().unwrap().as_ref().unwrap().data, [1, 2, 3]);

        VIDEO_CAPTURE_ENABLED.store(true, Ordering::Release);
        unsafe {
            video_refresh((&pixel as *const u32).cast::<c_void>(), 1, 1, 4);
        }

        assert_eq!(
            FRAME.lock().unwrap().as_ref().unwrap().data,
            [0x12, 0x34, 0x56]
        );

        *FRAME.lock().unwrap() = previous_frame;
        PIXEL_FORMAT.store(previous_format, Ordering::Relaxed);
    }

    #[test]
    fn input_state_validates_libretro_query_fields() {
        let _test = LIFECYCLE_TEST.lock().unwrap();
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

    #[test]
    fn successful_construction_initializes_before_loading() {
        let _test = prepare_lifecycle_test(true);
        let rom = tempfile::NamedTempFile::new().unwrap();

        let game = fake_library().load_game(rom.path(), None).unwrap();

        assert_eq!(&*CALLS.lock().unwrap(), &["init", "load"]);
        drop(game);
    }

    #[test]
    fn dropping_loaded_game_unloads_before_deinitializing() {
        let _test = prepare_lifecycle_test(true);
        let rom = tempfile::NamedTempFile::new().unwrap();
        let game = fake_library().load_game(rom.path(), None).unwrap();

        drop(game);

        assert_eq!(
            &*CALLS.lock().unwrap(),
            &["init", "load", "unload", "deinit"]
        );
    }

    #[test]
    fn rejected_rom_deinitializes_without_unloading() {
        let _test = prepare_lifecycle_test(false);
        let rom = tempfile::NamedTempFile::new().unwrap();

        let result = fake_library().load_game(rom.path(), None);

        assert!(matches!(result, Err(super::LoadGameError::Rejected)));
        assert_eq!(&*CALLS.lock().unwrap(), &["init", "load", "deinit"]);
    }

    #[test]
    fn rom_preparation_failure_deinitializes_without_unloading() {
        let _test = prepare_lifecycle_test(true);
        let missing = std::path::Path::new("missing-lifecycle-test.rom");

        let result = fake_library().load_game(missing, None);

        assert!(matches!(result, Err(super::LoadGameError::Prepare(_))));
        assert_eq!(&*CALLS.lock().unwrap(), &["init", "deinit"]);
    }

    #[test]
    fn rom_preparation_error_preserves_the_source_message() {
        let error =
            super::LoadGameError::Prepare(Box::new(std::io::Error::other("ROM bytes unavailable")));

        assert_eq!(error.to_string(), "ROM bytes unavailable");
    }

    #[test]
    fn active_game_prevents_a_second_core_from_initializing() {
        let _test = prepare_lifecycle_test(true);
        let rom = tempfile::NamedTempFile::new().unwrap();
        let game = fake_library().load_game(rom.path(), None).unwrap();

        let result = fake_library().load_game(rom.path(), None);

        assert!(matches!(result, Err(super::LoadGameError::AlreadyActive)));
        assert_eq!(&*CALLS.lock().unwrap(), &["init", "load"]);
        drop(game);
    }

    #[test]
    fn frame_execution_is_owned_by_loaded_game() {
        let _test = prepare_lifecycle_test(true);
        let rom = tempfile::NamedTempFile::new().unwrap();
        let game = fake_library().load_game(rom.path(), None).unwrap();

        game.run_frame();

        assert_eq!(&*CALLS.lock().unwrap(), &["init", "load", "run"]);
        drop(game);
    }
}
