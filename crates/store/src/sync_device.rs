//! The User database's side of ADR 0022 rule 7 (S28c): a Synced device's id changes when the
//! database it is stops being the one that wrote its op files. Restoring a Backup gives the
//! device a new id ([`UserStore::restore_from`]), and so does finding op files under its own
//! id that it did not write (a copied database, [`UserStore::take_new_device_id`]).
//!
//! - The ids the database had before are kept in `former_device` (migration 17), and the
//!   merge counts a clock naming any of them as this device's own ([`UserStore::own_device_ids`]),
//!   so a Note text or list item written under the old id is still kept as a Sync conflict's
//!   copy or "Name (2)" when a remote edit that never saw it replaces it. A restore keeps the
//!   device's own ids (the one it had and its former ones) and adds the Backup's id, with the
//!   Backup's former ones, only when that id is one of them: a Backup of another device's
//!   (restored on a new computer while that device still syncs) does not make that device's
//!   ops this one's own, which would make Sync conflict copies of them no one needs for good.
//! - The `applied_up_to` rows stay as they are: the other devices' are what the restored (or
//!   copied) data has merged of them, and the old id's own row, the last of its op files
//!   written, is now what it has merged of that id, now another device's, whose later files
//!   the next sync merges. The new id has no row, so the device records no ops until it joins
//!   again (its snapshot brings what it holds to the others).
//! - The outbox is emptied: its ops are under the old id, and what they changed is in the
//!   rows, which the new id's join snapshot carries.
//! - After a restore on a device that was syncing, `device.restore_choice` is set until the
//!   user chooses to catch up or replace ([`UserStore::restore_choice_pending`]).

use rusqlite::{Connection, params};

use crate::annotation::new_id;
use crate::error::StoreError;
use crate::sync_clock::{read_device_id, records_ops};
use crate::synced_store::has_table;
use crate::user::{UserStore, sql};

/// This device's id and the ids it had before, its own first.
pub(crate) fn read_own_ids(c: &Connection) -> Result<Vec<String>, StoreError> {
    let mut ids = vec![read_device_id(c)?];
    if has_table(c, "former_device")? {
        let former: Vec<String> = c
            .prepare_cached("SELECT device_id FROM former_device ORDER BY device_id")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
            .map_err(sql)?;
        let own = ids[0].clone();
        ids.extend(former.into_iter().filter(|id| *id != own));
    }
    Ok(ids)
}

/// Whether a Backup restored on this device waits for the user's choice to catch up or replace.
pub(crate) fn read_restore_choice(c: &Connection) -> Result<bool, StoreError> {
    if !has_table(c, "former_device")? {
        return Ok(false);
    }
    c.query_row("SELECT restore_choice FROM device WHERE id = 1", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|v| v != 0)
    .map_err(sql)
}

/// Give the database `c` a new device id in one step: each of `former` goes to
/// `former_device`, the outbox is emptied, and (`pending`) a restore's choice is waited for.
/// Returns the new id.
pub(crate) fn renew_device_id(
    c: &Connection,
    former: &[String],
    pending: bool,
) -> Result<String, StoreError> {
    let new = new_id();
    let mut insert = c
        .prepare_cached("INSERT INTO former_device (device_id) VALUES (?1) ON CONFLICT DO NOTHING")
        .map_err(sql)?;
    for id in former {
        if *id != new && crate::annotation::is_valid_id(id) {
            insert.execute([id]).map_err(sql)?;
        }
    }
    c.execute(
        "UPDATE device SET device_id = ?1, restore_choice = ?2 WHERE id = 1",
        params![new, i64::from(pending)],
    )
    .map_err(sql)?;
    c.execute("DELETE FROM op_outbox", []).map_err(sql)?;
    Ok(new)
}

impl UserStore {
    /// This device's id and the ids this database had before (ADR 0022 rule 7), its own
    /// first: a clock naming any of them is this device's own to the merge.
    pub fn own_device_ids(&self) -> Result<Vec<String>, StoreError> {
        read_own_ids(&self.conn)
    }

    /// Give this device a new id, as a copied database does when it finds op files under its
    /// id that it did not write (ADR 0022 rule 7): its old id is kept as a former one, the
    /// outbox is emptied, and it records no ops until it joins again. Returns the new id.
    pub fn take_new_device_id(&self) -> Result<String, StoreError> {
        let tx = self.write_tx()?;
        let old = read_device_id(&tx)?;
        let new = renew_device_id(&tx, &[old], false)?;
        tx.commit().map_err(sql)?;
        self.reseed_clock();
        Ok(new)
    }

    /// Whether a Backup restored on this device while it was syncing waits for the user to
    /// choose to catch up with the Sync folder or replace what is in it (ADR 0022 rule 7).
    pub fn restore_choice_pending(&self) -> Result<bool, StoreError> {
        read_restore_choice(&self.conn)
    }

    /// The user chose: the restore no longer waits.
    pub fn clear_restore_choice(&self) -> Result<(), StoreError> {
        self.conn
            .execute("UPDATE device SET restore_choice = 0 WHERE id = 1", [])
            .map(drop)
            .map_err(sql)
    }

    /// The name the user gave this Synced device (empty until then).
    pub fn device_name(&self) -> Result<String, StoreError> {
        self.conn
            .query_row("SELECT name FROM device WHERE id = 1", [], |r| r.get(0))
            .map_err(sql)
    }

    /// Name this Synced device ("Ian's iPhone"), as other devices show it.
    pub fn set_device_name(&self, name: &str) -> Result<(), StoreError> {
        self.conn
            .execute("UPDATE device SET name = ?1 WHERE id = 1", [name])
            .map(drop)
            .map_err(sql)
    }

    /// The last op file of each other Synced device merged here, by id (this device's own
    /// row, the last of its own written, left out).
    pub fn merged_devices(&self) -> Result<std::collections::BTreeMap<String, u64>, StoreError> {
        let own = read_device_id(&self.conn)?;
        let rows: Vec<(String, i64)> = self
            .conn
            .prepare("SELECT device_id, seq FROM applied_up_to ORDER BY device_id")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
            .map_err(sql)?;
        Ok(rows
            .into_iter()
            .filter(|(id, _)| *id != own)
            .map(|(id, seq)| (id, u64::try_from(seq).unwrap_or_default()))
            .collect())
    }

    /// The largest op file number reserved for the outbox's ops (`None`: none reserved): with
    /// the last own op file written, the files under this device's id that it wrote or will
    /// write again. A file above both was not written here (a copied database, rule 7).
    pub fn last_reserved_op_file(&self) -> Result<Option<u64>, StoreError> {
        let seq: Option<i64> = self
            .conn
            .query_row("SELECT MAX(file_seq) FROM op_outbox", [], |r| r.get(0))
            .map_err(sql)?;
        Ok(seq.and_then(|s| u64::try_from(s).ok()))
    }

    /// Whether this device syncs: it records ops, or a restore's choice is pending.
    pub(crate) fn syncs(&self) -> Result<bool, StoreError> {
        Ok(records_ops(&self.conn, &read_device_id(&self.conn)?)?
            || read_restore_choice(&self.conn)?)
    }
}
