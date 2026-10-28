//! Local history of dictations and commands, plus lifetime statistics.

use std::{fs, path::PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub id: u64,
    /// Unix time in milliseconds.
    pub ts: u64,
    /// "dictation" | "command" | "action"
    pub kind: String,
    /// Text that was inserted.
    pub text: String,
    /// Raw speech-to-text output (the instruction, for commands).
    pub transcript: String,
    pub selection: Option<String>,
    pub audio_secs: f32,
    pub latency_ms: u64,
    pub stt_model: String,
    pub llm_model: Option<String>,
    pub words: usize,
    pub inserted: bool,
    /// Application the text was typed into, when known.
    pub app: Option<String>,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            id: 0,
            ts: 0,
            kind: "dictation".into(),
            text: String::new(),
            transcript: String::new(),
            selection: None,
            audio_secs: 0.0,
            latency_ms: 0,
            stt_model: String::new(),
            llm_model: None,
            words: 0,
            inserted: true,
            app: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Stats {
    pub words: u64,
    pub audio_secs: f64,
    pub dictations: u64,
    pub commands: u64,
    /// Sum of processing latency, for the average.
    pub latency_ms_total: u64,
    pub first_use: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct Store {
    stats: Stats,
    entries: Vec<Entry>,
}

pub struct History {
    path: PathBuf,
    store: Mutex<Store>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

impl History {
    pub fn load(path: PathBuf) -> Self {
        let store = fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { path, store: Mutex::new(store) }
    }

    fn persist(&self, store: &Store) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string(store) {
            if fs::write(&tmp, text).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
                }
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn add(&self, mut entry: Entry, keep: bool, limit: usize) -> Entry {
        let mut store = self.store.lock();
        entry.ts = now_ms();
        entry.id = entry.ts;
        if let Some(last) = store.entries.first() {
            if entry.id <= last.id {
                entry.id = last.id + 1;
            }
        }
        entry.words = count_words(&entry.text);
        let s = &mut store.stats;
        if s.first_use == 0 {
            s.first_use = entry.ts;
        }
        s.words += entry.words as u64;
        s.audio_secs += entry.audio_secs as f64;
        s.latency_ms_total += entry.latency_ms;
        if entry.kind == "dictation" {
            s.dictations += 1;
        } else {
            s.commands += 1;
        }
        if keep {
            store.entries.insert(0, entry.clone());
            store.entries.truncate(limit.max(1));
        }
        self.persist(&store);
        entry
    }

    pub fn list(&self) -> Vec<Entry> {
        self.store.lock().entries.clone()
    }

    pub fn delete(&self, id: u64) {
        let mut store = self.store.lock();
        store.entries.retain(|e| e.id != id);
        self.persist(&store);
    }

    pub fn clear(&self) {
        let mut store = self.store.lock();
        store.entries.clear();
        self.persist(&store);
    }

    pub fn stats(&self) -> Stats {
        self.store.lock().stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words() {
        assert_eq!(count_words("Hello, world - this is   a test."), 6);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn add_and_limit() {
        let dir = std::env::temp_dir().join(format!("od-hist-{}", now_ms()));
        let h = History::load(dir.join("history.json"));
        for i in 0..5 {
            h.add(Entry { text: format!("entry {i}"), ..Default::default() }, true, 3);
        }
        assert_eq!(h.list().len(), 3);
        assert_eq!(h.stats().words, 10);
        assert_eq!(h.stats().dictations, 5);
        let h2 = History::load(dir.join("history.json"));
        assert_eq!(h2.list().len(), 3);
        let _ = fs::remove_dir_all(dir);
    }
}
