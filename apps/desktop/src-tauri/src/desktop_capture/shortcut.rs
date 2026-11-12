//! Platform-neutral desktop shortcut representation. Renderers live next to the
//! backend that registers the shortcut.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Function(u8),
}

impl Shortcut {
    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || "Invalid desktop shortcut".to_string();
        let parts: Vec<_> = value.split('+').map(str::trim).collect();
        let (key, modifiers) = parts.split_last().ok_or_else(invalid)?;
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for modifier in modifiers {
            let slot = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut ctrl,
                "alt" => &mut alt,
                "shift" => &mut shift,
                _ => return Err(invalid()),
            };
            if *slot {
                return Err(invalid());
            }
            *slot = true;
        }
        if !(ctrl || alt || shift) {
            return Err(invalid());
        }
        let key = key.to_ascii_uppercase();
        let key = if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() {
            Key::Char(key.as_bytes()[0] as char)
        } else if let Some(number) = key
            .strip_prefix('F')
            .and_then(|value| value.parse::<u8>().ok())
        {
            if !(1..=11).contains(&number) {
                return Err(invalid());
            }
            Key::Function(number)
        } else {
            return Err(invalid());
        };
        Ok(Self {
            ctrl,
            alt,
            shift,
            key,
        })
    }

    /// `RegisterHotKey` modifiers and virtual-key code.
    #[cfg(any(windows, test))]
    pub fn windows_registration(self) -> (u32, u32) {
        let modifiers =
            u32::from(self.alt) | (u32::from(self.ctrl) << 1) | (u32::from(self.shift) << 2);
        let key = match self.key {
            Key::Char(key) => u32::from(key),
            Key::Function(number) => 111 + u32::from(number),
        };
        (modifiers, key)
    }

    /// XDG GlobalShortcuts preferred trigger.
    #[cfg(any(target_os = "linux", test))]
    pub fn portal_trigger(self) -> String {
        let mut trigger = String::new();
        for (pressed, name) in [
            (self.ctrl, "CTRL"),
            (self.alt, "ALT"),
            (self.shift, "SHIFT"),
        ] {
            if pressed {
                trigger.push_str(name);
                trigger.push('+');
            }
        }
        match self.key {
            Key::Char(key) => trigger.push(key),
            Key::Function(number) => trigger.push_str(&format!("F{number}")),
        }
        trigger
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_letters_numbers_and_function_keys() {
        let shortcut = Shortcut::parse("Ctrl+Alt+O").unwrap();
        assert_eq!(shortcut.key, Key::Char('O'));
        assert_eq!(shortcut.windows_registration(), (3, 79));
        assert_eq!(shortcut.portal_trigger(), "CTRL+ALT+O");
        assert_eq!(
            Shortcut::parse(" shift + ctrl + f8 ")
                .unwrap()
                .windows_registration(),
            (6, 119)
        );
        assert_eq!(
            Shortcut::parse("Alt+5").unwrap().windows_registration(),
            (1, 53)
        );
        assert_eq!(
            Shortcut::parse("Shift+F11").unwrap().portal_trigger(),
            "SHIFT+F11"
        );
    }

    #[test]
    fn rejects_incomplete_reserved_or_ambiguous_shortcuts() {
        for value in [
            "",
            "O",
            "Ctrl",
            "Ctrl+Alt",
            "Ctrl+O+P",
            "Ctrl+Ctrl+O",
            "Ctrl+F12",
            "Win+O",
            "Ctrl+Space",
        ] {
            assert!(Shortcut::parse(value).is_err(), "{value}");
        }
    }
}
