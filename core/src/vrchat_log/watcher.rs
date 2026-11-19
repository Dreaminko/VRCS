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
    started: SystemTime,
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

    fn new(directory: PathBuf, started: SystemTime) -> Self {
        Self {
            directory,
            started,
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
        if self.refresh().is_err() {
            self.ready = false;
        }
        self.ready && self.caught_up
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
            self.ready = false;
            self.identity_stamp = None;
            self.world_stamp = None;
            self.world_name = None;
            self.name_search_end = None;
            return Ok(());
        };
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(7);
        }
        let mut file = options.open(&path)?;
        let length = file.metadata()?.len();
        if self.path.as_ref() != Some(&path) || length < self.offset {
            self.path = Some(path);
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
            let bytes = read_range(
                &mut file,
                self.offset,
                (length - self.offset).min(READ_CHUNK),
            )?;
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
        if let Some(end) = self.search_end {
            let begin = end.saturating_sub(READ_CHUNK);
            // Overlap covers a marker line split across backwards read blocks.
            let overlap = begin.saturating_sub(LINE_LIMIT as u64);
            let bytes = read_range(&mut file, overlap, end - overlap)?;
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
        }
        if let Some(end) = self.name_search_end {
            let begin = end.saturating_sub(READ_CHUNK);
            // Overlap covers a name line split across backwards read blocks.
            let overlap = begin.saturating_sub(LINE_LIMIT as u64);
            let bytes = read_range(&mut file, overlap, end - overlap)?;
            if let Some(update) = latest_name_update(&bytes) {
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
        let name = rest.strip_prefix(' ').unwrap_or(rest);
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
        assert!(!WorldWatcher::new(
            directory.path().into(),
            SystemTime::now() + Duration::from_secs(5)
        )
        .poll());
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
}
