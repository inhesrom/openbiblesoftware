//! The app's network transport for SWORD repositories (Get Bibles, #91).
//!
//! The only place in the workspace that opens HTTP connections. Bytes come straight from the
//! repository's own URL; there is no project server in the path. `ftp://` and `file://` use
//! the SWORD crate's built-in transport, which needs no networking crate.

use std::io::Read;
use std::time::Duration;

use obs_formats::sword_repo::{Fetch, FetchError, Progress, builtin_get, local_size};

/// Largest single file accepted (the SWORD installer's own cap).
const LIMIT: u64 = 4 << 30;
const CHUNK: usize = 64 * 1024;

#[derive(Debug)]
pub struct HttpFetch {
    agent: ureq::Agent,
}

impl HttpFetch {
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }

    /// `timeout` bounds connecting and each wait for data; a slow but moving download is
    /// never cut off.
    pub fn with_timeout(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(timeout))
            .timeout_recv_response(Some(timeout))
            .user_agent(concat!("OpenBible/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for HttpFetch {
    fn default() -> Self {
        Self::new()
    }
}

fn scheme(url: &str) -> String {
    url.split_once("://")
        .map(|(s, _)| s.to_ascii_lowercase())
        .unwrap_or_default()
}

impl Fetch for HttpFetch {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        match scheme(url).as_str() {
            "http" | "https" => {}
            "ftp" | "file" => {
                let bytes = builtin_get(url)?;
                let n = bytes.len() as u64;
                return if progress(n, Some(n)) {
                    Ok(bytes)
                } else {
                    Err(FetchError::Cancelled)
                };
            }
            other => {
                return Err(FetchError::Network(format!(
                    "{url}: unsupported protocol {other:?}"
                )));
            }
        }
        let mut response = match self.agent.get(url).call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(404 | 410)) => return Err(FetchError::NotFound),
            Err(e) => return Err(FetchError::Network(format!("{url}: {e}"))),
        };
        let total = response.body().content_length();
        if total.is_some_and(|t| t > LIMIT) {
            return Err(FetchError::Network(format!("{url}: larger than 4 GiB")));
        }
        if !progress(0, total) {
            return Err(FetchError::Cancelled);
        }
        let mut reader = response.body_mut().with_config().limit(LIMIT).reader();
        let mut out = Vec::with_capacity(total.unwrap_or(0).min(LIMIT) as usize);
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| FetchError::Network(format!("{url}: {e}")))?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            if !progress(out.len() as u64, total) {
                return Err(FetchError::Cancelled);
            }
        }
        Ok(out)
    }

    fn size(&self, url: &str) -> Result<Option<u64>, FetchError> {
        match scheme(url).as_str() {
            "http" | "https" => {}
            "file" => return local_size(url),
            _ => return Ok(None),
        }
        match self.agent.head(url).call() {
            Ok(r) => Ok(r
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok())),
            Err(ureq::Error::StatusCode(404 | 410)) => Err(FetchError::NotFound),
            Err(e) => Err(FetchError::Network(format!("{url}: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use super::*;

    /// A one-request-per-connection HTTP server on localhost: `/ok` answers `len` bytes,
    /// anything else 404.
    fn serve(len: usize, requests: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                }
                if line.starts_with("HEAD /ok ") {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
                    );
                } else if line.starts_with("GET /ok ") {
                    let body = vec![b'x'; len];
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
                    );
                    // Ignore errors: a cancelling client hangs up mid-body.
                    for chunk in body.chunks(16 * 1024) {
                        if stream.write_all(chunk).is_err() {
                            break;
                        }
                    }
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn http_download_streams_with_progress_up_to_the_content_length() {
        let base = serve(300_000, 1);
        let mut seen = Vec::new();
        let bytes = HttpFetch::new()
            .get(&format!("{base}/ok"), &mut |done, total| {
                seen.push((done, total));
                true
            })
            .unwrap();
        assert_eq!(bytes.len(), 300_000);
        assert!(seen.len() > 2, "{seen:?}");
        assert!(seen.iter().all(|(_, t)| *t == Some(300_000)));
        assert_eq!(seen.last().unwrap().0, 300_000);
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn returning_false_from_progress_cancels_mid_download() {
        let base = serve(4_000_000, 1);
        let mut calls = 0;
        let err = HttpFetch::new()
            .get(&format!("{base}/ok"), &mut |done, _| {
                calls += 1;
                done < 100_000
            })
            .unwrap_err();
        assert_eq!(err, FetchError::Cancelled);
        assert!(calls < 20, "stopped early, not after the whole body");
    }

    #[test]
    fn missing_files_are_not_found_and_unreachable_hosts_are_network_errors() {
        let base = serve(0, 1);
        assert_eq!(
            HttpFetch::new()
                .get(&format!("{base}/missing"), &mut |_, _| true)
                .unwrap_err(),
            FetchError::NotFound
        );
        // Nothing listens on a port we just bound and closed.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = HttpFetch::with_timeout(Duration::from_secs(2))
            .get(&format!("http://127.0.0.1:{port}/x"), &mut |_, _| true)
            .unwrap_err();
        assert!(matches!(err, FetchError::Network(_)), "{err:?}");
        assert!(matches!(
            HttpFetch::new().get("gopher://x/y", &mut |_, _| true),
            Err(FetchError::Network(m)) if m.contains("unsupported")
        ));
    }

    #[test]
    fn size_asks_the_server_with_head_and_reads_files_locally() {
        let base = serve(4_472_092, 2);
        let fetch = HttpFetch::new();
        assert_eq!(fetch.size(&format!("{base}/ok")).unwrap(), Some(4_472_092));
        assert_eq!(
            fetch.size(&format!("{base}/missing")).unwrap_err(),
            FetchError::NotFound
        );
        let dir = std::env::temp_dir().join(format!("obs-http-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.zip"), [0u8; 123]).unwrap();
        let url = obs_formats::sword_repo::file_url(&dir.join("p.zip"));
        assert_eq!(fetch.size(&url).unwrap(), Some(123));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_urls_use_the_builtin_transport() {
        let dir = std::env::temp_dir().join(format!("obs-http-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.conf"), "[A]\n").unwrap();
        let url = obs_formats::sword_repo::file_url(&dir.join("a.conf"));
        assert_eq!(
            HttpFetch::new().get(&url, &mut |_, _| true).unwrap(),
            b"[A]\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
