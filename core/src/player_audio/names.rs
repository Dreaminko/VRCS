//! Bounded Photon Protocol18 reader for the actor's user ID and display name.
//! Avatar authors and unrelated profile fields are not player identities.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Update {
    Name {
        actor: u16,
        display_name: Option<String>,
        user_id: Option<String>,
    },
    Left(u16),
}

enum Value<'a> {
    Int(i64),
    Text(&'a str),
    Map(Vec<(Value<'a>, Value<'a>)>),
    Other,
}
impl<'a> Value<'a> {
    fn field(&self, name: &str) -> Option<&Self> {
        let Self::Map(items) = self else { return None };
        items.iter().find_map(|(key, value)| {
            matches!(key, Self::Text(text) if *text == name).then_some(value)
        })
    }
    fn actor(&self) -> Option<u16> {
        let Self::Int(value) = self else { return None };
        u16::try_from(*value).ok().filter(|v| *v != 0)
    }
}
enum Descriptor {
    Plain(u8),
    Dictionary(Box<(Descriptor, Descriptor)>),
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(length)?;
        let result = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(result)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn varint(&mut self) -> Option<u64> {
        let mut result = 0;
        for shift in (0..70).step_by(7) {
            let byte = self.byte()?;
            if shift == 63 && byte > 1 {
                return None;
            }
            result |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return Some(result);
            }
        }
        None
    }
    fn length(&mut self) -> Option<usize> {
        usize::try_from(self.varint()?).ok()
    }
    fn count(&mut self) -> Option<usize> {
        self.length().filter(|count| *count <= 4096)
    }
    fn descriptor(&mut self, kind: u8, depth: usize) -> Option<Descriptor> {
        if depth > 16 {
            return None;
        }
        Some(if kind == 20 {
            let key = self.byte()?;
            let key = self.descriptor(key, depth + 1)?;
            let value = self.byte()?;
            Descriptor::Dictionary(Box::new((key, self.descriptor(value, depth + 1)?)))
        } else {
            Descriptor::Plain(kind)
        })
    }
    fn dynamic(&mut self, depth: usize) -> Option<Value<'a>> {
        let kind = self.byte()?;
        // Zero means dynamic only inside a dictionary descriptor.
        if kind == 0 {
            return None;
        }
        let descriptor = self.descriptor(kind, depth)?;
        self.value(&descriptor, depth)
    }
    fn value(&mut self, descriptor: &Descriptor, depth: usize) -> Option<Value<'a>> {
        self.nodes += 1;
        if depth > 16 || self.nodes > 32768 {
            return None;
        }
        let kind = match descriptor {
            Descriptor::Plain(kind) => *kind,
            Descriptor::Dictionary(pair) => {
                let count = self.count()?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push((
                        self.value(&pair.0, depth + 1)?,
                        self.value(&pair.1, depth + 1)?,
                    ));
                }
                return Some(Value::Map(items));
            }
        };
        Some(match kind {
            0 => return self.dynamic(depth + 1),
            7 => {
                let length = self.length()?;
                Value::Text(std::str::from_utf8(self.take(length)?).ok()?)
            }
            21 => {
                let count = self.count()?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push((self.dynamic(depth + 1)?, self.dynamic(depth + 1)?));
                }
                Value::Map(items)
            }
            3 | 11 | 15 => Value::Int(i64::from(self.byte()?)),
            4 => Value::Int(i64::from(i16::from_le_bytes(
                self.take(2)?.try_into().ok()?,
            ))),
            13 | 17 => Value::Int(i64::from(u16::from_le_bytes(
                self.take(2)?.try_into().ok()?,
            ))),
            12 | 16 => Value::Int(-i64::from(self.byte()?)),
            14 | 18 => Value::Int(-i64::from(u16::from_le_bytes(
                self.take(2)?.try_into().ok()?,
            ))),
            9 | 10 => {
                let n = self.varint()?;
                Value::Int(((n >> 1) as i64) ^ -((n & 1) as i64))
            }
            2 => {
                self.take(1)?;
                Value::Other
            }
            5 => {
                self.take(4)?;
                Value::Other
            }
            6 => {
                self.take(8)?;
                Value::Other
            }
            8 | 27 | 28 => Value::Other,
            29..=34 => Value::Int(0),
            19 | 128..=255 => {
                if kind == 19 {
                    self.byte()?;
                }
                let length = self.length()?;
                self.take(length)?;
                Value::Other
            }
            66 | 67 => {
                let length = self.length()?;
                self.take(if kind == 66 {
                    length.checked_add(7)? / 8
                } else {
                    length
                })?;
                Value::Other
            }
            23 | 68..=71 | 73 | 74 | 85 => {
                let count = self.count()?;
                for _ in 0..count {
                    if kind == 23 {
                        self.dynamic(depth + 1)?;
                    } else {
                        self.value(&Descriptor::Plain(kind - 64), depth + 1)?;
                    }
                }
                Value::Other
            }
            _ => return None,
        })
    }
}

fn name(actor: u16, properties: &Value<'_>) -> Option<Update> {
    let user = properties.field("user")?;
    let display_name = match user.field("displayName") {
        Some(Value::Text(text))
            if !text.is_empty()
                && text.chars().count() <= 128
                && !text.chars().any(char::is_control) =>
        {
            Some((*text).to_owned())
        }
        _ => None,
    };
    let user_id = match user.field("id") {
        Some(Value::Text(id)) if crate::models::is_vrchat_user_id(id) => Some((*id).to_owned()),
        _ => None,
    };
    (display_name.is_some() || user_id.is_some()).then_some(Update::Name {
        actor,
        display_name,
        user_id,
    })
}

