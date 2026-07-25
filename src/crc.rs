//! CRC-32 (IEEE 802.3), used to detect torn or corrupted WAL records.
//!
//! A checksum is what separates "the file ended" from "the file lied". Without
//! it a half-written record whose length field happens to look plausible would
//! be replayed as real data.

const POLYNOMIAL: u32 = 0xEDB8_8320;

/// Compute the CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            // Branchless: mask is all-ones when the low bit is set.
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (POLYNOMIAL & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn known_vectors() {
        // Standard CRC-32 test vectors.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn detects_single_bit_flip() {
        let original = crc32(b"hello world");
        let flipped = crc32(b"hello worle"); // one bit differs
        assert_ne!(original, flipped);
    }
}
