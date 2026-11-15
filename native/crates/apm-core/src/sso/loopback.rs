//! The loopback listener the authorize redirect comes back to (`http://127.0.0.1:<port>/oauth/callback`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub const PORT_RANGE: (u16, u16) = (54321, 54331);

pub struct CallbackServer {
    pub redirect_uri: String,
    pub received: oneshot::Receiver<HashMap<String, String>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for CallbackServer {
    /// Closing the server: the accept loop and the listener go with the task.
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/oauth/callback")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page(params: &HashMap<String, String>) -> String {
    let error = params.get("error");
    let (title, text) = match error {
        Some(e) => ("Sign-in failed", params.get("error_description").unwrap_or(e).clone()),
        None => ("Signed in", "You can close this window and return to AWS Profile Manager.".to_string()),
    };
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Sign-in complete</title>\
         <body style=\"font:15px system-ui;padding:3rem;text-align:center;color:#222\">\
         <h2>{title}</h2><p>{}</p>",
        html_escape(&text)
    )
}

/// A cached registration pins the redirect URI, so when one exists we must bind that exact port
/// and cannot roam. Without a cached registration, take the first free port in the range.
pub async fn start(fixed_port: Option<u16>) -> std::io::Result<CallbackServer> {
    let mut port = fixed_port.unwrap_or(PORT_RANGE.0);
    let listener = loop {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => break l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && fixed_port.is_none() && port < PORT_RANGE.1 => {
                port += 1;
            }
            Err(e) => return Err(e),
        }
    };
    let (tx, rx) = oneshot::channel();
    let settle = Arc::new(Mutex::new(Some(tx)));
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { continue };
            let settle = settle.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 2048];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let target = head.lines().next().and_then(|l| l.split(' ').nth(1)).unwrap_or("/");
                let url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}{target}")).ok();
                let response = match url.filter(|u| u.path() == "/oauth/callback") {
                    None => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(),
                    Some(u) => {
                        let params: HashMap<String, String> = u.query_pairs().into_owned().collect();
                        let body = page(&params);
                        if let Some(tx) = settle.lock().unwrap().take() {
                            let _ = tx.send(params);
                        }
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                };
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok(CallbackServer { redirect_uri: redirect_uri(port), received: rx, task })
}
