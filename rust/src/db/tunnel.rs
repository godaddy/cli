//! `gddy db tunnel` — MySQL-over-WebSocket bridge to an application's agent.
//!
//! Opens a local TCP listener; for every MySQL client connection it opens one
//! WebSocket to the agent at `/apps/:id/database/tunnel` and relays raw MySQL
//! bytes in both directions (one WebSocket per TCP connection, no multiplexing).
//! The agent dials the app's configured MySQL host:port, so end-to-end MySQL
//! authentication and TLS are preserved — this client forwards bytes only and
//! never injects or inspects credentials. Progress is streamed as JSON events,
//! matching `platform app deploy`.
//!
//! Auth: the CLI mints a short-lived agent token from the hosting API
//! (`POST /v1/hosting/nodejs/apps/:id/agent-token`) using your GoDaddy OAuth
//! credential, then connects to the agent URL that call returns, sending the
//! minted token as `Authorization: Bearer`. The agent URL and token both come
//! from the service — neither is a user-supplied flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use cli_engine::{CommandContext, CommandSpec, RuntimeCommandSpec, StreamSender, Tier};
use futures_util::stream::FuturesUnordered;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header::AUTHORIZATION};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};

use crate::application::client::api_url_for_env;
use crate::error::GddyError;
use crate::hosting::nodejs::client::HostingClient;
use crate::scopes::HOSTING_DEPLOY_EXECUTE as DEPLOY_EXECUTE;

/// A connected agent WebSocket (TLS for `wss`, plain for `ws`).
type AgentSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Bytes read from the local MySQL client per WebSocket frame. Kept well under
/// the agent's 32 MiB frame cap; MySQL reassembles its own packets from the
/// byte stream regardless of framing.
const UP_CHUNK_BYTES: usize = 64 * 1024;

/// Ceiling for a single inbound WebSocket frame from the agent. Matches the
/// agent's documented 32 MiB frame cap so a large downstream result frame is
/// not rejected by tungstenite's 16 MiB default and torn down mid-query.
const MAX_AGENT_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// How often to flush the WebSocket sink while a connection is idle, so the
/// agent's ping keepalive is answered (it pings every 30s and drops the tunnel
/// if a pong misses the next tick). Comfortably inside that window.
const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, clap::Args)]
struct TunnelArgs {
    /// Application/site id — the app whose database to tunnel to. The CLI mints
    /// an agent token for this app and connects to its assigned agent.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Local TCP port MySQL clients connect to.
    #[arg(long, value_name = "PORT", default_value_t = 3306)]
    port: u16,

    /// Local interface to bind. Defaults to loopback.
    #[arg(long = "listen-host", value_name = "HOST", default_value = "127.0.0.1")]
    listen_host: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_streaming::<TunnelArgs, _, _>(
        CommandSpec::from_args::<TunnelArgs>(
            "tunnel",
            "Bridge a local port to an app's MySQL over the agent WebSocket",
        )
        .with_long(
            "Open a local TCP listener and forward raw MySQL traffic to an \
             application's agent over a WebSocket. Each client connection gets \
             its own WebSocket to the agent's `/apps/<app-id>/database/tunnel` \
             endpoint; the agent dials the app's configured database. Point any \
             MySQL client at the local port, for example:\n\n\
             \tmysql -h 127.0.0.1 -P 3306 -u <user> -p\n\n\
             MySQL authentication and TLS are negotiated end-to-end with the \
             database — the tunnel forwards bytes only and never injects or \
             inspects credentials. The CLI authorizes with your GoDaddy \
             credentials and connects to the app's assigned agent automatically. \
             Runs until interrupted (Ctrl-C).",
        )
        .with_system("database")
        .with_tier(Tier::Mutate)
        .with_scopes(&[DEPLOY_EXECUTE]),
        |ctx, args: TunnelArgs, sender: StreamSender| async move {
            run_tunnel(&ctx, args, &sender).await
        },
    )
}

