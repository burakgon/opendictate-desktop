//! The dictation engine: a small state machine driven by hotkeys, tray/CLI triggers and timers.
//!
//! ```text
//!   Idle ──press──▶ Recording ──release/press──▶ Processing ──▶ Idle
//!                       │                            │
//!                       └──── Esc / interrupted ─────┴──▶ Idle (cancelled)
//! ```

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
    audio::{self, Captured, Recorder, Sound},
    history::{Entry, History},
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
    Processing { id: u64, abort: tokio::task::AbortHandle },
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

    fn emit_status(&self, status: Status) {
        let _ = self.app.emit("od://status", &status);
        *self.last_status.lock() = Some(status);
    }

    fn status(&self, phase: &'static str, mode: Mode, session: u64) -> Status {
        Status { phase, mode, message: None, preview: None, session, selection_chars: None, step: None }
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
                if self.settings.read().hotkeys.escape_cancels {
                    let active = !matches!(&*self.phase.lock(), Phase::Idle);
                    if active {
                        self.cancel(true);
                    }
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
                let (idle, recording) = {
                    let p = self.phase.lock();
                    (matches!(*p, Phase::Idle), matches!(*p, Phase::Recording(_)))
                };
                if idle {
                    let mode = if matches!(t, Trigger::ToggleDictate) { Mode::Dictate } else { Mode::Command };
                    self.start_recording(None, mode, HotkeyMode::Toggle, false);
                } else if recording {
                    self.stop_and_process();
                }
            }
            Trigger::Stop => {
                if matches!(*self.phase.lock(), Phase::Recording(_)) {
                    self.stop_and_process();
                }
            }
            Trigger::Cancel => self.cancel(true),
            Trigger::QuickAction(id) => self.quick_action(id),
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

    fn cancel(self: &Arc<Self>, audible: bool) {
        let previous = std::mem::replace(&mut *self.phase.lock(), Phase::Idle);
        let (id, mode, had_feedback) = match previous {
            Phase::Idle => return,
            Phase::Recording(s) => {
                self.recorder.cancel();
                (s.id, s.mode, s.feedback)
            }
            Phase::Processing { id, abort } => {
                abort.abort();
                (id, Mode::Dictate, true)
            }
        };
        log::info!("session {id} cancelled");
        if audible && had_feedback {
            self.sound(Sound::Cancel);
        }
        let mut st = self.status("cancelled", mode, id);
        st.message = Some("Cancelled".into());
        self.emit_status(st);
        overlay::hide(&self.app);
        tray::set_active(&self.app, false);
    }

    fn fail(self: &Arc<Self>, id: u64, mode: Mode, message: String) {
        self.sound(Sound::Error);
        let mut st = self.status("error", mode, id);
        st.message = Some(message);
        self.emit_status(st);
        let settings = self.settings.read().clone();
        if settings.general.show_overlay {
            overlay::show(&self.app, settings.general.overlay_position);
        }
        self.hide_later(id, Duration::from_millis(4500));
        tray::set_active(&self.app, false);
    }

    fn hide_later(self: &Arc<Self>, id: u64, after: Duration) {
        let engine = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(after).await;
            let idle = matches!(*engine.phase.lock(), Phase::Idle);
            let same = engine.last_status.lock().as_ref().map(|s| s.session == id).unwrap_or(true);
            if idle && same {
                overlay::hide(&engine.app);
            }
        });
    }

    fn stop_and_process(self: &Arc<Self>) {
        let session = {
            let mut p = self.phase.lock();
            match std::mem::replace(&mut *p, Phase::Idle) {
                Phase::Recording(s) => s,
                other => {
                    *p = other;
                    return;
                }
            }
        };
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
            tray::set_active(&self.app, false);
            return;
        }
        self.sound(Sound::Stop);
        let engine = self.clone();
        let id = session.id;
        let task = tauri::async_runtime::spawn(async move {
            let result = engine.clone().process(id, mode, captured).await;
            engine.finish(id, mode, result);
        });
        *self.phase.lock() = Phase::Processing { id, abort: task.inner().abort_handle() };
    }

    fn finish(self: &Arc<Self>, id: u64, mode: Mode, result: Result<Outcome>) {
        {
            let mut p = self.phase.lock();
            if matches!(&*p, Phase::Processing { id: pid, .. } if *pid == id) {
                *p = Phase::Idle;
            } else if !matches!(*p, Phase::Idle) {
                return; // a newer session took over
            }
        }
        tray::set_active(&self.app, false);
        match result {
            Ok(Outcome::Done { preview, copied_only }) => {
                self.sound(Sound::Done);
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

    async fn transcribe(&self, settings: &Settings, captured: Captured) -> Result<Option<(String, f32)>> {
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
        let wav = audio::encode_wav(slice, audio::TARGET_RATE)?;
        let d = &settings.dictation;
        let ep = Endpoint::resolve(&settings.providers, &d.stt.provider)?;
        let language = if d.language.is_empty() || d.language == "auto" { None } else { Some(d.language.as_str()) };
        let vocab = d.vocabulary.trim();
        let started = Instant::now();
        let text = providers::transcribe(
            &self.http,
            &ep,
            SttRequest { model: &d.stt.model, wav, language, prompt: if vocab.is_empty() { None } else { Some(vocab) } },
        )
        .await?;
        log::info!("transcribed {:.1}s of audio in {} ms", secs, started.elapsed().as_millis());
        Ok(Some((text, secs)))
    }

    async fn llm(&self, settings: &Settings, model: &ModelRef, system: &str, user: String, temperature: f32) -> Result<String> {
        let ep = Endpoint::resolve(&settings.providers, &model.provider)?;
        let started = Instant::now();
        let out = providers::chat(&self.http, &ep, &model.model, &[msg("system", system), msg("user", user)], Some(temperature)).await?;
        log::info!("llm '{}' answered in {} ms", model.model, started.elapsed().as_millis());
        Ok(out)
    }

    async fn insert(&self, text: &str, settings: &Settings) -> Result<bool> {
        self.wait_keys_released().await;
        let output = self.output.clone();
        let text = text.to_string();
        let cfg = settings.output.clone();
        let outcome = tokio::task::spawn_blocking(move || output.insert(&text, &cfg)).await??;
        Ok(outcome == InsertOutcome::CopiedOnly)
    }

    async fn capture_selection(&self, method: SelectionMethod) -> Option<String> {
        self.wait_keys_released().await;
        let output = self.output.clone();
        tokio::task::spawn_blocking(move || output.capture_selection(method).0).await.ok().flatten()
    }

    async fn process(self: Arc<Self>, id: u64, mode: Mode, captured: Captured) -> Result<Outcome> {
        let started = Instant::now();
        let settings = self.settings.read().clone();
        self.step(id, mode, "transcribing", None);

        if mode == Mode::Command {
            // Read the selection while the transcription is in flight.
            let selection_fut = self.capture_selection(settings.command.selection_method);
            let transcript_fut = self.transcribe(&settings, captured);
            let (selection, transcript) = tokio::join!(selection_fut, transcript_fut);
            let Some((instruction, secs)) = transcript? else {
                return Ok(Outcome::Nothing("No speech detected".into()));
            };
            if instruction.trim().is_empty() {
                return Ok(Outcome::Nothing("No speech detected".into()));
            }
            self.step(id, mode, "thinking", selection.as_ref().map(|s| s.chars().count()));
            let c = &settings.command;
            let (system, user) = match &selection {
                Some(sel) => (
                    c.prompt_selection.clone(),
                    format!("<instruction>\n{instruction}\n</instruction>\n\n<selected_text>\n{sel}\n</selected_text>"),
                ),
                None => (c.prompt_generate.clone(), instruction.clone()),
            };
            let result = self.llm(&settings, &c.llm, &system, user, c.temperature).await?;
            if result.trim().is_empty() {
                return Err(anyhow!("The model returned an empty answer"));
            }
            self.step(id, mode, "inserting", None);
            let copied_only = self.insert(&result, &settings).await?;
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
                ..Default::default()
            });
            return Ok(Outcome::Done { preview: preview(&result), copied_only });
        }

        // Dictation.
        let Some((raw, secs)) = self.transcribe(&settings, captured).await? else {
            return Ok(Outcome::Nothing("No speech detected".into()));
        };
        let d = &settings.dictation;
        let mut text = raw.clone();
        let mut llm_model = None;
        if d.polish && !text.trim().is_empty() {
            self.step(id, mode, "polishing", None);
            match self.llm(&settings, &d.polish_model, &d.polish_prompt, format!("<transcript>\n{text}\n</transcript>"), 0.0).await {
                Ok(t) if !t.trim().is_empty() => {
                    text = t;
                    llm_model = Some(d.polish_model.model.clone());
                }
                Ok(_) => {}
                Err(e) => log::warn!("polish failed, inserting the raw transcript: {e:#}"),
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
        let copied_only = self.insert(&text, &settings).await?;
        self.record(Entry {
            kind: "dictation".into(),
            text: text.trim_end().to_string(),
            transcript: raw,
            audio_secs: secs,
            latency_ms: started.elapsed().as_millis() as u64,
            stt_model: d.stt.model.clone(),
            llm_model,
            inserted: !copied_only,
            ..Default::default()
        });
        Ok(Outcome::Done { preview: preview(&text), copied_only })
    }

    fn record(&self, entry: Entry) {
        let g = self.settings.read().general.clone();
        let saved = self.history.add(entry, g.history_enabled, g.history_limit);
        let _ = self.app.emit("od://history", &saved);
    }

    /// Applies a saved prompt to the selected text without speaking.
    fn quick_action(self: &Arc<Self>, action_id: String) {
        if !matches!(*self.phase.lock(), Phase::Idle) {
            return;
        }
        let settings = self.settings.read().clone();
        let Some(action) = settings.command.quick_actions.iter().find(|a| a.id == action_id).cloned() else {
            log::warn!("unknown quick action '{action_id}'");
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let engine = self.clone();
        let task = tauri::async_runtime::spawn(async move {
            let started = Instant::now();
            let run = async {
                let mut st = engine.status("processing", Mode::Action, id);
                st.message = Some(action.name.clone());
                st.step = Some("thinking");
                engine.emit_status(st);
                if settings.general.show_overlay {
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
                let user = format!(
                    "<instruction>\n{}\n</instruction>\n\n<selected_text>\n{selection}\n</selected_text>",
                    action.prompt
                );
                let result = engine.llm(&settings, &c.llm, &c.prompt_selection, user, c.temperature).await?;
                let copied_only = engine.insert(&result, &settings).await?;
                engine.record(Entry {
                    kind: "action".into(),
                    text: result.clone(),
                    transcript: action.name.clone(),
                    selection: Some(selection.chars().take(2000).collect()),
                    latency_ms: started.elapsed().as_millis() as u64,
                    llm_model: Some(c.llm.model.clone()),
                    inserted: !copied_only,
                    ..Default::default()
                });
                Ok(Outcome::Done { preview: preview(&result), copied_only })
            };
            let result = run.await;
            engine.finish(id, Mode::Action, result);
        });
        *self.phase.lock() = Phase::Processing { id, abort: task.inner().abort_handle() };
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

    #[test]
    fn hallucinations() {
        assert!(is_hallucination("Thank you.", 1.0));
        assert!(!is_hallucination("Thank you.", 5.0));
        assert!(!is_hallucination("Thank you for the report", 1.0));
    }
}
