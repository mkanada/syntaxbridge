use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const DEFAULT_ADDR: &str = "127.0.0.1:37651";

pub struct SyntaxBridgeServer {
    listener: TcpListener,
}

impl SyntaxBridgeServer {
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn spawn(self) -> io::Result<ServerHandle> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("syntax-bridge-server".to_owned())
            .spawn(move || self.serve(Some(shutdown_rx)))?;

        Ok(ServerHandle {
            shutdown_tx,
            thread: Some(thread),
        })
    }

    pub fn run(self) -> io::Result<()> {
        self.serve(None)
    }

    fn serve(self, shutdown_rx: Option<Receiver<()>>) -> io::Result<()> {
        self.listener.set_nonblocking(true)?;

        loop {
            if shutdown_rx.as_ref().is_some_and(|rx| rx.try_recv().is_ok()) {
                return Ok(());
            }

            match self.listener.accept() {
                Ok((stream, _)) => handle_connection(stream)?,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub struct ServerHandle {
    shutdown_tx: Sender<()>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl ServerHandle {
    pub fn shutdown(mut self) -> io::Result<()> {
        let _ = self.shutdown_tx.send(());

        let Some(thread) = self.thread.take() else {
            return Ok(());
        };

        thread
            .join()
            .map_err(|_| io::Error::other("server thread panicked"))?
    }
}

pub fn run_blocking(addr: &str) -> io::Result<()> {
    let server = SyntaxBridgeServer::bind(addr)?;
    eprintln!("syntax-bridge-server listening on {}", server.local_addr()?);
    server.run()
}

fn handle_connection(mut stream: TcpStream) -> io::Result<()> {
    let mut buffer = [0_u8; 1024];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);

    if request.starts_with("GET /health ") {
        write_response(
            &mut stream,
            "200 OK",
            r#"{"service":"syntax-bridge-server","status":"ok"}"#,
        )
    } else {
        write_response(&mut stream, "404 Not Found", r#"{"error":"not_found"}"#)
    }
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
