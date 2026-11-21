//! The Christian Open Library download end to end (#347, ADR 0024): the app's own transport
//! against a mock GitHub served on localhost. Connect with a token, read the repository's
//! list and catalogue, download five works (file by file, then everything as a tarball through
//! GitHub's redirect), see them in the Library as Books, change the repository, Check for new
//! books, and Update in place with a Note's anchor kept. Also: a refused token, no access, a
//! rate limit, and that no Authorization header ever leaves for another host.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use obs_engine::col_get::SessionSecret;
use obs_engine::{ImportPhase, JobMode, Session};
use obs_model::ResourceType;
use serde_json::json;

use crate::fetch::HttpFetch;

const TOKEN: &str = "github_pat_TEST0123456789";
const OWNER: &str = "Christian-open-library";
const REPO: &str = "books";

/// The mock repository: files by path, its commit, and what it saw.
#[derive(Default)]
struct Repo {
    files: BTreeMap<String, Vec<u8>>,
    commit: u32,
    /// Answer every request with this status (401, 404, 429) when set.
    refuse: Option<u16>,
    /// Requests by path, with whether they carried the token.
    seen: Vec<(String, bool)>,
}

impl Repo {
    fn sha(&self) -> String {
        format!("{:040x}", self.commit + 0xc0de)
    }
}

fn blob_sha(bytes: &[u8]) -> String {
    obs_engine::col::git_blob_sha(bytes)
}

fn tree_json(repo: &Repo) -> String {
    let entries: Vec<String> = repo
        .files
        .iter()
        .map(|(path, bytes)| {
            json!({"path": path, "mode": "100644", "type": "blob",
                   "sha": blob_sha(bytes), "size": bytes.len()})
            .to_string()
        })
        .collect();
    format!(
        r#"{{"sha":"{}","truncated":false,"tree":[{}]}}"#,
        repo.sha(),
        entries.join(",")
    )
}

/// The repository as GitHub's tarball: `owner-repo-sha/` before every path, in path order.
fn tarball(repo: &Repo) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    let prefix = format!("{OWNER}-{REPO}-{}", &repo.sha()[..7]);
    let mut dir = tar::Header::new_gnu();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_size(0);
    dir.set_mode(0o755);
    tar.append_data(&mut dir, format!("{prefix}/"), std::io::empty())
        .unwrap();
    for (path, bytes) in &repo.files {
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        tar.append_data(&mut h, format!("{prefix}/{path}"), bytes.as_slice())
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn respond(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
}

/// Serve the mock GitHub on localhost until the test ends; its API base URL.
fn serve(repo: Arc<Mutex<Repo>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let api = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let repo = Arc::clone(&repo);
            let base = base.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut auth = None;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("authorization:") {
                        auth = Some(v.trim().to_string());
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let path = path.replace("%E2%80%94", "—").replace("%20", " ");
                let mut r = repo.lock().unwrap();
                let authorized =
                    auth.as_deref() == Some(&format!("bearer {}", TOKEN.to_ascii_lowercase()));
                r.seen.push((path.clone(), auth.is_some()));
                if path.starts_with("/codeload/") {
                    // GitHub's tarball host: the redirect's URL is the permission; a token
                    // must never arrive here.
                    let body = tarball(&r);
                    drop(r);
                    respond(
                        &mut stream,
                        "200 OK",
                        "Content-Type: application/x-gzip\r\n",
                        &body,
                    );
                    return;
                }
                if let Some(code) = r.refuse {
                    let status = match code {
                        401 => "401 Unauthorized",
                        404 => "404 Not Found",
                        _ => "429 Too Many Requests",
                    };
                    respond(&mut stream, status, "", b"{}");
                    return;
                }
                if !authorized {
                    respond(&mut stream, "401 Unauthorized", "", b"{}");
                    return;
                }
                let root = format!("/repos/{OWNER}/{REPO}");
                let Some(rest) = path.strip_prefix(&root) else {
                    respond(&mut stream, "404 Not Found", "", b"{}");
                    return;
                };
                if rest.is_empty() {
                    respond(&mut stream, "200 OK", "", br#"{"default_branch":"main"}"#);
                } else if rest == "/commits/main" {
                    let sha = r.sha();
                    respond(&mut stream, "200 OK", "", sha.as_bytes());
                } else if rest.starts_with("/git/trees/") {
                    let body = tree_json(&r);
                    respond(&mut stream, "200 OK", "", body.as_bytes());
                } else if let Some(sha) = rest.strip_prefix("/git/blobs/") {
                    match r.files.values().find(|b| blob_sha(b) == sha) {
                        Some(b) => {
                            let b = b.clone();
                            respond(&mut stream, "200 OK", "", &b);
                        }
                        None => respond(&mut stream, "404 Not Found", "", b"{}"),
                    }
                } else if rest.starts_with("/tarball/") {
                    let location =
                        format!("Location: {base}/codeload/{OWNER}/{REPO}/legacy.tar.gz/x\r\n");
                    respond(&mut stream, "302 Found", &location, b"");
                } else {
                    respond(&mut stream, "404 Not Found", "", b"{}");
                }
            });
        }
    });
    api
}

