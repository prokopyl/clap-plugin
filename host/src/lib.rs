#![doc(html_logo_url = "https://raw.githubusercontent.com/prokopyl/clack/main/logo.svg")]
#![deny(clippy::undocumented_unsafe_blocks)]

//!
//! A low-level library to create [CLAP](https://github.com/free-audio/clap) audio hosts in safe Rust.
//!
//! TODO: make crate general description.
//!
//! # Plugin Lifecycle
//!
//! CLAP hosts and plugins go through a specific set of steps before they are able to process audio.
//! The general lifecycle is explained below, with links to more detailed documentation for each
//! step. Also see the [example](#example) section below for a code snippet presenting how this
//! lifecycle can be generally implemented using Clack.
//!
//! Before starting to deal with the plugins themselves, CLAP hosts need implementations for all the
//! different callbacks a plugin may use to interact with the host. This is done by implementing the
//! [`Host`](host::HostHandlers) trait, and its associated sub-types (one for each thread specification).
//! See the [`host`] module documentation for more information about how to implement the
//! [`Host`](host::HostHandlers) trait, and on CLAP's thread specifications model.
//!
//! 1. CLAP plugins are distributed in binary files called bundles. These are prebuilt
//!    dynamically-loaded libraries (with a `.clap` extension), and can contain the implementations
//!    of multiple plugins. They can be loaded with the [`PluginBundle::load`](bundle::PluginBundle::load)
//!    method. See the [`bundle`] module documentation for more information.
//! 2. Bundles' entry points can expose multiple [factories](factory::FactoryPointer). These are singleton
//!    objects that can provide various functionality. The one of main interest is the
//!    [`PluginFactory`](factory::PluginFactory), which exposes methods to list and instantiate plugins.
//!    It can be retrieved from a [`PluginBundle`](bundle::PluginBundle) using the
//!    [`PluginBundle::get_plugin_factory`](bundle::PluginBundle::get_plugin_factory) method.
//!
//!    See the [`factory`] module documentation to learn more about factories.
//! 3. The [`PluginFactory`](factory::PluginFactory) can be used to list
//!    [`PluginDescriptor`s](factory::PluginDescriptor), each of which contains various information
//!    (displayed name, author, etc.) about the plugins included in this bundle, including their
//!    unique IDs. These can be displayed in a list for the user to chose from.
//! 4. The selected plugin's ID can now be used to create a new
//!    [`PluginInstance`](plugin::PluginInstance) using its
//!    [`new`](plugin::PluginInstance::new) method. This is also where the [`Host`](host::HostHandlers)
//!    types come into play, as they need to be ready to handle the plugin instance's callbacks.
//!
//!    See the [`PluginInstance::new`](plugin::PluginInstance::new) method's documentation for
//!    for more detail.
//! 5. The plugin instance now needs to be activated for audio processing, using the
//!    [`activate`](plugin::PluginInstance::activate) method. This method receives the current
//!    [`PluginAudioConfiguration`](process::PluginAudioConfiguration), which allows it to
//!    allocate its buffers with proper sizes depending on sample rate, for instance. This is also
//!    where the host should allocate its own audio and event buffers (see
//!    [`EventBuffers`](events::io::EventBuffer) and
//!    [`AudioPorts`](process::audio_buffers::AudioPorts)), and also where the
//!    [`AudioProcessor`](host::HostHandlers::AudioProcessor) type is created to handle audio processing
//!    callbacks.
//!
//!    If the plugin activation is successful, the plugin's
//!    [`StoppedAudioProcessor`](process::StoppedPluginAudioProcessor) is returned.
//!
//!    All of this allocation always happens on the main thread. Also, because the allocated buffers
//!    are dependent on the given configuration, plugins have to be deactivated and then
//!    re-activated whenever it changes.
//! 6. Once the [`StoppedAudioProcessor`](process::StoppedPluginAudioProcessor) is
//!    created and active, we can send it to another thread dedicated to audio processing, while the
//!    main instance type has to stay on the main thread (it is not [`Send`], while the audio
//!    processor is). Once there, all we need to do is to indicate that continuous processing
//!    is about to start, using the
//!    [`start_processing`](process::StoppedPluginAudioProcessor::start_processing)
//!    method, which consumes the
//!    [`StoppedAudioProcessor`](process::StoppedPluginAudioProcessor) and returns a
//!    [`StartedAudioProcessor`](process::StartedPluginAudioProcessor).
//!
//!    Note that if this pattern of consuming the audio processors is too cumbersome, they can be
//!    converted into a [`PluginAudioProcessor`](process::PluginAudioProcessor) using
//!    the [`Into`] trait, which handles switching between both states using only a `&mut` reference,
//!    at the cost of making the
//!    [`start_processing`](process::PluginAudioProcessor::start_processing),
//!    [`process`](process::StartedPluginAudioProcessor::process), and
//!    [`stop_processing`](process::PluginAudioProcessor::stop_processing) operations
//!    fallible at runtime if the lifecycle isn't properly handled. See the
//!    [`PluginAudioProcessor`](process::PluginAudioProcessor) documentation for more
//!    information.
//! 7. Perform the processing of a block of audio and events using the
//!    [`process`](process::StartedPluginAudioProcessor::process) method.
//!    
//!    Because CLAP Audio and Event buffers are generic, some cheap, short-lived wrappers around the
//!    audio and event buffers must be crated for each process call, in order to be passed to the
//!    plugin's [`process`](process::StartedPluginAudioProcessor::process) method.
//!
//!    Those buffer wrappers are [`InputEvents`](events::io::InputEvents) and
//!    [`OutputEvents`](events::io::OutputEvents) for events, and
//!    [`AudioBuffers`](process::audio_buffers::AudioBuffers) for audio (obtained via a call to
//!    [`AudioPorts::with_input_buffers`](process::audio_buffers::AudioPorts::with_input_buffers) or
//!    [`AudioPorts::with_output_buffers`](process::audio_buffers::AudioPorts::with_output_buffers)
//!    ).
//!
//!    See the documentation of those buffer types for more detail on what types they support, as
//!    well as the [`process`](process::StartedPluginAudioProcessor::process) method's
//!    documentation for more information.
//! 8. Once continuous processing has stopped, the host needs to call
//!    [`start_processing`](process::StartedPluginAudioProcessor::stop_processing),
//!    in a similar fashion to Step 6 above.
//!
//! 9. The audio processor now has to be sent back to the main thread to be deactivated (in order to
//!    not perform de-allocations in the realtime audio thread).
//!    The [`PluginInstance::deactivate`](plugin::PluginInstance::deactivate) can then be used
//!    to consume the Audio Processor. Only then, the
//!    [`PluginInstance`](plugin::PluginInstance) itself can be dropped to destroy the instance
//!    entirely.
//!
//! # Example
//!
//! Note that this example isn't a full implementation and many details are left out: it is meant to
//! give an overview of Clack's APIs and how they relate to a CLAP plugin's lifecycle.
//!
//! For more details instruction on how to create a full, correct Clack host implementation, refer
//! to the submodules' and types' documentation on each topic linked above.
//!
//!```rust
//! use clack_host::events::event_types::*;
//! use clack_host::prelude::*;
//!
//! // Prepare our (extremely basic) host implementation
//!
//! struct MyHostShared;
//!
//! impl<'a> SharedHandler<'a> for MyHostShared {
//!   /* ... */
//!     # fn request_restart(&self) { unimplemented!() }
//!     # fn request_process(&self) { unimplemented!() }
//!     # fn request_callback(&self) { unimplemented!() }
//! }
//!
//! struct MyHost;
//! impl HostHandlers for MyHost {
//!     type Shared<'a> = MyHostShared;
//!
//!     type MainThread<'a> = ();
//!     type AudioProcessor<'a> = ();
//! }
//! # pub fn main() -> Result<(), Box<dyn std::error::Error>> {
//!
//! // Information about our totally legit host.
//! let host_info = HostInfo::new("Legit Studio", "Legit Ltd.", "https://example.com", "4.3.2")?;
//!
//! // Step 1: Load the bundle in memory.
//! # mod diva { include!("./bundle/diva_stub.rs"); }
//! # let bundle = unsafe { PluginBundle::load_from_raw(&diva::DIVA_STUB_ENTRY, "/home/user/.clap/u-he/libdiva.so")? };
//! # /*
//! let bundle = PluginBundle::load("/home/user/.clap/u-he/libdiva.so")?;
//! # */
//!
//! // Step 2: Get the Plugin factory of this bundle.
//! let plugin_factory = bundle.get_plugin_factory().unwrap();
//!
//! // Step 3: Find the descriptor of plugin we're interested in.
//! let plugin_descriptor = plugin_factory.plugin_descriptors()
//!     // We're assuming this specific plugin is in this bundle for this example.
//!     // A real host would store all descriptors in a list and show them to the user.
//!     .find(|d| d.id().unwrap().to_bytes() == b"com.u-he.diva")
//!     .unwrap();
//!
//! // Let's check we are indeed loading the right plugin.
//! assert_eq!(plugin_descriptor.name().unwrap().to_bytes(), b"Diva");
//!
//! // Step 4: Create the plugin instance
//! let mut plugin_instance = PluginInstance::<MyHost>::new(
//!     |_| MyHostShared,
//!     |_| (),
//!     &bundle,
//!     plugin_descriptor.id().unwrap(),
//!     &host_info
//! )?;
//!
//! // Step 5: Activate the plugin (allocating the audio processor),
//! // and allocate associated buffers.
//!
//! // In this example, we will only process 4 samples at a time
//! let audio_configuration = PluginAudioConfiguration {
//!     sample_rate: 48_000.0,
//!     min_frames_count: 4,
//!     max_frames_count: 4,
//! };
//! let audio_processor = plugin_instance.activate(|_, _| (), audio_configuration)?;
//!
//! // Event buffers
//! // For this example, we'll only have a single input event.
//! let note_on_event = NoteOnEvent::new(0, Pckn::new(0u16, 0u16, 12u16, 60u32), 4.2);
//! let input_events_buffer = [note_on_event];
//! let mut output_events_buffer = EventBuffer::new();
//!
//! // Audio buffers
//! let mut input_audio_buffers = [[0.0f32; 4]; 2]; // 2 channels (stereo), 1 port
//! let mut output_audio_buffers = [[0.0f32; 4]; 2];
//!
//! // Audio port buffers
//! let mut input_ports = AudioPorts::with_capacity(2, 1); // 2 channels (stereo), 1 port
//! let mut output_ports = AudioPorts::with_capacity(2, 1);
//!
//! // Let's send the audio processor to a dedicated audio processing thread.
//! let audio_processor = std::thread::scope(|s| s.spawn(|| {
//!    // Step 6: Start the audio processor.
//!    let mut audio_processor = audio_processor.start_processing().unwrap();
//!
//!    // Step 7: Process audio and events.
//!    // Borrow all buffers, and wrap them in cheap CLAP-compatible structs.
//!    let input_events = InputEvents::from_buffer(&input_events_buffer);
//!    let mut output_events = OutputEvents::from_buffer(&mut output_events_buffer);
//!
//!    let mut input_audio = input_ports.with_input_buffers([AudioPortBuffer {
//!        latency: 0,
//!        // We only use F32 (32-bit floating point) audio
//!        channels: AudioPortBufferType::f32_input_only(
//!            // These buffers can be marked constant, as they only contain zeros
//!            input_audio_buffers.iter_mut().map(|b| InputChannel::constant(b))
//!        )
//!    }]);
//!
//!    let mut output_audio = output_ports.with_output_buffers([AudioPortBuffer {
//!        latency: 0,
//!        channels: AudioPortBufferType::f32_output_only(
//!            output_audio_buffers.iter_mut().map(|b| b.as_mut_slice())
//!        )
//!    }]);
//!
//!    // Finally do the processing itself.
//!    let status = audio_processor.process(
//!        &input_audio,
//!        &mut output_audio,
//!        &input_events,
//!        &mut output_events,
//!        None,
//!        None
//!    ).unwrap();
//!
//!    // This plugin has finished processing and requests to be put to sleep.
//!    assert_eq!(status, ProcessStatus::Sleep);
//!
//!    // Step 8: if requested by the plugin or required to by the host or user, stop processing
//!    let audio_processor = audio_processor.stop_processing();
//!
//!    // Send the audio processor back to be deallocated by the main thread.
//!    audio_processor
//! }).join().unwrap());
//!
//! // A real host implementation might forward the results to an output device,
//! // or pass it along a processing chain.
//! // Here, we are only going to check the results manually.
//!
//!
//! // The raw audio for each channel
//! assert_eq!(&[42.0f32, 69.0, 21.0, 34.5], &output_audio_buffers[0]);
//! assert_eq!(&[42.0f32, 69.0, 21.0, 34.5], &output_audio_buffers[1]);
//!
//! // The input note event has been passed through
//! assert_eq!(output_events_buffer.get(0).unwrap(), &note_on_event);
//! assert_eq!(output_events_buffer.len(), 1);
//!
//! // Step 9: stop and deactivate the plugin.
//!
//! plugin_instance.deactivate(audio_processor);
//! # Ok(()) }
//! ```

pub mod bundle;
pub mod extensions;
pub mod factory;
pub mod host;
pub mod plugin;
pub mod process;
mod util;

pub use clack_common::events;
pub use clack_common::stream;
pub use clack_common::utils;

/// A helpful prelude re-exporting all the types related to host implementation.
pub mod prelude {
    pub use crate::{
        bundle::PluginBundle,
        events::{
            io::{EventBuffer, InputEvents, OutputEvents},
            Event, EventHeader, Pckn, UnknownEvent,
        },
        host::{
            AudioProcessorHandler, HostError, HostExtensions, HostHandlers, HostInfo,
            MainThreadHandler, SharedHandler,
        },
        plugin::{
            InitializedPluginHandle, InitializingPluginHandle, PluginAudioProcessorHandle,
            PluginInstance, PluginInstanceError, PluginMainThreadHandle, PluginSharedHandle,
        },
        process::{
            audio_buffers::{
                AudioBuffers, AudioPortBuffer, AudioPortBufferType, AudioPorts, InputChannel,
            },
            AudioPortProcessingInfo, PluginAudioConfiguration, ProcessStatus,
            StoppedPluginAudioProcessor,
        },
        utils::ClapId,
    };
}
