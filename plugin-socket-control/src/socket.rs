use std::{path::Path, time::Duration};

use alumet::pipeline::control::{AnonymousControlHandle, ScopedControlHandle};
use anyhow::Context;
use tokio::{
    net::{unix::SocketAddr, UnixListener, UnixStream},
    runtime::Runtime,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::command;

pub struct SocketControl {
    rt: Runtime,
    cancel_token: CancellationToken,
}

impl SocketControl {
    pub fn start_new<P: AsRef<Path>>(
        alumet_handle: ScopedControlHandle,
        socket_path: P,
    ) -> anyhow::Result<SocketControl> {
        // get socket_path as a PathBuf, so that we can send it across threads
        let socket_path = socket_path.as_ref().to_owned();

        // delete existing socket
        let _ = std::fs::remove_file(&socket_path);

        // create single-threaded runtime
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_io()
            .build()?;
        let rt_handle = rt.handle().clone();

        // create token to stop the server on demand
        let cancel_token = CancellationToken::new();
        let cloned_token = cancel_token.clone();

        let _task_handle: JoinHandle<anyhow::Result<()>> = rt.spawn(async move {
            // bind listener
            let listener = UnixListener::bind(socket_path.clone())
                .with_context(|| format!("could not bind to {}", socket_path.display()))?;

            // listen for new connections
            loop {
                tokio::select! {
                    biased;

                    _ = cloned_token.cancelled() => {
                        // stop listening
                        break
                    },
                    new_connection = listener.accept() => {
                        // handle the new connection
                        let alumet_handle = alumet_handle.anonymous().clone();
                        let rt_handle = rt_handle.clone();

                        rt_handle.spawn(async move {
                            match new_connection {
                                Ok((stream, addr)) => {
                                    if let Err(e) = handle_socket_connection(stream, addr, &alumet_handle).await {
                                        log::error!("Error in unix socket processing: {e:#}");
                                    }
                                },
                                Err(e) => {
                                    log::error!("Failed to accept new connection on unix socket: {e:#}");
                                },
                            }
                        });
                    }
                }
            }

            Ok(())
        });

        Ok(SocketControl { rt, cancel_token })
    }

    pub fn stop(&self) {
        self.cancel_token.cancel();
    }

    pub fn join(self) {
        self.rt.shutdown_timeout(Duration::from_secs(1));
    }
}

async fn handle_socket_connection(
    stream: UnixStream,
    _addr: SocketAddr,
    alumet_handle: &AnonymousControlHandle,
) -> anyhow::Result<()> {
    use anyhow::anyhow;
    use tokio::io::{AsyncBufReadExt, BufStream};

    let buf = BufStream::new(stream);
    let mut lines = buf.lines();
    while let Some(line) = lines.next_line().await? {
        let cmd = command::parse(&line)?;
        cmd.run(alumet_handle)
            .await
            .map_err(|e| anyhow!("failed to run command {line}: {e}"))?;
    }
    Ok(())
}
