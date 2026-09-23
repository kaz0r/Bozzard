//! A local HTTP fixture with bounded, interruptible delays; no external network service.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Response {
    pub status: &'static str,
    pub headers: String,
    pub body: Vec<u8>,
    pub declared: usize,
    pub chunk: usize,
    pub delay: Duration,
}
impl Response {
    pub fn ok(body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            headers: String::new(),
            declared: body.len(),
            body,
            chunk: usize::MAX,
            delay: Duration::ZERO,
        }
    }
    pub fn redirect(to: &str) -> Self {
        Self {
            status: "302 Found",
            headers: format!("Location: {to}\r\n"),
            ..Self::ok(Vec::new())
        }
    }
}
pub struct Server {
    pub base: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    pub fn start(handler: impl Fn(&str) -> Response + Send + 'static) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let base = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (worker_stop, worker_requests) = (stop.clone(), requests.clone());
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        configure_connection(&stream).unwrap();
                        let mut input = Vec::new();
                        let mut byte = [0];
                        while input.len() < 8192 && stream.read_exact(&mut byte).is_ok() {
                            input.push(byte[0]);
                            if input.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        // A client can abandon a speculative/retried connection before
                        // sending headers (especially after a rejected redirect).
                        if !input.ends_with(b"\r\n\r\n") {
                            continue;
                        }
                        let request = String::from_utf8_lossy(&input);
                        let path = request.split_whitespace().nth(1).unwrap_or("");
                        worker_requests.lock().unwrap().push(path.to_owned());
                        let _ = reply(&mut stream, handler(path), &worker_stop);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("HTTP fixture accept: {e}"),
                }
            }
        });
        Ok(Self {
            base,
            requests,
            stop,
            worker: Some(worker),
        })
    }
    pub fn wait_for(&self, path: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.requests.lock().unwrap().iter().any(|p| p == path) {
            assert!(
                Instant::now() < deadline,
                "HTTP request did not arrive: {path}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let joined = self.worker.take().unwrap().join();
        if !thread::panicking() {
            assert!(joined.is_ok(), "HTTP fixture worker panicked");
        }
    }
}
fn configure_connection(stream: &TcpStream) -> std::io::Result<()> {
    // Windows inherits the listener's nonblocking mode. Request/response I/O
    // needs to wait for bytes; only accept polling should be nonblocking.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))
}
fn reply(stream: &mut TcpStream, response: Response, stop: &AtomicBool) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
        response.status, response.declared, response.headers
    )?;
    for chunk in response.body.chunks(response.chunk) {
        let deadline = Instant::now() + response.delay;
        while Instant::now() < deadline {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(5));
        }
        stream.write_all(chunk)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, RecvTimeoutError};

    #[test]
    fn accepted_connection_waits_for_request_bytes() -> std::io::Result<()> {
        const REQUEST: &[u8] = b"GET /catalog.json HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let mut client = TcpStream::connect(listener.local_addr()?)?;
        let (mut stream, _) = listener.accept()?;
        // Reproduce Windows' inherited listener mode on every platform.
        stream.set_nonblocking(true)?;
        configure_connection(&stream)?;

        thread::scope(|scope| {
            let (ready_tx, ready_rx) = mpsc::channel();
            let (result_tx, result_rx) = mpsc::channel();
            scope.spawn(move || {
                let mut input = [0; REQUEST.len()];
                ready_tx.send(()).unwrap();
                let result = stream.read_exact(&mut input).map(|()| input);
                let _ = result_tx.send(result);
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                matches!(
                    result_rx.recv_timeout(Duration::from_millis(100)),
                    Err(RecvTimeoutError::Timeout)
                ),
                "accepted connection must wait for request bytes, not return WouldBlock"
            );
            client.write_all(REQUEST)?;
            let received = result_rx.recv_timeout(Duration::from_secs(2)).unwrap()?;
            assert_eq!(received, REQUEST);
            Ok(())
        })
    }
}
