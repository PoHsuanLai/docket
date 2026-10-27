//! Newline-delimited JSON over a byte stream, for the binary (stdin and stdout), for an agent's
//! pipes and for tests (the two ends of an in-memory duplex): nothing else on the stream.
//! [`capped_lines`] is what both ACP edges hand the SDK: the SDK's own line source holds a line of
//! any length, so a peer we do not trust gets ours.

use agent_client_protocol::Lines;
use futures_util::{Sink, Stream};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// The longest line a peer may send, in bytes: the agent child's stdout and the editor's input
/// both end at it. A line past it is not a message, and nothing buffers it.
pub const LINE_MAX: usize = 4 * 1024 * 1024;

/// How long a write to the peer may take. A peer that stops reading ends the connection after it,
/// as a closed one does, instead of stalling what writes to it for good.
const WRITE_SPAN: Duration = Duration::from_secs(60);

/// Why a line could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LineFault {
    /// The peer sent more than the cap before a newline.
    #[error("a line over {cap} bytes")]
    TooLong {
        /// The cap that was passed.
        cap: usize,
    },
    /// The line is not UTF-8.
    #[error("a line that is not text")]
    NotText,
    /// The stream failed.
    #[error("the stream failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The next line of `reader`, without its newline (or CR), holding at most `cap` bytes.
/// `buf` keeps what has been read of an unfinished line between calls, so a dropped read loses
/// no input and a retry carries on. `None` is the end of the stream. A line over the cap is an
/// error; the stream is then in the middle of it and is to be closed, not read on.
pub async fn read_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> Result<Option<String>, LineFault> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return match buf.is_empty() {
                true => Ok(None),
                false => text_of(std::mem::take(buf)).map(Some),
            };
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if buf.len().saturating_add(take) > cap {
            buf.clear();
            return Err(LineFault::TooLong { cap });
        }
        buf.extend_from_slice(&chunk[..take]);
        std::pin::Pin::new(&mut *reader).consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return text_of(std::mem::take(buf)).map(Some);
        }
    }
}

fn text_of(mut bytes: Vec<u8>) -> Result<String, LineFault> {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| LineFault::NotText)
}

/// The lines of `reader` as the SDK reads them: blank lines skipped, and the stream ends, with a
/// warning, at a line over `cap` bytes or one that is not text. The SDK's own line source holds a
/// line of any length in memory, so it is not used for a peer we do not trust.
fn lines_of<R>(reader: R, cap: usize) -> impl Stream<Item = io::Result<String>> + Send + 'static
where
    R: AsyncBufRead + Unpin + Send + 'static,
{
    futures_util::stream::unfold(
        (reader, Vec::new()),
        move |(mut reader, mut pending)| async move {
            loop {
                match read_capped(&mut reader, &mut pending, cap).await {
                    Ok(Some(line)) if line.trim().is_empty() => {}
                    Ok(Some(line)) => return Some((Ok(line), (reader, pending))),
                    Ok(None) => return None,
                    Err(fault) => {
                        // The connection ends on a line it cannot take; the peer is not told why.
                        tracing::warn!(%fault, "closing the wire");
                        return None;
                    }
                }
            }
        },
    )
}

/// `writer` as a sink of messages, one line each, flushed, each written within `span`.
fn sink_of<W>(writer: W, span: Duration) -> impl Sink<String, Error = io::Error> + Send + 'static
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    futures_util::sink::unfold(writer, move |mut writer: W, line: String| async move {
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        let written = tokio::time::timeout(span, async {
            writer.write_all(&bytes).await?;
            writer.flush().await
        })
        .await;
        match written {
            Ok(Ok(())) => Ok(writer),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the peer does not read",
            )),
        }
    })
}

/// A transport for the SDK over a reader and a writer, with lines held to [`LINE_MAX`].
pub fn capped_lines<R, W>(
    reader: R,
    writer: W,
) -> Lines<
    impl Sink<String, Error = io::Error> + Send + 'static,
    impl Stream<Item = io::Result<String>> + Send + 'static,
>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    Lines::new(sink_of(writer, WRITE_SPAN), lines_of(reader, LINE_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn the_sdk_gets_whole_lines_and_the_stream_ends_at_one_over_the_cap() {
        let input: &[u8] = b"one\r\n\ntwo\n0123456789\nrest";
        let mut lines = Box::pin(lines_of(BufReader::new(input), 8));
        assert_eq!(
            lines.next().await.transpose().expect("line").as_deref(),
            Some("one")
        );
        assert_eq!(
            lines.next().await.transpose().expect("line").as_deref(),
            Some("two")
        );
        // Ten bytes against a cap of eight: the stream ends, nothing is truncated or read on.
        assert!(lines.next().await.is_none());
    }

    #[tokio::test]
    async fn a_writer_nobody_reads_ends_the_sink_at_its_span() {
        let (writer, _unread) = tokio::io::duplex(1);
        let mut sink = Box::pin(sink_of(writer, Duration::from_millis(20)));
        let sent = sink.send("a line longer than the buffer".to_owned()).await;
        assert_eq!(sent.expect_err("timed out").kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn an_unfinished_last_line_is_still_a_line() {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(&b"tail"[..]);
        assert_eq!(
            read_capped(&mut reader, &mut buf, 8).await.expect("line"),
            Some("tail".to_owned())
        );
        assert_eq!(
            read_capped(&mut reader, &mut buf, 8).await.expect("end"),
            None
        );
    }
}
