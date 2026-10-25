//! Opt-in verification against a locally supplied capture. No keys, packets, or PCM
//! are embedded in fixtures or written to disk by these tests.
use super::{
    crypto::SessionKey,
    opus::Opus,
    protocol::{self, Parser},
};
use ring::digest::{Context, SHA256};
use std::{collections::BTreeMap, io::BufRead, path::Path, time::Instant};
use zeroize::Zeroizing;

pub(super) fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
#[ignore = "Requires VRCS_TEST_CAPTURE, VRCS_TEST_PHOTON_KEY and VRCS_TEST_OPUS_DLL"]
fn replay_local_capture_authenticates_and_decodes_players() {
    let path = std::env::var("VRCS_TEST_CAPTURE").expect("local capture path");
    let key_text =
        Zeroizing::new(std::env::var("VRCS_TEST_PHOTON_KEY").expect("local capture key"));
    assert_eq!(key_text.len(), 64);
    let key_bytes = Zeroizing::new(hex(&key_text));
    let key = SessionKey::new(key_bytes.as_slice().try_into().unwrap());
    let opus = Opus::load(Path::new(
        &std::env::var("VRCS_TEST_OPUS_DLL").expect("Opus DLL"),
    ))
    .unwrap();
    let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    let mut parsers = [Parser::default(), Parser::default()];
    let mut players = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut property_messages = 0;
    let mut scan_samples: Vec<Vec<u8>> = Vec::new();
    let (mut lines, mut authenticated, mut messages) = (0, 0, 0);
    for line in file.lines() {
        let line = line.unwrap();
        lines += 1;
        let parts: Vec<_> = line.split(',').collect();
        if parts.len() != 3 {
            continue;
        }
        let packet = hex(parts[2].trim());
        let Some(plaintext) = key.decrypt(&packet) else {
            continue;
        };
        authenticated += 1;
        if scan_samples.len() < 2 && scan_samples.iter().all(|p| p[7..19] != packet[7..19]) {
            scan_samples.push(packet.clone());
        }
        let incoming = parts[0] == "S";
        for message in parsers[usize::from(incoming)].messages(&plaintext, Instant::now()) {
            messages += 1;
            if !incoming {
                continue;
            }
            let updates = super::names::updates(&message);
            if message.windows(11).any(|part| part == b"displayName") {
                assert!(!updates.is_empty(), "player property message must parse");
                property_messages += 1;
            }
            for update in updates {
                if let super::names::Update::Name {
                    actor,
                    display_name: Some(display_name),
                    ..
                } = update
                {
                    names.insert(actor, display_name);
                }
            }
            let Some(voice) = protocol::voice(&message) else {
                continue;
            };
            for frame in voice {
                let (decoder, count, nonzero, digest) =
                    players.entry(frame.actor).or_insert_with(|| {
                        (opus.decoder().unwrap(), 0u64, 0u64, Context::new(&SHA256))
                    });
                let samples = decoder.decode(&frame.opus).expect("valid 20 ms player PCM");
                assert_eq!(samples.len(), 320);
                assert!(samples.iter().all(|v| v.is_finite()));
                *count += 1;
                *nonzero += u64::from(samples.iter().any(|s| s.abs() > 0.0001));
                for sample in samples {
                    digest.update(&sample.to_le_bytes());
                }
            }
        }
    }
    assert!(
        authenticated >= 2,
        "capture must authenticate with the supplied key"
    );
    // Exercise the optimized candidate filter and authentication with real
    // local packets, using an in-memory object rather than an actual pagefile.
    let mut object = Zeroizing::new(32u64.to_le_bytes().to_vec());
    object.extend_from_slice(&key_bytes);
    assert!(super::key::scan_bytes(
        &object,
        &scan_samples,
        &std::sync::atomic::AtomicBool::new(false)
    )
    .is_some());
    if std::env::var_os("VRCS_TEST_REQUIRE_VOICE").is_some() {
        assert!(!players.is_empty(), "capture must contain inbound voice");
    }
    if std::env::var_os("VRCS_TEST_REQUIRE_NAMES").is_some() {
        assert!(!names.is_empty());
        assert!(
            players.keys().all(|actor| names.contains_key(actor)),
            "all speaking actors must have a VRChat display name"
        );
    }
    let summaries: Vec<_> = players
        .into_iter()
        .map(|(actor, (_, frames, nonzero, digest))| {
            let hash: String = digest
                .finish()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            serde_json::json!({"actor": actor, "frames": frames, "samples": frames * 320,
            "nonzero_frames": nonzero, "pcm_sha256": hash})
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"lines": lines, "authenticated": authenticated,
        "messages": messages, "property_messages": property_messages, "named_actors": names.len(), "players": summaries})
    );
}
