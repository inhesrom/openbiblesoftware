//! The app's network transport for SWORD repositories (Get Bibles, #91), the Starter library
//! download (#131), the Update check (#276) and the Christian Open Library's books (ADR 0024).
//!
//! The only place in the workspace that opens HTTP connections. Bytes come straight from the
//! repository's own URL; there is no project server in the path. `ftp://` and `file://` use
//! the SWORD crate's built-in transport, which needs no networking crate. A request to
//! GitHub's API (the Update check) asks for its JSON format and API version, as GitHub
//! documents; no other header names the user, and no request carries a cookie. Only the
//! Christian Open Library's requests carry a token (the user's own, ADR 0024): an
//! `Authorization` header goes to GitHub's API alone and never follows a redirect, so the
//! tarball GitHub redirects to its download host is fetched without it.

use std::io::Read;
use std::time::Duration;

use obs_formats::sword_repo::{Fetch, FetchError, Progress, builtin_get, local_size};

/// Largest single file accepted (the SWORD installer's own cap).
const LIMIT: u64 = 4 << 30;
const CHUNK: usize = 64 * 1024;
/// GitHub's REST API, whose requests ask for its JSON format.
const GITHUB_API: &str = "https://api.github.com/";

/// Whether `url` may be sent an `Authorization` header: GitHub's API over HTTPS, or this
/// computer (the tests' mock GitHub).
fn may_authorize(url: &str) -> bool {
    if url.starts_with(GITHUB_API) {
        return true;
    }
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?']).next().unwrap_or(""))
        .unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

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
            // The version About, the log and the Update check report (ADR 0017).
            .user_agent(format!("OpenBible/{}", obs_engine::updates::APP_VERSION))
            // Statuses are read here: a 403 from GitHub may mean "rate limited" (#276).
            .http_status_as_error(false)
            // A token never follows a redirect (ureq's default, kept explicit: ADR 0024).
            .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
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

/// Whether a response's status is a success; else what it means.
fn check_status<B>(url: &str, response: &ureq::http::Response<B>) -> Result<(), FetchError> {
    let code = response.status().as_u16();
    let remaining = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok());
    match code {
        200..=299 => Ok(()),
        404 | 410 => Err(FetchError::NotFound),
        401 => Err(FetchError::Unauthorized),
        429 => Err(FetchError::RateLimited),
        403 if remaining == Some("0") => Err(FetchError::RateLimited),
        _ => Err(FetchError::Network(format!("{url}: http status: {code}"))),
    }
}

fn scheme(url: &str) -> String {
    url.split_once("://")
        .map(|(s, _)| s.to_ascii_lowercase())
        .unwrap_or_default()
}

impl HttpFetch {
    /// Send a GET with `headers` and check its status; the response's body is still to read.
    fn request(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
        if headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            && !may_authorize(url)
        {
            return Err(FetchError::Network(format!(
                "{url}: credentials are not sent to this host"
            )));
        }
        let mut request = self.agent.get(url);
        if url.starts_with(GITHUB_API) {
            request = request
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = match request.call() {
            Ok(r) => r,
            Err(e) => return Err(FetchError::Network(format!("{url}: {e}"))),
        };
        check_status(url, &response)?;
        Ok(response)
    }

    /// Fetch over `ftp://` or `file://` with the built-in transport; `None` for HTTP(S).
    fn builtin(url: &str, progress: Progress<'_>) -> Option<Result<Vec<u8>, FetchError>> {
        match scheme(url).as_str() {
            "http" | "https" => None,
            "ftp" | "file" => Some(builtin_get(url).and_then(|bytes| {
                let n = bytes.len() as u64;
                if progress(n, Some(n)) {
                    Ok(bytes)
                } else {
                    Err(FetchError::Cancelled)
                }
            })),
            other => Some(Err(FetchError::Network(format!(
                "{url}: unsupported protocol {other:?}"
            )))),
        }
    }
}

impl Fetch for HttpFetch {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        self.get_with(url, &[], progress)
    }

    fn get_with(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        progress: Progress<'_>,
    ) -> Result<Vec<u8>, FetchError> {
        if let Some(result) = Self::builtin(url, progress) {
            return result;
        }
        let mut response = self.request(url, headers)?;
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

    fn open(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<Box<dyn Read + Send>, FetchError> {
        if let Some(result) = Self::builtin(url, &mut |_, _| true) {
            return result.map(|b| Box::new(std::io::Cursor::new(b)) as Box<dyn Read + Send>);
        }
        let response = self.request(url, headers)?;
        // A whole library is a few GB of Markdown, compressed: no limit but the stream's end.
        Ok(Box::new(
            response
                .into_body()
                .into_with_config()
                .limit(u64::MAX)
                .reader(),
        ))
    }

    fn size(&self, url: &str) -> Result<Option<u64>, FetchError> {
        match scheme(url).as_str() {
            "http" | "https" => {}
            "file" => return local_size(url),
            _ => return Ok(None),
        }
        match self.agent.head(url).call() {
            Ok(r) => {
                check_status(url, &r)?;
                Ok(r.headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok()))
            }
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

    /// A localhost server answering each of `requests` connections with `response`; the
    /// request lines and headers it got come back on the receiver.
    fn serve_fixed(
        response: &'static str,
        requests: usize,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    head.push_str(&line);
                }
                let _ = tx.send(head);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    #[test]
    fn rate_limited_answers_are_told_apart_from_other_refusals() {
        // GitHub's primary rate limit: 403 with no requests remaining; its secondary: 429.
        let (base, _) = serve_fixed(
            "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            1,
        );
        assert_eq!(
            HttpFetch::new()
                .get(&format!("{base}/releases/latest"), &mut |_, _| true)
                .unwrap_err(),
            FetchError::RateLimited
        );
        let (base, _) = serve_fixed(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            1,
        );
        assert_eq!(
            HttpFetch::new()
                .get(&format!("{base}/x"), &mut |_, _| true)
                .unwrap_err(),
            FetchError::RateLimited
        );
        // Any other refusal is a plain failure, with its status.
        let (base, _) = serve_fixed(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            1,
        );
        let err = HttpFetch::new()
            .get(&format!("{base}/x"), &mut |_, _| true)
            .unwrap_err();
        assert!(
            matches!(&err, FetchError::Network(m) if m.contains("403")),
            "{err:?}"
        );
    }

    #[test]
    fn a_request_sends_no_cookie_token_or_identifier_beyond_the_app_version() {
        let (base, heads) = serve_fixed(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            1,
        );
        let body = HttpFetch::new()
            .get(
                &format!("{base}/repos/inhesrom/openbiblesoftware/releases/latest"),
                &mut |_, _| true,
            )
            .unwrap();
        assert_eq!(body, b"{}");
        let head = heads.recv().unwrap().to_ascii_lowercase();
        assert!(
            head.starts_with("get /repos/inhesrom/openbiblesoftware/releases/latest http/1.1\r\n"),
            "{head}"
        );
        assert!(
            head.contains(&format!(
                "user-agent: openbible/{}",
                obs_engine::updates::APP_VERSION.to_ascii_lowercase()
            )),
            "{head}"
        );
        for header in ["cookie", "authorization", "x-forwarded", "referer"] {
            assert!(!head.contains(header), "{head}");
        }
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
