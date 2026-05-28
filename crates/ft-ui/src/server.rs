//! Server bootstrap, app state, and the `run` entrypoint.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use axum::Router;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::Parser;
use cookie::Key;
use ft_ops::{EmittedEvent, EventBus, Workspace};
use rand::RngCore;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

use crate::auth::SingleUseToken;
use crate::sse::RingBuffer;

/// CLI options for the ft-ui binary.
#[derive(Debug, Clone, Parser)]
#[command(name = "ft-ui", about = "Firetrail local web UI server")]
pub struct ServerOpts {
    /// Path to the firetrail workspace.
    #[arg(long)]
    pub workspace: PathBuf,

    /// Socket address to bind to.
    #[arg(long, default_value = "127.0.0.1:0")]
    pub bind: SocketAddr,

    /// Shortcut: override the port on `--bind`.
    #[arg(long)]
    pub port: Option<u16>,

    /// `ft ui` honors this; the server itself is a no-op.
    #[arg(long, default_value_t = false)]
    pub no_open: bool,

    /// Run in foreground; suppress heartbeat-driven idle exit.
    #[arg(long, default_value_t = false)]
    pub foreground: bool,

    /// Development mode: relax `Origin` to allow Vite (`:5173`).
    #[arg(long, default_value_t = false)]
    pub dev: bool,
}

/// Shared application state.
#[derive(Debug)]
pub struct AppState {
    /// The opened workspace.
    pub workspace: Workspace,
    /// In-process event bus shared with future ops.
    pub events: EventBus,
    /// Bootstrap token (single-use).
    pub bootstrap_token: SingleUseToken,
    /// Signing key for the session cookie.
    pub session_key: Key,
    /// Address the server is actually bound to.
    pub bound_addr: SocketAddr,
    /// Process start time.
    pub started_at: Instant,
    /// Most recent heartbeat (for idle-exit).
    pub last_heartbeat: Mutex<Instant>,
    /// Whether a heartbeat has ever been seen.
    pub heartbeat_seen: AtomicBool,
    /// SSE sequence counter.
    pub sse_seq: Arc<AtomicU64>,
    /// SSE replay ring buffer.
    pub sse_ring: Arc<Mutex<RingBuffer<EmittedEvent>>>,
    /// Whether `--dev` is enabled.
    pub dev_mode: bool,
}

/// Construct an app state + router for tests. Binds nothing.
///
/// Returns the state and the router. The router does **not** have the
/// `TraceLayer` wrapped (tests don't need it).
pub fn test_app(
    workspace_root: &std::path::Path,
    bound_addr: SocketAddr,
    dev_mode: bool,
) -> anyhow::Result<(Arc<AppState>, Router)> {
    let workspace = Workspace::open(workspace_root)?;
    Ok(build_state_and_router(workspace, bound_addr, dev_mode))
}

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(&buf)
}

fn build_state_and_router(
    workspace: Workspace,
    bound_addr: SocketAddr,
    dev_mode: bool,
) -> (Arc<AppState>, Router) {
    let bootstrap_token = SingleUseToken::new(random_b64(32));
    let mut secret = vec![0u8; 64];
    rand::thread_rng().fill_bytes(&mut secret);
    let session_key = Key::from(&secret);

    let state = Arc::new(AppState {
        workspace,
        events: EventBus::default(),
        bootstrap_token,
        session_key,
        bound_addr,
        started_at: Instant::now(),
        last_heartbeat: Mutex::new(Instant::now()),
        heartbeat_seen: AtomicBool::new(false),
        sse_seq: Arc::new(AtomicU64::new(0)),
        sse_ring: Arc::new(Mutex::new(RingBuffer::new(256))),
        dev_mode,
    });

    let router = crate::routes::build(state.clone());
    (state, router)
}

/// Run the ft-ui server. Blocks until shutdown.
pub async fn run(opts: ServerOpts) -> anyhow::Result<()> {
    let workspace = Workspace::open(&opts.workspace)?;

    let mut bind = opts.bind;
    if let Some(p) = opts.port {
        bind.set_port(p);
    }

    let listener = TcpListener::bind(bind).await?;
    let bound_addr = listener.local_addr()?;

    let (state, router) = build_state_and_router(workspace, bound_addr, opts.dev);

    let app = router.layer(TraceLayer::new_for_http());

    // Single mandatory line on stdout — parsed by the `ft ui` subcommand.
    println!(
        "firetrail-ui ready: http://{}/?token={}",
        bound_addr, state.bootstrap_token.value
    );
    tracing::info!(addr = %bound_addr, "firetrail-ui ready");

    // Heartbeat watchdog.
    if !opts.foreground {
        let watch_state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(10));
            ticker.tick().await; // first tick is immediate
            loop {
                ticker.tick().await;
                if !watch_state.heartbeat_seen.load(Ordering::SeqCst) {
                    // No heartbeat ever seen — give the SPA more time.
                    continue;
                }
                let last = *watch_state.last_heartbeat.lock().expect("hb mutex");
                if last.elapsed() > Duration::from_secs(60) {
                    tracing::info!("no heartbeat for 60s — exiting");
                    std::process::exit(0);
                }
            }
        });
    }

    let shutdown = async move {
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        #[cfg(unix)]
        let terminate = async {
            if let Ok(mut sig) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                sig.recv().await;
            }
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            () = ctrl_c => {},
            () = terminate => {},
        }
        tracing::info!("graceful shutdown signal received");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}
