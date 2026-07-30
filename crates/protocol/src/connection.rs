use crate::codec::IpcCodec;
use crate::types::IpcMessage;
use bytes::BytesMut;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Encoder};

/// How much room to give the read buffer up front. One IPC message is
/// typically a few hundred bytes; this is enough for a burst without growing.
const READ_BUFFER_BYTES: usize = 8 * 1024;

/// A framed [`IpcMessage`] conversation over any byte stream.
///
/// Both ends of the socket use this, so client and daemon cannot drift apart
/// on framing. It deliberately drives [`IpcCodec`] by hand rather than through
/// `tokio_util::codec::Framed`: `Framed` needs the `futures` `SinkExt`/
/// `StreamExt` traits, and the whole dependency is not worth two method calls.
pub struct IpcConnection<S> {
    stream: S,
    codec: IpcCodec,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl<S> IpcConnection<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            codec: IpcCodec::new(),
            read_buf: BytesMut::with_capacity(READ_BUFFER_BYTES),
            write_buf: BytesMut::new(),
        }
    }
}

impl<S: AsyncWrite + Unpin> IpcConnection<S> {
    /// Write one message and flush it.
    ///
    /// Not cancellation-safe: a cancelled send can leave a half-written frame
    /// on the wire. Send from the body of a `select!` branch, never as one of
    /// its arms.
    pub async fn send(&mut self, message: IpcMessage) -> io::Result<()> {
        self.write_buf.clear();
        self.codec.encode(message, &mut self.write_buf)?;
        self.stream.write_all(&self.write_buf).await?;
        self.stream.flush().await
    }
}

impl<S: AsyncRead + Unpin> IpcConnection<S> {
    /// Read the next message, or `None` when the peer hangs up cleanly between
    /// frames.
    ///
    /// Cancellation-safe: every byte read lands in `read_buf` before this
    /// returns, so a cancelled `recv` loses nothing and the next one resumes
    /// mid-frame without noticing.
    pub async fn recv(&mut self) -> io::Result<Option<IpcMessage>> {
        loop {
            if let Some(message) = self.codec.decode(&mut self.read_buf)? {
                return Ok(Some(message));
            }
            if self.stream.read_buf(&mut self.read_buf).await? == 0 {
                // A peer that vanishes part way through a frame is a different
                // problem from one that hangs up politely, and callers treat
                // them differently.
                return if self.read_buf.is_empty() {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "peer hung up part way through a frame",
                    ))
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Request, Response};

    #[tokio::test]
    async fn messages_survive_a_round_trip_over_a_real_socket() {
        let (client, server) = tokio::io::duplex(64);
        let mut client = IpcConnection::new(client);
        let mut server = IpcConnection::new(server);

        // A duplex buffer smaller than the frames forces the reader to loop,
        // which is the case a single `read` would silently get wrong.
        let sent = IpcMessage::response(
            3,
            Response::Pong {
                version: crate::LAZYDAP_PROTOCOL_VERSION,
                instance: "lazydap-".to_string() + &"x".repeat(200),
                uptime_ms: 5,
            },
        );
        let expected = sent.clone();
        tokio::spawn(async move { client.send(sent).await });

        let received = server.recv().await.expect("recv").expect("a message");
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn a_clean_hang_up_between_frames_reads_as_end_of_stream() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = IpcConnection::new(client);
        let mut server = IpcConnection::new(server);

        client
            .send(IpcMessage::request(1, Request::Ping))
            .await
            .expect("send");
        drop(client);

        assert!(server.recv().await.expect("recv").is_some());
        assert!(
            server.recv().await.expect("recv").is_none(),
            "a polite hang-up is not an error",
        );
    }

    #[tokio::test]
    async fn a_hang_up_mid_frame_is_an_error_not_an_end_of_stream() {
        let (mut client, server) = tokio::io::duplex(1024);
        let mut server = IpcConnection::new(server);

        // Claim a 64-byte body, then send four bytes and leave.
        client
            .write_all(&64u32.to_be_bytes())
            .await
            .expect("write header");
        client
            .write_all(b"{\"ve")
            .await
            .expect("write partial body");
        drop(client);

        let err = server
            .recv()
            .await
            .expect_err("a truncated frame must not look like a clean close");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "got: {err}");
    }
}
