//! The Sync folder's files (ADR 0022 rules 1, 2 and 6; plan S28a): op files round-trip byte
//! for byte through a folder on disk; a truncated, damaged or still-downloading op file is
//! skipped and read whole next time; a newer format is left alone and asks for an update;
//! readers ignore temporary and other names; a reserved op file written again after a stop
//! has the same bytes; `sync.json` and `device.json`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Mutex;

use obs_store::annotation::new_id;
use obs_store::{Change, Notebook, Row, UserStore};

use super::clock::Hlc;
use super::folder::{FolderError, FsSyncFolder, MemSyncFolder, SyncFolder};
use super::ops::{
    OP_FORMAT, Op, OpFileRead, OpsStop, decode_op_file, encode_op_file, op_file_path, op_file_seq,
    ops_dir, read_ops_after, write_pending_ops,
};
use super::{
    DeviceJson, FolderFormat, SYNC_JSON, SyncError, device_json_path, ensure_sync_json,
    read_devices, write_device_json,
};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn notebook(name: &str) -> Notebook {
    Notebook {
        id: new_id(),
        name: name.into(),
        is_default: false,
        show_in_text: true,
        created_at: 1,
        modified_at: 1,
    }
}

/// A User database recording ops.
fn recording_store(dir: &Dir) -> UserStore {
    let (user, _) = UserStore::open(&dir.0.join("user.db")).unwrap();
    user.start_recording_ops().unwrap();
    user.take_applied_changes();
    user
}

/// Every kind of edit, on a Notebook: made, renamed, deleted.
fn edit(user: &UserStore, name: &str) {
    let nb = notebook(name);
    user.apply_changes(&[Change::insert(Row::Notebook(nb.clone()))])
        .unwrap();
    let renamed = Notebook {
        name: format!("{name} (renamed) “ünïcödé”\n"),
        ..nb.clone()
    };
    user.apply_changes(&[Change::update(
        Row::Notebook(nb),
        Row::Notebook(renamed.clone()),
    )])
    .unwrap();
    user.apply_changes(&[Change::delete(Row::Notebook(renamed))])
        .unwrap();
}

fn ops(device: &str, n: usize) -> Vec<Op> {
    (0..n)
        .map(|i| Op {
            device: device.into(),
            clock: Hlc {
                wall_ms: 1_800_000_000_000,
                counter: u16::try_from(i).unwrap(),
                device_id: device.into(),
            },
            before: None,
            after: Some(Row::Notebook(notebook(&format!("N{i}")))),
        })
        .collect()
}

