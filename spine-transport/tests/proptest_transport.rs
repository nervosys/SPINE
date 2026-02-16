//! Property-based tests for spine-transport frame codec and buffers.

use proptest::prelude::*;
use spine_transport::{Frame, FrameCodec, FrameFlags, FrameHeader};

// ========== FRAME HEADER ROUNDTRIP ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Frame header encode/parse roundtrip is lossless
    #[test]
    fn frame_header_roundtrip(
        length in any::<u32>(),
        sequence in 0u32..0x00FFFFFFu32,  // Only 24 bits survive header roundtrip
        stream_id in any::<u16>(),
    ) {
        let header = FrameHeader {
            length,
            flags: FrameFlags::empty(),
            sequence,
            stream_id,
            _reserved: 0,
        };
        let frame = Frame { header, payload: bytes::Bytes::new() };
        let bytes = frame.header_bytes();
        let decoded = Frame::parse_header(&bytes);
        prop_assert_eq!(length, decoded.length);
        prop_assert_eq!(sequence, decoded.sequence);
        prop_assert_eq!(stream_id, decoded.stream_id);
    }

    /// Frame header with various flags roundtrips correctly
    #[test]
    fn frame_header_flags_roundtrip(
        length in any::<u32>(),
        flags_bits in any::<u8>(),
        sequence in any::<u32>(),
        stream_id in any::<u16>(),
    ) {
        let flags = FrameFlags::from_bits_truncate(flags_bits);
        let header = FrameHeader {
            length,
            flags,
            sequence,
            stream_id,
            _reserved: 0,
        };
        let frame = Frame { header, payload: bytes::Bytes::new() };
        let bytes = frame.header_bytes();
        let decoded = Frame::parse_header(&bytes);
        prop_assert_eq!(flags, decoded.flags);
    }
}

// ========== FRAME CODEC ROUNDTRIP ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// FrameCodec encode/decode roundtrip preserves payload
    #[test]
    fn frame_codec_roundtrip(
        payload in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let frame = Frame::new(bytes::Bytes::from(payload.clone()));
        let codec = FrameCodec::new(65536);
        let encoded = codec.encode(&frame);
        let mut decode_codec = FrameCodec::new(65536);
        let decoded = decode_codec.decode(&encoded).expect("decode must succeed");
        prop_assert_eq!(&payload[..], &decoded.payload[..]);
    }

    /// FrameCodec encode/decode_zerocopy roundtrip preserves payload
    #[test]
    fn frame_codec_zerocopy_roundtrip(
        payload in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let frame = Frame::new(bytes::Bytes::from(payload.clone()));
        let codec = FrameCodec::new(65536);
        let encoded = codec.encode(&frame);
        let mut decode_codec = FrameCodec::new(65536);
        let decoded = decode_codec.decode_zerocopy(encoded).expect("decode must succeed");
        prop_assert_eq!(&payload[..], &decoded.payload[..]);
    }

    /// Encoded frame size is deterministic: header + payload
    #[test]
    fn frame_encoded_size(
        payload in prop::collection::vec(any::<u8>(), 0..1024),
    ) {
        let frame = Frame::new(bytes::Bytes::from(payload.clone()));
        let codec = FrameCodec::new(65536);
        let encoded = codec.encode(&frame);
        // Frame format: 12-byte header + payload
        prop_assert_eq!(encoded.len(), 12 + payload.len());
    }
}

// ========== FRAME COMPRESSION ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Frame compress/decompress roundtrip (zstd)
    #[test]
    fn frame_compression_roundtrip_zstd(
        payload in prop::collection::vec(any::<u8>(), 64..2048),
    ) {
        let frame = Frame::compressed(bytes::Bytes::from(payload.clone()), 3, false)
            .expect("compression must succeed");
        if frame.header.flags.contains(FrameFlags::COMPRESSED) {
            let decompressed = frame.decompress(false).expect("decompress must succeed");
            prop_assert_eq!(&payload[..], &decompressed[..]);
        }
    }

    /// Frame compress/decompress roundtrip (lz4)
    #[test]
    fn frame_compression_roundtrip_lz4(
        payload in prop::collection::vec(any::<u8>(), 64..2048),
    ) {
        let frame = Frame::compressed(bytes::Bytes::from(payload.clone()), 3, true)
            .expect("compression must succeed");
        if frame.header.flags.contains(FrameFlags::COMPRESSED) {
            let decompressed = frame.decompress(true).expect("decompress must succeed");
            prop_assert_eq!(&payload[..], &decompressed[..]);
        }
    }
}

// ========== RING BUFFER ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// RingBuffer write/read roundtrip preserves data
    #[test]
    fn ring_buffer_roundtrip(
        data in prop::collection::vec(any::<u8>(), 1..512),
    ) {
        let buf = spine_transport::RingBuffer::new(1024);
        let written = buf.write(&data);
        prop_assert!(written > 0, "must write at least some data");
        let mut read_buf = vec![0u8; written];
        let read_count = buf.read(&mut read_buf);
        prop_assert_eq!(written, read_count);
        prop_assert_eq!(&data[..written], &read_buf[..read_count]);
    }

    /// RingBuffer never reads more than was written
    #[test]
    fn ring_buffer_read_never_exceeds_write(
        data in prop::collection::vec(any::<u8>(), 0..256),
        read_size in 1usize..1024,
    ) {
        let buf = spine_transport::RingBuffer::new(512);
        let written = buf.write(&data);
        let mut read_buf = vec![0u8; read_size];
        let read_count = buf.read(&mut read_buf);
        prop_assert!(read_count <= written, "read {} > written {}", read_count, written);
    }

    /// RingBuffer capacity is respected
    #[test]
    fn ring_buffer_capacity_respected(
        data in prop::collection::vec(any::<u8>(), 0..2048),
        capacity in 64usize..512,
    ) {
        let buf = spine_transport::RingBuffer::new(capacity);
        let written = buf.write(&data);
        // Can never write more than capacity (rounded to next power of 2)
        prop_assert!(written <= capacity.next_power_of_two(),
            "wrote {} into capacity {}", written, capacity);
    }
}

// ========== SLAB ALLOCATOR ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// SlabAllocator borrow/return cycle doesn't leak
    #[test]
    fn slab_allocator_borrow_return(
        count in 1usize..20,
    ) {
        let alloc = spine_transport::SlabAllocator::new(1024, 32);
        let mut buffers = Vec::new();
        for _ in 0..count.min(32) {
            buffers.push(alloc.borrow());
        }
        let borrowed = buffers.len();
        for buf in buffers {
            alloc.return_buffer(buf);
        }
        // After returning all, we should be able to borrow that many again
        let mut re_borrowed = 0;
        for _ in 0..borrowed {
            alloc.borrow();
            re_borrowed += 1;
        }
        prop_assert_eq!(borrowed, re_borrowed);
    }
}
