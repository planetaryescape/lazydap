use crate::types::IpcMessage;
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

/// Frames on the socket are a 4-byte big-endian length followed by that many
/// bytes of JSON (D004, `docs/blueprint/04-protocol.md`).
const LENGTH_FIELD_BYTES: usize = 4;

/// Refuse to buffer a frame larger than this. A client that claims a 4 GiB
/// message is either broken or hostile; either way the daemon should not
/// allocate for it.
///
/// It caps what can be *sent* as well, which is the half a caller notices:
/// `variables --max 0` on a container with a million elements has an answer
/// that does not fit down this socket. Public so the sender can say so.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Whether a failure means the message could never have become a frame, as
/// opposed to the socket going away.
///
/// The distinction is worth a function because the two want opposite
/// reactions. A frame that cannot be encoded — too large, or holding
/// something `serde_json` refuses — never reaches the wire at all, so the
/// request that produced it can still be answered, with an error saying why.
/// Treating that as a dead peer is what made an over-sized reply look like
/// "the daemon closed the connection before answering" (D-WP3-4).
///
/// Both kinds come out of [`IpcCodec`] itself: `InvalidInput` from the length
/// prefix refusing an over-long body, `InvalidData` from serialisation. A
/// Unix-socket write produces neither.
pub fn is_unframeable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData
    )
}

/// Tokio codec that frames [`IpcMessage`]s on the daemon socket.
///
/// Malformed JSON surfaces as [`std::io::ErrorKind::InvalidData`] rather than
/// tearing the connection down silently, so the daemon can answer with a
/// `BadRequest` before hanging up.
pub struct IpcCodec {
    inner: LengthDelimitedCodec,
}

impl IpcCodec {
    pub fn new() -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .length_field_length(LENGTH_FIELD_BYTES)
                .max_frame_length(MAX_FRAME_BYTES)
                .new_codec(),
        }
    }
}

impl IpcCodec {
    /// Frame bytes that are already the JSON body.
    ///
    /// Used only by the shutdown escape hatch, which builds its frame by hand
    /// so that it cannot follow this build's schema. Framing is still applied
    /// here, because the framing is the part that has never changed.
    pub fn encode_raw(&mut self, body: &[u8], dst: &mut BytesMut) -> std::io::Result<()> {
        self.inner.encode(bytes::Bytes::copy_from_slice(body), dst)
    }
}

impl Default for IpcCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for IpcCodec {
    type Item = IpcMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.inner.decode(src)? {
            Some(frame) => {
                let message: IpcMessage = serde_json::from_slice(&frame)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(Some(message))
            }
            None => Ok(None),
        }
    }
}

impl Encoder<IpcMessage> for IpcCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: IpcMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let json = serde_json::to_vec(&item)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.inner.encode(json.into(), dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Request, Response};
    use std::io::ErrorKind;

    fn encoded(message: IpcMessage) -> BytesMut {
        let mut buffer = BytesMut::new();
        IpcCodec::new()
            .encode(message, &mut buffer)
            .expect("encode");
        buffer
    }

    #[test]
    fn a_message_survives_a_round_trip_through_the_codec() {
        let message = IpcMessage::response(
            9,
            Response::Pong {
                version: crate::LAZYDAP_PROTOCOL_VERSION,
                instance: "lazydap-test".into(),
                uptime_ms: 12,
            },
        );

        let mut buffer = encoded(message.clone());
        let decoded = IpcCodec::new()
            .decode(&mut buffer)
            .expect("decode")
            .expect("a whole frame");

        assert_eq!(decoded, message);
        assert!(buffer.is_empty(), "the frame should be consumed whole");
    }

    #[test]
    fn a_partial_frame_decodes_to_nothing_until_the_rest_arrives() {
        let whole = encoded(IpcMessage::request(1, Request::Ping));
        let mut codec = IpcCodec::new();

        let (head, tail) = whole.split_at(whole.len() - 1);
        let mut buffer = BytesMut::from(head);
        assert!(
            codec.decode(&mut buffer).expect("decode").is_none(),
            "a frame one byte short is not a frame",
        );

        buffer.extend_from_slice(tail);
        let decoded = codec.decode(&mut buffer).expect("decode").expect("frame");
        assert_eq!(decoded.payload, crate::IpcPayload::Request(Request::Ping));
    }

    #[test]
    fn the_length_prefix_is_four_big_endian_bytes() {
        let buffer = encoded(IpcMessage::request(1, Request::Ping));
        let body_len = buffer.len() - LENGTH_FIELD_BYTES;
        let prefix = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);

        assert_eq!(prefix as usize, body_len, "got prefix: {prefix}");
    }

    #[test]
    fn a_frame_of_malformed_json_is_invalid_data_not_a_hang_up() {
        let body = b"{ this is not json }";
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buffer.extend_from_slice(body);

        let err = IpcCodec::new()
            .decode(&mut buffer)
            .expect_err("malformed JSON must not decode");
        assert_eq!(err.kind(), ErrorKind::InvalidData, "got: {err}");
    }

    #[test]
    fn a_reply_too_large_to_frame_is_told_apart_from_a_dead_socket() {
        // What the daemon branches on to answer the request rather than hang
        // up on it (D-WP3-4).
        let oversized = IpcCodec::new()
            .encode(
                IpcMessage::response(
                    1,
                    Response::Pong {
                        version: crate::LAZYDAP_PROTOCOL_VERSION,
                        instance: "x".repeat(MAX_FRAME_BYTES + 1),
                        uptime_ms: 0,
                    },
                ),
                &mut BytesMut::new(),
            )
            .expect_err("a body past the cap must not encode");

        assert!(is_unframeable(&oversized), "got: {oversized:?}");
        assert!(
            !is_unframeable(&std::io::Error::from(ErrorKind::BrokenPipe)),
            "a peer that went away is not an encoding failure",
        );
    }

    #[test]
    fn a_frame_larger_than_the_cap_is_refused_before_it_is_buffered() {
        let claimed = (MAX_FRAME_BYTES + 1) as u32;
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(&claimed.to_be_bytes());
        buffer.extend_from_slice(b"{}");

        let err = IpcCodec::new()
            .decode(&mut buffer)
            .expect_err("an oversized frame must be refused");
        assert_eq!(err.kind(), ErrorKind::InvalidData, "got: {err}");
    }
}
