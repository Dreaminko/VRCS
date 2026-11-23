//! Read only lifecycle markers from the current VRChat process's output log.
//! World names are emitted only as the parsed room label; account IDs and the
//! rest of the log contents stay inside this module.
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const READ_CHUNK: u64 = 1024 * 1024;
const LINE_LIMIT: usize = 4096;

pub(super) struct WorldWatcher {
    directory: PathBuf,
    #[cfg(target_os = "linux")]
    pid: Option<u32>,
    error: Option<String>,
    started: SystemTime,
    file: Option<File>,
    dir_modified: Option<SystemTime>,
    path: Option<PathBuf>,
    offset: u64,
    search_end: Option<u64>,
    name_search_end: Option<u64>,
    partial: Vec<u8>,
    ready: bool,
    caught_up: bool,
    identity_stamp: Option<(u64, u64)>,
    world_stamp: Option<(u64, u64)>,
    world_name: Option<String>,
}

impl WorldWatcher {
    #[cfg(windows)]
    pub(super) fn for_process(owner: crate::vrchat_process::ProcessIdentity) -> Self {
        let directory = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("AppData/LocalLow/VRChat/VRChat");
        let ticks = owner.created.saturating_sub(116_444_736_000_000_000);
        Self::new(
            directory,
            SystemTime::UNIX_EPOCH + Duration::from_nanos(ticks.saturating_mul(100)),
        )
    }

    #[cfg(target_os = "linux")]
    pub(super) fn for_process(owner: crate::vrchat_process::ProcessIdentity) -> Self {
        let mut watcher = Self::new(PathBuf::new(), linux_process_start(owner.created));
        watcher.pid = Some(owner.pid);
        watcher
    }

    fn new(directory: PathBuf, started: SystemTime) -> Self {
        Self {
            directory,
            #[cfg(target_os = "linux")]
            pid: None,
            error: None,
            started,
            file: None,
            dir_modified: None,
            path: None,
            offset: 0,
            search_end: None,
            name_search_end: None,
            partial: Vec::new(),
            ready: false,
            caught_up: false,
            identity_stamp: None,
            world_stamp: None,
            world_name: None,
        }
    }

