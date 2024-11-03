//! Implementations and helpers for our polyphonic oscillator.

use crate::oscillator::SquareOscillator;
use crate::params::PARAM_VOLUME_ID;
use clack_plugin::events::event_types::{
    NoteOffEvent, NoteOnEvent, ParamModEvent, ParamValueEvent,
};
use clack_plugin::events::Match;
use clack_plugin::process::audio::AudioBuffer;

/// A voice in the polyphonic oscillator.
///
/// It contains Channel, Key and NoteID information, so that this voice can be found and targeted
/// by polyphonic modulation.
///
/// It also stores dedicated value and modulation for the polyphonic volume parameter, if the host
/// set it.
#[derive(Copy, Clone)]
struct Voice {
    /// The oscillator itself.
    oscillator: SquareOscillator,
    /// The MIDI channel of the note this voice is playing.
    channel: u8,
    /// The MIDI number of the note this voice is playing.
    key_number: u8,
    /// The unique ID of the note this voice is playing.
    /// This is None if no ID was assigned to this note by the host.
    note_id: Option<u32>,

    /// The voice-specific value of the volume parameter.
    /// This is None if the host didn't apply polyphonic modulation to this voice.
    volume: Option<f32>,

    /// The voice-specific modulation amount of the volume parameter.
    /// This is None if the host didn't apply polyphonic modulation to this voice.
    volume_mod: Option<f32>,
}

impl Voice {
    /// Returns whether this voice matches the given matchers.
    #[inline]
    fn matches(&self, channel: Match<u16>, note_key: Match<u16>, note_id: Match<u32>) -> bool {
        if !channel.matches(self.channel) {
            return false;
        }

        if !note_key.matches(self.key_number) {
            return false;
        }

        note_id.matches(match self.note_id {
            None => Match::All,
            Some(id) => Match::Specific(id),
        })
    }
}

/// A simple polyphonic oscillator.
///
/// It tracks multiple oscillator voices, up to a given maximum.
///
/// This struct manages the buffer so that active voices are at the beginning, and inactive ones
/// at the end of the buffer. Then, to iterate only on the inactive voices, once can simply iterate
/// on the `0..active_voice_count` range.
pub struct PolyOscillator {
    /// The fixed buffer of voices.
    voice_buffer: Box<[Voice]>,
    /// The number of current
    active_voice_count: usize,
}

impl PolyOscillator {
    /// Initializes the oscillators with the given sample rate, and allocates the buffer to handle
    /// the given number of voices.
    pub fn new(voice_count: usize, sample_rate: f32) -> Self {
        Self {
            voice_buffer: vec![
                Voice {
                    oscillator: SquareOscillator::new(sample_rate),
                    channel: 0,
                    key_number: 0,
                    note_id: None,
                    volume: None,
                    volume_mod: None,
                };
                voice_count
            ]
            .into_boxed_slice(),
            active_voice_count: 0,
        }
    }

    /// Starts a new voice, playing the given MIDI note key.
    ///
    /// If there are no more voices available, this does nothing.
    fn start_new_voice(&mut self, channel: u8, new_note_key: u8, note_id: Option<u32>) {
        // Skip the event if we are out of voices
        let Some(available_voice) = self.voice_buffer.get_mut(self.active_voice_count) else {
            return;
        };

        available_voice.oscillator.reset();
        available_voice.oscillator.set_note_number(new_note_key);
        available_voice.channel = channel;
        available_voice.key_number = new_note_key;
        available_voice.note_id = note_id;

        self.active_voice_count += 1;
    }

    /// Stops all voices that match the given MIDI note key and note ID matcher.
    ///
    /// If no matching voice is found, this does nothing.
    fn stop_voices(&mut self, channel: Match<u16>, note_key: Match<u16>, note_id: Match<u32>) {
        while let Some(voice_index) = self
            .active_voice_buffer()
            .iter()
            .position(|v| v.matches(channel, note_key, note_id))
        {
            // Swap the targeted voice with the last one.
            self.voice_buffer
                .swap(voice_index, self.active_voice_count - 1);

            // Remove the last voice from the active pool.
            self.active_voice_count -= 1;
        }
    }

    /// Stops all active voices.
    pub fn stop_all(&mut self) {
        self.active_voice_count = 0;
    }

    /// Handles the given Note On input event.
    pub fn handle_note_on(&mut self, event: &NoteOnEvent) {
        dbg!(event);
        if !event.port_index().matches(0u16) {
            return;
        }

        if let (Match::Specific(channel), Match::Specific(key)) = (event.channel(), event.key()) {
            self.start_new_voice(channel as u8, key as u8, event.note_id().into_specific())
        }
    }

    /// Handles the given Note Off input event.
    pub fn handle_note_off(&mut self, event: &NoteOffEvent) {
        if !event.port_index().matches(0u16) {
            return;
        }

        self.stop_voices(event.channel(), event.key(), event.note_id())
    }

    /// Handles the given polyphonic Parameter Value event.
    pub fn handle_param_value(&mut self, event: &ParamValueEvent) {
        if !event.port_index().matches(0u16) {
            return;
        }

        if event.param_id() != PARAM_VOLUME_ID {
            return;
        }

        for voice in self
            .active_voice_buffer_mut()
            .iter_mut()
            .filter(|v| v.matches(event.channel(), event.key(), event.note_id()))
        {
            voice.volume = Some(event.value() as f32);
        }
    }

    /// Handles the given polyphonic Parameter Modulation event.
    pub fn handle_param_mod(&mut self, event: &ParamModEvent) {
        if !event.port_index().matches(0u16) {
            return;
        }

        if event.param_id() != PARAM_VOLUME_ID {
            return;
        }

        for voice in self
            .active_voice_buffer_mut()
            .iter_mut()
            .filter(|v| v.matches(event.channel(), event.key(), event.note_id()))
        {
            voice.volume_mod = Some(event.amount() as f32);
        }
    }

    /// Generates the next batch of samples of all the currently active oscillators.
    /// Each voice will play at the given volume.
    ///
    /// This method assumes the buffer is initialized with `0`s.
    pub fn generate_next_samples(
        &mut self,
        output_buffer: &AudioBuffer<f32>,
        global_volume: f32,
        global_volume_mod: f32,
    ) {
        for voice in self.active_voice_buffer_mut() {
            let volume = voice.volume.unwrap_or(global_volume);
            let volume_mod = voice.volume_mod.unwrap_or(global_volume_mod);

            voice
                .oscillator
                .add_next_samples_to_buffer(output_buffer, volume + volume_mod);
        }
    }

    /// Returns `true` if any voices are currently playing, `false` otherwise.
    #[inline]
    pub fn has_active_voices(&self) -> bool {
        self.active_voice_count > 0
    }

    /// Returns a shared reference to the part of the buffer that only contains the active voices.
    #[inline]
    fn active_voice_buffer(&self) -> &[Voice] {
        &self.voice_buffer[..self.active_voice_count]
    }

    /// Returns a mutable reference to the part of the buffer that only contains the active voices.
    #[inline]
    fn active_voice_buffer_mut(&mut self) -> &mut [Voice] {
        &mut self.voice_buffer[..self.active_voice_count]
    }
}
