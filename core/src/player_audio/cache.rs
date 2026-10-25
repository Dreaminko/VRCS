//! In-memory login key and room identity caches. Neither is serialized. The key
//! survives world changes; actor identities require unchanged room membership.
use super::crypto::{encrypted, SessionKey};
use std::collections::HashSet;
use std::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}
struct Entry {
    owner: ProcessIdentity,
    key: Zeroizing<[u8; 32]>,
}
#[derive(Default)]
pub(crate) struct SessionCache {
    key: Mutex<Option<Entry>>,
    #[cfg(windows)]
    pub(super) room: Mutex<Option<super::worker::RoomMetadata>>,
}
pub(super) enum Lookup {
    Verified(Zeroizing<[u8; 32]>),
    Missing,
    Pending,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verification {
    Valid,
    Invalid,
    Pending,
}

pub(super) fn verify(key: &[u8; 32], packets: &[Vec<u8>]) -> Verification {
    let key = SessionKey::new(key);
    let mut seen = HashSet::new();
    let (mut tested, mut passed) = (0, 0);
    for packet in packets {
        if !encrypted(packet) || !seen.insert(<[u8; 12]>::try_from(&packet[7..19]).unwrap()) {
            continue;
        }
        tested += 1;
        passed += usize::from(key.decrypt(packet).is_some());
    }
    if passed >= 2 {
        Verification::Valid
    } else if tested >= 2 && passed == 0 {
        Verification::Invalid
    } else {
        Verification::Pending
    }
}

impl SessionCache {
    pub(super) fn lookup(&self, owner: ProcessIdentity, packets: &[Vec<u8>]) -> Lookup {
        let mut entry = self.key.lock().expect("session key cache");
        let Some(current) = entry.as_ref() else {
            return Lookup::Missing;
        };
        if current.owner != owner {
            *entry = None;
            self.clear_room();
            return Lookup::Missing;
        }
        match verify(&current.key, packets) {
            Verification::Valid => Lookup::Verified(current.key.clone()),
            Verification::Pending => Lookup::Pending,
            Verification::Invalid => {
                *entry = None;
                self.clear_room();
                Lookup::Missing
            }
        }
    }
    pub(super) fn store(&self, owner: ProcessIdentity, key: Zeroizing<[u8; 32]>) {
        *self.key.lock().expect("session key cache") = Some(Entry { owner, key });
    }
    pub(super) fn invalidate(&self) {
        *self.key.lock().expect("session key cache") = None;
        self.clear_room();
    }
    fn clear_room(&self) {
        #[cfg(windows)]
        {
            self.room.lock().expect("room identity cache").take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_audio::crypto::seal;
    #[test]
    fn reuse_survives_capture_restart_but_relogin_and_process_reuse_invalidate() {
        let cache = SessionCache::default();
        let owner = ProcessIdentity {
            pid: 1,
            created: 10,
        };
        let first = [1; 32];
        let next = [2; 32];
        let packets = [
            seal(&first, 1, b"first sample"),
            seal(&first, 2, b"second sample"),
        ];
        cache.store(owner, Zeroizing::new(first));
        for _ in 0..3 {
            assert!(matches!(cache.lookup(owner, &packets), Lookup::Verified(_)));
        }
        assert!(matches!(
            cache.lookup(owner, &[packets[0].clone(), packets[0].clone()]),
            Lookup::Pending
        ));
        let mixed = [packets[0].clone(), seal(&next, 3, b"bad or changed sample")];
        assert!(matches!(cache.lookup(owner, &mixed), Lookup::Pending));
        assert!(matches!(cache.lookup(owner, &packets), Lookup::Verified(_)));
        let fresh = [
            seal(&next, 4, b"new login sample"),
            seal(&next, 5, b"new login sample"),
        ];
        assert!(matches!(cache.lookup(owner, &fresh), Lookup::Missing));
        cache.store(owner, Zeroizing::new(next));
        assert!(matches!(cache.lookup(owner, &fresh), Lookup::Verified(_)));
        assert!(matches!(
            cache.lookup(
                ProcessIdentity {
                    created: 11,
                    ..owner
                },
                &fresh
            ),
            Lookup::Missing
        ));
    }
}
