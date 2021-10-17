use anyhow::Result;
use async_stream::stream;
use core::task::{Context, Poll};
use futures_util::Stream;
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use std::{fs, io, pin::Pin, sync};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{rustls, server::TlsStream, TlsAcceptor};
use tracing::info;

fn error(s: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}

pub fn load_single_cert(filename: &str) -> io::Result<Vec<u8>> {
    // Open certificate file.
    let certfile = fs::File::open(filename)
        .map_err(|e| error(format!("failed to open {}: {}", filename, e)))?;
    let mut reader = io::BufReader::new(certfile);

    // Load and return certificate.
    let certs = certs(&mut reader).map_err(|_| error("failed to load certificate".into()))?;

    if certs.len() != 1 {
        return Err(error("expected a single cert".into()));
    }

    Ok(certs[0].clone())
}

// Load private key from file.
pub fn load_private_key(filename: &str) -> io::Result<Vec<u8>> {
    // Open keyfile.
    let keyfile = fs::File::open(filename)
        .map_err(|e| error(format!("failed to open {}: {}", filename, e)))?;
    let mut reader = io::BufReader::new(keyfile);

    // Load and return a single private key.
    let keys =
        rsa_private_keys(&mut reader).map_err(|_| error("failed to load private key".into()))?;
    if keys.len() != 1 {
        return Err(error(format!(
            "expected a single private key, got {}",
            keys.len()
        )));
    }
    Ok(keys[0].clone())
}

pub struct HyperTlsAcceptor<'a> {
    acceptor: Pin<Box<dyn Stream<Item = Result<TlsStream<TcpStream>, io::Error>> + 'a>>,
}

impl hyper::server::accept::Accept for HyperTlsAcceptor<'_> {
    type Conn = TlsStream<TcpStream>;
    type Error = io::Error;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        Pin::new(&mut self.acceptor).poll_next(cx)
    }
}

pub async fn build_tls<'a>(addr: &str, cert: &str, key: &str) -> Result<HyperTlsAcceptor<'a>> {
    let tls_cfg = {
        let cert = load_single_cert(cert)?;
        let key = load_private_key(key)?;

        let mut cfg = rustls::ServerConfig::new(tokio_rustls::rustls::NoClientAuth::new());
        cfg.set_single_cert(vec![rustls::Certificate(cert)], rustls::PrivateKey(key))?;
        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        sync::Arc::new(cfg)
    };

    let tcp = TcpListener::bind(&addr).await?;
    let tls_acceptor = TlsAcceptor::from(tls_cfg);
    let incoming_tls_stream = stream! {
        loop {
            let (socket, _) = tcp.accept().await?;
            let stream = tls_acceptor.accept(socket).await.map_err(|e| {
                info!("Voluntary server halt due to client-connection error");
                error(format!("TLS error: {:?}", e))
            });
            yield stream;
        }
    };

    Ok(HyperTlsAcceptor {
        acceptor: Box::pin(incoming_tls_stream),
    })
}
