use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
struct Lut12Doc {
    codepoints: Vec<u16>,
}

struct Lut12Table {
    idx_to_cp: Vec<u16>,
    cp_to_idx: HashMap<u16, u16>,
}

static TABLE: OnceLock<Lut12Table> = OnceLock::new();

fn table() -> &'static Lut12Table {
    TABLE.get_or_init(|| {
        let doc: Lut12Doc = serde_json::from_str(include_str!("lut12.json"))
            .expect("embedded lut12.json must be valid");
        assert_eq!(
            doc.codepoints.len(),
            4096,
            "lut12 codebook must have 4096 entries"
        );
        let mut cp_to_idx = HashMap::with_capacity(doc.codepoints.len());
        for (idx, &cp) in doc.codepoints.iter().enumerate() {
            cp_to_idx.entry(cp).or_insert(idx as u16);
        }
        Lut12Table {
            idx_to_cp: doc.codepoints,
            cp_to_idx,
        }
    })
}

pub(crate) fn cp_to_lut_idx(cp: u16) -> Option<u16> {
    table().cp_to_idx.get(&cp).copied()
}

pub(crate) fn lut_idx_to_cp(idx: u16) -> u16 {
    table().idx_to_cp.get(idx as usize).copied().unwrap_or(0)
}
