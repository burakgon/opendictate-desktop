//! The dictation engine: a small state machine driven by hotkeys, tray/CLI triggers and timers.
//!
//! ```text
//!   Idle ──press──▶ Recording ──release/press──▶ Idle  (+ a processing task in flight)
//!                       │
//!                       └── Esc / interrupted ──▶ Idle (cancelled)
//! ```
//!
//! Recording is exclusive, but processing is not: you can start the next dictation while the
//! previous one is still being transcribed. Results are inserted strictly in the order they
//! were spoken, and all clipboard/keyboard work is serialised.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::{
    active_app,
    audio::{self, Captured, Recorder, Sound},
    history::{self, Entry, History},
    hotkeys::{HotkeyEvent, Slot},
    output::{InsertOutcome, Output},
    overlay,
    providers::{self, msg, Endpoint, SttRequest},
    settings::{HotkeyMode, ModelRef, SelectionMethod, Settings},
    tray,
};

#[derive(Debug, Clone)]
pub enum Trigger {
    ToggleDictate,
    ToggleCommand,
    Stop,
    Cancel,
    QuickAction(String),
    /// Re-run the last recording whose transcription failed.
    Retry,
}

#[derive(Debug)]
pub enum Input {
    Hotkey(HotkeyEvent),
    Trigger(Trigger),
    Timer { session: u64, kind: TimerKind },
}

#[derive(Debug, Clone, Copy)]
pub enum TimerKind {
    Feedback,
    HoldThreshold,
    MaxDuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Dictate,
    Command,
    /// Shared key is held; becomes Command once held past the threshold.
    Pending,
    Action,
}

#[derive(Debug, Clone)]
struct Session {
    id: u64,
    slot: Option<Slot>,
    mode: Mode,
    started: Instant,
    key_down: bool,
    latched: bool,
    feedback: bool,
    key_mode: HotkeyMode,
}

enum Phase {
    Idle,
    Recording(Session),
}

/// Hands out insertion turns so results are typed in the order they were recorded.
#[derive(Default)]
struct Turnstile {
    next_ticket: AtomicU64,
    /// (ticket being served, tickets finished early)
    state: Mutex<(u64, std::collections::BTreeSet<u64>)>,
    notify: tokio::sync::Notify,
}

impl Turnstile {
    fn take(&self) -> u64 {
        self.next_ticket.fetch_add(1, Ordering::SeqCst)
    }

    async fn wait(&self, ticket: u64) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().0 >= ticket {
                return;
            }
            notified.await;
        }
    }

    fn release(&self, ticket: u64) {
        {
            let mut st = self.state.lock();
            if st.0 == ticket {
                st.0 += 1;
                loop {
                    let serving = st.0;
                    if !st.1.remove(&serving) {
                        break;
                    }
                    st.0 += 1;
                }
            } else if ticket > st.0 {
                st.1.insert(ticket);
            }
        }
        self.notify.notify_waiters();
    }
}

/// A processing task's place in the insertion queue; released when dropped.
struct Turn {
    ticket: u64,
    stile: Arc<Turnstile>,
}