/// COL's catalogue, authors and five works (one split into Parts).
fn col_files() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let mut put = |p: &str, t: &str| {
        f.insert(p.to_string(), t.as_bytes().to_vec());
    };
    put(
        "catalog_full.csv",
        "File,Author,Title,Year,Type,License,Source\n\
         EEBO-TCP/John Owen/Owen - Mortification.md,John Owen,Of the Mortification of Sin,1656,book,public domain,https://example.org/owen\n\
         Creeds/Westminster Confession.md,Westminster Assembly,The Westminster Confession of Faith,1646,confession,public domain,\n\
         Creeds/Heidelberg Catechism.md,Ursinus,The Heidelberg Catechism,1563,catechism,public domain,\n\
         Sermons/Spurgeon - Psalm 23.md,C. H. Spurgeon,The Lord Is My Shepherd,1896,book,public domain,\n\
         Sermons/Edwards - Sinners.md,Jonathan Edwards,Sinners in the Hands of an Angry God,1741,book,public domain,\n",
    );
    put(
        "authors.csv",
        "Author,Dates,Tradition,Note\nJohn Owen,1616–1683,Puritan,English Puritan\nJonathan Edwards,1703–1758,Congregationalist,\n",
    );
    put(
        "EEBO-TCP/John Owen/Owen - Mortification.md",
        "---\ntitle: Of the Mortification of Sin\nauthor: John Owen\ndate: 1656\nlicense: public domain\nsource: https://example.org/owen\n---\n\n# Of the Mortification of Sin\n\n- [Part 1](x)\n",
    );
    put(
        "EEBO-TCP/John Owen/Owen - Mortification — Part 1.md",
        "## Chapter I\n\nIf ye through the Spirit do mortify the deeds of the body, Rom. 8. 13.\n",
    );
    put(
        "EEBO-TCP/John Owen/Owen - Mortification — Part 2.md",
        "## Chapter II\n\nThe second <!-- p. 2 --> chapter.\n",
    );
    for (p, t) in [
        (
            "Creeds/Westminster Confession.md",
            "The Westminster Confession of Faith",
        ),
        ("Creeds/Heidelberg Catechism.md", "The Heidelberg Catechism"),
        ("Sermons/Spurgeon - Psalm 23.md", "The Lord Is My Shepherd"),
        (
            "Sermons/Edwards - Sinners.md",
            "Sinners in the Hands of an Angry God",
        ),
    ] {
        put(
            p,
            &format!(
                "---\ntitle: {t}\nlicense: public domain\n---\n\n## One\n\nThe text of {t}, John 3:16.\n"
            ),
        );
    }
    f
}