/// Build the terminal `{"type":"error",...}` event, reusing
/// `cli_engine::build_error_envelope` so `code`/`message`/`fix` match what a
/// non-streaming command would render for the same error.
fn tunnel_error_event(err: &cli_engine::CliCoreError) -> Value {
    let envelope = cli_engine::build_error_envelope(err, "database");
    let (code, message) = envelope
        .error
        .map(|e| (e.code, e.message))
        .unwrap_or_else(|| ("ERROR".to_owned(), err.to_string()));
    let mut event = json!({
        "type": "error",
        "ok": false,
        "error": { "code": code, "message": message },
        "next_actions": [],
    });
    if let Some(fix) = envelope.fix.filter(|f| !f.is_empty()) {
        event["fix"] = json!(fix);
    }
    event
}

/// Emit the terminal error event, then return the error so the handler can fail
/// the run via `?` — every failure path produces exactly one terminal line.
async fn fail(sender: &StreamSender, err: cli_engine::CliCoreError) -> cli_engine::CliCoreError {
    sender.send(tunnel_error_event(&err)).await;
    err
}

async fn run_tunnel(
    ctx: &CommandContext,
    args: TunnelArgs,
    sender: &StreamSender,
) -> cli_engine::Result<()> {
    // Validate the app id before it is interpolated into any URL (the
    // agent-token request path and the WebSocket route), so a malformed value
    // fails fast with a clean error instead of silently corrupting a request
    // path and surfacing a confusing 404/405 from the hosting API.
    if let Err(e) = validate_app_id(&args.app_id) {
        return Err(fail(sender, e.into_cli_error()).await);
    }

    // Mint an agent token (and learn the agent's URL) before binding a port, so
    // an auth or lookup failure fails fast with a single terminal error line.
    sender
        .send(json!({ "type": "step", "name": "authorize", "status": "started" }))
        .await;
    let (agent_url, token) = match mint_agent_token(ctx, &args.app_id).await {
        Ok(pair) => pair,
        Err(e) => return Err(fail(sender, e).await),
    };
    sender
        .send(json!({ "type": "step", "name": "authorize", "status": "completed" }))
        .await;

    let ws_url = match build_tunnel_ws_url(&agent_url, &args.app_id) {
        Ok(url) => url,
        Err(e) => return Err(fail(sender, e.into_cli_error()).await),
    };

    let bind_addr = format!("{}:{}", args.listen_host, args.port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            let err = GddyError::network(format!("could not bind {bind_addr}: {e}"))
                .with_fix("Pick a free port with --port, or stop the process already using it.")
                .into_cli_error();
            return Err(fail(sender, err).await);
        }
    };
    let local_addr = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| bind_addr.clone());

    sender
        .send(json!({
            "type": "listening",
            "address": local_addr,
            "agent": ws_url,
            "appId": args.app_id,
        }))
        .await;
    sender
        .send(json!({
            "type": "hint",
            "message": format!(
                "Connect a MySQL client: mysql -h {} -P {} -u <user> -p",
                args.listen_host, args.port
            ),
        }))
        .await;

    let token = token.as_str();
    let mut conns = FuturesUnordered::new();
    let mut accepted: u64 = 0;

    // Register the Ctrl-C handler once and poll the same future each iteration;
    // recreating it per loop turn re-registers the signal handler needlessly.
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer)) => {
                        accepted += 1;
                        conns.push(handle_connection(accepted, stream, peer.to_string(), &ws_url, token, sender));
                    }
                    Err(e) => {
                        sender
                            .send(json!({ "type": "warning", "message": format!("accept failed: {e}") }))
                            .await;
                        // Back off briefly so a persistent accept error (e.g. the
                        // process is out of file descriptors) cannot spin into a
                        // tight loop that floods warnings and pins a core.
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
            // Drive in-flight connections; each emits its own close event.
            _ = conns.next(), if !conns.is_empty() => {}
            _ = &mut shutdown => {
                sender
                    .send(json!({ "type": "step", "name": "shutdown", "status": "started" }))
                    .await;
                break;
            }
        }
    }

    // Dropping `conns` cancels any still-open relays (closing their sockets);
    // that is the expected outcome of an interactive Ctrl-C.
    sender
        .send(json!({
            "type": "result",
            "ok": true,
            "result": { "appId": args.app_id, "connections": accepted },
            "next_actions": [],
        }))
        .await;
    Ok(())
}