impl Turn {
    async fn wait(&self) {
        self.stile.wait(self.ticket).await
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.stile.release(self.ticket);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// idle | recording | processing | done | error | cancelled
    pub phase: &'static str,
    pub mode: Mode,
    pub message: Option<String>,
    pub preview: Option<String>,
    pub session: u64,
    /// Characters of selected text a command operates on.
    pub selection_chars: Option<usize>,
    /// Pipeline step while processing: transcribing | thinking | inserting
    pub step: Option<&'static str>,
    /// The recording was kept and can be retried.
    pub retryable: bool,
}

/// A recording whose transcription failed, kept so it can be retried.
#[derive(Clone)]
struct FailedJob {
    wav: Arc<Vec<u8>>,
    secs: f32,
    mode: Mode,
}

pub struct Engine {
    app: AppHandle,
    pub settings: Arc<RwLock<Settings>>,
    recorder: Arc<Recorder>,
    pub output: Arc<Output>,
    http: reqwest::Client,
    pub history: Arc<History>,
    phase: Mutex<Phase>,
    tx: mpsc::UnboundedSender<Input>,
    next_id: AtomicU64,
    held: Mutex<HashSet<Slot>>,
    /// Which bindings consist only of modifiers (their feedback is delayed slightly so that
    /// ordinary shortcuts like Ctrl+C don't flash the overlay).
    pub modifier_only: RwLock<HashMap<Slot, bool>>,
    last_status: Mutex<Option<Status>>,
    /// Set while the settings UI records a new shortcut.
    pub paused: std::sync::atomic::AtomicBool,
    /// Processing tasks in flight, by session.
    inflight: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    turns: Arc<Turnstile>,
    /// Serialises everything that touches the clipboard or presses keys.
    io_lock: tokio::sync::Mutex<()>,
    failed: Mutex<Option<FailedJob>>,
}

const FEEDBACK_DELAY_MS: u64 = 130;
/// A second tap within this time after the first one starts a voice command instead.
const DOUBLE_TAP_MS: u64 = 450;

impl Engine {
    pub fn new(
        app: AppHandle,
        settings: Arc<RwLock<Settings>>,
        history: Arc<History>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Input>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let engine = Arc::new(Self {
            app,
            settings,
            recorder: Arc::new(Recorder::new()),
            output: Arc::new(Output::new()),
            http: providers::client(),
            history,
            phase: Mutex::new(Phase::Idle),
            tx,
            next_id: AtomicU64::new(1),
            held: Mutex::new(HashSet::new()),
            modifier_only: RwLock::new(HashMap::new()),
            last_status: Mutex::new(None),
            paused: std::sync::atomic::AtomicBool::new(false),
            inflight: Mutex::new(HashMap::new()),
            turns: Arc::new(Turnstile::default()),
            io_lock: tokio::sync::Mutex::new(()),
            failed: Mutex::new(None),
        });
        (engine, rx)
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Input> {
        self.tx.clone()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn recorder(&self) -> &Arc<Recorder> {
        &self.recorder
    }

    pub fn last_status(&self) -> Option<Status> {
        self.last_status.lock().clone()
    }

    pub fn send(&self, input: Input) {
        let _ = self.tx.send(input);
    }

    /// Runs the event loop until the channel closes.
    pub async fn run(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Input>) {
        while let Some(input) = rx.recv().await {
            let engine = self.clone();
            tokio::task::block_in_place(|| engine.handle(input));
        }
    }

    fn sound(&self, sound: Sound) {
        let s = self.settings.read();
        if s.audio.sounds {
            audio::play(sound, s.audio.sound_volume);
        }
    }

    fn recording_id(&self) -> Option<u64> {
        match &*self.phase.lock() {
            Phase::Recording(s) => Some(s.id),
            Phase::Idle => None,
        }
    }

    fn busy(&self) -> bool {
        self.recording_id().is_some() || !self.inflight.lock().is_empty()
    }

    fn emit_status(&self, status: Status) {
        // While a newer recording is running, older sessions finish quietly.
        if let Some(rid) = self.recording_id() {
            if rid != status.session {
                return;
            }
        }
        let _ = self.app.emit("od://status", &status);
        *self.last_status.lock() = Some(status);
    }

    fn status(&self, phase: &'static str, mode: Mode, session: u64) -> Status {
        Status { phase, mode, message: None, preview: None, session, selection_chars: None, step: None, retryable: false }
    }

    fn schedule(&self, session: u64, kind: TimerKind, after: Duration) {
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = tx.send(Input::Timer { session, kind });
        });
    }

