//! Click sounds without native audio linking.
//!
//! Mirrors `ClickSoundMixer` in `main.py`: short synthesized `key.wav`
//! (1350 Hz, 22 ms) and `error.wav` (260 Hz, 75 ms) at 22050 Hz mono 16-bit,
//! played through the first available backend:
//!
//! * Linux: `pw-cat --playback --raw ... -`, else `pw-play` / `paplay` / `aplay`
//! * macOS: `afplay`
//! * Windows: PowerShell `System.Media.SoundPlayer`
//!
//! A background thread mixes queued clicks FIFO (one onset per 128-frame
//! chunk, like the Python mixer) so rapid keystrokes never sound swapped.

use std::collections::VecDeque;
#[cfg(not(target_os = "windows"))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(target_os = "windows"))]
use std::process::Stdio;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

pub const RATE: u32 = 22050;

pub struct ClickPlayer {
    inner: Arc<Inner>,
    _thread: Option<JoinHandle<()>>,
}

struct Inner {
    queue: Mutex<VecDeque<QueuedClick>>,
    wake: Condvar,
    stop: Mutex<bool>,
    key_wav: PathBuf,
    error_wav: PathBuf,
    key_samples: Vec<i16>,
    error_samples: Vec<i16>,
    enabled: Mutex<bool>,
}

/// One click: decoded samples (Linux PipeWire mixer) plus which wav to
/// replay on backends that play whole files (Windows/macOS/ALSA).
#[derive(Debug, Clone)]
struct QueuedClick {
    samples: Vec<i16>,
    correct: bool,
}

/// Pick the wav for an event (pure helper, unit-tested).
fn event_wav<'a>(key_wav: &'a Path, error_wav: &'a Path, correct: bool) -> &'a Path {
    if correct {
        key_wav
    } else {
        error_wav
    }
}

impl ClickPlayer {
    pub fn new(sounds_dir: &Path) -> Self {
        let key_wav = sounds_dir.join("key.wav");
        let error_wav = sounds_dir.join("error.wav");
        // Decode once: per-keystroke disk reads stutter on some systems.
        let key_samples = read_wav_samples(&key_wav).unwrap_or_default();
        let error_samples = read_wav_samples(&error_wav).unwrap_or_default();
        let inner = Arc::new(Inner {
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            stop: Mutex::new(false),
            key_wav,
            error_wav,
            key_samples,
            error_samples,
            enabled: Mutex::new(true),
        });
        let worker = Arc::clone(&inner);
        let handle = std::thread::Builder::new()
            .name("bit-typing-audio".into())
            .spawn(move || run_loop(worker))
            .ok();
        Self { inner, _thread: handle }
    }

    pub fn set_enabled(&self, enabled: bool) {
        *self.inner.enabled.lock().unwrap() = enabled;
    }

    pub fn play(&self, correct: bool) {
        if !*self.inner.enabled.lock().unwrap() {
            return;
        }
        let samples = if correct {
            self.inner.key_samples.clone()
        } else {
            self.inner.error_samples.clone()
        };
        if samples.is_empty() {
            return;
        }
        let mut queue = self.inner.queue.lock().unwrap();
        push_capped(&mut queue, QueuedClick { samples, correct }, 8);
        drop(queue);
        self.inner.wake.notify_one();
    }
}

/// Bounded queue: fresh clicks evict stale ones, so a stalled backend can
/// never pile up minutes of late sounds — feedback stays real-time.
fn push_capped<T>(queue: &mut VecDeque<T>, item: T, cap: usize) {
    while queue.len() >= cap.max(1) {
        queue.pop_front();
    }
    queue.push_back(item);
}

impl Drop for ClickPlayer {
    fn drop(&mut self) {
        *self.inner.stop.lock().unwrap() = true;
        self.inner.wake.notify_all();
    }
}

