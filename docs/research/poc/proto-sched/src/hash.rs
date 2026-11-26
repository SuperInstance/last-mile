//! FNV-1a 64, written by hand (std only).

pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Streaming FNV-1a 64-bit hash.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a64 {
    state: u64,
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1a64 {
    pub fn new() -> Self {
        Fnv1a64 { state: FNV_OFFSET_BASIS }
    }

    pub fn write(&mut self, bytes: &[u8]) -> &mut Self {
        for &b in bytes {
            self.state ^= b as u64;
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        self
    }

    pub fn write_u8(&mut self, v: u8) -> &mut Self {
        self.write(&[v])
    }

    pub fn write_u16(&mut self, v: u16) -> &mut Self {
        self.write(&v.to_le_bytes())
    }

    pub fn write_u32(&mut self, v: u32) -> &mut Self {
        self.write(&v.to_le_bytes())
    }

    pub fn write_u64(&mut self, v: u64) -> &mut Self {
        self.write(&v.to_le_bytes())
    }

    pub fn write_i16(&mut self, v: i16) -> &mut Self {
        self.write(&v.to_le_bytes())
    }

    pub fn write_i32(&mut self, v: i32) -> &mut Self {
        self.write(&v.to_le_bytes())
    }

    pub fn finish(&self) -> u64 {
        self.state
    }
}

/// One-shot convenience wrapper.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write(bytes);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn streaming_matches_one_shot() {
        let mut h = Fnv1a64::new();
        h.write(b"foo").write(b"bar");
        assert_eq!(h.finish(), fnv1a64(b"foobar"));
    }
}
