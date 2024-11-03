use clack_plugin::clack_entry;
use clack_plugin::prelude::*;

pub struct DivaPluginStub;

pub struct DivaPluginStubAudioProcessor<'a> {
    shared: &'a DivaPluginStubShared<'a>,
}
pub struct DivaPluginStubShared<'a> {
    host: HostSharedHandle<'a>,
}

impl<'a> PluginShared<'a> for DivaPluginStubShared<'a> {}

impl Plugin for DivaPluginStub {
    type AudioProcessor<'a> = DivaPluginStubAudioProcessor<'a>;
    type Shared<'a> = DivaPluginStubShared<'a>;
    type MainThread<'a> = ();
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
        Ok(())
    }
}

impl<'a> PluginAudioProcessor<'a, DivaPluginStubShared<'a>, ()>
    for DivaPluginStubAudioProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut (),
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
