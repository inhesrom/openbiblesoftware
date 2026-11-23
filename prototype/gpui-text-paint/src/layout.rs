//! PROTOTYPE — throwaway (#372). Layout off the UI thread, as the egui app does it
//! (`crates/ui-egui/src/{workers,view}.rs`): a pool of worker threads lays blocks out with
//! `obs_layout::layout` (harfrust shaping through the shared `ShapeCache`, bidi, line
//! breaking), into a cache of laid blocks keyed by Bible, block and width. The UI thread lays a
//! block out itself only within a per-frame budget (`SYNC_BUDGET`, 2 ms as in the egui app),
//! or for the block a jump lands in, which is exempt.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use obs_fonts::ShapeCache;
use obs_layout::{HeightEstimate, LaidBlock, TextSettings, layout};

use crate::data::Bible;

/// The UI thread's layout budget per frame, as in the egui app (#19).
pub const SYNC_BUDGET: Duration = Duration::from_millis(2);

/// (Bible, block, width in quarter points).
pub type Key = (usize, usize, u32);

pub fn width_key(width: f32) -> u32 {
    (width * 4.0).round().max(0.0) as u32
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Key>,
    queued: HashSet<Key>,
}

pub struct Layouts {
    bibles: Vec<Arc<Bible>>,
    /// Height estimates per Bible and block (width-independent part).
    estimates: Vec<Vec<HeightEstimate>>,
    pub settings: TextSettings,
    done: Mutex<HashMap<Key, Arc<LaidBlock>>>,
    queue: Mutex<Queue>,
    wake: Condvar,
    pub worker_layouts: AtomicU64,
    pub ui_layouts: AtomicU64,
}

impl Layouts {
    pub fn start(bibles: Vec<Arc<Bible>>, estimates: Vec<Vec<HeightEstimate>>) -> Arc<Self> {
        let me = Arc::new(Self {
            bibles,
            estimates,
            settings: TextSettings::default(),
            done: Mutex::default(),
            queue: Mutex::default(),
            wake: Condvar::new(),
            worker_layouts: AtomicU64::new(0),
            ui_layouts: AtomicU64::new(0),
        });
        let n = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
        for i in 0..n {
            let me = Arc::clone(&me);
            let _ = std::thread::Builder::new()
                .name(format!("layout-{i}"))
                .spawn(move || me.work());
        }
        me
    }

    fn work(&self) {
        loop {
            let key = {
                let mut q = self.queue.lock().unwrap_or_else(|p| p.into_inner());
                loop {
                    if let Some(k) = q.jobs.pop_front() {
                        break k;
                    }
                    q = self.wake.wait(q).unwrap_or_else(|p| p.into_inner());
                }
            };
            if self.get(key).is_none() {
                if let Some(l) = self.lay_out(key) {
                    self.worker_layouts.fetch_add(1, Ordering::Relaxed);
                    self.done.lock().unwrap_or_else(|p| p.into_inner()).insert(key, l);
                }
            }
            self.queue.lock().unwrap_or_else(|p| p.into_inner()).queued.remove(&key);
        }
    }

    fn lay_out(&self, (b, i, w): Key) -> Option<Arc<LaidBlock>> {
        let para = self.bibles.get(b)?.paragraph(i, &self.settings)?;
        Some(Arc::new(layout(&para, w as f32 / 4.0, ShapeCache::shared())))
    }

    pub fn get(&self, key: Key) -> Option<Arc<LaidBlock>> {
        self.done.lock().unwrap_or_else(|p| p.into_inner()).get(&key).cloned()
    }

    /// Lay a block out on this thread now (a jump's block, or within the sync budget).
    pub fn now(&self, key: Key) -> Option<Arc<LaidBlock>> {
        if let Some(l) = self.get(key) {
            return Some(l);
        }
        let l = self.lay_out(key)?;
        self.ui_layouts.fetch_add(1, Ordering::Relaxed);
        self.done.lock().unwrap_or_else(|p| p.into_inner()).insert(key, Arc::clone(&l));
        Some(l)
    }

    /// Queue blocks for the workers; nearer ones first (the caller passes them in order).
    pub fn prefetch(&self, keys: impl IntoIterator<Item = Key>) {
        let mut q = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        let done = self.done.lock().unwrap_or_else(|p| p.into_inner());
        let mut added = false;
        for k in keys {
            if !done.contains_key(&k) && q.queued.insert(k) {
                q.jobs.push_back(k);
                added = true;
            }
        }
        drop(done);
        if added {
            self.wake.notify_all();
        }
    }

    #[allow(dead_code)]
    pub fn pending(&self) -> usize {
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).queued.len()
    }

    /// Height of a block at a width: real if laid out, else estimated.
    pub fn height(&self, (b, i, w): Key) -> f32 {
        if let Some(l) = self.get((b, i, w)) {
            return l.height;
        }
        self.estimate(b, i, w as f32 / 4.0)
    }

    pub fn estimate(&self, b: usize, i: usize, width: f32) -> f32 {
        self.estimates
            .get(b)
            .and_then(|e| e.get(i))
            .map_or(self.settings.size * 2.0, |e| e.at(width))
    }
}

/// The per-frame budget for synchronous layout on the UI thread.
pub struct Budget {
    until: Instant,
}

impl Budget {
    pub fn new() -> Self {
        Self { until: Instant::now() + SYNC_BUDGET }
    }
    pub fn left(&self) -> bool {
        Instant::now() < self.until
    }
}
