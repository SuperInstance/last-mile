//! FNV-1a 64-bit over positions, fed in canonical little-endian byte order.

use crate::position::Position;

pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Mix bytes into a running FNV-1a 64 state (the "state hash": callers can
/// chain multiple buffers through `h`).
#[inline]
pub fn fnv1a64_state(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_state(bytes, FNV_OFFSET_BASIS)
}

/// Hash a trajectory of positions. Each position is normalized first (so the
/// hash is independent of representation), then fed as 16 bytes: chunk.x,
/// chunk.y, off.x, off.y, each i32 in little-endian order.
pub fn hash_positions(positions: &[Position]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for p in positions {
        let c = p.normalized();
        for part in [c.chunk.0, c.chunk.1, c.off.0, c.off.1] {
            h = fnv1a64_state(&part.to_le_bytes(), h);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn state_chaining_equals_single_pass() {
        assert_eq!(
            fnv1a64_state(b"foo", fnv1a64_state(b"bar", FNV_OFFSET_BASIS)),
            fnv1a64(b"barfoo")
        );
    }

    #[test]
    fn position_hash_is_canonical() {
        let a = Position { chunk: (1, 2), off: (100, 200) };
        // same global position, unnormalized offsets
        let b = Position { chunk: (0, 2), off: (100 + 32768, 200) };
        let c = Position { chunk: (2, 2), off: (100 - 32768, 200) };
        assert_eq!(hash_positions(&[a]), hash_positions(&[b]));
        assert_eq!(hash_positions(&[a]), hash_positions(&[c]));
        assert_ne!(hash_positions(&[a]), hash_positions(&[Position::origin()]));

        // matches the explicit byte stream
        let mut bytes = Vec::new();
        for part in [1i32, 2, 100, 200] {
            bytes.extend_from_slice(&part.to_le_bytes());
        }
        assert_eq!(hash_positions(&[a]), fnv1a64(&bytes));
    }
}