    fn handle(self: &Arc<Self>, input: Input) {
        match input {
            Input::Hotkey(ev) => self.on_hotkey(ev),
            Input::Trigger(t) => self.on_trigger(t),
            Input::Timer { session, kind } => self.on_timer(session, kind),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Event handling
    // -----------------------------------------------------------------------------------------

    fn on_hotkey(self: &Arc<Self>, ev: HotkeyEvent) {
        log::debug!("hotkey event: {ev:?}");
        if self.paused.load(Ordering::Relaxed) {
            self.held.lock().clear();
            return;
        }
        match ev {
            HotkeyEvent::Pressed(slot) => {
                self.held.lock().insert(slot);
                self.on_press(slot);
            }
            HotkeyEvent::Released(slot) => {
                self.held.lock().remove(&slot);
                self.on_release(slot);
            }
            HotkeyEvent::Interrupted(slot) => {
                self.held.lock().remove(&slot);
                let cancel = matches!(&*self.phase.lock(), Phase::Recording(s) if s.slot == Some(slot) && s.key_down && !s.latched);
                if cancel {
                    self.cancel(false);
                }
            }
            HotkeyEvent::Escape => {
                if self.settings.read().hotkeys.escape_cancels && self.busy() {
                    self.cancel(true);
                }
            }
        }
    }

    fn on_press(self: &Arc<Self>, slot: Slot) {
        let command_enabled = self.settings.read().command.enabled;
        let action = {
            let mut phase = self.phase.lock();
            match &mut *phase {
                Phase::Idle => PressAction::Start,
                // Double-tap on the shared key: the second tap turns the dictation that the
                // first tap started into a voice command. Works with keys that can't be held
                // (e.g. the Copilot key, which only ever sends quick taps).
                Phase::Recording(s)
                    if command_enabled
                        && s.slot == Some(Slot::Shared)
                        && s.mode == Mode::Dictate
                        && s.latched
                        && s.started.elapsed() < Duration::from_millis(DOUBLE_TAP_MS) =>
                {
                    s.mode = Mode::Command;
                    s.feedback = true;
                    PressAction::SwitchToCommand(s.id)
                }
                Phase::Recording(s) if s.latched => PressAction::Stop,
                _ => PressAction::Ignore,
            }
        };
        match action {
            PressAction::Start => {
                let hk = self.settings.read().hotkeys.clone();
                let (mode, key_mode) = match slot {
                    Slot::Dictate => (Mode::Dictate, hk.dictate_mode),
                    Slot::Command => (Mode::Command, hk.command_mode),
                    Slot::Shared => (Mode::Pending, HotkeyMode::Hybrid),
                };
                let delay = self.modifier_only.read().get(&slot).copied().unwrap_or(false);
                self.start_recording(Some(slot), mode, key_mode, delay);
            }
            PressAction::Stop => self.stop_and_process(),
            PressAction::SwitchToCommand(id) => {
                log::info!("double tap: session {id} is now a voice command");
                self.sound(Sound::CommandStart);
                let mut st = self.status("recording", Mode::Command, id);
                st.message = Some("Speak your command".into());
                self.emit_status(st);
            }
            PressAction::Ignore => {}
        }
    }

    fn on_release(self: &Arc<Self>, slot: Slot) {
        let threshold = Duration::from_millis(self.settings.read().hotkeys.hold_threshold_ms);
        let mut stop = false;
        let mut became_dictation = None;
        {
            let mut phase = self.phase.lock();
            if let Phase::Recording(s) = &mut *phase {
                if s.slot != Some(slot) || !s.key_down {
                    return;
                }
                s.key_down = false;
                let held = s.started.elapsed();
                match s.key_mode {
                    HotkeyMode::Toggle => s.latched = true,
                    HotkeyMode::Hold => stop = true,
                    HotkeyMode::Hybrid => {
                        if held >= threshold {
                            stop = true;
                        } else {
                            s.latched = true;
                            if s.mode == Mode::Pending {
                                s.mode = Mode::Dictate;
                                became_dictation = Some(s.id);
                            }
                        }
                    }
                }
                // A tap shorter than the feedback delay: show feedback now.
                if !stop && !s.feedback {
                    s.feedback = true;
                    let (id, mode) = (s.id, s.mode);
                    drop(phase);
                    self.show_recording_feedback(id, mode);
                    return;
                }
            }
        }
        if let Some(id) = became_dictation {
            let mut st = self.status("recording", Mode::Dictate, id);
            st.message = Some("Listening".into());
            self.emit_status(st);
        }
        if stop {
            self.stop_and_process();
        }
    }

    fn on_trigger(self: &Arc<Self>, t: Trigger) {
        log::info!("trigger: {t:?}");
        match t {
            Trigger::ToggleDictate | Trigger::ToggleCommand => {
                if self.recording_id().is_some() {
                    self.stop_and_process();
                } else {
                    let mode = if matches!(t, Trigger::ToggleDictate) { Mode::Dictate } else { Mode::Command };
                    self.start_recording(None, mode, HotkeyMode::Toggle, false);
                }
            }
            Trigger::Stop => {
                if matches!(*self.phase.lock(), Phase::Recording(_)) {
                    self.stop_and_process();
                }
            }
            Trigger::Cancel => self.cancel(true),
            Trigger::QuickAction(id) => self.quick_action(id),
            Trigger::Retry => self.retry(),
        }
    }

    fn on_timer(self: &Arc<Self>, session: u64, kind: TimerKind) {
        match kind {
            TimerKind::Feedback => {
                let show = {
                    let mut p = self.phase.lock();
                    match &mut *p {
                        Phase::Recording(s) if s.id == session && !s.feedback => {
                            s.feedback = true;
                            Some(s.mode)
                        }
                        _ => None,
                    }
                };
                if let Some(mode) = show {
                    self.show_recording_feedback(session, mode);
                }
            }
            TimerKind::HoldThreshold => {
                let switched = {
                    let mut p = self.phase.lock();
                    match &mut *p {
                        Phase::Recording(s) if s.id == session && s.key_down && s.mode == Mode::Pending => {
                            s.mode = Mode::Command;
                            true
                        }
                        _ => false,
                    }
                };
                if switched {
                    self.sound(Sound::CommandStart);
                    let mut st = self.status("recording", Mode::Command, session);
                    st.message = Some("Speak your command".into());
                    self.emit_status(st);
                }
            }
            TimerKind::MaxDuration => {
                let same = matches!(&*self.phase.lock(), Phase::Recording(s) if s.id == session);
                if same {
                    log::info!("maximum recording length reached");
                    self.stop_and_process();
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------------------------

    fn start_recording(self: &Arc<Self>, slot: Option<Slot>, mode: Mode, key_mode: HotkeyMode, delay_feedback: bool) {
        let settings = self.settings.read().clone();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = self.recorder.start(&settings.audio.input_device) {
            log::error!("microphone error: {e:#}");
            self.fail(id, mode, format!("Microphone error: {e:#}"));
            return;
        }
        let session = Session {
            id,
            slot,
            mode,
            started: Instant::now(),
            key_down: slot.is_some(),
            latched: slot.is_none() || key_mode == HotkeyMode::Toggle,
            feedback: false,
            key_mode,
        };
        *self.phase.lock() = Phase::Recording(session);
        log::info!("recording started (session {id}, {mode:?})");

        if delay_feedback {
            self.schedule(id, TimerKind::Feedback, Duration::from_millis(FEEDBACK_DELAY_MS));
        } else {
            if let Phase::Recording(s) = &mut *self.phase.lock() {
                s.feedback = true;
            }
            self.show_recording_feedback(id, mode);
        }
        if mode == Mode::Pending {
            self.schedule(id, TimerKind::HoldThreshold, Duration::from_millis(settings.hotkeys.hold_threshold_ms));
        }
        self.schedule(id, TimerKind::MaxDuration, Duration::from_secs(settings.audio.max_seconds.max(5) as u64));

        if settings.audio.pause_media {
            tauri::async_runtime::spawn(crate::media::pause_playing());
        }
        // Pre-open the TLS connection while the user speaks.
        if let Ok(ep) = Endpoint::resolve(&settings.providers, &settings.dictation.stt.provider) {
            let http = self.http.clone();
            tauri::async_runtime::spawn(async move { providers::warm_up(&http, &ep).await });
        }
        self.spawn_level_meter(id);
    }

    fn show_recording_feedback(&self, id: u64, mode: Mode) {
        let settings = self.settings.read().clone();
        self.sound(if mode == Mode::Command { Sound::CommandStart } else { Sound::Start });
        let mut st = self.status("recording", mode, id);
        st.message = Some(if mode == Mode::Command { "Speak your command".into() } else { "Listening".into() });
        self.emit_status(st);
        if settings.general.show_overlay {
            overlay::show(&self.app, settings.general.overlay_position);
        }
        tray::set_active(&self.app, true);
    }

    fn spawn_level_meter(self: &Arc<Self>, id: u64) {
        let engine = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(33)).await;
                let recording = matches!(&*engine.phase.lock(), Phase::Recording(s) if s.id == id);
                if !recording {
                    break;
                }
                let _ = engine.app.emit("od://level", engine.recorder.current_level());
            }
        });
    }

    /// Cancels the recording if there is one, otherwise everything still being processed.
    fn cancel(self: &Arc<Self>, audible: bool) {
        let previous = std::mem::replace(&mut *self.phase.lock(), Phase::Idle);
        let (id, mode, had_feedback) = match previous {
            Phase::Recording(s) => {
                self.recorder.cancel();
                tauri::async_runtime::spawn(crate::media::resume_paused());
                (s.id, s.mode, s.feedback)
            }
            Phase::Idle => {
                let tasks: Vec<(u64, tokio::task::AbortHandle)> = self.inflight.lock().drain().collect();
                let Some(last) = tasks.iter().map(|(id, _)| *id).max() else { return };
                for (_, abort) in tasks {
                    abort.abort();
                }
                (last, Mode::Dictate, true)
            }
        };
        log::info!("session {id} cancelled");
        if audible && had_feedback {
            self.sound(Sound::Cancel);
        }
        let mut st = self.status("cancelled", mode, id);
        st.message = Some("Cancelled".into());
        self.emit_status(st);
        if !self.busy() {
            overlay::hide(&self.app);
        }
        tray::set_active(&self.app, self.busy());
    }

    fn fail(self: &Arc<Self>, id: u64, mode: Mode, message: String) {
        self.sound(Sound::Error);
        let mut st = self.status("error", mode, id);
        st.message = Some(message);
        st.retryable = self.failed.lock().is_some();
        self.emit_status(st);
        let settings = self.settings.read().clone();
        if settings.general.show_overlay {
            overlay::show(&self.app, settings.general.overlay_position);
        }
        self.hide_later(id, Duration::from_millis(4500));
        tray::set_active(&self.app, self.busy());
    }

    fn hide_later(self: &Arc<Self>, id: u64, after: Duration) {
        let engine = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(after).await;
            let same = engine.last_status.lock().as_ref().map(|s| s.session == id).unwrap_or(true);
            if !engine.busy() && same {
                overlay::hide(&engine.app);
            }
        });
    }