fn run_loop(inner: Arc<Inner>) {
    // Windows keeps one persistent wave-out stream: the open stream keeps
    // sleepy (USB/Bluetooth) endpoints awake so short clicks are never
    // swallowed cold, and overlapping clicks mix instead of cutting out.
    #[cfg(target_os = "windows")]
    {
        run_wave_out(inner);
        return;
    }
    // Prefer a streaming raw mixer when pw-cat exists (lowest latency,
    // exact Python parity). Otherwise play whole files per event.
    #[cfg(not(target_os = "windows"))]
    if pw_cat_available() {
        run_pw_cat_stream(inner);
    } else {
        run_per_event(inner);
    }
}

#[cfg(not(target_os = "windows"))]
fn pw_cat_available() -> bool {
    which("pw-cat").is_some()
}

#[cfg(not(target_os = "windows"))]
fn run_pw_cat_stream(inner: Arc<Inner>) {
    let mut child = Command::new("pw-cat")
        .args([
            "--playback", "--raw", "--rate", &RATE.to_string(), "--channels", "1",
            "--format", "s16", "--latency", "20ms", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok();
    let mut stdin = child.as_mut().and_then(|c| c.stdin.take());
    let mut active: Vec<(Vec<i16>, usize)> = Vec::new();
    let mut next_chunk = std::time::Instant::now();
    loop {
        if *inner.stop.lock().unwrap() {
            break;
        }
        // One onset per chunk (FIFO parity with Python).
        let next = inner.queue.lock().unwrap().pop_front();
        if let Some(click) = next {
            active.push((click.samples, 0));
        }
        const CHUNK: usize = 128;
        let mut mixed = [0i32; CHUNK];
        active.retain_mut(|(samples, pos)| {
            let count = (*pos + CHUNK).min(samples.len()) - *pos;
            for i in 0..count {
                mixed[i] += samples[*pos + i] as i32;
            }
            *pos += count;
            *pos < samples.len()
        });
        let mut pcm = Vec::with_capacity(CHUNK * 2);
        for v in mixed {
            pcm.extend_from_slice(&v.clamp(-32768, 32767).to_le_bytes());
        }
        if let Some(stdin) = stdin.as_mut() {
            if stdin.write_all(&pcm).is_err() {
                // Backend died; fall back to per-event playback.
                let _ = stdin;
                if let Some(mut child) = child.take() {
                    let _ = child.kill();
                }
                run_per_event(inner);
                return;
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }
        next_chunk += std::time::Duration::from_secs_f64(CHUNK as f64 / RATE as f64);
        let now = std::time::Instant::now();
        if next_chunk > now {
            std::thread::sleep(next_chunk - now);
        } else {
            next_chunk = now;
        }
        if active.is_empty() {
            // Park until the next click to avoid busy zero-fill.
            let guard = inner.queue.lock().unwrap();
            if guard.is_empty() {
                let _ = inner
                    .wake
                    .wait_timeout(guard, std::time::Duration::from_millis(50))
                    .unwrap();
            }
        }
    }
    if let Some(mut child) = child {
        let _ = child.kill();
    }
}

/// Take the next click, blocking; drains anything stale behind it so only
/// the freshest click ever plays (shared by every backend loop).
fn take_newest(inner: &Inner) -> Option<QueuedClick> {
    let mut guard = inner.queue.lock().unwrap();
    let first = loop {
        if *inner.stop.lock().unwrap() {
            return None;
        }
        if let Some(click) = guard.pop_front() {
            break click;
        }
        guard = inner.wake.wait(guard).unwrap();
    };
    Some(keep_newest(first, &mut guard))
}

#[cfg(not(target_os = "windows"))]
fn run_per_event(inner: Arc<Inner>) {
    while let Some(click) = take_newest(&inner) {
        play_event_best_effort(&inner, &click);
    }
}

/// Drain stale queued clicks, keeping only the newest for playback.
fn keep_newest<T>(mut current: T, queue: &mut VecDeque<T>) -> T {
    while let Some(newer) = queue.pop_front() {
        current = newer;
    }
    current
}

/// Win32 wave-out playback: one stream held open for the whole session.
/// An open stream keeps sleepy (USB/Bluetooth) endpoints awake, so short
/// clicks are never swallowed cold, and overlapping clicks mix instead of
/// cutting each other out (single-voice `PlaySound` cannot do either).
#[cfg(target_os = "windows")]
mod win32 {
    #[allow(non_snake_case)]
    #[link(name = "winmm")]
    extern "system" {
        pub fn waveOutOpen(
            phwo: *mut *mut std::ffi::c_void,
            uDeviceID: u32,
            pwfx: *const WaveFormatEx,
            dwCallback: usize,
            dwInstance: usize,
            fdwOpen: u32,
        ) -> u32;
        pub fn waveOutPrepareHeader(
            hwo: *const std::ffi::c_void,
            pwh: *mut WaveHdr,
            cbwh: u32,
        ) -> u32;
        pub fn waveOutWrite(
            hwo: *const std::ffi::c_void,
            pwh: *mut WaveHdr,
            cbwh: u32,
        ) -> u32;
        pub fn waveOutUnprepareHeader(
            hwo: *const std::ffi::c_void,
            pwh: *mut WaveHdr,
            cbwh: u32,
        ) -> u32;
        pub fn waveOutReset(hwo: *const std::ffi::c_void) -> u32;
        pub fn waveOutClose(hwo: *const std::ffi::c_void) -> u32;
    }

    pub const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
    pub const WAVE_FORMAT_PCM: u16 = 1;
    pub const CALLBACK_NULL: u32 = 0;
    pub const MMSYSERR_NOERROR: u32 = 0;

    /// 22050 Hz mono 16-bit PCM descriptor (20 bytes on the wire).
    /// Field names follow the Win32 API spelling.
    #[allow(non_snake_case)]
    #[repr(C)]
    pub struct WaveFormatEx {
        pub wFormatTag: u16,
        pub nChannels: u16,
        pub nSamplesPerSec: u32,
        pub nAvgBytesPerSec: u32,
        pub nBlockAlign: u16,
        pub wBitsPerSample: u16,
        pub cbSize: u16,
    }

    /// Wave header (48 bytes on 64-bit). The sample buffer lives in a
    /// separate `Box<[i16]>`; both heap addresses stay stable for the
    /// header's whole lifetime. Field names follow the Win32 API spelling.
    #[allow(non_snake_case)]
    #[repr(C)]
    pub struct WaveHdr {
        pub lpData: *mut u8,
        pub dwBufferLength: u32,
        pub dwBytesRecorded: u32,
        pub dwUser: usize,
        pub dwFlags: u32,
        pub dwLoops: u32,
        pub lpNext: *mut u8,
        pub reserved: usize,
    }

    /// One in-flight buffer: dropping it after a successful unprepare frees
    /// both allocations. Boxes never move on the heap, so driver-held
    /// pointers stay valid.
    pub struct Inflight {
        pub hdr: Box<WaveHdr>,
        // Held alive for the header's lifetime (lpData points into it).
        #[allow(dead_code)]
        pub buf: Box<[i16]>,
    }

    /// Open wave device. `None` when no audio device exists.
    pub struct WaveDevice {
        handle: *mut std::ffi::c_void,
        inflight: Vec<Inflight>,
    }

    // The handle is only touched on the audio thread.
    unsafe impl Send for WaveDevice {}

    impl WaveDevice {
        pub fn open() -> Option<Self> {
            let fmt = WaveFormatEx {
                wFormatTag: WAVE_FORMAT_PCM,
                nChannels: 1,
                nSamplesPerSec: super::RATE,
                nAvgBytesPerSec: super::RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
            // SAFETY: plain-old-data structs, valid params; winmm.dll is a
            // core Windows system library. Synchronous open, no callback.
            let rc = unsafe {
                waveOutOpen(
                    &mut handle,
                    WAVE_MAPPER,
                    &fmt,
                    0,
                    0,
                    CALLBACK_NULL,
                )
            };
            if rc != MMSYSERR_NOERROR || handle.is_null() {
                return None;
            }
            Some(Self { handle, inflight: Vec::new() })
        }

        /// Queue samples for async mixed playback. Never blocks.
        /// Returns false when the device rejected the buffer (unplugged?).
        pub fn play(&mut self, samples: &[i16]) -> bool {
            if samples.is_empty() {
                return true;
            }
            self.sweep();
            let mut buf: Box<[i16]> = samples.to_vec().into_boxed_slice();
            let mut hdr = Box::new(WaveHdr {
                lpData: buf.as_mut_ptr() as *mut u8,
                dwBufferLength: (buf.len() * 2) as u32,
                dwBytesRecorded: 0,
                dwUser: 0,
                dwFlags: 0,
                dwLoops: 0,
                lpNext: std::ptr::null_mut(),
                reserved: 0,
            });
            // SAFETY: `hdr`/`buf` are heap-pinned for their whole lifetime;
            // sizes match the descriptors exactly.
            let ok = unsafe {
                waveOutPrepareHeader(
                    self.handle,
                    hdr.as_mut() as *mut WaveHdr,
                    std::mem::size_of::<WaveHdr>() as u32,
                ) == MMSYSERR_NOERROR
                    && waveOutWrite(
                        self.handle,
                        hdr.as_mut() as *mut WaveHdr,
                        std::mem::size_of::<WaveHdr>() as u32,
                    ) == MMSYSERR_NOERROR
            };
            if ok {
                self.inflight.push(Inflight { hdr, buf });
            }
            // On failure the boxes simply drop (nothing was queued).
            ok
        }

        /// Free finished buffers. Cheap: usually 0-2 headers in flight.
        fn sweep(&mut self) {
            let handle = self.handle;
            self.inflight.retain(|slot| {
                // SAFETY: same pinned pointers as at Write time.
                let done = unsafe {
                    waveOutUnprepareHeader(
                        handle,
                        slot.hdr.as_ref() as *const WaveHdr as *mut WaveHdr,
                        std::mem::size_of::<WaveHdr>() as u32,
                    )
                };
                done != MMSYSERR_NOERROR
            });
        }
    }

    impl Drop for WaveDevice {
        fn drop(&mut self) {
            unsafe {
                waveOutReset(self.handle);
                for slot in &self.inflight {
                    waveOutUnprepareHeader(
                        self.handle,
                        slot.hdr.as_ref() as *const WaveHdr as *mut WaveHdr,
                        std::mem::size_of::<WaveHdr>() as u32,
                    );
                }
                waveOutClose(self.handle);
            }
        }
    }
}

/// Windows playback loop: open the stream lazily on the first click (so a
/// missing device costs nothing), then mix through it forever; without a
/// device, or if it dies mid-session, fall back to detached PowerShell
/// one-shots (audible, just higher latency).
#[cfg(target_os = "windows")]
fn run_wave_out(inner: Arc<Inner>) {
    let mut device: Option<win32::WaveDevice> = None;
    let mut wave_ok = true;
    while let Some(click) = take_newest(&inner) {
        if wave_ok {
            if device.is_none() {
                device = win32::WaveDevice::open();
                wave_ok = device.is_some();
            }
            if let Some(dev) = device.as_mut() {
                if dev.play(&click.samples) {
                    continue;
                }
                // Device died: drop (closes) it and degrade gracefully.
                device = None;
                wave_ok = false;
            }
        }
        powershell_play(&event_wav(&inner.key_wav, &inner.error_wav, click.correct).to_path_buf());
    }
}

/// Detached PowerShell one-shot (never blocks keystrokes).
#[cfg(target_os = "windows")]
fn powershell_play(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(New-Object System.Media.SoundPlayer $args[0]).PlaySync()",
        ])
        .arg(path.as_os_str())
        .creation_flags(0x08000000)
        .spawn();
}

/// Play one queued click through the best backend. Fire-and-forget on every
/// platform so keystrokes never block on audio. (The terminal bell at the
/// end is unreachable on Windows, where the branch above always returns.)
#[cfg(not(target_os = "windows"))]
fn play_event_best_effort(inner: &Inner, click: &QueuedClick) {
    // Whole-file players so bundled wavs are honoured, with the correct
    // file per outcome. Every backend is fire-and-forget: keystrokes must
    // never block on audio. (Windows uses its own persistent wave-out
    // loop instead; see `run_wave_out`.)
    let wav = event_wav(&inner.key_wav, &inner.error_wav, click.correct).to_path_buf();
    #[cfg(target_os = "macos")]
    {
        if which("afplay").is_some() {
            let _ = Command::new("afplay").arg(&wav).spawn();
            return;
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for (cmd, args) in [
            ("pw-play", vec![wav.to_string_lossy().to_string()]),
            ("paplay", vec![wav.to_string_lossy().to_string()]),
            ("aplay", vec![wav.to_string_lossy().to_string()]),
        ] {
            if which(cmd).is_some() {
                let _ = Command::new(cmd).args(&args).spawn();
                return;
            }
        }
    }
    // Last resort: terminal bell.
    print!("\x07");
    let _ = std::io::stdout().flush();
}

#[cfg(not(target_os = "windows"))]
fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let p = dir.join(cmd);
            if p.is_file() { Some(p) } else { None }
        })
    })
}

