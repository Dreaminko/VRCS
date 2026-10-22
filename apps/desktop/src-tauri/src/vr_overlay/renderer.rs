#[derive(Debug, Clone)]
pub struct Texture {
    /// Row-major RGBA8 pixels for capture and recognition.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
