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
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
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