/// Mint a short-lived agent token for `app_id` via the hosting API and return
/// `(agent_url, token)`. Uses the CLI's OAuth credential stepped up to the
/// deploy-execute scope — the same authorization `hosting nodejs deployment
/// publish` requires — so no separate site JWT is needed.
async fn mint_agent_token(
    ctx: &CommandContext,
    app_id: &str,
) -> cli_engine::Result<(String, String)> {
    let required = vec![DEPLOY_EXECUTE.to_owned()];
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = api_url_for_env(&ctx.middleware.env)?;
    let client = HostingClient::new(base_url, token);
    let resp = client
        .get_agent_token(app_id)
        .await
        .map_err(|e| GddyError::from(e).into_cli_error())?;
    let agent_url = field_str(&resp, "agentUrl")?;
    let token = field_str(&resp, "token")?;
    Ok((agent_url, token))
}

/// Pull a required string field out of the agent-token response, mapping a
/// missing or non-string value to a coded error (the service contract is broken).
fn field_str(resp: &Value, key: &str) -> cli_engine::Result<String> {
    resp.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            GddyError::network(format!("hosting agent-token response missing '{key}'"))
                .with_fix("Retry; if it persists, the app may not support database tunneling yet.")
                .into_cli_error()
        })
}

/// Handle one accepted MySQL client: open its own agent WebSocket and relay
/// bytes until either side closes. Emits `connection` open/close/error events;
/// a failure here never tears down the listener.
async fn handle_connection(
    id: u64,
    stream: TcpStream,
    peer: String,
    ws_url: &str,
    token: &str,
    sender: &StreamSender,
) {
    let _ = stream.set_nodelay(true);

    let ws = match connect_agent(ws_url, token).await {
        Ok(ws) => ws,
        Err(e) => {
            sender
                .send(json!({
                    "type": "connection",
                    "event": "error",
                    "id": id,
                    "peer": peer,
                    "error": e.to_string(),
                }))
                .await;
            return;
        }
    };
    sender
        .send(json!({ "type": "connection", "event": "open", "id": id, "peer": peer }))
        .await;

    let (up, down, reason) = relay(stream, ws).await;

    sender
        .send(json!({
            "type": "connection",
            "event": "close",
            "id": id,
            "peer": peer,
            "bytesUp": up,
            "bytesDown": down,
            "reason": reason,
        }))
        .await;
}

/// Open a WebSocket to the agent's tunnel endpoint, attaching the minted token
/// as `Authorization: Bearer <token>`.
async fn connect_agent(ws_url: &str, token: &str) -> Result<AgentSocket, GddyError> {
    let mut request = ws_url
        .into_client_request()
        .map_err(|e| GddyError::validation(format!("invalid agent URL '{ws_url}': {e}")))?;
    let value: HeaderValue = format!("Bearer {token}")
        .parse()
        .map_err(|_| GddyError::validation("minted agent token is not a valid header value"))?;
    request.headers_mut().insert(AUTHORIZATION, value);
    // Raise the inbound frame limit to the agent's documented 32 MiB cap;
    // tungstenite's 16 MiB default would otherwise reject a large downstream
    // result frame with a Capacity error and tear the connection down mid-query.
    let config = WebSocketConfig::default().max_frame_size(Some(MAX_AGENT_FRAME_BYTES));
    let (ws, _response) = connect_async_with_config(request, Some(config), false)
        .await
        .map_err(map_ws_err)?;
    Ok(ws)
}

