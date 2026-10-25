use std::collections::HashMap;

/// USpeak indices wrap at 16 bits. Only a bounded forward window is accepted.
pub(super) struct JitterBuffer {
    next: u16,
    packets: HashMap<u16, Vec<u8>>,
    pub reset: bool,
}
impl JitterBuffer {
    pub(super) fn new(sequence: u16) -> Self {
        Self {
            next: sequence,
            packets: HashMap::new(),
            reset: false,
        }
    }
    pub(super) fn push(&mut self, sequence: u16, bytes: Vec<u8>) -> bool {
        let distance = sequence.wrapping_sub(self.next) as i16;
        if distance < 0 || self.packets.contains_key(&sequence) {
            return false;
        }
        if distance > 50 {
            self.packets.clear();
            self.next = sequence;
            self.reset = true;
        }
        if self.packets.len() >= 32 {
            return false;
        }
        self.packets.insert(sequence, bytes);
        true
    }
    pub(super) fn pop(&mut self) -> Option<Vec<u8>> {
        if self.packets.is_empty() {
            return None;
        }
        let packet = self.packets.remove(&self.next);
        self.next = self.next.wrapping_add(1);
        packet
    }
    pub(super) fn resume_after_idle(&mut self, sequence: u16) {
        if self.packets.is_empty() && sequence.wrapping_sub(self.next) as i16 >= 0 {
            self.next = sequence;
            self.reset = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorders_without_replaying_duplicates_or_resetting_on_old_packets() {
        let mut buffer = JitterBuffer::new(100);
        for index in [100, 102, 101] {
            assert!(buffer.push(index, vec![index as u8]));
        }
        assert!(!buffer.push(102, vec![102]));
        for index in [100, 101, 102] {
            assert_eq!(buffer.pop(), Some(vec![index]));
        }
        assert!(!buffer.push(101, vec![101]));
        assert!(!buffer.reset);
    }
    #[test]
    fn wrap_loss_and_dtx_are_bounded() {
        let mut buffer = JitterBuffer::new(65535);
        buffer.push(0, vec![2]);
        assert_eq!(buffer.pop(), None);
        assert_eq!(buffer.pop(), Some(vec![2]));
        assert!(buffer.push(400, vec![3]));
        assert!(buffer.reset);
        assert_eq!(buffer.pop(), Some(vec![3]));
    }
}
