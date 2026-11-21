//! Op files (ADR 0022 rule 2): each Synced device writes the `Change`s applied on it, in
//! batches, as `Open Bible Sync/devices/<device-id>/ops/<seq>.jsonl`, numbered from 1 and
//! never changed once written. Each line is one op, the `Change` with the device's id and its
//! clock; the last line is the trailer, with the file's format, how many ops it holds, the
//! length of everything before it and that text's SHA-256. A reader skips a file that is
//! incomplete or fails its hash and reads it again next time, and leaves a file of a newer
//! format alone (rule 6). `docs/dev/sync.md` has the format.

use serde::{Deserialize, Serialize};

use obs_store::{Change, Row};

use super::SyncFolder;
use super::clock::Hlc;
use super::folder::FolderError;

/// The op file format this build writes and reads.
pub const OP_FORMAT: u32 = 1;

/// One line of an op file: a `Change` applied on `device`, at `clock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    /// The Synced device whose op file it is in.
    pub device: String,
    pub clock: Hlc,
    pub before: Option<Row>,
    pub after: Option<Row>,
}

impl Op {
    pub fn change(&self) -> Change {
        Change {
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }
}

/// An op file's last line. Its `format` field is in every format to come, so a reader that
/// meets a newer one knows to leave the file alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Trailer {
    format: u32,
    /// How many op lines come before it.
    ops: u64,
    /// The length in bytes of everything before it.
    bytes: u64,
    /// The SHA-256 of everything before it, lowercase hexadecimal.
    sha256: String,
}

/// What a reader makes of an op file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpFileRead {
    /// Every op, in the order written.
    Whole(Vec<Op>),
    /// Written in a format this build does not know; it is left alone.
    Newer { format: u32 },
    /// Incomplete (still arriving), damaged, or failing its length or hash; why, in words.
    NotWhole(String),
}

/// The bytes of an op file holding `ops`, in order. The same ops always give the same bytes.
pub fn encode_op_file(ops: &[Op]) -> Vec<u8> {
    let mut body = Vec::new();
    for op in ops {
        // An `Op` is plain data (strings, numbers, options and lists), which serializes
        // without fail.
        if let Ok(line) = serde_json::to_vec(op) {
            body.extend_from_slice(&line);
            body.push(b'\n');
        }
    }
    let trailer = Trailer {
        format: OP_FORMAT,
        ops: u64::try_from(ops.len()).unwrap_or(u64::MAX),
        bytes: u64::try_from(body.len()).unwrap_or(u64::MAX),
        sha256: crate::starter::sha256_hex(&body),
    };
    if let Ok(line) = serde_json::to_vec(&trailer) {
        body.extend_from_slice(&line);
    }
    body.push(b'\n');
    body
}

/// Read an op file's `bytes`.
pub fn decode_op_file(bytes: &[u8]) -> OpFileRead {
    let not_whole = |why: &str| OpFileRead::NotWhole(why.to_string());
    let Some(text) = bytes.strip_suffix(b"\n") else {
        return not_whole("it does not end with a whole line");
    };
    let start = text.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let (body, last) = text.split_at(start);
    #[derive(Deserialize)]
    struct Format {
        format: u32,
    }
    let Ok(Format { format }) = serde_json::from_slice::<Format>(last) else {
        return not_whole("its last line is not a trailer");
    };
    if format > OP_FORMAT {
        return OpFileRead::Newer { format };
    }
    let Ok(trailer) = serde_json::from_slice::<Trailer>(last) else {
        return not_whole("its trailer is damaged");
    };
    if trailer.format != OP_FORMAT {
        return not_whole("its trailer names no format this app wrote");
    }
    if u64::try_from(body.len()).ok() != Some(trailer.bytes) {
        return not_whole("its length is not the trailer's");
    }
    if crate::starter::sha256_hex(body) != trailer.sha256 {
        return not_whole("its hash is not the trailer's");
    }
    let mut ops = Vec::new();
    // `body` is empty or ends with the newline of its last op.
    if let Some(lines) = body.strip_suffix(b"\n") {
        for (i, line) in lines.split(|&b| b == b'\n').enumerate() {
            match serde_json::from_slice::<Op>(line) {
                Ok(op) => ops.push(op),
                Err(e) => return OpFileRead::NotWhole(format!("op {} is not one: {e}", i + 1)),
            }
        }
    }
    if u64::try_from(ops.len()).ok() != Some(trailer.ops) {
        return not_whole("it holds another number of ops than its trailer says");
    }
    OpFileRead::Whole(ops)
}