#[test]
fn ops_round_trip_byte_identically_through_a_folder_on_disk() {
    let dir = Dir::new("fs-round-trip");
    let user = recording_store(&dir);
    let device = user.device_id().unwrap();
    edit(&user, "Romans");
    let changes = user.take_applied_changes();
    let folder = FsSyncFolder::new(dir.0.join("cloud"));
    assert_eq!(write_pending_ops(&user, &folder).unwrap(), [1]);
    assert_eq!(
        write_pending_ops(&user, &folder).unwrap(),
        Vec::<u64>::new()
    );

    let path = op_file_path(&device, 1);
    assert_eq!(
        path,
        format!("Open Bible Sync/devices/{device}/ops/1.jsonl")
    );
    let bytes = folder.read(&path).unwrap();
    let on_disk = std::fs::read(dir.0.join("cloud").join(&path)).unwrap();
    assert_eq!(bytes, on_disk);
    let OpFileRead::Whole(read) = decode_op_file(&bytes) else {
        panic!("not whole: {}", String::from_utf8_lossy(&bytes));
    };
    assert_eq!(read.iter().map(Op::change).collect::<Vec<_>>(), changes);
    assert!(
        read.iter()
            .all(|o| o.device == device && o.clock.device_id == device)
    );
    for pair in read.windows(2) {
        assert!(pair[0].clock < pair[1].clock, "lines in clock order");
    }
    // Encoded again, the ops read are the same bytes.
    assert_eq!(encode_op_file(&read), bytes);
    // One op per line, then the trailer; no temporary file is left behind.
    let text = String::from_utf8(bytes.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), changes.len() + 1);
    assert!(lines[0].starts_with(&format!(r#"{{"device":"{device}","clock":""#)));
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with(r#"{"format":1,"ops":3,"bytes":"#)
    );
    let names = folder.list(&ops_dir(&device)).unwrap();
    assert_eq!(names, ["1.jsonl"]);
    // Read through the folder, it is the device's first op file.
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files, vec![(1, read)]);
    assert_eq!(got.stopped, None);
    assert_eq!(
        read_ops_after(&folder, &device, 1).unwrap().files,
        Vec::new()
    );
}

#[test]
fn a_truncated_op_file_is_skipped_and_read_whole_next_time() {
    let folder = MemSyncFolder::new();
    let device = new_id();
    let first = encode_op_file(&ops(&device, 2));
    let second_ops = ops(&device, 3);
    let second = encode_op_file(&second_ops);
    folder.put(&op_file_path(&device, 1), &first);
    // Every way a file arrives part way: cut anywhere, the trailer missing, the trailer cut.
    let cuts = [
        second.len() / 2,
        second.len() - 1,
        second[..second.len() - 1]
            .iter()
            .rposition(|&b| b == b'\n')
            .unwrap()
            + 1,
        0,
    ];
    for cut in cuts {
        folder.put(&op_file_path(&device, 2), &second[..cut]);
        let got = read_ops_after(&folder, &device, 0).unwrap();
        assert_eq!(got.files.len(), 1, "cut at {cut}: the file before is read");
        assert!(
            matches!(got.stopped, Some((2, OpsStop::NotWhole(_)))),
            "cut at {cut}: {:?}",
            got.stopped
        );
    }
    // Next time it is there whole.
    folder.put(&op_file_path(&device, 2), &second);
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files.iter().map(|f| f.0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(got.files[1].1, second_ops);
    assert_eq!(got.stopped, None);
}

#[test]
fn a_damaged_op_file_fails_its_length_or_hash() {
    let device = new_id();
    let whole = encode_op_file(&ops(&device, 2));
    // One byte changed, the length kept.
    let mut flipped = whole.clone();
    let i = flipped.iter().position(|&b| b == b'N').unwrap();
    flipped[i] = b'M';
    assert!(matches!(decode_op_file(&flipped), OpFileRead::NotWhole(w) if w.contains("hash")));
    // A line added before the trailer.
    let mut longer = b"{}\n".to_vec();
    longer.extend_from_slice(&whole);
    assert!(matches!(decode_op_file(&longer), OpFileRead::NotWhole(w) if w.contains("length")));
    // A line that is not an op, with a trailer that matches it.
    let body = b"{\"not\":\"an op\"}\n";
    let mut bad = body.to_vec();
    bad.extend_from_slice(
        format!(
            "{{\"format\":1,\"ops\":1,\"bytes\":{},\"sha256\":\"{}\"}}\n",
            body.len(),
            crate::starter::sha256_hex(body)
        )
        .as_bytes(),
    );
    assert!(matches!(decode_op_file(&bad), OpFileRead::NotWhole(w) if w.contains("op 1")));
    // An empty batch is a whole file.
    assert_eq!(
        decode_op_file(&encode_op_file(&[])),
        OpFileRead::Whole(Vec::new())
    );
    assert!(matches!(decode_op_file(b""), OpFileRead::NotWhole(_)));
}

#[test]
fn a_newer_op_file_is_left_alone_and_asks_for_an_update() {
    let folder = MemSyncFolder::new();
    let device = new_id();
    folder.put(&op_file_path(&device, 1), &encode_op_file(&ops(&device, 1)));
    // A later app's file: its own lines, a trailer naming format 2 with fields this one lacks.
    let newer = b"{\"v2\":true}\n{\"format\":2,\"chunks\":[1,2]}\n".to_vec();
    folder.put(&op_file_path(&device, 2), &newer);
    folder.put(&op_file_path(&device, 3), &encode_op_file(&ops(&device, 1)));
    assert_eq!(decode_op_file(&newer), OpFileRead::Newer { format: 2 });
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files.len(), 1);
    assert_eq!(got.stopped, Some((2, OpsStop::Newer { format: 2 })));
    assert!(got.needs_newer_app());
    // Left as it was: nothing removed or rewritten.
    assert_eq!(folder.get(&op_file_path(&device, 2)), Some(newer));
    assert_eq!(folder.paths().len(), 3);
    assert_eq!(OP_FORMAT, 1);
}

#[test]
fn readers_skip_temporary_and_other_names_and_stop_at_a_gap() {
    let folder = MemSyncFolder::new();
    let device = new_id();
    let dir = ops_dir(&device);
    folder.put(&op_file_path(&device, 1), &encode_op_file(&ops(&device, 1)));
    for other in [
        ".2.jsonl.0193a1b2-c3d4-7000-8000-000000000001.tmp",
        ".syncthing.2.jsonl.tmp",
        "2 (Ian's conflicted copy).jsonl",
        "02.jsonl",
        "2.jsonl.icloud",
        "notes.txt",
        "-1.jsonl",
    ] {
        folder.put(&format!("{dir}/{other}"), b"junk");
    }
    for name in [
        "02.jsonl",
        ".1.jsonl.tmp",
        "1.json",
        "x.jsonl",
        "0.jsonl",
        ".jsonl",
    ] {
        assert_eq!(op_file_seq(name), None, "{name}");
    }
    assert_eq!(op_file_seq("12.jsonl"), Some(12));
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files.len(), 1);
    assert_eq!(got.stopped, None, "nothing after 1 is an op file");
    // 3 arrived before 2: reading stops at 2 until it is there.
    folder.put(&op_file_path(&device, 3), &encode_op_file(&ops(&device, 1)));
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files.len(), 1);
    assert_eq!(got.stopped, Some((2, OpsStop::Missing)));
    // Applied up to 1 already: the gap is still where reading stops.
    assert_eq!(
        read_ops_after(&folder, &device, 1).unwrap().stopped,
        Some((2, OpsStop::Missing))
    );
    assert_eq!(
        read_ops_after(&folder, &device, 3).unwrap(),
        Default::default()
    );
}

