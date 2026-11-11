use std::path::Path;

#[cfg(target_os = "linux")]
pub(super) use super::linux::{game, process_identity, Network};
#[cfg(windows)]
pub(super) use super::windows::{game, process_identity, Network, NETWORK_CAPTURE_SUPPORTED};

pub(super) fn opus_path(game: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        game.parent()
            .unwrap_or_else(|| Path::new(""))
            .join("VRChat_Data/Plugins/x86_64/opus.dll")
    }
    #[cfg(target_os = "linux")]
    {
        let _ = game;
        Path::new("libopus.so.0").to_path_buf()
    }
}

pub(super) fn world(owner: super::cache::ProcessIdentity) -> super::world::WorldWatcher {
    super::world::WorldWatcher::for_process(owner)
}
