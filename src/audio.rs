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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        let inner = Arc::new(Inner {
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            stop: Mutex::new(false),
            key_wav: sounds_dir.join("key.wav"),
            error_wav: sounds_dir.join("error.wav"),
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
        let path = event_wav(&self.inner.key_wav, &self.inner.error_wav, correct).to_path_buf();
        let samples = read_wav_samples(&path).unwrap_or_default();
        if samples.is_empty() {
            return;
        }
        self.inner
            .queue
            .lock()
            .unwrap()
            .push_back(QueuedClick { samples, correct });
        self.inner.wake.notify_one();
    }
}

impl Drop for ClickPlayer {
    fn drop(&mut self) {
        *self.inner.stop.lock().unwrap() = true;
        self.inner.wake.notify_all();
    }
}

fn run_loop(inner: Arc<Inner>) {
    // Prefer a streaming raw mixer when pw-cat exists (lowest latency,
    // exact Python parity). Otherwise play whole files per event.
    if pw_cat_available() {
        run_pw_cat_stream(inner);
    } else {
        run_per_event(inner);
    }
}

fn pw_cat_available() -> bool {
    which("pw-cat").is_some()
}

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

fn run_per_event(inner: Arc<Inner>) {
    loop {
        let click = {
            let mut guard = inner.queue.lock().unwrap();
            loop {
                if *inner.stop.lock().unwrap() {
                    return;
                }
                if let Some(click) = guard.pop_front() {
                    break click;
                }
                guard = inner.wake.wait(guard).unwrap();
            }
        };
        play_event_best_effort(&inner, &click);
    }
}

/// Win32 wave-out playback without helper processes: instant and async.
/// Falls back to a detached PowerShell one-shot when it fails.
#[cfg(target_os = "windows")]
mod win32 {
    #[allow(non_snake_case)]
    #[link(name = "winmm")]
    extern "system" {
        pub fn PlaySoundW(
            pszSound: *const u16,
            hmod: *const std::ffi::c_void,
            fdwSound: u32,
        ) -> i32;
    }

    pub const SND_FILENAME: u32 = 0x00020000;
    pub const SND_ASYNC: u32 = 0x00000001;

    /// Queue a wav file for async playback. Returns true when accepted.
    pub fn play_wav_async(path: &std::path::Path) -> bool {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        // SAFETY: PlaySoundW only reads the NUL-terminated file name during
        // the call; winmm.dll is a core Windows system library.
        unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null(), SND_FILENAME | SND_ASYNC) != 0 }
    }
}

/// Play one queued click through the best backend. Fire-and-forget on every
/// platform so keystrokes never block on audio. (The terminal bell at the
/// end is unreachable on Windows, where the branch above always returns.)
#[cfg_attr(target_os = "windows", allow(unreachable_code))]
fn play_event_best_effort(inner: &Inner, click: &QueuedClick) {
    // Whole-file players so bundled wavs are honoured, with the correct
    // file per outcome. Every backend is fire-and-forget: keystrokes must
    // never block on audio (the old blocking PlaySync made Windows sounds
    // pile up seconds behind typing).
    let wav = event_wav(&inner.key_wav, &inner.error_wav, click.correct).to_path_buf();
    #[cfg(target_os = "windows")]
    {
        if win32::play_wav_async(&wav) {
            return;
        }
        use std::os::windows::process::CommandExt;
        let _ = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "(New-Object System.Media.SoundPlayer $args[0]).PlaySync()"])
            .arg(wav.as_os_str())
            .creation_flags(0x08000000)
            .spawn();
        return;
    }
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