    pub fn poll(&mut self) -> bool {
        self.error = match self.refresh() {
            Ok(()) => None,
            Err(error) => {
                self.ready = false;
                self.file = None;
                self.dir_modified = None;
                Some(format!("Cannot read the current VRChat world log: {error}"))
            }
        };
        self.ready && self.caught_up
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn world_name(&self) -> Option<&str> {
        self.world_name.as_deref()
    }

    /// Cache identity only while the same process, room and membership remain.
    /// Byte position distinguishes repeated joins with identical timestamp/name.
    pub fn room_token(&self) -> Option<u64> {
        self.stamp_token(self.identity_stamp)
    }

    /// Stable across player join/leave events within the same joined world.
    pub fn world_session_token(&self) -> Option<u64> {
        self.stamp_token(self.world_stamp)
    }

    fn stamp_token(&self, stamp: Option<(u64, u64)>) -> Option<u64> {
        if !self.ready || !self.caught_up {
            return None;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.path.as_ref()?.hash(&mut hash);
        stamp?.hash(&mut hash);
        Some(hash.finish())
    }

    fn refresh(&mut self) -> std::io::Result<()> {
        let earliest = self
            .started
            .checked_sub(Duration::from_secs(2))
            .unwrap_or(self.started);
        #[cfg(target_os = "linux")]
        if self.path.is_none()
            && let Some(pid) = self.pid
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            let env_dir = || {
                let environment = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
                linux_log_directory(&environment, &home, earliest)
            };
            self.directory = linux_proc_fd_log_directory(pid, earliest)
                .or_else(env_dir)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "waiting for an output_log in the target Wine/Proton prefix",
                    )
                })?;
        }
        let dir_modified = fs::metadata(&self.directory)?.modified().ok();
        let mut path_changed = false;
        if self.file.is_none() || dir_modified.is_none() || dir_modified != self.dir_modified {
            let newest = fs::read_dir(&self.directory)?
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with("output_log_") || !name.ends_with(".txt") {
                        return None;
                    }
                    let metadata = entry.metadata().ok()?;
                    let created = metadata.created().or_else(|_| metadata.modified()).ok()?;
                    (metadata.is_file() && created >= earliest).then(|| (created, entry.path()))
                })
                .max_by_key(|(created, _)| *created)
                .map(|(_, path)| path);
            let Some(path) = newest else {
                self.file = None;
                self.path = None;
                self.dir_modified = dir_modified;
                self.ready = false;
                self.identity_stamp = None;
                self.world_stamp = None;
                self.world_name = None;
                self.name_search_end = None;
                return Ok(());
            };
            path_changed = self.path.as_ref() != Some(&path);
            if path_changed || self.file.is_none() {
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.share_mode(7);
                }
                self.file = Some(options.open(&path)?);
                self.path = Some(path);
            }
            self.dir_modified = dir_modified;
        }
        let file = self.file.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "VRChat log file missing")
        })?;
        let length = file.metadata()?.len();
        if path_changed || length < self.offset {
            self.identity_stamp = None;
            self.world_stamp = None;
            self.offset = length;
            self.search_end = Some(length);
            self.name_search_end = Some(length);
            self.partial.clear();
            self.ready = false;
            self.world_name = None;
        }
        if length > self.offset {
            let bytes = read_range(file, self.offset, (length - self.offset).min(READ_CHUNK))?;
            self.offset += bytes.len() as u64;
            self.partial.extend(bytes);
            if let Some(end) = self.partial.iter().rposition(|byte| *byte == b'\n') {
                let base = self.offset - self.partial.len() as u64;
                let slice = &self.partial[..=end];
                keep_latest_stamp(&mut self.identity_stamp, identity_marker(slice, base, true));
                keep_latest_stamp(&mut self.world_stamp, identity_marker(slice, base, false));
                if let Some(ready) = last_marker(&self.partial[..=end]) {
                    self.ready = ready;
                    self.search_end = None;
                }
                if let Some(update) = latest_name_update(&self.partial[..=end]) {
                    self.world_name = update.into_name();
                    self.name_search_end = None;
                }
                self.partial.drain(..=end);
            }
            if self.partial.len() > LINE_LIMIT {
                self.partial.clear();
            }
        }
        let mut reused_chunk: Option<(u64, Vec<u8>)> = None;
        if let Some(end) = self.search_end {
            let begin = end.saturating_sub(READ_CHUNK);
            // Overlap covers a marker line split across backwards read blocks.
            let overlap = begin.saturating_sub(LINE_LIMIT as u64);
            let bytes = read_range(file, overlap, end - overlap)?;
            keep_latest_stamp(
                &mut self.identity_stamp,
                identity_marker(&bytes, overlap, true),
            );
            keep_latest_stamp(
                &mut self.world_stamp,
                identity_marker(&bytes, overlap, false),
            );
            if let Some(ready) = last_marker(&bytes) {
                self.ready = ready;
                self.search_end = None;
            } else {
                self.search_end = (begin > 0).then_some(begin);
            }
            if end == length && self.partial.is_empty() {
                if let Some(at) = bytes.iter().rposition(|byte| *byte == b'\n') {
                    if bytes.len() - at <= LINE_LIMIT {
                        self.partial.extend_from_slice(&bytes[at + 1..]);
                    }
                } else if bytes.len() <= LINE_LIMIT {
                    self.partial.extend_from_slice(&bytes);
                }
            }
            reused_chunk = Some((end, bytes));
        }
        if let Some(end) = self.name_search_end {
            let begin = end.saturating_sub(READ_CHUNK);
            let fetched;
            let bytes = match &reused_chunk {
                Some((cached_end, bytes)) if *cached_end == end => bytes.as_slice(),
                _ => {
                    // Overlap covers a name line split across backwards read blocks.
                    let overlap = begin.saturating_sub(LINE_LIMIT as u64);
                    fetched = read_range(file, overlap, end - overlap)?;
                    fetched.as_slice()
                }
            };
            if let Some(update) = latest_name_update(bytes) {
                self.world_name = update.into_name();
                self.name_search_end = None;
            } else {
                self.name_search_end = (begin > 0).then_some(begin);
            }
        }
        // Catch up to the log's current end before treating an older marker as ready.
        self.caught_up = self.offset >= length;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn linux_proc_fd_log_directory(pid: u32, earliest: SystemTime) -> Option<PathBuf> {
    let entries = fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let target = fs::read_link(entry.path()).ok()?;
            let name = target.file_name()?.to_str()?;
            if !name.starts_with("output_log_") || !name.ends_with(".txt") {
                return None;
            }
            let metadata = fs::metadata(&target).ok()?;
            if !metadata.is_file() {
                return None;
            }
            let created = metadata.created().or_else(|_| metadata.modified()).ok()?;
            (created >= earliest).then(|| Some((created, target.parent()?.to_path_buf())))?
        })
        .max_by_key(|(created, _)| *created)
        .map(|(_, directory)| directory)
}

