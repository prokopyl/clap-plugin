#![doc = include_str!("../README.md")]
#![doc(html_logo_url = "https://raw.githubusercontent.com/prokopyl/clack/main/logo.svg")]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(missing_docs)]

#[macro_use]
pub mod entry;
pub mod extensions;
pub mod factory;
pub mod host;
pub mod plugin;
pub mod process;

pub(crate) mod internal_utils;

pub use clack_common::events;
pub use clack_common::stream;
pub use clack_common::utils;

/// A helpful prelude re-exporting all the types related to plugin implementation.
pub mod prelude {
    pub use crate::{
        clack_export_entry,
        entry::{DefaultPluginFactory, Entry, EntryDescriptor, SinglePluginEntry},
        events::{
            io::{InputEvents, OutputEvents},
            Event, EventHeader, Pckn, UnknownEvent,
        },
        extensions::PluginExtensions,
        host::{HostAudioProcessorHandle, HostMainThreadHandle, HostSharedHandle},
        plugin::{
            Plugin, PluginAudioProcessor, PluginDescriptor, PluginError, PluginMainThread,
            PluginShared,
        },
        process::{
            audio::{AudioBuffer, ChannelPair, SampleType},
            Audio, Events, PluginAudioConfiguration, Process, ProcessStatus,
        },
        utils::ClapId,
    };
}

#[doc = include_str!("../../README.md")]
const _MAIN_README_TEST: () = {};
