//! Tiny HTTP server that receives the probe's POSTed report.
use anyhow::{anyhow, Result};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server};

pub struct ReportServer {
    port: u16,
    rx: Mutex<Receiver<String>>,
    server: Arc<Server>,
}

impl ReportServer {
    pub fn start() -> Result<ReportServer> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(|e| anyhow!("{e}"))?);
        let port = server.server_addr().to_ip().ok_or_else(|| anyhow!("no ip addr"))?.port();
        let (tx, rx) = channel();
        let s = server.clone();
        std::thread::spawn(move || {
            for mut req in s.incoming_requests() {
                if *req.method() == Method::Post {
                    let mut body = String::new();
                    let _ = req.as_reader().read_to_string(&mut body);
                    let _ = tx.send(body);
                }
                let cors = Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap();
                let hdrs = Header::from_bytes("Access-Control-Allow-Headers", "*").unwrap();
                let _ = req.respond(Response::empty(204).with_header(cors).with_header(hdrs));
            }
        });
        Ok(ReportServer { port, rx: Mutex::new(rx), server })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/report", self.port)
    }

    pub fn wait_report(&self, timeout: Duration) -> Option<String> {
        self.rx.lock().unwrap().recv_timeout(timeout).ok()
    }
}

impl Drop for ReportServer {
    fn drop(&mut self) {
        self.server.unblock();
    }
}
