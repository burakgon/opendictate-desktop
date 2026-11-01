//! UI sounds through NSSound: the chimes are rendered once, then replayed from memory without
//! opening an audio stream or spawning a thread each time.

use std::{
    cell::RefCell,
    collections::{hash_map::Entry, HashMap},
};

use objc2::{rc::Retained, AllocAnyThread};
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;

use crate::audio::Sound;

thread_local! {
    /// Main thread only.
    static CACHE: RefCell<HashMap<u8, Retained<NSSound>>> = RefCell::new(HashMap::new());
}

pub fn play(sound: Sound, volume: f32) {
    super::on_main(move || {
        CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let s = match cache.entry(sound as u8) {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => {
                    let wav = match crate::audio::sound_wav(sound) {
                        Ok(w) => w,
                        Err(err) => {
                            log::debug!("could not render sound: {err:#}");
                            return;
                        }
                    };
                    let Some(s) = NSSound::initWithData(NSSound::alloc(), &NSData::with_bytes(&wav)) else { return };
                    e.insert(s)
                }
            };
            s.stop();
            s.setVolume(volume.clamp(0.0, 1.0));
            s.play();
        });
    });
}