/// The folder of `device`'s op files.
pub fn ops_dir(device: &str) -> String {
    format!("{}/{device}/ops", super::DEVICES_DIR)
}

/// The path of `device`'s op file `seq`.
pub fn op_file_path(device: &str, seq: u64) -> String {
    format!("{}/{seq}.jsonl", ops_dir(device))
}

/// The number of the op file named `name`: `<seq>.jsonl`, the number in decimal from 1
/// without leading zeros. Every other name (a temporary file, a provider's conflict copy) is
/// none.
pub fn op_file_seq(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".jsonl")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Why reading a device's op files stopped before the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsStop {
    /// There is no op file with this number, though later ones are listed: it has not
    /// arrived yet.
    Missing,
    /// The provider has it only in the cloud; it is downloading.
    NotDownloaded,
    /// It is incomplete or damaged ([`OpFileRead::NotWhole`]); it is read again next time.
    NotWhole(String),
    /// A newer app wrote it: update the app to sync with this device (ADR 0022 rule 6).
    Newer { format: u32 },
}

/// A device's op files read in order after the last one applied: each whole one, and, when
/// one could not be read, where and why reading stopped (only a whole run of files can be
/// applied, since a device's ops are applied up to one number).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceOps {
    pub files: Vec<(u64, Vec<Op>)>,
    pub stopped: Option<(u64, OpsStop)>,
}

impl DeviceOps {
    /// Whether a newer app wrote the op file reading stopped at.
    pub fn needs_newer_app(&self) -> bool {
        matches!(self.stopped, Some((_, OpsStop::Newer { .. })))
    }
}

/// Read `device`'s op files numbered after `applied`, in order, as far as they are whole.
pub fn read_ops_after(
    folder: &dyn SyncFolder,
    device: &str,
    applied: u64,
) -> Result<DeviceOps, FolderError> {
    let dir = ops_dir(device);
    let mut seqs: Vec<u64> = folder
        .list(&dir)?
        .iter()
        .filter_map(|name| op_file_seq(name))
        .filter(|&seq| seq > applied)
        .collect();
    seqs.sort_unstable();
    let mut out = DeviceOps::default();
    let mut expected = applied + 1;
    for seq in seqs {
        if seq != expected {
            out.stopped = Some((expected, OpsStop::Missing));
            return Ok(out);
        }
        let path = op_file_path(device, seq);
        let stop = match folder.ensure_available(&path) {
            Ok(true) => match folder.read(&path) {
                Ok(bytes) => match decode_op_file(&bytes) {
                    OpFileRead::Whole(ops) => {
                        out.files.push((seq, ops));
                        expected += 1;
                        continue;
                    }
                    OpFileRead::Newer { format } => OpsStop::Newer { format },
                    OpFileRead::NotWhole(why) => OpsStop::NotWhole(why),
                },
                Err(FolderError::NotFound(_)) => OpsStop::Missing,
                Err(e) => OpsStop::NotWhole(e.to_string()),
            },
            Ok(false) => OpsStop::NotDownloaded,
            Err(FolderError::NotFound(_)) => OpsStop::Missing,
            Err(e) => return Err(e),
        };
        out.stopped = Some((seq, stop));
        return Ok(out);
    }
    Ok(out)
}

/// Write this device's ops waiting in the User database's outbox to op files in `folder`,
/// one file for each reserved batch; returns the numbers written. A file reserved by an
/// earlier try that stopped half way is written again with the same bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_pending_ops(
    user: &obs_store::UserStore,
    folder: &dyn SyncFolder,
) -> Result<Vec<u64>, super::SyncError> {
    let device = user.device_id()?;
    let mut written = Vec::new();
    for file in user.reserve_op_files()? {
        let ops: Vec<Op> = file
            .ops
            .into_iter()
            .map(|o| Op {
                device: device.clone(),
                clock: o.clock,
                before: o.change.before,
                after: o.change.after,
            })
            .collect();
        folder.write_new(&op_file_path(&device, file.seq), &encode_op_file(&ops))?;
        user.op_file_written(file.seq)?;
        written.push(file.seq);
    }
    Ok(written)
}