struct Lab {
    root: PathBuf,
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn session(name: &str, api: &str) -> (Session, Lab) {
    let root = std::env::temp_dir().join(format!("obs-col-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let mut s = Session::new(JobMode::Threads(2));
    s.library.set_directory(root.join("library"));
    s.library.scan(&root.join("library"));
    let (user, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(user));
    s.set_search_cache_dir(root.join("cache"));
    s.set_fetch(Arc::new(HttpFetch::with_timeout(
        std::time::Duration::from_secs(10),
    )));
    s.col.set_secrets(Arc::new(SessionSecret::default()));
    s.col.set_api(api);
    (s, Lab { root })
}

/// Pump until `done` holds (threaded jobs; events applied as the UI's frames would).
fn until(s: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
    let start = std::time::Instant::now();
    while !done(s) {
        assert!(
            start.elapsed().as_secs() < 60,
            "timed out waiting for {what}"
        );
        s.poll_imports();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn books(s: &Session) -> Vec<String> {
    let mut t: Vec<String> = s
        .library
        .of_type(ResourceType::Book)
        .map(|e| e.manifest.title.clone())
        .collect();
    t.sort();
    t
}

#[test]
fn col_downloads_five_works_then_updates_a_changed_one_in_place() {
    let repo = Arc::new(Mutex::new(Repo {
        files: col_files(),
        ..Repo::default()
    }));
    let api = serve(Arc::clone(&repo));
    let (mut s, _lab) = session("five", &api);

    // Nothing is asked for until the user connects.
    s.col_restore();
    assert!(repo.lock().unwrap().seen.is_empty());

    s.run("col.connect_token", &json!({ "token": TOKEN }))
        .unwrap();
    until(&mut s, "the list", |s| {
        s.col.catalog().is_some() || s.col.error().is_some()
    });
    assert!(s.col.error().is_none(), "{:?}", s.col.error());
    let works = s.col.catalog().unwrap().works.clone();
    assert_eq!(works.len(), 5);
    let owen = works.iter().find(|w| w.author == "John Owen").unwrap();
    assert_eq!(owen.tradition, "Puritan");
    assert_eq!(owen.parts.len(), 2);

    // Filter, choose two, download them file by file.
    s.run("col.filter", &json!({ "kind": "confession" }))
        .unwrap();
    assert_eq!(s.col.shown().len(), 1);
    s.run("col.filter", &json!({})).unwrap();
    s.run(
        "col.select",
        &json!({ "paths": "EEBO-TCP/John Owen/Owen - Mortification.md\nCreeds/Westminster Confession.md" }),
    )
    .unwrap();
    let out = s
        .run("col.download", &json!({ "scope": "selected" }))
        .unwrap();
    let batch = out["id"].as_str().unwrap().to_string();
    until(&mut s, "two books", |s| s.imports.job_of(&batch).is_none());
    assert_eq!(
        books(&s),
        [
            "Of the Mortification of Sin",
            "The Westminster Confession of Faith"
        ]
    );
    let owen_id = owen.resource_id();
    let entry = s.library.get(&owen_id).expect("the Book has its COL id");
    assert!(
        entry
            .manifest
            .attribution
            .contains("Christian Open Library")
    );
    assert!(
        entry
            .manifest
            .attribution
            .contains("https://example.org/owen")
    );
    assert_eq!(entry.manifest.license, "LicenseRef-PublicDomain");
    let origin = s.col_books().get(&owen_id).cloned().unwrap();
    assert_eq!(origin.blobs.len(), 3);

    // Everything: the tarball, through GitHub's redirect; the two already in are updated
    // in place (same ids), the other three are new.
    let out = s
        .run("col.download", &json!({ "scope": "everything" }))
        .unwrap();
    let batch = out["id"].as_str().unwrap().to_string();
    until(&mut s, "every book", |s| s.imports.job_of(&batch).is_none());
    assert_eq!(books(&s).len(), 5, "{:?}", s.imports.report(&batch));
    {
        let r = repo.lock().unwrap();
        let tarball: Vec<&(String, bool)> = r
            .seen
            .iter()
            .filter(|(p, _)| p.contains("tarball") || p.contains("codeload"))
            .collect();
        assert_eq!(tarball.len(), 2, "{tarball:?}");
        assert!(
            tarball
                .iter()
                .any(|(p, auth)| p.contains("tarball") && *auth)
        );
        assert!(
            tarball
                .iter()
                .any(|(p, auth)| p.starts_with("/codeload/") && !*auth),
            "the token never goes to another host: {tarball:?}"
        );
    }

    // A Note on Owen survives an update to his text.
    let before = s.library.get(&owen_id).unwrap().manifest.revision;
    let unit_text = |s: &Session, unit: &str| -> String {
        use obs_store::ResourceStore;
        let path = s.library.get(&owen_id).unwrap().path.clone();
        let reader = obs_store::ObrReader::open(&path).unwrap();
        reader
            .unit(&obs_model::UnitId::new(unit).unwrap())
            .unwrap()
            .unwrap()
            .text
    };
    let text = unit_text(&s, "div.2");
    let at = text.find("second").unwrap();
    let note = s
        .run(
            "annotation.note",
            &json!({ "resource": owen_id, "start_unit": "div.2", "start": at,
                     "end": at + "second".len(), "body": "Mortify daily." }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    {
        let mut r = repo.lock().unwrap();
        r.files.insert(
            "EEBO-TCP/John Owen/Owen - Mortification — Part 2.md".into(),
            b"## Chapter II\n\nThe second <!-- p. 2 --> chapter, corrected.\n".to_vec(),
        );
        r.files.insert(
            "Sermons/Whitefield - New Birth.md".into(),
            b"---\ntitle: The Nature and Necessity of Our New Birth\n---\n\n## One\n\nJohn 3:3.\n"
                .to_vec(),
        );
        r.files.remove("Sermons/Edwards - Sinners.md");
        r.commit += 1;
    }
    s.run("col.refresh", &json!({})).unwrap();
    until(&mut s, "the new list", |s| !s.col.is_loading());
    let changes = s.col.changes().cloned().unwrap();
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].0, owen_id);
    assert_eq!(changes.new, ["Sermons/Whitefield - New Birth.md"]);
    assert_eq!(changes.removed.len(), 1);
    assert_eq!(changes.current, 3);
    let out = s.run("col.update", &json!({})).unwrap();
    let batch = out["id"].as_str().unwrap().to_string();
    until(&mut s, "the update", |s| s.imports.job_of(&batch).is_none());
    let after = s.library.get(&owen_id).unwrap().clone();
    assert_eq!(
        after.manifest.revision,
        before + 1,
        "the same Resource, its next revision"
    );
    // The Note follows its words into the new revision.
    let hash = after.manifest.content_hash.clone();
    until(&mut s, "the Note re-anchored", |s| {
        !obs_engine::reanchor::is_stale(&s.annotation(&note).unwrap(), &owen_id, &hash)
    });
    let anchored = s.annotation(&note).unwrap();
    let range = anchored
        .anchor
        .as_ref()
        .and_then(|a| a.text_range())
        .unwrap()
        .clone();
    assert!(!range.orphaned);
    let text = unit_text(&s, range.unit.as_str());
    assert!(text.contains("corrected"), "{text}");
    assert_eq!(&text[range.start as usize..range.end as usize], "second");
    assert_eq!(books(&s).len(), 5, "the removed book stays in the Library");
    let report = s.imports.report(&batch).unwrap();
    assert_eq!(report.phase, ImportPhase::Complete);
    assert!(report.books.as_ref().unwrap().imported[0].updated);
    s.run("col.refresh", &json!({})).unwrap();
    until(&mut s, "the list again", |s| !s.col.is_loading());
    assert!(s.col.changes().unwrap().changed.is_empty());

    // The token is in no file the app writes.
    let db = std::fs::read(_lab.root.join("user.db")).unwrap();
    assert!(!db.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes()));
}

#[test]
fn refused_tokens_no_access_and_rate_limits_say_so_plainly() {
    let repo = Arc::new(Mutex::new(Repo {
        files: col_files(),
        ..Repo::default()
    }));
    let api = serve(Arc::clone(&repo));
    let (mut s, _lab) = session("refused", &api);
    for (code, says) in [
        (404, "cannot see the Christian Open Library's books yet"),
        (429, "limiting requests"),
        (401, "did not accept this token"),
    ] {
        repo.lock().unwrap().refuse = Some(code);
        s.run("col.connect_token", &json!({ "token": TOKEN }))
            .unwrap();
        until(&mut s, "the answer", |s| !s.col.is_loading());
        let message = s.col.error().unwrap().message();
        assert!(message.contains(says), "{code}: {message}");
        assert!(!message.contains(TOKEN));
    }
    // A refused token is forgotten.
    assert!(s.col.access().is_none());
    // Offline: nothing listens there.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    s.col.set_api(format!("http://127.0.0.1:{port}"));
    s.run("col.connect_token", &json!({ "token": TOKEN }))
        .unwrap();
    until(&mut s, "the answer", |s| !s.col.is_loading());
    assert!(
        s.col
            .error()
            .unwrap()
            .message()
            .contains("Could not reach GitHub")
    );
    assert!(s.library.of_type(ResourceType::Book).next().is_none());
}

#[test]
fn the_token_goes_only_to_the_api_host() {
    let (base, heads) = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let mut head = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).unwrap();
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                    head.push_str(&l);
                }
                let _ = tx.send(head);
                respond(&mut stream, "200 OK", "", b"ok");
            }
        });
        (format!("http://{addr}"), rx)
    };
    let fetch = HttpFetch::new();
    use obs_formats::sword_repo::Fetch;
    // A host other than GitHub's API (or this test's localhost) is refused with a token.
    let err = fetch
        .get_with(
            "https://example.org/x",
            &[("Authorization", "Bearer secret")],
            &mut |_, _| true,
        )
        .unwrap_err();
    assert!(format!("{err}").contains("not sent"), "{err}");
    let body = fetch
        .get_with(
            &format!("{base}/x"),
            &[("Authorization", "Bearer secret")],
            &mut |_, _| true,
        )
        .unwrap();
    assert_eq!(body, b"ok");
    assert!(
        heads
            .recv()
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer secret")
    );
    let mut r = fetch.open(&format!("{base}/x"), &[]);
    // The listener took one connection only: a second fails, as offline does.
    let mut buf = Vec::new();
    if let Ok(reader) = r.as_mut() {
        let _ = reader.read_to_end(&mut buf);
    }
}
