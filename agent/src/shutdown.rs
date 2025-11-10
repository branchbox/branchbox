use tokio::signal;
use tokio_util::sync::CancellationToken;

pub struct Shutdown {
    token: CancellationToken,
}

impl Shutdown {
    pub fn new() -> Self {
        let token = CancellationToken::new();
        let signal_token = token.clone();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            signal_token.cancel();
        });
        Self { token }
    }

    pub fn subscribe(&self) -> CancellationToken {
        self.token.clone()
    }

    pub async fn wait(&self) {
        self.token.cancelled().await;
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
