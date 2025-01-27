use std::process::Stdio;
use std::str::from_utf8;

use anyhow::{Context, Error};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::select;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use wtransport::{Connection, RecvStream};

use remote_do_shared::pipe::pipe;
use remote_do_shared::pipe::CancellationPolicy::{DiscardInput, ReadToEnd};
use remote_do_shared::try_join_all;

pub(crate) async fn handler(
    connection: Connection,
    program: String,
    interrupt: CancellationToken,
) -> Result<(), Error> {
    let (mut sys_send, sys_recv) = connection.accept_bi().await?;

    try_join_all! {
        let mut cmd = Ok::<_, Error>(make_exec(sys_recv, program).await?);
        let (strout, strin) = Ok(connection.accept_bi().await?);
        let strerr = Ok(connection.open_uni().await?.await?);
    }

    let mut child = cmd.spawn().context("Failed to spawn process")?;
    let stdin = child.stdin.take().context("stdin should be piped")?;
    let stdout = child.stdout.take().context("stdout should be piped")?;
    let stderr = child.stderr.take().context("stderr should be piped")?;

    let mut wg = JoinSet::new();
    let exited = CancellationToken::new();

    wg.spawn(pipe(strin, stdin, exited.clone(), DiscardInput));
    wg.spawn(pipe(stdout, strout, exited.clone(), ReadToEnd));
    wg.spawn(pipe(stderr, strerr, exited.clone(), ReadToEnd));

    let exit = select! {
        exit = child.wait() => {
            exit
        }

        _ = interrupt.cancelled() => {
            child.start_kill()?;
            child.wait().await
        }
    }?;

    let data = exit.code().context("process should be exited")?;
    sys_send.write_u32(data as u32).await?;

    exited.cancel();
    while wg.join_next().await.is_some() {}

    Ok(())
}

async fn make_exec(mut sys_recv: RecvStream, program: String) -> Result<Command, Error> {
    let mut buf = vec![];
    sys_recv.read_to_end(&mut buf).await?;
    let data = from_utf8(&buf)?;
    let args = shell_words::split(data)?;

    let mut cmd = Command::new(program);
    cmd.args(args.as_slice())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(cmd)
}