    fn stop_and_process(self: &Arc<Self>) {
        let session = {
            let mut p = self.phase.lock();
            match std::mem::replace(&mut *p, Phase::Idle) {
                Phase::Recording(s) => s,
                Phase::Idle => return,
            }
        };
        tauri::async_runtime::spawn(crate::media::resume_paused());
        let captured = match self.recorder.stop() {
            Ok(c) => c,
            Err(e) => {
                self.fail(session.id, session.mode, format!("Recording failed: {e:#}"));
                return;
            }
        };
        let mode = if session.mode == Mode::Pending { Mode::Dictate } else { session.mode };
        log::info!("recording stopped (session {}, {:.1}s, {:?})", session.id, captured.duration_secs(), mode);
        if !session.feedback {
            // Released before any feedback was shown: treat as an accidental tap.
            tray::set_active(&self.app, self.busy());
            return;
        }
        self.sound(Sound::Stop);
        let id = session.id;
        self.spawn_processing(id, mode, move |engine, turn| async move { engine.process(id, mode, captured, turn).await });
    }

    /// Runs a processing job in the background, holding a place in the insertion queue.
    fn spawn_processing<F, Fut>(self: &Arc<Self>, id: u64, mode: Mode, job: F)
    where
        F: FnOnce(Arc<Self>, Turn) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Outcome>> + Send + 'static,
    {
        let turn = Turn { ticket: self.turns.take(), stile: self.turns.clone() };
        let engine = self.clone();
        let mut inflight = self.inflight.lock();
        let task = tauri::async_runtime::spawn(async move {
            let result = job(engine.clone(), turn).await;
            engine.finish(id, mode, result);
        });
        inflight.insert(id, task.inner().abort_handle());
        drop(inflight);
        tray::set_active(&self.app, true);
    }

