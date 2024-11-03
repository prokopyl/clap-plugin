#![doc(html_logo_url = "https://raw.githubusercontent.com/prokopyl/clack/main/logo.svg")]
#![doc = include_str!("../README.md")]
#![deny(missing_docs, clippy::missing_docs_in_private_items, unsafe_code)]

use crate::params::{PolySynthParamModulations, PolySynthParams};
use crate::poly_oscillator::PolyOscillator;
use clack_extensions::state::PluginState;
use clack_extensions::{audio_ports::*, note_ports::*, params::*};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;

mod oscillator;
mod params;
mod poly_oscillator;

/// The type that represents our plugin in Clack.
///
/// This is what implements the [`Plugin`] trait, and where all the other subtypes are attached.
pub struct PolySynthPlugin;

impl Plugin for PolySynthPlugin {
    type AudioProcessor<'a> = PolySynthAudioProcessor<'a>;
    type Shared<'a> = PolySynthPluginShared;
    type MainThread<'a> = PolySynthPluginMainThread<'a>;

    fn declare_extensions(
        builder: &mut PluginExtensions<Self>,
        _shared: Option<&PolySynthPluginShared>,
    ) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<PluginParams>()
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for PolySynthPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new("org.rust-audio.clack.polysynth", "Clack PolySynth Example")
            .with_features([SYNTHESIZER, MONO, INSTRUMENT])
    }

    fn new_shared(_host: HostSharedHandle) -> Result<PolySynthPluginShared, PluginError> {
        Ok(PolySynthPluginShared {
            params: PolySynthParams::new(),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a PolySynthPluginShared,
    ) -> Result<PolySynthPluginMainThread<'a>, PluginError> {
        Ok(PolySynthPluginMainThread { shared })
    }
}

/// Our plugin's audio processor. It lives in the audio thread.
///
/// It receives note and parameter events, and generates a mono output by running the oscillators.
pub struct PolySynthAudioProcessor<'a> {
    /// The oscillator bank.
    poly_osc: PolyOscillator,
    /// The modulation values for the plugin's parameters.
    modulation_values: PolySynthParamModulations,
    /// A reference to the plugin's shared data.
    shared: &'a PolySynthPluginShared,
}

impl<'a> PluginAudioProcessor<'a, PolySynthPluginShared, PolySynthPluginMainThread<'a>>
    for PolySynthAudioProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut PolySynthPluginMainThread,
        shared: &'a PolySynthPluginShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            poly_osc: PolyOscillator::new(16, audio_config.sample_rate as f32),
            modulation_values: PolySynthParamModulations::new(),
            shared,
        })
    }

    fn process(
        &mut self,
        _process: Process,
        audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        // First, we have to make a few sanity checks.
        // We want at least a single output port, which contains at least one channel of `f32`
        // audio sample data.
        let output_port = audio
            .output_port(0)
            .ok_or(PluginError::Message("No output port found"))?;

        let output_channels = output_port
            .channels()?
            .to_f32()
            .ok_or(PluginError::Message("Expected f32 output"))?;

        let output_buffer = output_channels
            .channel(0)
            .ok_or(PluginError::Message("Expected at least one channel"))?;

        // Ensure the buffer is zero-filled, as all oscillators will just add to it.
        output_buffer.fill(0.0);

        // We use the `EventBatcher` to handle incoming events in a sample-accurate way.
        for event_batch in events.input.batch() {
            // Handle all the events (note or param) for this batch.
            for event in event_batch.events() {
                self.handle_event(event);
            }

            // With all the events out of the way, we can now handle a whole batch of sample
            // all at once.
            let output_buffer = &output_buffer[event_batch.sample_bounds()];
            self.poly_osc.generate_next_samples(
                output_buffer,
                self.shared.params.get_volume(),
                self.modulation_values.volume(),
            );
        }

        // If somehow the host didn't give us a mono output, we copy the output to all channels
        if output_channels.channel_count() > 1 {
            // PANIC: we just checked that channel_count is > 1.
            let first_channel = &output_channels[0];

            // Copy the first channel into all the other channels.
            for other_channel in output_channels.iter().skip(1) {
                other_channel.copy_from_buffer(first_channel)
            }
        }

        // Return either the Continue state or the Sleep state, depending on if we have active
        // voices running or not.
        if self.poly_osc.has_active_voices() {
            Ok(ProcessStatus::Continue)
        } else {
            Ok(ProcessStatus::Sleep)
        }
    }

    fn stop_processing(&mut self) {
        // When audio processing stops, we stop all the oscillator voices just in case.
        self.poly_osc.stop_all();
    }
}

impl PolySynthAudioProcessor<'_> {
    /// Handles an incoming event.
    fn handle_event(&mut self, event: &UnknownEvent) {
        match event.as_core_event() {
            Some(CoreEventSpace::NoteOn(event)) => self.poly_osc.handle_note_on(event),
            Some(CoreEventSpace::NoteOff(event)) => self.poly_osc.handle_note_off(event),
            Some(CoreEventSpace::ParamValue(event)) => {
                // This is a global modulation event
                if event.pckn().matches_all() {
                    self.shared.params.handle_event(event)
                } else {
                    self.poly_osc.handle_param_value(event)
                }
            }
            Some(CoreEventSpace::ParamMod(event)) => {
                // This is a global modulation event
                if event.pckn().matches_all() {
                    self.modulation_values.handle_event(event)
                } else {
                    self.poly_osc.handle_param_mod(event)
                }
            }
            _ => {}
        }
    }
}

impl PluginAudioPortsImpl for PolySynthPluginMainThread<'_> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input {
            0
        } else {
            1
        }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if !is_input && index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(1),
                name: b"main",
                channel_count: 1,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::MONO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginNotePortsImpl for PolySynthPluginMainThread<'_> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input {
            1
        } else {
            0
        }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if is_input && index == 0 {
            writer.set(&NotePortInfo {
                id: ClapId::new(1),
                name: b"main",
                preferred_dialect: Some(NoteDialect::Clap),
                supported_dialects: NoteDialects::CLAP,
            })
        }
    }
}

/// The plugin data that gets shared between the Main Thread and the Audio Thread.
pub struct PolySynthPluginShared {
    /// The plugin's parameter values.
    params: PolySynthParams,
}

impl PluginShared<'_> for PolySynthPluginShared {}

/// The data that belongs to the main thread of our plugin.
pub struct PolySynthPluginMainThread<'a> {
    /// A reference to the plugin's shared data.
    shared: &'a PolySynthPluginShared,
}

impl<'a> PluginMainThread<'a, PolySynthPluginShared> for PolySynthPluginMainThread<'a> {}

clack_export_entry!(SinglePluginEntry<PolySynthPlugin>);
