//! Frame-header and fixed-point interop between `spine-nostd` and
//! `spine-embedded`. These were in `spine-protocol/tests/e2e_protocol.rs`
//! under "Frame Format Interop"; they never used `spine-protocol`, and the root
//! crate may not depend on `research/`, so they live here.

#[test]
fn e2e_nostd_frame_header_roundtrip() {
    use spine_nostd::codec::{decode_frame_header, encode_frame_header};
    use spine_nostd::types::FrameHeader;

    let header = FrameHeader::new(1024, 0x01, 42);
    let mut buf = [0u8; 12];
    encode_frame_header(&header, &mut buf);

    let decoded = decode_frame_header(&buf).unwrap();
    assert_eq!(decoded.payload_len, 1024);
    assert_eq!(decoded.frame_type, 0x01);
    assert_eq!(decoded.sequence, 42);
    assert_eq!(decoded.checksum, header.checksum);
}

#[test]
fn e2e_embedded_message_frame_compat() {
    use spine_embedded::EmbeddedMessage;
    use spine_nostd::codec::decode_frame_header;
    use spine_nostd::types::AgentIdBytes;

    let src = AgentIdBytes([1; 16]);
    let dst = AgentIdBytes([2; 16]);
    let mut msg = EmbeddedMessage::new(src, dst, 0x05);
    msg.set_payload(b"hello");
    msg.sequence = 100;

    let mut buf = [0u8; 12];
    msg.encode_header(&mut buf);

    let decoded = decode_frame_header(&buf).unwrap();
    assert_eq!(decoded.payload_len, msg.payload_len as u32);
    assert_eq!(decoded.frame_type, 0x05);
    assert_eq!(decoded.sequence, 100);
}

#[test]
fn e2e_embedded_latent_vector_compatibility() {
    use spine_nostd::math::{cosine_similarity_fixed, dot_product_fixed};
    use spine_nostd::types::LatentVectorFixed;

    let v1 = LatentVectorFixed::<8> {
        data: [256, 128, 64, 32, 16, 8, 4, 2],
        len: 8,
    };
    let v2 = LatentVectorFixed::<8> {
        data: [256, 256, 256, 256, 256, 256, 256, 256],
        len: 8,
    };

    let dot = dot_product_fixed(&v1.data, &v2.data);
    assert!(dot > 0, "Dot product should be positive");

    let sim = cosine_similarity_fixed(&v1.data, &v2.data);
    assert!(
        sim > 0,
        "Cosine similarity should be positive for aligned vectors"
    );
}
