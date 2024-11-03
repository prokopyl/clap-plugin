use clack_host::prelude::*;

mod diva_stub {
    #[cfg(test)]
    #[allow(clippy::items_after_test_module)] // This is not really a test module
    mod clack_extensions {
        pub use crate::*;
    }

    use clack_extensions::state::*;

    use clack_common::stream::{InputStream, OutputStream};
    use clack_plugin::clack_entry;
    use clack_plugin::prelude::*;
    use std::io::{Read, Write};

    pub struct DivaPluginStub;

    pub struct DivaPluginStubAudioProcessor<'a> {
        shared: &'a DivaPluginStubShared<'a>,
    }
    pub struct DivaPluginStubShared<'a> {
        host: HostSharedHandle<'a>,
    }

    pub struct DivaPluginStubMainThread {}

    impl<'a> PluginMainThread<'a, DivaPluginStubShared<'a>> for DivaPluginStubMainThread {}

    impl PluginStateImpl for DivaPluginStubMainThread {
        fn save(&mut self, output: &mut OutputStream) -> Result<(), PluginError> {
            output.write_all(b"Hello, world!")?;
            Ok(())
        }

        fn load(&mut self, input: &mut InputStream) -> Result<(), PluginError> {
            let mut buf = String::new();
            input.read_to_string(&mut buf)?;

            Ok(())
        }
    }

    impl<'a> PluginShared<'a> for DivaPluginStubShared<'a> {}

    impl Plugin for DivaPluginStub {
        type AudioProcessor<'a> = DivaPluginStubAudioProcessor<'a>;
        type Shared<'a> = DivaPluginStubShared<'a>;
        type MainThread<'a> = DivaPluginStubMainThread;

        fn declare_extensions(
            builder: &mut PluginExtensions<Self>,
            _shared: Option<&Self::Shared<'_>>,
        ) {
            builder.register::<PluginState>();
        }
    }

    impl DefaultPluginFactory for DivaPluginStub {
        fn get_descriptor() -> PluginDescriptor {
            use clack_plugin::plugin::features::*;

            PluginDescriptor::new("com.u-he.diva", "Diva").with_features([SYNTHESIZER, STEREO])
        }

        fn new_shared(host: HostSharedHandle) -> Result<Self::Shared<'_>, PluginError> {
            Ok(DivaPluginStubShared { host })
        }

        fn new_main_thread<'a>(
            _host: HostMainThreadHandle<'a>,
            _shared: &'a Self::Shared<'a>,
        ) -> Result<Self::MainThread<'a>, PluginError> {
            Ok(DivaPluginStubMainThread {})
        }
    }

    impl<'a> PluginAudioProcessor<'a, DivaPluginStubShared<'a>, DivaPluginStubMainThread>
        for DivaPluginStubAudioProcessor<'a>
    {
        fn activate(
            _host: HostAudioProcessorHandle<'a>,
            _main_thread: &mut DivaPluginStubMainThread,
            shared: &'a DivaPluginStubShared<'a>,
            _audio_config: PluginAudioConfiguration,
        ) -> Result<Self, PluginError> {
            Ok(Self { shared })
        }

        fn process(
            &mut self,
            _process: Process,
            audio: Audio,
            _events: Events,
        ) -> Result<ProcessStatus, PluginError> {
            self.shared.host.request_callback();

            for event in _events.input {
                _events.output.try_push(event)?;
            }

            let output_channels = audio.output_port(0).unwrap().channels()?;
            let output_buf = output_channels.to_f32().unwrap();

            for channel in output_buf {
                channel.copy_from_slice(&[42.0f32, 69.0, 21.0, 34.5]);
            }
            Ok(ProcessStatus::Sleep)
        }
    }

    #[allow(unused)] // This is only used in doctests
    pub static DIVA_STUB_ENTRY: EntryDescriptor = clack_entry!(SinglePluginEntry<DivaPluginStub>);
}

pub fn get_working_instance<H: HostHandlers, FS, FH>(
    shared: FS,
    main_thread: FH,
) -> Result<PluginInstance<H>, Box<dyn std::error::Error>>
where
    FS: for<'b> FnOnce(&'b ()) -> <H as HostHandlers>::Shared<'b>,
    FH: for<'b> FnOnce(&'b <H as HostHandlers>::Shared<'b>) -> <H as HostHandlers>::MainThread<'b>,
{
    let host_info = HostInfo::new("Legit Studio", "Legit Ltd.", "https://example.com", "4.3.2")?;

    // SAFETY: we're loading our own bundle here
    let bundle = unsafe {
        PluginBundle::load_from_raw(
            &diva_stub::DIVA_STUB_ENTRY,
            "/home/user/.clap/u-he/libdiva.so",
        )?
    };

    let plugin_descriptor = bundle
        .get_plugin_factory()
        .unwrap()
        .plugin_descriptors()
        // We're assuming this specific plugin is in this bundle for this example.
        // A real host would store all descriptors in a list and show them to the user.
        .find(|d| d.id().unwrap().to_bytes() == b"com.u-he.diva")
        .unwrap();

    let plugin_instance = PluginInstance::<H>::new(
        shared,
        main_thread,
        &bundle,
        plugin_descriptor.id().unwrap(),
        &host_info,
    )?;

    Ok(plugin_instance)
}
