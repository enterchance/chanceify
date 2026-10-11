//! The audio state shared between the player's thread and the interface:
//! the sound on its way out, the equalizer, and the playback speed.

use std::sync::Arc;

use crate::vis::AudioTap;

pub struct AudioShared {
    /// The sound on its way out, for the visualisers.
    pub tap: Arc<AudioTap>,
    /// The equalizer as the player's thread reads it.
    pub eq: crate::eq::SharedEq,
    /// The playback speed, shared with whatever sets it.
    pub speed: crate::speed::SharedSpeed,
}

impl AudioShared {
    pub fn new(tap: Arc<AudioTap>, eq: crate::eq::SharedEq, speed: crate::speed::SharedSpeed) -> Self {
        Self { tap, eq, speed }
    }
}