#[test]
fn a_placeholder_op_file_is_read_once_it_has_downloaded() {
    let folder = MemSyncFolder::new();
    let device = new_id();
    let path = op_file_path(&device, 1);
    folder.put(&path, &encode_op_file(&ops(&device, 2)));
    folder.keep_in_cloud(&path);
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files, Vec::new());
    assert_eq!(got.stopped, Some((1, OpsStop::NotDownloaded)));
    folder.download(&path);
    assert_eq!(read_ops_after(&folder, &device, 0).unwrap().files.len(), 1);
}

/// A folder that keeps what is written but fails as the write ends, as when the app stops
/// between writing an op file and noting it written.
struct StopsAfterWriting(MemSyncFolder, Mutex<bool>);

impl SyncFolder for StopsAfterWriting {
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError> {
        self.0.list(dir)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError> {
        self.0.read(path)
    }
    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError> {
        self.0.write_new(path, bytes)?;
        let mut stop = self.1.lock().unwrap();
        if *stop {
            *stop = false;
            return Err(FolderError::Io("stopped".into()));
        }
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), FolderError> {
        self.0.remove(path)
    }
    fn ensure_available(&self, path: &str) -> Result<bool, FolderError> {
        self.0.ensure_available(path)
    }
}

#[test]
fn a_reserved_op_file_written_again_after_a_stop_has_the_same_bytes() {
    let dir = Dir::new("retry");
    let user = recording_store(&dir);
    let device = user.device_id().unwrap();
    edit(&user, "Romans");
    let folder = StopsAfterWriting(MemSyncFolder::new(), Mutex::new(true));
    assert!(matches!(
        write_pending_ops(&user, &folder),
        Err(SyncError::Folder(FolderError::Io(_)))
    ));
    let first = folder.0.get(&op_file_path(&device, 1)).unwrap();
    // Edits made before the next try go in the next file; file 1 is written the same again.
    edit(&user, "Hebrews");
    assert_eq!(write_pending_ops(&user, &folder).unwrap(), [1, 2]);
    assert_eq!(folder.0.get(&op_file_path(&device, 1)).unwrap(), first);
    let got = read_ops_after(&folder, &device, 0).unwrap();
    assert_eq!(got.files.len(), 2);
    assert_eq!(got.files[0].1.len(), 3);
    assert_eq!(got.files[1].1.len(), 3);
    assert!(got.files[0].1.last().unwrap().clock < got.files[1].1[0].clock);
}

#[test]
fn a_device_not_recording_writes_no_op_files() {
    let dir = Dir::new("not-recording");
    let (user, _) = UserStore::open(&dir.0.join("user.db")).unwrap();
    edit(&user, "Romans");
    let folder = MemSyncFolder::new();
    assert_eq!(
        write_pending_ops(&user, &folder).unwrap(),
        Vec::<u64>::new()
    );
    assert_eq!(folder.paths(), Vec::<String>::new());
}

#[test]
fn sync_json_is_written_once_and_a_newer_one_asks_for_an_update() {
    let folder = MemSyncFolder::new();
    assert_eq!(ensure_sync_json(&folder).unwrap(), FolderFormat::Known);
    assert_eq!(folder.get(SYNC_JSON).unwrap(), b"{\"format\":1}\n");
    assert_eq!(ensure_sync_json(&folder).unwrap(), FolderFormat::Known);
    folder.put(SYNC_JSON, b"{\"format\":3,\"more\":true}\n");
    assert_eq!(
        ensure_sync_json(&folder).unwrap(),
        FolderFormat::Newer { format: 3 }
    );
    assert_eq!(
        folder.get(SYNC_JSON).unwrap(),
        b"{\"format\":3,\"more\":true}\n",
        "left alone"
    );
    folder.put(SYNC_JSON, b"{\"form");
    assert!(matches!(
        ensure_sync_json(&folder),
        Err(SyncError::Damaged(_))
    ));
    assert_eq!(folder.get(SYNC_JSON).unwrap(), b"{\"form");
}

#[test]
fn a_device_rewrites_only_its_own_device_json_and_reads_every_other() {
    let folder = MemSyncFolder::new();
    let me = new_id();
    let mine = DeviceJson::new(&me, "Ian's Mac", "macos");
    assert!(write_device_json(&folder, &mine).unwrap());
    assert!(
        !write_device_json(&folder, &mine).unwrap(),
        "unchanged: not written"
    );
    let renamed = DeviceJson {
        name: "Study Mac".into(),
        ..mine.clone()
    };
    assert!(write_device_json(&folder, &renamed).unwrap());
    let text = String::from_utf8(folder.get(&device_json_path(&me)).unwrap()).unwrap();
    assert_eq!(
        text,
        format!(
            "{{\"format\":1,\"device\":\"{me}\",\"name\":\"Study Mac\",\"platform\":\"macos\",\"app_version\":\"{}\"}}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    let phone = new_id();
    folder.put(
        &device_json_path(&phone),
        format!("{{\"format\":1,\"device\":\"{phone}\",\"name\":\"Ian's iPhone\",\"platform\":\"ios\",\"app_version\":\"9.9.9\",\"later\":1}}\n").as_bytes(),
    );
    let broken = new_id();
    folder.put(&device_json_path(&broken), b"{");
    folder.put("Open Bible Sync/devices/.DS_Store", b"");
    let devices = read_devices(&folder).unwrap();
    let mut ids = vec![me.clone(), phone.clone(), broken.clone()];
    ids.sort();
    assert_eq!(
        devices.iter().map(|d| d.id.clone()).collect::<Vec<_>>(),
        ids
    );
    let find = |id: &str| devices.iter().find(|d| d.id == id).unwrap().file.clone();
    assert_eq!(find(&me).unwrap(), renamed);
    assert_eq!(find(&phone).unwrap().name, "Ian's iPhone");
    assert!(find(&broken).is_err());
}

#[test]
fn a_folder_on_disk_writes_whole_files_and_refuses_paths_outside_it() {
    let dir = Dir::new("fs-folder");
    let folder = FsSyncFolder::new(dir.0.join("cloud"));
    assert_eq!(
        folder.list("Open Bible Sync").unwrap(),
        Vec::<String>::new()
    );
    folder.write_new("Open Bible Sync/a/b.txt", b"one").unwrap();
    folder.write_new("Open Bible Sync/a/b.txt", b"two").unwrap();
    assert_eq!(folder.read("Open Bible Sync/a/b.txt").unwrap(), b"two");
    assert_eq!(folder.list("Open Bible Sync/a").unwrap(), ["b.txt"]);
    assert!(folder.ensure_available("Open Bible Sync/a/b.txt").unwrap());
    assert!(matches!(
        folder.read("Open Bible Sync/a/none"),
        Err(FolderError::NotFound(_))
    ));
    for bad in [
        "",
        "/etc/passwd",
        "../x",
        "a/../../x",
        "a//b",
        "./a",
        "a\\b",
    ] {
        assert!(
            matches!(folder.write_new(bad, b"x"), Err(FolderError::BadPath(_))),
            "{bad}"
        );
    }
    folder.remove("Open Bible Sync/a/b.txt").unwrap();
    folder.remove("Open Bible Sync/a/b.txt").unwrap();
    assert_eq!(
        folder.list("Open Bible Sync/a").unwrap(),
        Vec::<String>::new()
    );
}
