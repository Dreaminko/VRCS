//! Visual design tokens matching `surface.css` and `dashboard.css`.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0.0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub const fn scale_alpha(self, factor: f32) -> Self {
        Self {
            a: self.a * factor,
            ..self
        }
    }

    /// CSS `color-mix(in srgb, self weight, other)` for straight sRGB colors.
    pub fn mix(self, other: Self, weight: f32) -> Self {
        let w = weight.clamp(0.0, 1.0);
        let inv = 1.0 - w;
        let a = self.a * w + other.a * inv;
        if a <= 1e-6 {
            return Self::TRANSPARENT;
        }
        Self {
            r: (self.r * self.a * w + other.r * other.a * inv) / a,
            g: (self.g * self.a * w + other.g * other.a * inv) / a,
            b: (self.b * self.a * w + other.b * other.a * inv) / a,
            a,
        }
    }

    pub fn premultiplied(self) -> [f32; 4] {
        [
            self.r * self.a,
            self.g * self.a,
            self.b * self.a,
            self.a,
        ]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub vr_shell: Color,
    pub surface: Color,
    pub surface_subtle: Color,
    pub surface_elevated: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_quiet: Color,
    pub border: Color,
    pub border_strong: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub primary_ink: Color,
    pub on_primary: Color,
    pub primary_soft: Color,
    pub primary_softer: Color,
    pub error: Color,
    pub error_soft: Color,
    pub error_border: Color,
    pub warning: Color,
    pub warning_soft: Color,
    pub warning_border: Color,
}

impl Theme {
    pub const LIGHT: Self = Self {
        dark: false,
        vr_shell: Color::rgb(0xf4, 0xf7, 0xfb),
        surface: Color::rgb(0xff, 0xff, 0xff),
        surface_subtle: Color::rgb(0xee, 0xf2, 0xf7),
        surface_elevated: Color::rgb(0xf8, 0xfa, 0xfd),
        text: Color::rgb(0x1d, 0x28, 0x34),
        text_secondary: Color::rgb(0x5a, 0x68, 0x78),
        text_quiet: Color::rgb(0x8b, 0x98, 0xa7),
        border: Color::rgb(0xdd, 0xe2, 0xe9),
        border_strong: Color::rgb(0xcd, 0xd4, 0xdd),
        primary: Color::rgb(0x74, 0xd6, 0xff),
        primary_hover: Color::rgb(0x56, 0xc8, 0xf5),
        primary_ink: Color::rgb(0x17, 0x61, 0x7d),
        on_primary: Color::rgb(0x12, 0x39, 0x4b),
        primary_soft: Color::rgb(0xdf, 0xf6, 0xff),
        primary_softer: Color::rgb(0xf0, 0xfb, 0xff),
        error: Color::rgb(0xc9, 0x44, 0x44),
        error_soft: Color::rgb(0xfd, 0xf0, 0xee),
        error_border: Color::rgb(0xe8, 0xc9, 0xc5),
        warning: Color::rgb(0x9a, 0x64, 0x18),
        warning_soft: Color::rgb(0xff, 0xf7, 0xe8),
        warning_border: Color::rgb(0xef, 0xd6, 0xa8),
    };

    pub const DARK: Self = Self {
        dark: true,
        vr_shell: Color::rgb(0x23, 0x27, 0x30),
        surface: Color::rgb(0x2f, 0x34, 0x3e),
        surface_subtle: Color::rgb(0x27, 0x2c, 0x36),
        surface_elevated: Color::rgb(0x39, 0x40, 0x4d),
        text: Color::rgb(0xdc, 0xe0, 0xe5),
        text_secondary: Color::rgb(0xa9, 0xaf, 0xbc),
        text_quiet: Color::rgb(0x87, 0x8a, 0x98),
        border: Color::rgb(0x3a, 0x40, 0x4c),
        border_strong: Color::rgb(0x4e, 0x55, 0x64),
        primary: Color::rgb(0x3f, 0xb9, 0xec),
        primary_hover: Color::rgb(0x2b, 0xa6, 0xdb),
        primary_ink: Color::rgb(0x7f, 0xd8, 0xff),
        on_primary: Color::rgb(0x0f, 0x24, 0x30),
        primary_soft: Color::rgba(116, 214, 255, 0.16),
        primary_softer: Color::rgba(116, 214, 255, 0.09),
        error: Color::rgb(0xe0, 0x7b, 0x80),
        error_soft: Color::rgba(208, 114, 119, 0.14),
        error_border: Color::rgb(0x5c, 0x32, 0x35),
        warning: Color::rgb(0xde, 0xc1, 0x84),
        warning_soft: Color::rgba(222, 193, 132, 0.12),
        warning_border: Color::rgb(0x5d, 0x4c, 0x2f),
    };

    pub const fn of(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }

    pub fn panel_bg(&self, opacity: f32) -> Color {
        self.surface.with_alpha(opacity.clamp(0.0, 1.0))
    }
}
