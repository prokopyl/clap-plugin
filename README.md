<div align="center"><img src="./logo.svg" width="128px" height="128px" alt="Clack logo"/><h1>Clack</h1></div>

A set of crates offering low-level, safe Rust wrappers to create audio plugins and hosts using
the [CLAP](https://github.com/free-audio/clap) audio plugin API.

This library is made of lightweight, low-level wrappers built on top
of [`clap-sys`](https://github.com/glowcoil/clap-sys),
and is split in two main crates: `clack-plugin`, which allows to implement CLAP plugins, and `clack-host`, which allows
to implement
CLAP hosts. A common, separate `clack-extensions` crate implements all the standard and stable CLAP extensions.

## Who is this crate for?

Clack is first and foremost designed to provide a common, safe and lightweight foundation for higher-level libraries
and tools be built on top of. However, it is also absolutely possible to create fully-featured plugins and hosts using
only Clack if you so choose, or if you do need low-level control and access to the CLAP API.

Because Clack is designed as a very low-level library, it will expose every single one of the CLAP API's details to you,
and let you deal with them. If you want to make a "normal" plugin, you may be better served with high-level, opinionated
libraries such as [NIH-Plug](https://github.com/robbert-vdh/nih-plug), which takes care of much of the "plumbing"
for you, has many built-in features, and will also give you VST3 and Standalone support for free.

As for making CLAP hosts in Rust however, there is (to the author's knowledge) no higher-level alternative
available and functional yet. You can check out the
[Clack host example](https://github.com/prokopyl/clack/tree/main/host/examples/cpal) to learn more about how to make a
CLAP host using Clack, as well as the snippets below and in the `clack-host` documentation.

To learn more about how to make plugins using Clack, check out the examples below, as well as the `clack-plugin`
documentation.

## Features

* **Safe**: The #1 goal of this project is to provide the full functionality of the CLAP plugin APIs through
  fully memory-safe and thread-safe wrappers.

  This allows for making safer, less crash-prone plugins and hosts, but also allows to fully trust Rust's
  type system when making large refactorings or performance optimizations, especially when dealing with CLAP's
  powerful, multithreaded model.
* **Low-level**: When safe designs permit it, Clack aims to be as close as possible to the underlying
  [CLAP](https://github.com/free-audio/clap) C API. Whenever it is safe to do so, it does not make any assumptions or
  interpretations about any operation, and simply passes data through to the user.

  Anything that's possible to do with the CLAP C API, should also be possible to do using only safe Rust and Clack.
* **Lightweight**: Considering the performance sensitivity of audio plugins and hosts, Clack aims to add as little
  overhead as possible to any operation (being a low-level library helps!). It reduces runtime checks and memory
  allocations to an absolute minimum, heavily leveraging zero-cost abstractions or user-provided buffers for example.
* **Extensible**: Following the intent of the underlying CLAP API, Clack allows third-party users and crates to use,
  add, or extend extensions, event types, factories, and more. As a matter of fact, all extensions in the
  `clack-extensions` crate are implemented using tools all Clack users have access to, and users may add or replace
  the provided implementations with their own!
* **Reasonably defensive**: While Clack performs very little extra sanity checks to keep its overhead as low as
  possible,
  it will report any erroneous behavior it stumbles upon to user code if possible, or just log it and fall back to
  the safest option otherwise (e.g. in case of panics originating from user code).
* **Fairly ergonomic**: Despite Clack being a low-level, fast API, it also tries to provide some additional ergonomic
  APIs
  when wrapping the C APIs, as long as they don't have any impact on performance or low-level capability. Examples
  include using Rust's `Option`, `Result`, allowing the use of iterators, implementing standard traits for e.g. I/O,
  `From` implementations for buffer types, etc.

### Development status

Clack is now in a feature-complete status, however it is still under active development and constant
refinement, therefore APIs can still have breaking changes made to them in the future. Documentation and examples,
while present, are also incomplete and still being actively worked on.

If you notice any issue or limitation with the documentation, examples, or APIs, please
[file a new issue](https://github.com/prokopyl/clack/issues/new)!

## `clack-plugin` example

This example code implements a very simple gain plugin that amplifies every input by `2.0`. More involved
examples are available in the [examples](./plugin/examples) directory, and in the crate's documentation.

```rust
use clack_plugin::prelude::*;

pub struct MyGainPlugin;

impl Plugin for MyGainPlugin {
    type AudioProcessor<'a> = MyGainPluginAudioProcessor;

    type Shared<'a> = ();
    type MainThread<'a> = ();
}

impl DefaultPluginFactory for MyGainPlugin {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("org.rust-audio.clack.gain", "Clack Gain Example")
    }

    fn new_shared(_host: HostSharedHandle) -> Result<Self::Shared<'_>, PluginError> {
        Ok(())
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        _shared: &'a Self::Shared<'a>,
    ) -> Result<Self::MainThread<'a>, PluginError> {
        Ok(())
    }
}

pub struct MyGainPluginAudioProcessor;

impl<'a> PluginAudioProcessor<'a, (), ()> for MyGainPluginAudioProcessor {
    fn activate(_host: HostAudioProcessorHandle<'a>, _main_thread: &mut (), _shared: &'a (), _audio_config: PluginAudioConfiguration) -> Result<Self, PluginError> {
        Ok(Self)
    }

    fn process(&mut self, _process: Process, audio: Audio, _events: Events) -> Result<ProcessStatus, PluginError> {
        for mut port_pair in audio {
            // For this example, we'll only care about 32-bit sample data.
            let Some(channel_pairs) = port_pair.channels()?.to_f32() else { continue; };

            for channel_pair in channel_pairs {
                match channel_pair {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(buf) => buf.fill(0.0),
                    ChannelPair::InputOutput(input, output) => {
                        for (input, output) in input.iter().zip(output) {
                            output.set(input.get() * 2.0)
                        }
                    }
                    ChannelPair::InPlace(buf) => {
                        for sample in buf {
                            sample.set(sample.get() * 2.0)
                        }
                    }
                }
            }
        }

        Ok(ProcessStatus::Continue)
    }
}

clack_export_entry!(SinglePluginEntry<MyGainPlugin>);
```

## `clack-host` example

This example implements a very simple host, which loads a specific plugin and processes a couple of
samples with it.

For a more featured and functional example, check out the
[CPAL-based host example](https://github.com/prokopyl/clack/tree/main/host/examples/cpal).

More details and short examples are also available in the `clack-host` crate documentation.

```rust
use clack_host::events::event_types::*;
use clack_host::prelude::*;

// Prepare our (extremely basic) host implementation

struct MyHostShared;

impl<'a> SharedHandler<'a> for MyHostShared {
    /* ... */
    fn request_restart(&self) { /* ... */ }
    fn request_process(&self) { /* ... */ }
    fn request_callback(&self) { /* ... */ }
}

struct MyHost;

impl HostHandlers for MyHost {
    type Shared<'a> = MyHostShared;

    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

pub fn load_and_process() -> Result<(), Box<dyn std::error::Error>> {
    // Information about our totally legit host.
    let host_info = HostInfo::new("Legit Studio", "Legit Ltd.", "https://example.com", "4.3.2")?;

    let bundle = unsafe { PluginBundle::load("/home/user/.clap/u-he/libdiva.so")? };
    let plugin_factory = bundle.get_plugin_factory().unwrap();

    let plugin_descriptor = plugin_factory.plugin_descriptors()
        .find(|d| d.id().unwrap().to_bytes() == b"com.u-he.diva")
        .unwrap();

    let mut plugin_instance = PluginInstance::<MyHost>::new(
        |_| MyHostShared,
        |_| (),
        &bundle,
        plugin_descriptor.id().unwrap(),
        &host_info
    )?;

    let audio_configuration = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: 4,
        max_frames_count: 4,
    };
    let audio_processor = plugin_instance.activate(|_, _| (), audio_configuration)?;

    let note_on_event = NoteOnEvent::new(0, Pckn::new(0u16, 0u16, 12u16, 60u32), 4.2);
    let input_events_buffer = [note_on_event];
    let mut output_events_buffer = EventBuffer::new();

    let mut input_audio_buffers = [[0.0f32; 4]; 2]; // 2 channels (stereo), 1 port
    let mut output_audio_buffers = [[0.0f32; 4]; 2];

    let mut input_ports = AudioPorts::with_capacity(2, 1); // 2 channels (stereo), 1 port
    let mut output_ports = AudioPorts::with_capacity(2, 1);

    // Let's send the audio processor to a dedicated audio processing thread.
    let audio_processor = std::thread::scope(|s| s.spawn(|| {
        let mut audio_processor = audio_processor.start_processing().unwrap();

        let input_events = InputEvents::from_buffer(&input_events_buffer);
        let mut output_events = OutputEvents::from_buffer(&mut output_events_buffer);

        let mut input_audio = input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                input_audio_buffers.iter_mut().map(|b| InputChannel::constant(b))
            )
        }]);

        let mut output_audio = output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                output_audio_buffers.iter_mut().map(|b| b.as_mut_slice())
            )
        }]);

        // Finally do the processing itself.
        let status = audio_processor.process(
            &input_audio,
            &mut output_audio,
            &input_events,
            &mut output_events,
            None,
            None
        ).unwrap();

        // Send the audio processor back to be deallocated by the main thread.
        audio_processor.stop_processing()
    }).join().unwrap());

    plugin_instance.deactivate(audio_processor);
    Ok(())
}
 ```

## License

`clack` is distributed under the terms of both the [MIT license](LICENSE-MIT) and
the [Apache license, version 2.0](LICENSE-APACHE).
Contributions are accepted under the same terms.

### Special Thanks

The Clack logo was created by [jaxter184](https://github.com/jaxter184) and provided under a CC-BY-SA license.