#[cfg(target_os = "linux")]
fn parse_vdf_library_paths(contents: &str) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    for line in contents.lines() {
        let mut tokens = Vec::new();
        let mut rest = line.trim();
        while let Some(start) = rest.find('"') {
            rest = &rest[start + 1..];
            let Some(end) = rest.find('"') else {
                break;
            };
            tokens.push(&rest[..end]);
            rest = &rest[end + 1..];
        }
        if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("path") && !tokens[1].is_empty() {
            let path = tokens[1].replace("\\\\", "\\");
            libraries.push(PathBuf::from(path));
        }
    }
    libraries
}

#[cfg(target_os = "linux")]
fn linux_log_directory(
    environment: &[u8],
    home: &std::path::Path,
    earliest: SystemTime,
) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;

    let variable = |name: &[u8]| {
        environment
            .split(|byte| *byte == 0)
            .find_map(|value| value.strip_prefix(name))
            .map(|value| PathBuf::from(std::ffi::OsStr::from_bytes(value)))
            .filter(|path| path.is_absolute())
    };
    let flatpak_id = environment
        .split(|byte| *byte == 0)
        .find_map(|value| value.strip_prefix(b"FLATPAK_ID="))
        .and_then(|value| std::str::from_utf8(value).ok())
        .filter(|id| !id.is_empty() && !id.contains('/') && !id.contains(".."));
    let mut prefixes = Vec::new();
    let push_unique = |prefixes: &mut Vec<PathBuf>, candidate: PathBuf| {
        if !prefixes.contains(&candidate) {
            prefixes.push(candidate);
        }
    };
    let push_prefix = |prefixes: &mut Vec<PathBuf>, prefix: PathBuf| {
        if let (Some(app_id), Ok(relative)) = (flatpak_id, prefix.strip_prefix(home)) {
            push_unique(prefixes, home.join(".var/app").join(app_id).join(relative));
        }
        push_unique(prefixes, prefix);
    };
    if let Some(prefix) = variable(b"WINEPREFIX=") {
        push_prefix(&mut prefixes, prefix);
    }
    if let Some(prefix) = variable(b"STEAM_COMPAT_DATA_PATH=").map(|path| path.join("pfx")) {
        push_prefix(&mut prefixes, prefix);
    }
    if let Some(prefix) = variable(b"STEAM_COMPAT_INSTALL_PATH=")
        .and_then(|path| Some(path.parent()?.parent()?.join("compatdata/438100/pfx")))
    {
        push_prefix(&mut prefixes, prefix);
    }
    // Only fall back to default Steam/Snap/Flatpak/libraryfolders.vdf prefixes
    // when no explicit prefix env var targeted this VRChat process.
    if prefixes.is_empty() {
        let steam_roots = [
            home.join(".steam/steam"),
            home.join(".local/share/Steam"),
            home.join("snap/steam/common/.local/share/Steam"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ];
        for root in &steam_roots {
            push_unique(&mut prefixes, root.join("steamapps/compatdata/438100/pfx"));
            for vdf_rel in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
                if let Ok(contents) = fs::read_to_string(root.join(vdf_rel)) {
                    for lib in parse_vdf_library_paths(&contents) {
                        push_unique(&mut prefixes, lib.join("steamapps/compatdata/438100/pfx"));
                    }
                }
            }
        }
    }
    prefixes
        .iter()
        .flat_map(|prefix| {
            fs::read_dir(prefix.join("drive_c/users"))
                .into_iter()
                .flatten()
        })
        .filter_map(Result::ok)
        .filter_map(|user| {
            let directory = user.path().join("AppData/LocalLow/VRChat/VRChat");
            let latest = fs::read_dir(&directory)
                .ok()?
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("output_log_") && name.ends_with(".txt")
                })
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .filter_map(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
                .filter(|created| *created >= earliest)
                .max()?;
            Some((latest, directory))
        })
        .max_by_key(|(latest, _)| *latest)
        .map(|(_, directory)| directory)
}

