use std::ffi::{c_int, c_void};
use std::path::Path;
use std::sync::Arc;

type Create = unsafe extern "C" fn(c_int, c_int, *mut c_int) -> *mut c_void;
type Decode = unsafe extern "C" fn(*mut c_void, *const u8, c_int, *mut f32, c_int, c_int) -> c_int;
type Destroy = unsafe extern "C" fn(*mut c_void);

pub(super) struct Opus {
    _library: libloading::Library,
    create: Create,
    decode: Decode,
    destroy: Destroy,
}

impl Opus {
    #[cfg(test)]
    pub(super) fn test_tone(&self, frequency: f32, frames: usize) -> Vec<Vec<u8>> {
        type EncoderCreate = unsafe extern "C" fn(c_int, c_int, c_int, *mut c_int) -> *mut c_void;
        type Encode = unsafe extern "C" fn(*mut c_void, *const f32, c_int, *mut u8, c_int) -> c_int;
        unsafe {
            let create = self
                ._library
                .get::<EncoderCreate>(b"opus_encoder_create\0")
                .unwrap();
            let encode = self._library.get::<Encode>(b"opus_encode_float\0").unwrap();
            let destroy = self
                ._library
                .get::<Destroy>(b"opus_encoder_destroy\0")
                .unwrap();
            let mut error = 0;
            let encoder = create(48_000, 2, 2049, &mut error);
            assert!(!encoder.is_null() && error == 0);
            let result = (0..frames)
                .map(|frame| {
                    let samples: Vec<_> = (0..960)
                        .flat_map(|i| {
                            let value =
                                (std::f32::consts::TAU * frequency * (frame * 960 + i) as f32
                                    / 48_000.0)
                                    .sin()
                                    * 0.2;
                            [value, value]
                        })
                        .collect();
                    let mut bytes = vec![0; 1275];
                    let count = encode(encoder, samples.as_ptr(), 960, bytes.as_mut_ptr(), 1275);
                    assert!(count > 0);
                    bytes.truncate(count as usize);
                    bytes
                })
                .collect();
            destroy(encoder);
            result
        }
    }

    pub(super) fn load(path: &Path) -> Result<Arc<Self>, String> {
        if !path.is_absolute() || !path.is_file() {
            return Err("Select an existing absolute path to opus.dll".into());
        }
        // The explicitly selected or current game's absolute path avoids DLL search-path loading.
        unsafe {
            let library =
                libloading::Library::new(path).map_err(|e| format!("Cannot load Opus: {e}"))?;
            let create = *library
                .get::<Create>(b"opus_decoder_create\0")
                .map_err(|e| e.to_string())?;
            let decode = *library
                .get::<Decode>(b"opus_decode_float\0")
                .map_err(|e| e.to_string())?;
            let destroy = *library
                .get::<Destroy>(b"opus_decoder_destroy\0")
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(Self {
                _library: library,
                create,
                decode,
                destroy,
            }))
        }
    }

    pub(super) fn decoder(self: &Arc<Self>) -> Result<Decoder, String> {
        let mut error = 0;
        // libopus performs the supported 48 kHz -> 16 kHz conversion and downmix.
        let handle = unsafe { (self.create)(16_000, 1, &mut error) };
        if handle.is_null() || error != 0 {
            return Err(format!("Opus decoder initialization failed ({error})"));
        }
        Ok(Decoder {
            opus: Arc::clone(self),
            handle,
        })
    }
}

pub(super) struct Decoder {
    opus: Arc<Opus>,
    handle: *mut c_void,
}
// Each decoder has one owner, always used sequentially by its audio worker.
unsafe impl Send for Decoder {}

impl Decoder {
    pub(super) fn decode(&mut self, packet: &[u8]) -> Result<Vec<f32>, String> {
        if packet.is_empty() || packet.len() > 1275 {
            return Err("Invalid Opus packet length".into());
        }
        let mut samples = vec![0.0f32; 1920];
        let count = unsafe {
            (self.opus.decode)(
                self.handle,
                packet.as_ptr(),
                packet.len() as i32,
                samples.as_mut_ptr(),
                samples.len() as i32,
                0,
            )
        };
        if count != 320 {
            return Err(format!(
                "Expected a 20 ms USpeak frame, Opus returned {count}"
            ));
        }
        samples.truncate(count as usize);
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("Invalid Opus audio samples".into());
        }
        Ok(samples)
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { (self.opus.destroy)(self.handle) };
    }
}
