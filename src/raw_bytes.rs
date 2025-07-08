use iceoryx2::prelude::*;

/// A minimal wrapper to hold raw byte payloads for Iceoryx2.
///
/// Iceoryx2 requires payload types to implement ZeroCopySend.
/// This struct makes raw Vec<u8> compatible.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawBytes {
    pub data: Vec<u8>,
}

unsafe impl ZeroCopySend for RawBytes {}

impl RawBytes {
    /// Converts this RawBytes into a Vec<u8>.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// Creates a RawBytes struct from a Vec<u8>.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { data: bytes }
    }
}
