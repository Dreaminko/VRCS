//! Read only lifecycle markers from the current VRChat process's output log.
//! No world names, account IDs or log contents leave this watcher.
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
    path: Option<PathBuf>,
    offset: u64,
    search_end: Option<u64>,
    partial: Vec<u8>,
    ready: bool,
    caught_up: bool,
    identity_stamp: Option<(u64, u64)>,
}

impl WorldWatcher {
    #[cfg(windows)]
    pub fn for_process(owner: super::cache::ProcessIdentity) -> Self {
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
    pub fn for_process(owner: super::cache::ProcessIdentity) -> Self {
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
            path: None,
            offset: 0,
            search_end: None,
            partial: Vec::new(),
            ready: false,
            caught_up: false,
            identity_stamp: None,
        }
    }

    pub fn poll(&mut self) -> bool {
        self.error = match self.refresh() {
            Ok(()) => None,
            Err(error) => {
                self.ready = false;
                Some(format!("Cannot read the current VRChat world log: {error}"))
            }
        };
        self.ready && self.caught_up
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    /// Cache identity only while the same process, room and membership remain.
    /// Byte position distinguishes repeated joins with identical timestamp/name.
    pub fn room_token(&self) -> Option<u64> {
        if !self.ready || !self.caught_up {
            return None;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.path.as_ref()?.hash(&mut hash);
        self.identity_stamp?.hash(&mut hash);
        Some(hash.finish())
    }

    fn refresh(&mut self) -> std::io::Result<()> {
        let earliest = self
            .started
            .checked_sub(Duration::from_secs(2))
            .unwrap_or(self.started);
        #[cfg(target_os = "linux")]
        if self.path.is_none() {
            if let Some(pid) = self.pid {
                let environment = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default();
                self.directory =
                    linux_log_directory(&environment, &home, earliest).ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "waiting for an output_log in the target Wine/Proton prefix",
                        )
                    })?;
            }
        }
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
            self.offset = length;
            self.search_end = Some(length);
            self.partial.clear();
            self.ready = false;
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
                keep_latest_stamp(
                    &mut self.identity_stamp,
                    identity_marker(
                        &self.partial[..=end],
                        self.offset - self.partial.len() as u64,
                    ),
                );
                if let Some(ready) = last_marker(&self.partial[..=end]) {
                    self.ready = ready;
                    self.search_end = None;
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
            keep_latest_stamp(&mut self.identity_stamp, identity_marker(&bytes, overlap));
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
        // Catch up to the log's current end before treating an older marker as ready.
        self.caught_up = self.offset >= length;
        Ok(())
    }
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
    let prefix = variable(b"WINEPREFIX=")
        .or_else(|| variable(b"STEAM_COMPAT_DATA_PATH=").map(|path| path.join("pfx")))
        .or_else(|| {
            variable(b"STEAM_COMPAT_INSTALL_PATH=")
                .and_then(|path| Some(path.parent()?.parent()?.join("compatdata/438100/pfx")))
        });
    let prefixes = prefix.map(|path| vec![path]).unwrap_or_else(|| vec![
        home.join(".steam/steam/steamapps/compatdata/438100/pfx"),
        home.join(".local/share/Steam/steamapps/compatdata/438100/pfx"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata/438100/pfx"),
    ]);
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
fn identity_marker(bytes: &[u8], base: u64) -> Option<(u64, u64)> {
    let end = bytes.iter().rposition(|byte| *byte == b'\n')?;
    let mut offset = end as u64 + base;
    for line in bytes[..end].rsplit(|byte| *byte == b'\n') {
        offset -= line.len() as u64;
        if let Some(message) = log_message(line) {
            if message.starts_with("[Behaviour] OnPlayerJoined ")
                || message.starts_with("[Behaviour] OnPlayerLeft ")
                || message.starts_with("Memory Usage: after world loaded")
                || message.starts_with("[Behaviour] Finished entering world")
                || message.starts_with("[Behaviour] Entering Room:")
                || message.starts_with("[Behaviour] Joining wrld_")
                || message == "[Behaviour] OnLeftRoom"
            {
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
    fn follows_loading_late_start_split_lines_log_rotation_and_previous_process_exclusion() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output_log_current.txt");
        fs::write(&path, line("[Behaviour] Entering Room: test")).unwrap();
        let mut watcher = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(!watcher.poll());
        let ready = line("Memory Usage: after world loaded [test]");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&ready.as_bytes()[..35]).unwrap();
        assert!(!watcher.poll());
        file.write_all(&ready.as_bytes()[35..]).unwrap();
        assert!(watcher.poll());
        // Starting long after entering a room still works, without reading the
        // entire log in one poll or a five-minute freshness cutoff.
        file.write_all(&vec![b'\n'; READ_CHUNK as usize + LINE_LIMIT + 100])
            .unwrap();
        let mut late = WorldWatcher::new(directory.path().into(), SystemTime::UNIX_EPOCH);
        assert!(!late.poll());
        assert!(late.poll());
        file.write_all(line("[Behaviour] OnLeftRoom").as_bytes())
            .unwrap();
        assert!(!late.poll());
        assert!(!WorldWatcher::new(
            directory.path().into(),
            SystemTime::now() + Duration::from_secs(5)
        )
        .poll());
        drop(file);
        fs::write(&path, line("[Behaviour] Finished entering world")).unwrap();
        assert!(late.poll());
        std::thread::sleep(Duration::from_millis(10));
        let next = directory.path().join("output_log_next.txt");
        fs::write(&next, line("[Behaviour] Entering Room: next")).unwrap();
        assert!(!late.poll());
        OpenOptions::new()
            .append(true)
            .open(next)
            .unwrap()
            .write_all(line("Memory Usage: after world loaded [next]").as_bytes())
            .unwrap();
        assert!(late.poll());
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
    }
}
