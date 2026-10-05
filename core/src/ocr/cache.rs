use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct TranslationCache {
    entries: VecDeque<([u8; 32], String)>,
    bytes: usize,
}

impl TranslationCache {
    pub fn get(&mut self, key: &[u8; 32]) -> Option<String> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        let entry = self.entries.remove(index)?;
        let text = entry.1.clone();
        self.entries.push_back(entry);
        Some(text)
    }

    pub fn insert(&mut self, key: [u8; 32], text: String) {
        const MAX_BYTES: usize = 1024 * 1024;
        if text.trim().is_empty() || text.len() > MAX_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|(entry, _)| *entry == key) {
            self.bytes -= self.entries.remove(index).unwrap().1.len();
        }
        self.bytes += text.len();
        self.entries.push_back((key, text));
        while self.entries.len() > 256 || self.bytes > MAX_BYTES {
            self.bytes -= self.entries.pop_front().unwrap().1.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_cache_evicts_by_recency_and_byte_budget() {
        let mut cache = TranslationCache::default();
        for index in 0u16..256 {
            let mut key = [0; 32];
            key[..2].copy_from_slice(&index.to_le_bytes());
            cache.insert(key, "text".into());
        }
        assert!(cache.get(&[0; 32]).is_some());
        cache.insert([255; 32], "new".into());
        let mut oldest = [0; 32];
        oldest[0] = 1;
        assert!(cache.get(&oldest).is_none());
        assert!(cache.get(&[0; 32]).is_some());
        cache.insert([254; 32], "x".repeat(1024 * 1024));
        assert_eq!(cache.entries.len(), 1);
        cache.insert([253; 32], " ".into());
        cache.insert([252; 32], "x".repeat(1024 * 1024 + 1));
        assert_eq!(cache.entries.len(), 1);
    }
}
