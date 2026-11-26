//! Canonical hashing: FNV-1a 64 over explicit little-endian encodings.
//!
//! A round's state hash has to be identical on every machine that replays it,
//! so nothing here depends on the host's endianness, on memory layout or on
//! the iteration order of a `HashMap`. Every value is written as explicit
//! little-endian bytes.

/// The FNV-1a 64-bit offset basis.
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV-1a 64-bit prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// An incremental FNV-1a 64 hasher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv64 {
    state: u64,
}

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    /// A fresh hasher, at the offset basis.
    pub fn new() -> Self {
        Self {
            state: FNV_OFFSET_BASIS,
        }
    }

    /// Absorb raw bytes.
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    /// Absorb a `u16`, little-endian.
    pub fn u16(&mut self, v: u16) {
        self.write(&v.to_le_bytes());
    }

    /// Absorb a `u32`, little-endian.
    pub fn u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }

    /// Absorb a `u64`, little-endian.
    pub fn u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    /// Absorb an `i32`, little-endian (two's complement).
    pub fn i32(&mut self, v: i32) {
        self.write(&v.to_le_bytes());
    }

    /// Absorb an `i64`, little-endian (two's complement).
    pub fn i64(&mut self, v: i64) {
        self.write(&v.to_le_bytes());
    }

    /// The hash of everything absorbed so far.
    pub fn finish(self) -> u64 {
        self.state
    }
}

/// Anything with a canonical byte encoding can be hashed.
pub trait StateHash {
    /// Write this value into the hash, in declaration order.
    fn hash_into(&self, h: &mut Fnv64);

    /// The canonical hash of this value.
    fn state_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        self.hash_into(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_known_vector() {
        let mut h = Fnv64::new();
        h.write(b"a");
        assert_eq!(h.finish(), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn fnv_empty_is_the_offset_basis() {
        assert_eq!(Fnv64::new().finish(), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn integers_are_little_endian() {
        let mut h = Fnv64::new();
        h.u16(0x0102);
        h.u32(0x0102_0304);
        h.u64(0x0102_0304_0506_0708);
        h.i32(-1);
        h.i64(-1);

        let mut expect = Fnv64::new();
        expect.write(&[0x02, 0x01]);
        expect.write(&[0x04, 0x03, 0x02, 0x01]);
        expect.write(&[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
        expect.write(&[0xff, 0xff, 0xff, 0xff]);
        expect.write(&[0xff; 8]);
        assert_eq!(h.finish(), expect.finish());
    }

    #[test]
    fn state_hash_uses_hash_into() {
        struct Two(u16, u16);
        impl StateHash for Two {
            fn hash_into(&self, h: &mut Fnv64) {
                h.u16(self.0);
                h.u16(self.1);
            }
        }
        let mut h = Fnv64::new();
        Two(1, 2).hash_into(&mut h);
        assert_eq!(Two(1, 2).state_hash(), h.finish());
        assert_ne!(Two(1, 2).state_hash(), Two(2, 1).state_hash());
    }
}