#[cfg(target_os = "linux")]
fn linux_process_start(created: u64) -> SystemTime {
    let seconds = fs::read_to_string("/proc/stat").ok().and_then(|stat| {
        stat.lines()
            .find_map(|line| line.strip_prefix("btime "))
            .and_then(|value| value.trim().parse::<u64>().ok())
    });
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = u64::try_from(hz).unwrap_or(100).max(1);
    seconds
        .map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds + created / hz))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn read_range(file: &mut File, offset: u64, length: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(length).read_to_end(&mut bytes)?;
    Ok(bytes)
}

enum NameUpdate<'a> {
    Name(&'a str),
    Clear,
}

impl NameUpdate<'_> {
    fn into_name(self) -> Option<String> {
        match self {
            Self::Name(name) => Some(name.to_owned()),
            Self::Clear => None,
        }
    }
}

fn name_update(message: &str) -> Option<NameUpdate<'_>> {
    if let Some(rest) = message.strip_prefix("[Behaviour] Entering Room:") {
        let name = rest.trim_prefix(' ');
        return Some(if name.trim().is_empty() {
            NameUpdate::Clear
        } else {
            NameUpdate::Name(name)
        });
    }
    (message == "[Behaviour] OnLeftRoom").then_some(NameUpdate::Clear)
}

fn latest_name_update(bytes: &[u8]) -> Option<NameUpdate<'_>> {
    // An unfinished line is retained for the next poll, not interpreted early.
    let end = bytes.iter().rposition(|byte| *byte == b'\n')?;
    bytes[..end]
        .rsplit(|byte| *byte == b'\n')
        .find_map(|line| log_message(line).and_then(name_update))
}

fn last_marker(bytes: &[u8]) -> Option<bool> {
    // An unfinished line is retained for the next poll, not interpreted early.
    let end = bytes.iter().rposition(|byte| *byte == b'\n')?;
    bytes[..end].rsplit(|byte| *byte == b'\n').find_map(|line| {
        let message = log_message(line)?;
        if message.starts_with("Memory Usage: after world loaded")
            || message.starts_with("[Behaviour] Finished entering world")
        {
            Some(true)
        } else if message.starts_with("[Behaviour] Entering Room:")
            || message.starts_with("[Behaviour] Joining wrld_")
            || message == "[Behaviour] OnLeftRoom"
        {
            Some(false)
        } else {
            None
        }
    })
}