/// Relay bytes between the MySQL client and the agent WebSocket until one side
/// closes. Returns `(bytes_up, bytes_down, close_reason)`.
///
/// Each direction runs as its own future so backpressure in one never blocks
/// the other. A single shared `select!` that awaited `send`/`write_all` inline
/// would, under simultaneous bidirectional backpressure, suspend the whole loop
/// on one direction's blocked write and stop draining the other — deadlocking
/// the connection and starving the keepalive flush. Splitting the directions
/// lets each make progress (and the flush timer fire) independently. Byte
/// counters are shared and incremented only after a successful forward, so the
/// reported totals never include a chunk that failed to reach the far side.
async fn relay(stream: TcpStream, ws: AgentSocket) -> (u64, u64, &'static str) {
    let (mut tcp_read, mut tcp_write) = stream.into_split();
    let (mut ws_tx, mut ws_rx) = ws.split();
    let up = Arc::new(AtomicU64::new(0));
    let down = Arc::new(AtomicU64::new(0));

    // Upstream: MySQL client -> agent. Owns the sink, so it also flushes the
    // sink periodically to push out tungstenite's auto-queued pong replies even
    // while the client is idle (the agent pings every 30s and drops the tunnel
    // if a pong misses the next tick).
    let upstream = {
        let up = Arc::clone(&up);
        async move {
            let mut buf = vec![0u8; UP_CHUNK_BYTES];
            let mut flush = tokio::time::interval(FLUSH_INTERVAL);
            flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            flush.tick().await; // consume the immediate first tick
            loop {
                tokio::select! {
                    read = tcp_read.read(&mut buf) => match read {
                        Ok(0) => {
                            let _ = ws_tx.send(Message::Close(None)).await;
                            return "client-closed";
                        }
                        Ok(n) => {
                            if ws_tx
                                .send(Message::Binary(Bytes::copy_from_slice(&buf[..n])))
                                .await
                                .is_err()
                            {
                                return "agent-send-failed";
                            }
                            up.fetch_add(n as u64, Ordering::Relaxed);
                        }
                        Err(_) => return "client-read-error",
                    },
                    _ = flush.tick() => {
                        let _ = ws_tx.flush().await;
                    }
                }
            }
        }
    };

    // Downstream: agent -> MySQL client.
    let downstream = {
        let down = Arc::clone(&down);
        async move {
            loop {
                match ws_rx.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        if tcp_write.write_all(&data).await.is_err() {
                            return "client-write-error";
                        }
                        down.fetch_add(data.len() as u64, Ordering::Relaxed);
                    }
                    Some(Ok(Message::Close(_))) => {
                        let _ = tcp_write.shutdown().await;
                        return "agent-closed";
                    }
                    // Ping is auto-answered by tungstenite; the pong is flushed by
                    // the upstream flush timer. Text/Pong/Frame are not part of the
                    // byte relay and are ignored.
                    Some(Ok(_)) => {}
                    Some(Err(_)) => return "agent-error",
                    None => {
                        let _ = tcp_write.shutdown().await;
                        return "agent-eof";
                    }
                }
            }
        }
    };

    // Stop as soon as either direction ends; dropping the other future closes
    // its half of each socket, tearing the paired connection down.
    let reason = tokio::select! {
        r = upstream => r,
        r = downstream => r,
    };
    (
        up.load(Ordering::Relaxed),
        down.load(Ordering::Relaxed),
        reason,
    )
}

/// Validate `--app-id` before it is interpolated into any URL. The value is
/// placed into both the agent-token request path and the WebSocket route, so
/// anything that could alter URL structure — path separators, query/fragment
/// markers, whitespace, percent-escapes — must be rejected up front rather than
/// silently corrupting a request. App ids are short alphanumeric slugs, so an
/// allowlist of letters, digits, `-` and `_` is both sufficient and safe.
fn validate_app_id(app_id: &str) -> Result<(), GddyError> {
    if app_id.is_empty() {
        return Err(GddyError::validation("--app-id must not be empty"));
    }
    if !app_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(GddyError::validation(
            "--app-id may contain only letters, digits, '-' and '_'",
        ));
    }
    Ok(())
}

/// Turn an agent base URL + app id into the WebSocket tunnel URL, mapping
/// `http`→`ws` and `https`→`wss` and replacing the path with the tunnel route.
fn build_tunnel_ws_url(agent_url: &str, app_id: &str) -> Result<String, GddyError> {
    validate_app_id(app_id)?;
    let parsed = url::Url::parse(agent_url).map_err(|e| {
        GddyError::network(format!(
            "hosting service returned an invalid agent URL '{agent_url}': {e}"
        ))
    })?;
    let ws_scheme = match parsed.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => {
            return Err(GddyError::network(format!(
                "agent URL has an unsupported scheme '{other}': expected http, https, ws, or wss"
            )));
        }
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| GddyError::network(format!("agent URL '{agent_url}' has no host")))?;
    let authority = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    Ok(format!(
        "{ws_scheme}://{authority}/apps/{app_id}/database/tunnel"
    ))
}

