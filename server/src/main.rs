use std::env;
use std::time::Duration;

use anyhow::{Context, Error};
use log::{info, warn, LevelFilter};
use tokio::select;
use tokio::signal::ctrl_c;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use wtransport::endpoint::IncomingSession;
use wtransport::{Endpoint, Identity, ServerConfig};

use remote_do_shared::DEFAULT_PORT;

use crate::handler::handler;

mod handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_module("remote_do_server", LevelFilter::Info)
        .init();

    let program = env::var("REMOTE_DO_PROGRAM").context("failed to read REMOTE_DO_PROGRAM")?;
    let domain = env::var("REMOTE_DO_DOMAIN").unwrap_or("localhost".to_string());
    let port = env::var("REMOTE_DO_PORT")
        .map(|s| s.parse())
        .unwrap_or(Ok(DEFAULT_PORT))?;

    let config = ServerConfig::builder()
        .with_bind_default(port)
        .with_identity(Identity::self_signed([&domain])?)
        .keep_alive_interval(Duration::from_secs(1).into())
        .max_idle_timeout(Duration::from_secs(3).into())?
        .build();

    info!(
        "remote-do-server (program: {}, domain: {})",
        program, domain,
    );

    let server = Endpoint::server(config)?;

    info!("listening on port {}", port);

    let mut wg = JoinSet::new();
    let ct = CancellationToken::new();

    loop {
        select! {
            session = server.accept() => {
                wg.spawn(with_logger(session, program.clone(), ct.clone()));
            }

            res = ctrl_c() => {
                res?;
                break;
            }
        }
    }

    while wg.try_join_next().is_some() {}
    if !wg.is_empty() {
        info!("waiting for running processes... (again for terminating them)");

        while !wg.is_empty() {
            select! {
                Some(res) = wg.join_next() => {
                    res?;
                }

                res = ctrl_c() => {
                    res?;

                    info!("terminating all processes...");
                    ct.cancel();
                }
            }
        }
    }

    info!("good bye!");

    Ok(())
}

async fn with_logger(session: IncomingSession, program: String, ct: CancellationToken) {
    match async { Ok::<_, Error>(session.await?.accept().await?) }.await {
        Ok(connection) => {
            let id = connection.stable_id();
            info!(
                "[{}] connection accepted from {}",
                id,
                connection.remote_address(),
            );
            if let Err(e) = handler(connection, program, ct).await {
                warn!("[{}] {:#}", id, e);
            }
            info!("[{}] connection closed", id);
        }

        Err(e) => {
            warn!("{:#}", e);
        }
    }
}