pub(super) fn updates(message: &[u8]) -> Vec<Update> {
    parse(message).unwrap_or_default()
}
fn parse(message: &[u8]) -> Option<Vec<Update>> {
    if message.len() > 1024 * 1024 {
        return None;
    }
    // Unreliable commands may prepend their sequence, just as voice commands do.
    let at = message
        .windows(2)
        .take(9)
        .position(|bytes| bytes == [0xf3, 4])?;
    let message = &message[at..];
    let event = *message.get(2)?;
    if !matches!(event, 42 | 253..=255) {
        return None;
    }
    let mut reader = Reader {
        bytes: message,
        offset: 3,
        nodes: 0,
    };
    let count = reader.byte()?;
    let mut parameters = Vec::new();
    for _ in 0..count {
        parameters.push((reader.byte()?, reader.dynamic(0)?));
    }
    if reader.offset != message.len() {
        return None;
    }
    let param = |code| {
        parameters
            .iter()
            .find(|(key, _)| *key == code)
            .map(|(_, value)| value)
    };
    let actor = param(254).and_then(Value::actor);
    if event == 254 {
        return Some(actor.map(Update::Left).into_iter().collect());
    }
    let properties = param(match event {
        253 => 251,
        255 => 249,
        _ => 245,
    })?;
    let mut result = Vec::new();
    if let Some(actor) = actor {
        result.extend(name(actor, properties));
    }
    // A property snapshot can contain several actor entries in any of these events.
    if matches!(event, 42 | 253 | 255) {
        if let Value::Map(items) = properties {
            for (key, properties) in items {
                if let Some(actor) = key.actor() {
                    result.extend(name(actor, properties));
                }
            }
        }
    }
    Some(result)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    fn text(value: &str) -> Vec<u8> {
        let mut out = vec![7, value.len() as u8];
        out.extend(value.as_bytes());
        out
    }
    pub(crate) fn fixture(actor: u8, display_name: &str) -> Vec<u8> {
        let mut out = vec![0xf3, 4, 255, 2, 254, 11, actor, 249, 21, 2];
        out.extend(text("avatarDict"));
        out.extend([21, 2]);
        out.extend(text("authorId"));
        out.extend(text("usr_00000000-0000-4000-8000-000000000099"));
        out.extend(text("displayName"));
        out.extend(text("Wrong author"));
        out.extend(text("user"));
        out.extend([20, 7, 0, 1]);
        out.push(11);
        out.extend(b"displayName");
        out.extend(text(display_name));
        out
    }
    pub(crate) fn user_fixture(
        actor: u8,
        display_name: Option<&str>,
        user_id: Option<&str>,
    ) -> Vec<u8> {
        let mut out = vec![0xf3, 4, 255, 2, 254, 11, actor, 249, 21, 1];
        out.extend(text("user"));
        out.extend([
            21,
            u8::from(display_name.is_some()) + u8::from(user_id.is_some()),
        ]);
        if let Some(name) = display_name {
            out.extend(text("displayName"));
            out.extend(text(name));
        }
        if let Some(id) = user_id {
            out.extend(text("id"));
            out.extend(text(id));
        }
        out
    }
    #[test]
    fn stable_user_id_is_read_only_from_the_actor_user_and_can_arrive_without_a_name() {
        let id = "usr_00000000-0000-4000-8000-000000000007";
        assert_eq!(
            updates(&user_fixture(7, None, Some(id))),
            vec![Update::Name {
                actor: 7,
                display_name: None,
                user_id: Some(id.into())
            }]
        );
        for invalid in [
            "Player name",
            "usr_test",
            "avtr_00000000-0000-4000-8000-000000000007",
            "usr_00000000000040008000000000000007",
        ] {
            assert!(updates(&user_fixture(7, None, Some(invalid))).is_empty());
        }
        let with_name = user_fixture(7, Some("Same name"), Some(id));
        for end in 0..with_name.len() {
            assert!(updates(&with_name[..end]).is_empty());
        }
        assert!(matches!(
            &updates(&fixture(7, "Player"))[0],
            Update::Name { user_id: None, .. }
        ));
    }
    #[test]
    fn selects_user_display_name_and_rejects_truncation_and_unbounded_input() {
        let message = fixture(7, "测试 <Player> あ");
        assert_eq!(
            updates(&message),
            vec![Update::Name {
                actor: 7,
                display_name: Some("测试 <Player> あ".into()),
                user_id: None,
            }]
        );
        for end in 0..message.len() {
            assert!(updates(&message[..end]).is_empty());
        }
        assert!(updates(&fixture(7, "bad\nname")).is_empty());
        assert!(updates(&[0xf3, 4, 253, 1, 251, 21, 255, 255, 255, 255, 127]).is_empty());
        assert_eq!(
            updates(&[0xf3, 4, 254, 1, 254, 11, 7]),
            vec![Update::Left(7)]
        );
    }
    #[test]
    fn nested_actor_property_snapshot() {
        let join = fixture(7, "Player A");
        let mut snapshot = vec![0xf3, 4, 253, 1, 251, 21, 1, 11, 7];
        snapshot.extend(&join[8..]);
        assert_eq!(updates(&snapshot), updates(&join));
    }
    #[test]
    fn prefixed_property_snapshots_keep_actor_names_in_every_supported_event() {
        let join = fixture(7, "Player A");
        for (event, parameter) in [(42, 245), (253, 251), (255, 249)] {
            for prefix in [0, 4, 8] {
                let mut message = vec![0; prefix];
                message.extend([0xf3, 4, event, 1, parameter, 21, 1, 11, 7]);
                message.extend(&join[8..]);
                assert_eq!(updates(&message), updates(&join));
            }
        }
    }
}
