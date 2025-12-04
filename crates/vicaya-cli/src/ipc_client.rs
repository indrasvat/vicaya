//! IPC client for communicating with the daemon.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use vicaya_core::ipc::{Request, Response};
use vicaya_core::Result;

/// IPC client for daemon communication.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// Connect to the daemon.
    pub fn connect() -> Result<Self> {
        let socket_path = vicaya_core::ipc::socket_path();

        let stream = UnixStream::connect(&socket_path).map_err(|e| {
            vicaya_core::Error::Ipc(format!(
                "Failed to connect to daemon at {}: {}. Is the daemon running?",
                socket_path.display(),
                e
            ))
        })?;

        Ok(Self { stream })
    }

    /// Send a request and receive a response.
    pub fn request(&mut self, req: &Request) -> Result<Response> {
        // Send request
        let mut request_json = req
            .to_json()
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to serialize request: {}", e)))?;
        request_json.push('\n');

        self.stream
            .write_all(request_json.as_bytes())
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to send request: {}", e)))?;

        // Read response
        let mut reader = BufReader::new(&self.stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to read response: {}", e)))?;

        Response::from_json(&line)
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to parse response: {}", e)))
    }
}