fn read_wav_samples(path: &Path) -> Option<Vec<i16>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    // Find the `data` chunk (we write a canonical 44-byte header).
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        if id == b"data" {
            let data = &bytes[offset + 8..(offset + 8 + size).min(bytes.len())];
            return Some(
                data.chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect(),
            );
        }
        offset += 8 + size;
    }
    None
}

/// Generate `key.wav` / `error.wav` when missing (same recipe as Python).
pub fn ensure_sounds(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    write_beep(&dir.join("key.wav"), 1350.0, 0.022, 0.16);
    write_beep(&dir.join("error.wav"), 260.0, 0.075, 0.24);
}

fn write_beep(path: &Path, freq: f64, duration: f64, volume: f64) {
    if path.is_file() {
        return;
    }
    let n = (RATE as f64 * duration) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / RATE as f64;
        let envelope = (1.0 - t / duration).powi(2);
        let v = 32767.0 * volume * envelope * (2.0 * std::f64::consts::PI * freq * t).sin();
        samples.push(v.clamp(-32768.0, 32767.0) as i16);
    }
    write_wav_mono16(path, &samples);
}

fn write_wav_mono16(path: &Path, samples: &[i16]) {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    let _ = std::fs::write(path, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_wav_selects_by_outcome() {
        let key = Path::new("/s/key.wav");
        let err = Path::new("/s/error.wav");
        assert_eq!(event_wav(key, err, true), key);
        assert_eq!(event_wav(key, err, false), err);
    }

    #[test]
    fn queue_cap_drops_oldest_keeps_newest() {
        use std::collections::VecDeque;
        let mut q: VecDeque<i32> = VecDeque::new();
        for i in 0..20 {
            push_capped(&mut q, i, 8);
        }
        assert_eq!(q.len(), 8);
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), (12..20).collect::<Vec<_>>());
    }

    #[test]
    fn keep_newest_returns_last_queued() {
        use std::collections::VecDeque;
        let mut q: VecDeque<i32> = VecDeque::from([2, 3, 4]);
        assert_eq!(keep_newest(1, &mut q), 4);
        assert!(q.is_empty());
        let mut empty: VecDeque<i32> = VecDeque::new();
        assert_eq!(keep_newest(7, &mut empty), 7);
    }

    /// Win32 ABI contract (Windows CI): WAVEFORMATEX is 20 bytes, WAVEHDR
    /// is 48 bytes on 64-bit. A wrong field type would corrupt driver calls.
    #[cfg(all(target_os = "windows", target_pointer_width = "64"))]
    #[test]
    fn wave_struct_layout() {
        assert_eq!(std::mem::size_of::<win32::WaveFormatEx>(), 20);
        assert_eq!(std::mem::size_of::<win32::WaveHdr>(), 48);
    }

    #[test]
    fn ensure_sounds_writes_canonical_wavs() {
        let dir = tempfile::tempdir().unwrap();
        ensure_sounds(dir.path());
        for name in ["key.wav", "error.wav"] {
            let bytes = std::fs::read(dir.path().join(name)).unwrap();
            assert!(bytes.len() > 44);
            assert_eq!(&bytes[0..4], b"RIFF");
            assert_eq!(&bytes[8..12], b"WAVE");
            assert!(read_wav_samples(&dir.path().join(name)).is_some());
        }
        // Second run keeps existing files (never overwrites user data).
        ensure_sounds(dir.path());
    }
}
