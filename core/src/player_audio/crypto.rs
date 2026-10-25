//! Authenticated Photon v5 datagrams. Keys and plaintext are never logged.
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use zeroize::Zeroizing;

pub(super) struct SessionKey(LessSafeKey);

impl SessionKey {
    pub(super) fn new(bytes: &[u8; 32]) -> Self {
        Self(LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, bytes).expect("AES key length"),
        ))
    }

    pub(super) fn decrypt(&self, packet: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        if !encrypted(packet) {
            return None;
        }
        let nonce = Nonce::try_assume_unique_for_key(&packet[7..19]).ok()?;
        let mut output = Zeroizing::new(packet[19..].to_vec());
        let length = self
            .0
            .open_in_place(nonce, Aad::from(&packet[..19]), &mut output)
            .ok()?
            .len();
        output.truncate(length);
        Some(output)
    }
}

pub(super) fn encrypted(packet: &[u8]) -> bool {
    packet.len() >= 40 && packet.len() <= 65_507 && packet[2] == 1
}

/// Verify all recent packets from one connection, never historic captures.
#[cfg(test)]
pub(super) fn candidate(bytes: &[u8; 32], packets: &[Vec<u8>]) -> bool {
    if packets.len() < 2 {
        return false;
    }
    let key = SessionKey::new(bytes);
    packets.iter().all(|packet| key.decrypt(packet).is_some())
}

#[cfg(test)]
pub(super) fn seal(key: &[u8; 32], counter: u64, plaintext: &[u8]) -> Vec<u8> {
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).unwrap());
    let mut header = vec![0, 1, 1, 4, 3, 2, 1];
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    header.extend_from_slice(&nonce);
    let mut data = plaintext.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(&header),
        &mut data,
    )
    .unwrap();
    header.extend(data);
    header
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticates_header_payload_and_every_recent_packet() {
        let key = [37; 32];
        let a = seal(&key, 1, b"first voice frame");
        let b = seal(&key, 2, b"second voice frame");
        assert!(candidate(&key, &[a.clone(), b]));
        assert!(!candidate(&key, std::slice::from_ref(&a)));
        let mut corrupt = a.clone();
        corrupt[5] ^= 1;
        assert!(SessionKey::new(&key).decrypt(&corrupt).is_none());
        assert!(!candidate(
            &key,
            &[a, seal(&[38; 32], 3, b"another session")]
        ));
    }
}