fn log_message(line: &[u8]) -> Option<&str> {
    if line.len() > LINE_LIMIT {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?.trim_end_matches('\r');
    let stamp = line.as_bytes().get(..19)?;
    if stamp[4] != b'.'
        || stamp[7] != b'.'
        || stamp[10] != b' '
        || stamp[13] != b':'
        || stamp[16] != b':'
    {
        return None;
    }
    Some(line.get(19..)?.split_once(" - ")?.1.trim_start())
}
fn keep_latest_stamp(current: &mut Option<(u64, u64)>, next: Option<(u64, u64)>) {
    if next.is_some_and(|next| current.is_none_or(|current| next.0 > current.0)) {
        *current = next;
    }
}
fn identity_marker(bytes: &[u8], base: u64, include_membership: bool) -> Option<(u64, u64)> {
    let end = bytes.iter().rposition(|byte| *byte == b'\n')?;
    let mut offset = end as u64 + base;
    for line in bytes[..end].rsplit(|byte| *byte == b'\n') {
        offset -= line.len() as u64;
        if let Some(message) = log_message(line) {
            let is_membership = message.starts_with("[Behaviour] OnPlayerJoined ")
                || message.starts_with("[Behaviour] OnPlayerLeft ");
            let is_world_marker = message.starts_with("Memory Usage: after world loaded")
                || message.starts_with("[Behaviour] Finished entering world")
                || message.starts_with("[Behaviour] Entering Room:")
                || message.starts_with("[Behaviour] Joining wrld_")
                || message == "[Behaviour] OnLeftRoom";
            if (include_membership && is_membership) || is_world_marker {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                line.hash(&mut hash);
                return Some((offset, hash.finish()));
            }
        }
        offset = offset.saturating_sub(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(target_os = "linux")]
    #[test]
    fn waits_for_the_target_prefix_log_instead_of_using_another_prefix() {
        let home = tempfile::tempdir().unwrap();
        let unrelated = home.path().join(".local/share/Steam/steamapps/compatdata/438100/pfx/drive_c/users/steamuser/AppData/LocalLow/VRChat/VRChat");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(
            unrelated.join("output_log_other.txt"),
            line("Memory Usage: after world loaded"),
        )
        .unwrap();
        let prefix = home.path().join("custom-wine-prefix");
        let environment = format!("WINEPREFIX={}\0", prefix.display());
        assert!(
            linux_log_directory(environment.as_bytes(), home.path(), SystemTime::UNIX_EPOCH)
                .is_none()
        );
        let directory = prefix.join("drive_c/users/player/AppData/LocalLow/VRChat/VRChat");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("output_log_current.txt"),
            line("Memory Usage: after world loaded"),
        )
        .unwrap();
        let discovered =
            linux_log_directory(environment.as_bytes(), home.path(), SystemTime::UNIX_EPOCH)
                .unwrap();
        assert_eq!(discovered, directory);
        assert!(WorldWatcher::new(discovered, SystemTime::UNIX_EPOCH).poll());
    }
    fn line(message: &str) -> String {
        format!("2026.10.25 03:00:00 Debug      -  {message}\n")
    }
    #[test]
    fn room_identity_cache_requires_unchanged_membership_even_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output_log_current.txt");
        fs::write(&path, line("Memory Usage: after world loaded [test]")).unwrap();
        let mut watcher = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(watcher.poll());
        let initial = watcher.room_token().unwrap();
        let mut log = OpenOptions::new().append(true).open(&path).unwrap();
        log.write_all(line("Unrelated log message").as_bytes())
            .unwrap();
        assert!(watcher.poll());
        assert_eq!(watcher.room_token(), Some(initial));
        log.write_all(line("[Behaviour] OnPlayerJoined Example (usr_test)").as_bytes())
            .unwrap();
        assert!(watcher.poll());
        let joined = watcher.room_token().unwrap();
        assert_ne!(joined, initial);
        let mut restarted = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(restarted.poll());
        assert_eq!(restarted.room_token(), Some(joined));
        log.write_all(line("[Behaviour] OnPlayerLeft Example (usr_test)").as_bytes())
            .unwrap();
        log.write_all(line("[Behaviour] OnPlayerJoined Example (usr_test)").as_bytes())
            .unwrap();
        assert!(restarted.poll());
        assert_ne!(restarted.room_token(), Some(joined));
        log.write_all(line("[Behaviour] OnLeftRoom").as_bytes())
            .unwrap();
        assert!(!restarted.poll());
        assert!(restarted.room_token().is_none());
    }

    #[test]
    fn world_session_token_ignores_player_membership_and_rotates_on_rejoin() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output_log_current.txt");
        fs::write(
            &path,
            format!(
                "{}{}",
                line("[Behaviour] Entering Room: Lobby"),
                line("[Behaviour] Finished entering world."),
            ),
        )
        .unwrap();
        let mut watcher = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(watcher.poll());
        let initial_world = watcher.world_session_token().unwrap();
        let initial_room = watcher.room_token().unwrap();
        let mut log = OpenOptions::new().append(true).open(&path).unwrap();
        log.write_all(line("[Behaviour] OnPlayerJoined Example (usr_test)").as_bytes())
            .unwrap();
        log.write_all(line("[Behaviour] OnPlayerLeft Example (usr_test)").as_bytes())
            .unwrap();
        assert!(watcher.poll());
        assert_eq!(watcher.world_session_token(), Some(initial_world));
        assert_ne!(watcher.room_token(), Some(initial_room));
        log.write_all(line("[Behaviour] OnLeftRoom").as_bytes())
            .unwrap();
        assert!(!watcher.poll());
        assert!(watcher.world_session_token().is_none());
        log.write_all(
            format!(
                "{}{}",
                line("[Behaviour] Entering Room: Lobby"),
                line("[Behaviour] Finished entering world."),
            )
            .as_bytes(),
        )
        .unwrap();
        assert!(watcher.poll());
        assert_ne!(watcher.world_session_token(), Some(initial_world));
    }

    #[test]
    fn follows_loading_late_start_split_lines_log_rotation_and_previous_process_exclusion() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output_log_current.txt");
        fs::write(&path, line("[Behaviour] Entering Room: test")).unwrap();
        let mut watcher = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(!watcher.poll());
        assert_eq!(watcher.world_name(), Some("test"));
        let ready = line("Memory Usage: after world loaded [test]");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&ready.as_bytes()[..35]).unwrap();
        assert!(!watcher.poll());
        file.write_all(&ready.as_bytes()[35..]).unwrap();
        assert!(watcher.poll());
        assert_eq!(watcher.world_name(), Some("test"));
        // Starting long after entering a room still works, without reading the
        // entire log in one poll or a five-minute freshness cutoff.
        file.write_all(&vec![b'\n'; READ_CHUNK as usize + LINE_LIMIT + 100])
            .unwrap();
        let mut late = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(!late.poll());
        assert!(late.poll());
        assert_eq!(late.world_name(), Some("test"));
        file.write_all(line("[Behaviour] OnLeftRoom").as_bytes())
            .unwrap();
        assert!(!late.poll());
        assert_eq!(late.world_name(), None);
        assert!(
            !WorldWatcher::new(
                directory.path().into(),
                SystemTime::now() + Duration::from_secs(5)
            )
            .poll()
        );
        drop(file);
        fs::write(&path, line("[Behaviour] Finished entering world")).unwrap();
        assert!(late.poll());
        assert_eq!(late.world_name(), None);
        std::thread::sleep(Duration::from_millis(10));
        let next = directory.path().join("output_log_next.txt");
        fs::write(&next, line("[Behaviour] Entering Room: next")).unwrap();
        assert!(!late.poll());
        assert_eq!(late.world_name(), Some("next"));
        OpenOptions::new()
            .append(true)
            .open(next)
            .unwrap()
            .write_all(line("Memory Usage: after world loaded [next]").as_bytes())
            .unwrap();
        assert!(late.poll());
        assert_eq!(late.world_name(), Some("next"));
    }

    #[test]
    fn keeps_entering_room_name_across_join_and_ready_and_clears_on_leave() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output_log_current.txt");
        fs::write(
            &path,
            format!(
                "{}{}{}{}",
                line("[Behaviour] Entering Room: Static\u{2024}"),
                line("[Behaviour] Joining wrld_test:1234~region(eu)"),
                line("[Behaviour] Joining or Creating Room: Static\u{2024}"),
                line("[Behaviour] Finished entering world."),
            ),
        )
        .unwrap();
        let mut watcher = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(watcher.poll());
        assert_eq!(watcher.world_name(), Some("Static\u{2024}"));
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(line("[Behaviour] Joining wrld_test:5678~region(eu)").as_bytes())
            .unwrap();
        assert!(!watcher.poll());
        assert_eq!(watcher.world_name(), Some("Static\u{2024}"));
        file.write_all(line("[Behaviour] Finished entering world.").as_bytes())
            .unwrap();
        assert!(watcher.poll());
        assert_eq!(watcher.world_name(), Some("Static\u{2024}"));
        file.write_all(line("[Behaviour] OnLeftRoom").as_bytes())
            .unwrap();
        assert!(!watcher.poll());
        assert_eq!(watcher.world_name(), None);
        file.write_all(
            line("[Behaviour] Entering Room: Apt\u{2024} 404\u{FF3B}CN\u{2044}test\u{FF3D} - cafe")
                .as_bytes(),
        )
        .unwrap();
        assert!(!watcher.poll());
        assert_eq!(
            watcher.world_name(),
            Some("Apt\u{2024} 404\u{FF3B}CN\u{2044}test\u{FF3D} - cafe")
        );
    }

    #[test]
    fn ignores_player_names_stack_traces_and_unfinished_markers() {
        assert_eq!(
            last_marker(line("[Behaviour] OnPlayerJoined Finished entering world").as_bytes()),
            None
        );
        assert_eq!(last_marker(b" at OnLeftRoom()\n"), None);
        assert_eq!(
            last_marker(
                line("Memory Usage: after world loaded")
                    .trim_end()
                    .as_bytes()
            ),
            None
        );
        assert_eq!(
            last_marker(
                format!(
                    "{}{}",
                    line("Memory Usage: after world loaded"),
                    line("[Behaviour] Joining wrld_test")
                )
                .as_bytes()
            ),
            Some(false)
        );
        assert!(matches!(
            latest_name_update(line("[Behaviour] Entering Room: ").as_bytes()),
            Some(NameUpdate::Clear)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resolves_flatpak_steam_compat_data_log_directory() {
        let home = tempfile::tempdir().unwrap();
        let compat = home
            .path()
            .join(".local/share/Steam/steamapps/compatdata/438100");
        let translated = home
            .path()
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata/438100/pfx/drive_c/users/steamuser/AppData/LocalLow/VRChat/VRChat");
        fs::create_dir_all(&translated).unwrap();
        fs::write(translated.join("output_log_2026-01-01.txt"), "ok\n").unwrap();
        let env = format!(
            "FLATPAK_ID=com.valvesoftware.Steam\0STEAM_COMPAT_DATA_PATH={}\0",
            compat.display()
        );
        let found = linux_log_directory(env.as_bytes(), home.path(), SystemTime::UNIX_EPOCH);
        assert_eq!(found, Some(translated));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resolves_libraryfolders_vdf_and_multiple_env_prefixes() {
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join(".local/share/Steam/steamapps");
        fs::create_dir_all(&steam).unwrap();
        let ext_lib = home.path().join("external_steam");
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            ext_lib.display()
        );
        fs::write(steam.join("libraryfolders.vdf"), vdf).unwrap();
        let logs = ext_lib
            .join("steamapps/compatdata/438100/pfx/drive_c/users/steamuser/AppData/LocalLow/VRChat/VRChat");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("output_log_2026-01-02.txt"), "ok\n").unwrap();
        assert_eq!(
            linux_log_directory(b"", home.path(), SystemTime::UNIX_EPOCH),
            Some(logs)
        );

        let compat = home.path().join("compat-prefix");
        let compat_logs = compat.join("pfx/drive_c/users/steamuser/AppData/LocalLow/VRChat/VRChat");
        fs::create_dir_all(&compat_logs).unwrap();
        fs::write(compat_logs.join("output_log_2026-01-03.txt"), "ok\n").unwrap();
        let env = format!(
            "WINEPREFIX={}\0STEAM_COMPAT_DATA_PATH={}\0",
            home.path().join("missing/pfx").display(),
            compat.display()
        );
        assert_eq!(
            linux_log_directory(env.as_bytes(), home.path(), SystemTime::UNIX_EPOCH),
            Some(compat_logs)
        );
    }
}
