use std::future::Future;

use anyhow::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Stderr, Stdout};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::{pin, select};
use tokio_fd::AsyncFd;
use tokio_util::sync::CancellationToken;
use wtransport::quinn::VarInt;
use wtransport::{RecvStream, SendStream};

pub trait Closable: Unpin {
    fn close(&mut self) -> impl Future<Output = Result<(), Error>>;
}

impl Closable for RecvStream {
    async fn close(&mut self) -> Result<(), Error> {
        self.quic_stream_mut().stop(VarInt::from_u32(0))?;
        Ok(())
    }
}

impl Closable for SendStream {
    async fn close(&mut self) -> Result<(), Error> {
        self.finish().await?;
        Ok(())
    }
}

impl Closable for AsyncFd {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Closable for Stdout {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Closable for Stderr {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Closable for ChildStdin {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Closable for ChildStdout {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Closable for ChildStderr {
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(PartialEq)]
pub enum CancellationPolicy {
    DiscardInput,
    ReadToEnd,
}

pub async fn pipe(
    from: impl AsyncRead + Closable,
    to: impl AsyncWrite + Closable,
    ct: CancellationToken,
    policy: CancellationPolicy,
) -> Result<(), Error> {
    pin!(from, to);
    let mut buf = [0; 8];

    loop {
        select! {
            size = from.read(&mut buf) => {
                let size = size?;
                to.write_all(&buf[..size]).await?;
            }

            _ = ct.cancelled() => {
                break;
            }
        }
    }

    if policy == CancellationPolicy::ReadToEnd {
        let mut buf = vec![];
        from.read_to_end(&mut buf).await?;
        to.write_all(&buf).await?;
    }

    from.close().await?;
    to.close().await?;

    Ok(())
}