    fn finish(self: &Arc<Self>, id: u64, mode: Mode, result: Result<Outcome>) {
        self.inflight.lock().remove(&id);
        tray::set_active(&self.app, self.busy());
        let recording = self.recording_id().is_some();
        match result {
            Ok(Outcome::Done { preview, copied_only }) => {
                if !recording {
                    self.sound(Sound::Done);
                }
                let mut st = self.status("done", mode, id);
                st.preview = Some(preview);
                st.message = Some(if copied_only { "Copied to clipboard".into() } else { "Inserted".into() });
                self.emit_status(st);
                self.hide_later(id, Duration::from_millis(1400));
            }
            Ok(Outcome::Nothing(reason)) => {
                let mut st = self.status("cancelled", mode, id);
                st.message = Some(reason);
                self.emit_status(st);
                self.hide_later(id, Duration::from_millis(1600));
            }
            Err(e) => {
                log::error!("session {id} failed: {e:#}");
                self.fail(id, mode, format!("{e:#}"));
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Processing pipeline
    // -----------------------------------------------------------------------------------------

    fn step(&self, id: u64, mode: Mode, step: &'static str, selection_chars: Option<usize>) {
        let mut st = self.status("processing", mode, id);
        st.step = Some(step);
        st.selection_chars = selection_chars;
        st.message = Some(
            match step {
                "transcribing" => "Transcribing",
                "thinking" => "Thinking",
                "polishing" => "Polishing",
                _ => "Inserting",
            }
            .into(),
        );
        self.emit_status(st);
    }

    /// Waits (briefly) until the user has let go of the hotkey so injected shortcuts aren't
    /// combined with physically held modifiers.
    async fn wait_keys_released(&self) {
        for _ in 0..75 {
            if self.held.lock().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Resamples, trims and encodes a recording. `None` when there is no speech in it.
    fn prepare_audio(settings: &Settings, captured: Captured) -> Result<Option<(Arc<Vec<u8>>, f32)>> {
        let captured = match std::env::var("OPENDICTATE_TEST_AUDIO") {
            Ok(path) if !path.is_empty() => audio::decode_wav(&std::fs::read(&path)?)?,
            _ => captured,
        };
        let pcm = audio::resample_to_16k(&captured.samples, captured.sample_rate);
        let analysis = audio::analyse(&pcm);
        if !analysis.has_speech {
            return Ok(None);
        }
        let slice = if settings.audio.trim_silence { &pcm[analysis.start..analysis.end] } else { &pcm[..] };
        let secs = slice.len() as f32 / audio::TARGET_RATE as f32;
        Ok(Some((Arc::new(audio::encode_wav(slice, audio::TARGET_RATE)?), secs)))
    }

    /// Transcribes a prepared recording. On failure the audio is kept for [`Trigger::Retry`].
    async fn transcribe(&self, settings: &Settings, wav: Arc<Vec<u8>>, secs: f32, mode: Mode) -> Result<String> {
        let d = &settings.dictation;
        let started = Instant::now();
        let result = async {
            let ep = Endpoint::resolve(&settings.providers, &d.stt.provider)?;
            let language = if d.language.is_empty() || d.language == "auto" { None } else { Some(d.language.as_str()) };
            let vocab = d.vocabulary.trim();
            providers::transcribe(
                &self.http,
                &ep,
                SttRequest {
                    model: &d.stt.model,
                    wav: wav.as_ref().clone(),
                    language,
                    prompt: if vocab.is_empty() { None } else { Some(vocab) },
                },
            )
            .await
        }
        .await;
        match result {
            Ok(text) => {
                log::info!("transcribed {:.1}s of audio in {} ms", secs, started.elapsed().as_millis());
                *self.failed.lock() = None;
                Ok(text)
            }
            Err(e) => {
                *self.failed.lock() = Some(FailedJob { wav, secs, mode });
                Err(e)
            }
        }
    }

    async fn llm(&self, settings: &Settings, model: &ModelRef, system: &str, user: String, temperature: f32) -> Result<String> {
        let ep = Endpoint::resolve(&settings.providers, &model.provider)?;
        let started = Instant::now();
        let reasoning = settings.command.reasoning;
        let out = providers::chat(&self.http, &ep, &model.model, &[msg("system", system), msg("user", user)], Some(temperature), reasoning)
            .await?;
        log::info!("llm '{}' answered in {} ms", model.model, started.elapsed().as_millis());
        Ok(out)
    }

    /// Types `text` into the focused app. Returns (copied_only, app id).
    async fn insert(&self, text: &str, settings: &Settings, turn: &Turn) -> Result<(bool, Option<String>)> {
        turn.wait().await;
        let _io = self.io_lock.lock().await;
        self.wait_keys_released().await;
        let app = active_app::active_app().await;
        let terminal = app.as_deref().map(active_app::is_terminal).unwrap_or(false);
        log::debug!("inserting into {app:?} (terminal: {terminal})");
        let output = self.output.clone();
        let text = text.to_string();
        let cfg = settings.output.clone();
        let outcome = tokio::task::spawn_blocking(move || output.insert(&text, &cfg, terminal)).await??;
        Ok((outcome == InsertOutcome::CopiedOnly, app))
    }

    async fn capture_selection(&self, method: SelectionMethod) -> Option<String> {
        if method == SelectionMethod::None {
            return None;
        }
        let _io = self.io_lock.lock().await;
        self.wait_keys_released().await;
        let terminal = match method {
            SelectionMethod::Copy => active_app::active_app().await.as_deref().map(active_app::is_terminal).unwrap_or(false),
            _ => false,
        };
        let output = self.output.clone();
        tokio::task::spawn_blocking(move || output.capture_selection(method, terminal).0).await.ok().flatten()
    }

    async fn process(self: Arc<Self>, id: u64, mode: Mode, captured: Captured, turn: Turn) -> Result<Outcome> {
        let settings = self.settings.read().clone();
        let prepared = {
            let settings = settings.clone();
            tokio::task::spawn_blocking(move || Self::prepare_audio(&settings, captured)).await??
        };
        let Some((wav, secs)) = prepared else {
            return Ok(Outcome::Nothing("No speech detected".into()));
        };
        self.process_wav(id, mode, wav, secs, turn).await
    }

    async fn process_wav(self: Arc<Self>, id: u64, mode: Mode, wav: Arc<Vec<u8>>, secs: f32, turn: Turn) -> Result<Outcome> {
        let started = Instant::now();
        let settings = self.settings.read().clone();
        self.step(id, mode, "transcribing", None);

        if mode == Mode::Command {
            // Read the selection while the transcription is in flight.
            let selection_fut = self.capture_selection(settings.command.selection_method);
            let transcript_fut = self.transcribe(&settings, wav, secs, mode);
            let (selection, transcript) = tokio::join!(selection_fut, transcript_fut);
            return self.run_command(id, &settings, transcript?, selection, secs, started, &turn).await;
        }

        // Dictation.
        let raw = self.transcribe(&settings, wav, secs, mode).await?;
        if settings.command.enabled {
            if let Some(instruction) = strip_trigger_word(&raw, &settings.command.trigger_word) {
                // "Jarvis, make this formal": the dictation turns into a command.
                let mut st = self.status("processing", Mode::Command, id);
                st.message = Some("Thinking".into());
                st.step = Some("thinking");
                self.emit_status(st);
                let selection = self.capture_selection(settings.command.selection_method).await;
                return self.run_command(id, &settings, instruction, selection, secs, started, &turn).await;
            }
        }
        let d = &settings.dictation;
        let mut text = raw.clone();
        let mut llm_model = None;
        if d.polish && !text.trim().is_empty() {
            self.step(id, mode, "polishing", None);
            // Formatting is a refinement: if it's slow or fails, insert the raw transcript.
            let budget = Duration::from_secs(6 + history::count_words(&text) as u64 / 40);
            let polish = self.llm(&settings, &d.polish_model, settings.polish_prompt(), format!("<transcript>\n{text}\n</transcript>"), 0.0);
            match tokio::time::timeout(budget, polish).await {
                Ok(Ok(t)) if !t.trim().is_empty() => {
                    text = t;
                    llm_model = Some(d.polish_model.model.clone());
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => log::warn!("formatting failed, inserting the raw transcript: {e:#}"),
                Err(_) => log::warn!("formatting took longer than {}s, inserting the raw transcript", budget.as_secs()),
            }
        }
        let mut text = apply_replacements(text.trim(), &d.replacements);
        if text.is_empty() || is_hallucination(&text, secs) {
            return Ok(Outcome::Nothing("No speech detected".into()));
        }
        if d.trailing_space && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        self.step(id, mode, "inserting", None);
        let (copied_only, app) = self.insert(&text, &settings, &turn).await?;
        self.record(Entry {
            kind: "dictation".into(),
            text: text.trim_end().to_string(),
            transcript: raw,
            audio_secs: secs,
            latency_ms: started.elapsed().as_millis() as u64,
            stt_model: d.stt.model.clone(),
            llm_model,
            inserted: !copied_only,
            app,
            ..Default::default()
        });
        Ok(Outcome::Done { preview: preview(&text), copied_only })
    }

    async fn run_command(
        &self,
        id: u64,
        settings: &Settings,
        instruction: String,
        selection: Option<String>,
        secs: f32,
        started: Instant,
        turn: &Turn,
    ) -> Result<Outcome> {
        let mode = Mode::Command;
        if instruction.trim().is_empty() {
            return Ok(Outcome::Nothing("No speech detected".into()));
        }
        self.step(id, mode, "thinking", selection.as_ref().map(|s| s.chars().count()));
        let c = &settings.command;
        let (system, user) = match &selection {
            Some(sel) => (
                settings.selection_prompt().to_string(),
                format!("<instruction>\n{instruction}\n</instruction>\n\n<selected_text>\n{sel}\n</selected_text>"),
            ),
            None => (settings.generate_prompt().to_string(), instruction.clone()),
        };
        let result = self.llm(settings, &c.llm, &system, user, c.temperature).await?;
        if result.trim().is_empty() {
            return Err(anyhow!("The model returned an empty answer"));
        }
        self.step(id, mode, "inserting", None);
        let (copied_only, app) = self.insert(&result, settings, turn).await?;
        self.record(Entry {
            kind: "command".into(),
            text: result.clone(),
            transcript: instruction,
            selection: selection.map(|s| s.chars().take(2000).collect()),
            audio_secs: secs,
            latency_ms: started.elapsed().as_millis() as u64,
            stt_model: settings.dictation.stt.model.clone(),
            llm_model: Some(c.llm.model.clone()),
            inserted: !copied_only,
            app,
            ..Default::default()
        });
        Ok(Outcome::Done { preview: preview(&result), copied_only })
    }

    fn record(&self, entry: Entry) {
        let g = self.settings.read().general.clone();
        let saved = self.history.add(entry, g.history_enabled, g.history_limit);
        let _ = self.app.emit("od://history", &saved);
    }

    /// Re-runs the last recording whose transcription failed.
    fn retry(self: &Arc<Self>) {
        let Some(job) = self.failed.lock().take() else {
            log::info!("nothing to retry");
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        log::info!("retrying a {:.1}s recording as session {id}", job.secs);
        let settings = self.settings.read().clone();
        if settings.general.show_overlay && self.recording_id().is_none() {
            overlay::show(&self.app, settings.general.overlay_position);
        }
        let FailedJob { wav, secs, mode } = job;
        self.spawn_processing(id, mode, move |engine, turn| async move { engine.process_wav(id, mode, wav, secs, turn).await });
    }

    /// Applies a saved prompt to the selected text without speaking.
    fn quick_action(self: &Arc<Self>, action_id: String) {
        let settings = self.settings.read().clone();
        let Some(action) = settings.command.quick_actions.iter().find(|a| a.id == action_id).cloned() else {
            log::warn!("unknown quick action '{action_id}'");
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.spawn_processing(id, Mode::Action, move |engine, turn| async move {
            let started = Instant::now();
            let mut st = engine.status("processing", Mode::Action, id);
            st.message = Some(action.name.clone());
            st.step = Some("thinking");
            engine.emit_status(st);
            if settings.general.show_overlay && engine.recording_id().is_none() {
                overlay::show(&engine.app, settings.general.overlay_position);
            }
            let method = match settings.command.selection_method {
                SelectionMethod::None => SelectionMethod::Copy,
                m => m,
            };
            let Some(selection) = engine.capture_selection(method).await else {
                return Err(anyhow!("Select some text first, then run “{}”.", action.name));
            };
            let c = &settings.command;
            let user = format!("<instruction>\n{}\n</instruction>\n\n<selected_text>\n{selection}\n</selected_text>", action.prompt);
            let result = engine.llm(&settings, &c.llm, settings.selection_prompt(), user, c.temperature).await?;
            let (copied_only, app) = engine.insert(&result, &settings, &turn).await?;
            engine.record(Entry {
                kind: "action".into(),
                text: result.clone(),
                transcript: action.name.clone(),
                selection: Some(selection.chars().take(2000).collect()),
                latency_ms: started.elapsed().as_millis() as u64,
                llm_model: Some(c.llm.model.clone()),
                inserted: !copied_only,
                app,
                ..Default::default()
            });
            Ok(Outcome::Done { preview: preview(&result), copied_only })
        });
    }
}

enum PressAction {
    Start,
    Stop,
    SwitchToCommand(u64),
    Ignore,
}

pub enum Outcome {
    Done { preview: String, copied_only: bool },
    Nothing(String),
}

fn preview(text: &str) -> String {
    let t = text.trim();
    let mut p: String = t.chars().take(140).collect();
    if t.chars().count() > 140 {
        p.push('…');
    }
    p
}

pub fn apply_replacements(text: &str, replacements: &[crate::settings::Replacement]) -> String {
    let mut out = text.to_string();
    for r in replacements {
        if r.from.trim().is_empty() {
            continue;
        }
        out = replace_case_insensitive(&out, r.from.trim(), &r.to);
    }
    out
}

fn replace_case_insensitive(haystack: &str, needle: &str, with: &str) -> String {
    let lower_h = haystack.to_lowercase();
    let lower_n = needle.to_lowercase();
    if lower_h.len() != haystack.len() || lower_n.is_empty() {
        // Lowercasing changed byte lengths (rare scripts): fall back to exact matching.
        return haystack.replace(needle, with);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while let Some(pos) = lower_h[i..].find(&lower_n) {
        let start = i + pos;
        let end = start + lower_n.len();
        let boundary_ok = haystack[..start].chars().last().map(|c| !c.is_alphanumeric()).unwrap_or(true)
            && haystack[end..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true);
        out.push_str(&haystack[i..start]);
        out.push_str(if boundary_ok { with } else { &haystack[start..end] });
        i = end;
    }
    out.push_str(&haystack[i..]);
    out
}

/// If `text` starts with the command word (case-insensitive, ignoring punctuation), returns
/// the rest of the sentence: the instruction.
pub fn strip_trigger_word(text: &str, word: &str) -> Option<String> {
    let word = word.trim();
    if word.is_empty() {
        return None;
    }
    let normalize = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let target = normalize(word);
    if target.is_empty() {
        return None;
    }
    // Walk the leading words of the transcript until they spell the trigger word.
    let mut acc = String::new();
    for (idx, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            acc.extend(ch.to_lowercase());
            if acc == target {
                let rest = &text[idx + ch.len_utf8()..];
                // Must end at a word boundary.
                if rest.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false) {
                    return None;
                }
                let rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
                return if rest.is_empty() { None } else { Some(rest.to_string()) };
            }
            if !target.starts_with(&acc) {
                return None;
            }
        }
    }
    None
}

/// Whisper-style models invent these on near-silent audio.
fn is_hallucination(text: &str, secs: f32) -> bool {
    if secs > 3.0 {
        return false;
    }
    let t: String = text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    let t = t.trim();
    const KNOWN: &[&str] = &[
        "thank you",
        "thanks for watching",
        "thank you for watching",
        "you",
        "bye",
        "subtitles by the amaraorg community",
        "altyazı mk",
        "abone olmayı unutmayın",
    ];
    KNOWN.contains(&t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Replacement;

    #[test]
    fn replacements_respect_word_boundaries() {
        let r = vec![Replacement { from: "open dictate".into(), to: "OpenDictate".into() }];
        assert_eq!(apply_replacements("I love Open Dictate a lot", &r), "I love OpenDictate a lot");
        let r = vec![Replacement { from: "cat".into(), to: "dog".into() }];
        assert_eq!(apply_replacements("concatenate the cat", &r), "concatenate the dog");
    }

    #[tokio::test]
    async fn turnstile_orders_insertions() {
        let stile = Arc::new(Turnstile::default());
        let t0 = Turn { ticket: stile.take(), stile: stile.clone() };
        let t1 = Turn { ticket: stile.take(), stile: stile.clone() };
        let t2 = Turn { ticket: stile.take(), stile: stile.clone() };
        // Ticket 1 is abandoned early (e.g. no speech): it must not block ticket 2.
        drop(t1);
        let order = Arc::new(Mutex::new(Vec::new()));
        let o2 = order.clone();
        let h2 = tokio::spawn(async move {
            t2.wait().await;
            o2.lock().push(2);
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(order.lock().is_empty(), "ticket 2 must wait for ticket 0");
        t0.wait().await;
        order.lock().push(0);
        drop(t0);
        h2.await.unwrap();
        assert_eq!(*order.lock(), vec![0, 2]);
    }

    #[test]
    fn trigger_word() {
        assert_eq!(strip_trigger_word("Jarvis, make this formal.", "jarvis").as_deref(), Some("make this formal."));
        assert_eq!(strip_trigger_word("Hey Jarvis translate to German", "Hey Jarvis").as_deref(), Some("translate to German"));
        assert_eq!(strip_trigger_word("Jarvisification is a word", "jarvis"), None);
        assert_eq!(strip_trigger_word("I told Jarvis hello", "jarvis"), None);
        assert_eq!(strip_trigger_word("anything", ""), None);
        assert_eq!(strip_trigger_word("Komut: bunu resmi yap", "komut").as_deref(), Some("bunu resmi yap"));
    }

    #[test]
    fn hallucinations() {
        assert!(is_hallucination("Thank you.", 1.0));
        assert!(!is_hallucination("Thank you.", 5.0));
        assert!(!is_hallucination("Thank you for the report", 1.0));
    }
}
