use crate::host::CpalHost;
use clack_host::prelude::*;
use clack_host::process::StartedPluginAudioProcessor;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BuildStreamError, Device, FromSample, OutputCallbackInfo, SampleFormat, Stream, StreamConfig,
};
use std::error::Error;

/// Handling of audio buffers.
mod buffers;
/// Negociation for audio stream and port configuration.
mod config;
/// MIDI handling.
mod midi;

use buffers::*;
use config::*;
use midi::*;

/// Activates the given plugin instance, and outputs its processed audio to a new CPAL stream.
pub fn activate_to_stream(
    instance: &mut PluginInstance<CpalHost>,
) -> Result<Stream, Box<dyn Error>> {
    // Initialize CPAL
    let cpal_host = cpal::default_host();

    let output_device = cpal_host.default_output_device().unwrap();

    let config = FullAudioConfig::find_best_from(&output_device, instance)?;
    println!("Using negociated audio output settings: {config}");

    let midi = MidiReceiver::new(44_100, instance)?;

    let plugin_audio_processor = instance
        .activate(|_, _| (), config.as_clack_plugin_config())?
        .start_processing()?;

    let sample_format = config.sample_format;
    let cpal_config = config.as_cpal_stream_config();
    let audio_processor = StreamAudioProcessor::new(plugin_audio_processor, midi, config);

    let stream = build_output_stream_for_sample_format(
        &output_device,
        audio_processor,
        &cpal_config,
        sample_format,
    )?;
    stream.play()?;

    Ok(stream)
}

/// Builds the output stream, with the data processing matching the given sample format.
fn build_output_stream_for_sample_format(
    device: &Device,
    processor: StreamAudioProcessor,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<Stream, BuildStreamError> {
    let err = |e| eprintln!("{e}");

    match sample_format {
        SampleFormat::I8 => {
            device.build_output_stream(config, make_stream_runner::<i8>(processor), err, None)
        }
        SampleFormat::I16 => {
            device.build_output_stream(config, make_stream_runner::<i16>(processor), err, None)
        }
        SampleFormat::I32 => {
            device.build_output_stream(config, make_stream_runner::<i32>(processor), err, None)
        }
        SampleFormat::I64 => {
            device.build_output_stream(config, make_stream_runner::<i64>(processor), err, None)
        }
        SampleFormat::U8 => {
            device.build_output_stream(config, make_stream_runner::<u8>(processor), err, None)
        }
        SampleFormat::U16 => {
            device.build_output_stream(config, make_stream_runner::<u16>(processor), err, None)
        }
        SampleFormat::U32 => {
            device.build_output_stream(config, make_stream_runner::<u32>(processor), err, None)
        }
        SampleFormat::U64 => {
            device.build_output_stream(config, make_stream_runner::<u64>(processor), err, None)
        }
        SampleFormat::F32 => {
            device.build_output_stream(config, make_stream_runner::<f32>(processor), err, None)
        }
        SampleFormat::F64 => {
            device.build_output_stream(config, make_stream_runner::<f64>(processor), err, None)
        }
        f => unimplemented!("Unknown sample format: {f:?}"),
    }
}

/// Creates a stream runner closure that processes the given sample type.
fn make_stream_runner<S: FromSample<f32>>(
    mut audio_processor: StreamAudioProcessor,
) -> impl FnMut(&mut [S], &OutputCallbackInfo) {
    move |data, _info| audio_processor.process(data)
}

/// Holds all of the data, buffers and state that are going to live and get used on the audio thread.
struct StreamAudioProcessor {
    /// The plugin's audio processor.
    audio_processor: StartedPluginAudioProcessor<CpalHost>,
    /// The audio buffers.
    buffers: HostAudioBuffers,
    /// The MIDI event receiver.
    midi_receiver: Option<MidiReceiver>,
    /// A steady frame counter, used by the plugin's process() method.
    steady_counter: u64,
}

impl StreamAudioProcessor {
    /// Initializes the audio thread data.
    pub fn new(
        plugin_instance: StartedPluginAudioProcessor<CpalHost>,
        midi_receiver: Option<MidiReceiver>,
        config: FullAudioConfig,
    ) -> Self {
        Self {
            audio_processor: plugin_instance,
            buffers: HostAudioBuffers::from_config(config),
            midi_receiver,
            steady_counter: 0,
        }
    }

    /// Processes the given output buffer using the loaded plugin.
    ///
    /// Because CPAL gives different, arbitrary buffer lengths for each process call, this method
    /// first ensures the host internal buffers are big enough, and resizes and reallocates them if
    /// necessary.
    ///
    /// This method also collects all the MIDI events that have been received since the last
    /// process call., and feeds them to the plugin.
    pub fn process<S: FromSample<f32>>(&mut self, data: &mut [S]) {
        self.buffers.ensure_buffer_size_matches(data.len());
        let sample_count = self.buffers.cpal_buf_len_to_frame_count(data.len());

        let (ins, outs) = self.buffers.prepare_plugin_buffers(data.len());

        let events = if let Some(midi) = self.midi_receiver.as_mut() {
            midi.receive_all_events(sample_count as u64)
        } else {
            InputEvents::empty()
        };

        match self.audio_processor.process(
            &ins,
            &outs,
            &events,
            &mut OutputEvents::void(),
            Some(self.steady_counter),
            None,
        ) {
            Ok(_) => self.buffers.write_to_cpal_buffer(data),
            Err(e) => eprintln!("{e}"),
        }

        self.steady_counter += sample_count as u64;
    }
}
