use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::factory::PluginFactory;
use clack_host::prelude::*;
use clack_host::utils::Cookie;

use clack_plugin_gain::clap_entry;

#[test]
pub fn it_works() {
    // Initialize host
    //let mut host = TestHost::instantiate(&clap_entry);
    // Initialize host with basic info
    let info = HostInfo::new("test", "", "", "").unwrap();

    // Get plugin entry from the exported static
    // SAFETY: only called this once here
    let bundle = unsafe { PluginBundle::load_from_raw(&clap_entry, "") }.unwrap();

    let descriptor = bundle
        .get_factory::<PluginFactory>()
        .unwrap()
        .plugin_descriptor(0)
        .unwrap();

    assert_eq!(
        descriptor.id().unwrap().to_bytes(),
        b"org.rust-audio.clack.gain"
    );
    assert_eq!(descriptor.name().unwrap().to_bytes(), b"Clack Gain Example");

    assert!(descriptor.vendor().is_none());
    assert!(descriptor.url().is_none());
    assert!(descriptor.manual_url().is_none());
    assert!(descriptor.support_url().is_none());
    assert!(descriptor.description().is_none());
    assert!(descriptor.version().is_none());

    assert_eq!(
        descriptor
            .features()
            .map(|s| s.to_bytes())
            .collect::<Vec<_>>(),
        &[&b"audio-effect"[..], &b"stereo"[..]]
    );

    // Instantiate the desired plugin
    let mut plugin = PluginInstance::<TestHostHandlers>::new(
        |_| TestHostShared,
        |_| TestHostMainThread,
        &bundle,
        descriptor.id().unwrap(),
        &info,
    )
    .unwrap();

    let mut plugin_main_thread = plugin.plugin_handle();
    let ports_ext = plugin_main_thread
        .get_extension::<PluginAudioPorts>()
        .unwrap();
    assert_eq!(1, ports_ext.count(&mut plugin_main_thread, true));
    assert_eq!(1, ports_ext.count(&mut plugin_main_thread, false));

    let mut buf = AudioPortInfoBuffer::new();
    let info = ports_ext
        .get(&mut plugin_main_thread, 0, false, &mut buf)
        .unwrap();

    assert_eq!(info.id, 0);
    assert_eq!(info.name, b"main");

    // Setting up some buffers
    let configuration = PluginAudioConfiguration {
        sample_rate: 44_100.0,
        min_frames_count: 32,
        max_frames_count: 32,
    };

    let processor = plugin
        .activate(|_, _| TestHostAudioProcessor, configuration)
        .unwrap();

    assert!(plugin.is_active());

    let mut input_events = EventBuffer::with_capacity(10);
    let mut output_events = EventBuffer::with_capacity(10);

    input_events.push(&ParamValueEvent::new(
        0,
        ClapId::new(1),
        Pckn::match_all(),
        0.5,
        Cookie::empty(),
    ));

    let mut input_buffers = [vec![69f32; 32], vec![69f32; 32]];
    let mut output_buffers = [vec![0f32; 32], vec![0f32; 32]];

    let mut processor = processor.start_processing().unwrap();

    let mut inputs_descriptors = AudioPorts::with_capacity(2, 1);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    let input_channels = inputs_descriptors.with_input_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_input_only(
            input_buffers.iter_mut().map(InputChannel::variable),
        ),
        latency: 0,
    }]);

    let output_channels = outputs_descriptors.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(
            output_buffers.iter_mut().map(|b| b.as_mut_slice()),
        ),
        latency: 0,
    }]);

    processor
        .process(
            &input_channels,
            &output_channels,
            &input_events.as_input(),
            &mut output_events.as_output(),
            None,
            None,
        )
        .unwrap();

    // Check the gain was applied properly
    for channel_index in 0..1 {
        let inbuf = &input_buffers[channel_index];
        let outbuf = &output_buffers[channel_index];
        for (input, output) in inbuf.iter().zip(outbuf.iter()) {
            assert_eq!(*output, *input * 0.5)
        }
    }

    plugin.deactivate(processor.stop_processing());
}

struct TestHostMainThread;
struct TestHostShared;
struct TestHostAudioProcessor;
struct TestHostHandlers;

impl SharedHandler<'_> for TestHostShared {
    fn request_restart(&self) {
        unimplemented!()
    }

    fn request_process(&self) {
        unimplemented!()
    }

    fn request_callback(&self) {
        unimplemented!()
    }
}

impl AudioProcessorHandler<'_> for TestHostAudioProcessor {}

impl MainThreadHandler<'_> for TestHostMainThread {}

impl HostHandlers for TestHostHandlers {
    type Shared<'a> = TestHostShared;
    type MainThread<'a> = TestHostMainThread;
    type AudioProcessor<'a> = TestHostAudioProcessor;
}
