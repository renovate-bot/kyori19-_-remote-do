use std::env;
use std::io::stdin;
use std::os::fd::AsRawFd;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Error;
use tokio::io::{stderr, stdout, AsyncReadExt};
use tokio::task::JoinSet;
use tokio_fd::AsyncFd;
use tokio_util::sync::CancellationToken;
use wtransport::{ClientConfig, Endpoint};

use remote_do_shared::pipe::pipe;
use remote_do_shared::pipe::CancellationPolicy::{DiscardInput, ReadToEnd};
use remote_do_shared::{try_join_all, DEFAULT_PORT};

#[tokio::main]
async fn main() -> Result<ExitCode, Error> {
    let endpoint = env::var("REMOTE_DO_ENDPOINT")
        .unwrap_or(format!("https://localhost:{}/rd/v1", DEFAULT_PORT));

    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Duration::from_secs(1).into())
        .max_idle_timeout(Duration::from_secs(3).into())?
        .build();

    let connection = Endpoint::client(config)?.connect(endpoint).await?;

    let args = shell_words::join(env::args().skip(1));
    let (mut sys_send, mut sys_recv) = connection.open_bi().await?.await?;

    try_join_all! {
        let _ = Ok({
            sys_send.write_all(args.as_ref()).await?;
            sys_send.finish().await?
        });
        let (strin, strout) = Ok(connection.open_bi().await?.await?);
        let strerr = Ok::<_, Error>(connection.accept_uni().await?);
    }

    let mut wg = JoinSet::new();
    let exited = CancellationToken::new();

    wg.spawn(pipe(
        AsyncFd::try_from(stdin().as_raw_fd())?,
        strin,
        exited.clone(),
        DiscardInput,
    ));
    wg.spawn(pipe(strout, stdout(), exited.clone(), ReadToEnd));
    wg.spawn(pipe(strerr, stderr(), exited.clone(), ReadToEnd));

    let code: u8 = sys_recv.read_u32().await?.try_into()?;
    let exit = ExitCode::from(code);

    exited.cancel();
    while wg.join_next().await.is_some() {}

    Ok(exit)
}
