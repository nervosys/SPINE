//! Frame encoding/decoding for no_std environments.

use crate::types::FrameHeader;

/// Encode a frame header into a 12-byte buffer.
pub fn encode_frame_header(header: &FrameHeader, buf: &mut [u8; 12]) {
    let len_bytes = header.payload_len.to_be_bytes();
    buf[0] = len_bytes[0];
    buf[1] = len_bytes[1];
    buf[2] = len_bytes[2];
    buf[3] = len_bytes[3];
    buf[4] = header.flags;
    buf[5] = header.frame_type;
    let seq_bytes = header.sequence.to_be_bytes();
    buf[6] = seq_bytes[0];
    buf[7] = seq_bytes[1];
    let chk_bytes = header.checksum.to_be_bytes();
    buf[8] = chk_bytes[0];
    buf[9] = chk_bytes[1];
    buf[10] = chk_bytes[2];
    buf[11] = chk_bytes[3];
}

/// Decode a frame header from a 12-byte buffer.
///
/// Returns `None` if the buffer is too small.
pub fn decode_frame_header(buf: &[u8]) -> Option<FrameHeader> {
    if buf.len() < 12 {
        return None;
    }
    Some(FrameHeader {
        payload_len: u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]),
        flags: buf[4],
        frame_type: buf[5],
        sequence: u16::from_be_bytes([buf[6], buf[7]]),
        checksum: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
    })
}

/// Compute a simple checksum over a payload (CRC-like, no_std friendly).
///
/// Uses a 32-bit variant of Adler-32 for speed.
pub fn checksum_adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let header = FrameHeader {
            payload_len: 0x12345678,
            flags: 0x03,
            frame_type: 0x05,
            sequence: 0xABCD,
            checksum: 0xDEADBEEF,
        };

        let mut buf = [0u8; 12];
        encode_frame_header(&header, &mut buf);
        let decoded = decode_frame_header(&buf).unwrap();

        assert_eq!(decoded, header);
    }

    #[test]
    fn test_decode_too_short() {
        let buf = [0u8; 8];
        assert!(decode_frame_header(&buf).is_none());
    }

    #[test]
    fn test_checksum_deterministic() {
        let data = b"hello spine";
        let c1 = checksum_adler32(data);
        let c2 = checksum_adler32(data);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_checksum_differs() {
        let c1 = checksum_adler32(b"hello");
        let c2 = checksum_adler32(b"world");
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_checksum_empty() {
        let c = checksum_adler32(b"");
        assert_eq!(c, 1); // Adler-32 of empty = 0x00000001
    }
}