/// Map a tungstenite connect error to a coded [`GddyError`] with a status-aware
/// recovery hint (the agent's handshake failures are the common case).
fn map_ws_err(err: tokio_tungstenite::tungstenite::Error) -> GddyError {
    use tokio_tungstenite::tungstenite::Error as WsErr;
    match err {
        WsErr::Http(response) => {
            let status = response.status();
            let hint = match status.as_u16() {
                401 => {
                    "The agent rejected the token. Retry to mint a fresh one; if it persists, contact support."
                }
                403 => {
                    "The token is not authorized for this app. Confirm --app-id is one of your applications."
                }
                404 => {
                    "Tunnel endpoint not found on the agent. Confirm the app has database tunneling enabled."
                }
                429 => "Too many active tunnels for this app. Close some and retry.",
                503 => "Agent is draining or shutting down. Retry shortly.",
                _ => "Retry the command; if it persists, contact support.",
            };
            GddyError::network(format!("agent handshake failed: HTTP {status}")).with_fix(hint)
        }
        other => GddyError::network(format!("could not connect to agent: {other}"))
            .with_fix("Check your network connection and retry."),
    }
}

#[cfg(test)]
mod tests {
    use super::build_tunnel_ws_url;

    #[test]
    fn builds_ws_url_from_http_agent_with_port() {
        let url = build_tunnel_ws_url("http://127.0.0.1:4000", "abcdef1234").expect("valid url");
        assert_eq!(url, "ws://127.0.0.1:4000/apps/abcdef1234/database/tunnel");
    }

    #[test]
    fn builds_wss_url_from_https_agent_without_port() {
        let url =
            build_tunnel_ws_url("https://agent.host.example", "abcdef1234").expect("valid url");
        assert_eq!(
            url,
            "wss://agent.host.example/apps/abcdef1234/database/tunnel"
        );
    }

    #[test]
    fn preserves_ws_and_wss_schemes() {
        assert!(
            build_tunnel_ws_url("ws://localhost:8080", "abcdef1234")
                .expect("valid")
                .starts_with("ws://")
        );
        assert!(
            build_tunnel_ws_url("wss://host.example", "abcdef1234")
                .expect("valid")
                .starts_with("wss://")
        );
    }

    #[test]
    fn ignores_any_path_on_agent_url() {
        // Only scheme://authority is used; any path on the agent URL is replaced.
        let url =
            build_tunnel_ws_url("https://host.example/ignored/path", "abcdef1234").expect("valid");
        assert_eq!(url, "wss://host.example/apps/abcdef1234/database/tunnel");
    }

    #[test]
    fn rejects_unsupported_scheme() {
        assert!(build_tunnel_ws_url("ftp://host", "abcdef1234").is_err());
    }

    #[test]
    fn rejects_missing_host() {
        assert!(build_tunnel_ws_url("http://", "abcdef1234").is_err());
    }

    #[test]
    fn rejects_bad_app_id() {
        assert!(build_tunnel_ws_url("http://host:3306", "").is_err());
        assert!(build_tunnel_ws_url("http://host:3306", "a/b").is_err());
    }

    #[test]
    fn rejects_app_id_with_url_metacharacters() {
        // Characters that would alter URL structure must be rejected before the
        // id reaches the agent-token path or the WebSocket route.
        for bad in ["abc#x", "abc?x", "abc x", "abc%2f", "a.b", "a:b"] {
            assert!(
                build_tunnel_ws_url("http://host:3306", bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn tunnel_error_event_carries_code_and_fix() {
        let err = crate::error::GddyError::network("boom")
            .with_fix("do the thing")
            .into_cli_error();
        let event = super::tunnel_error_event(&err);
        assert_eq!(event["type"], "error");
        assert_eq!(event["ok"], false);
        assert_eq!(event["error"]["code"], crate::error::codes::NETWORK_ERROR);
        assert_eq!(event["error"]["message"], "boom");
        assert_eq!(event["fix"], "do the thing");
    }
}
