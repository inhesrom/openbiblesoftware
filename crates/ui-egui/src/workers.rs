//! Layout off the UI thread (#19 rule 3): a pool of worker threads shared by every pane and
//! snippet, each requester with its own reply channel. On wasm (no threads) jobs run inline
//! when submitted.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use obs_fonts::ShapeCache;
use obs_layout::{LaidBlock, Paragraph, layout};

/// Extra time added to every layout, for the bench's seeded-slowdown check (0 normally).
static SLOWDOWN_US: AtomicU64 = AtomicU64::new(0);

/// Make every layout this much slower (bench regression check only).
pub fn set_layout_slowdown(d: Duration) {
    SLOWDOWN_US.store(d.as_micros() as u64, Ordering::Relaxed);
}

/// Lay a paragraph out with the shared shape cache (plus any seeded slowdown).
pub fn lay_out(para: &Paragraph, width: f32) -> LaidBlock {
    let slow = SLOWDOWN_US.load(Ordering::Relaxed);
    if slow > 0 {
        std::thread::sleep(Duration::from_micros(slow));
    }
    layout(para, width, ShapeCache::shared())
}

/// Builds the layout on a worker: everything, including turning spans into a paragraph,
/// happens off the UI thread.
type Work = Box<dyn FnOnce() -> LaidBlock + Send>;

struct Job {
    key: u64,
    work: Work,
    reply: Sender<(u64, Arc<LaidBlock>)>,
}

/// The shared worker pool.
struct Pool {
    tx: Mutex<Option<Sender<Job>>>,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let tx = spawn_workers();
        Pool { tx: Mutex::new(tx) }
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_workers() -> Option<Sender<Job>> {
    let (tx, rx) = channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    // Leave one core for the UI thread; at most 4 layout workers.
    let n = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .saturating_sub(1)
        .clamp(1, 4);
    let mut started = 0;
    for i in 0..n {
        let rx = Arc::clone(&rx);
        let spawned = std::thread::Builder::new()
            .name(format!("obs-layout-{i}"))
            .spawn(move || {
                loop {
                    let job = {
                        let guard = rx.lock().unwrap_or_else(|p| p.into_inner());
                        guard.recv()
                    };
                    let Ok(job) = job else { return };
                    let laid = Arc::new((job.work)());
                    let _ = job.reply.send((job.key, laid));
                }
            });
        if spawned.is_ok() {
            started += 1;
        }
    }
    (started > 0).then_some(tx)
}

#[cfg(target_arch = "wasm32")]
fn spawn_workers() -> Option<Sender<Job>> {
    None
}

/// One requester's connection to the pool: submit paragraphs, collect finished layouts.
pub struct LayoutClient {
    reply_tx: Sender<(u64, Arc<LaidBlock>)>,
    reply_rx: Receiver<(u64, Arc<LaidBlock>)>,
    pending: std::collections::HashSet<u64>,
}

impl std::fmt::Debug for LayoutClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutClient")
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl Default for LayoutClient {
    fn default() -> Self {
        let (reply_tx, reply_rx) = channel();
        Self {
            reply_tx,
            reply_rx,
            pending: std::collections::HashSet::new(),
        }
    }
}

impl LayoutClient {
    /// Queue a paragraph's layout unless one with this key is already pending.
    pub fn submit(&mut self, key: u64, para: Paragraph, width: f32) {
        self.submit_with(key, move || lay_out(&para, width));
    }

    /// Queue `work` (which builds and lays out a paragraph) unless `key` is already pending.
    pub fn submit_with(&mut self, key: u64, work: impl FnOnce() -> LaidBlock + Send + 'static) {
        if !self.pending.insert(key) {
            return;
        }
        let tx = pool().tx.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let job = Job {
            key,
            work: Box::new(work),
            reply: self.reply_tx.clone(),
        };
        match tx {
            Some(tx) => {
                if let Err(e) = tx.send(job) {
                    run_inline(e.0);
                }
            }
            None => run_inline(job),
        }
    }

    pub fn is_pending(&self, key: u64) -> bool {
        self.pending.contains(&key)
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Finished layouts since the last call.
    pub fn drain(&mut self) -> Vec<(u64, Arc<LaidBlock>)> {
        let done: Vec<_> = self.reply_rx.try_iter().collect();
        for (k, _) in &done {
            self.pending.remove(k);
        }
        done
    }

    /// Forget pending work (its results will be ignored by key).
    pub fn forget_pending(&mut self) {
        self.pending.clear();
    }
}

fn run_inline(job: Job) {
    let laid = Arc::new((job.work)());
    let _ = job.reply.send((job.key, laid));
}

/// Inline-snippet layouts (cross-reference previews, popups, Word Info, search results), laid
/// out by the workers and ready a frame or two after first being asked for (#19 rule 3).
#[derive(Debug, Default)]
pub struct SnippetCache {
    client: LayoutClient,
    ready: std::collections::HashMap<u64, Arc<LaidBlock>>,
}

impl SnippetCache {
    /// Collect completed layouts even when no visible text requests a snippet this frame.
    pub fn drain(&mut self) {
        for (k, laid) in self.client.drain() {
            self.ready.insert(k, laid);
        }
    }

    /// The laid snippet if ready; otherwise queue it and return `None` (draw nothing yet).
    pub fn get(
        &mut self,
        text: &str,
        role: obs_layout::ColorRole,
        rtl: bool,
        width: f32,
        settings: &obs_layout::TextSettings,
    ) -> Option<Arc<LaidBlock>> {
        use std::hash::{Hash, Hasher};
        self.drain();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (text, role, rtl, width.to_bits(), settings.key()).hash(&mut h);
        let key = h.finish();
        if let Some(l) = self.ready.get(&key) {
            return Some(Arc::clone(l));
        }
        if self.ready.len() > 2000 {
            self.ready.clear();
        }
        let para = obs_layout::snippet_paragraph(text, role, rtl, settings);
        self.client.submit(key, para, width);
        None
    }

    /// Like [`Self::get`], with the byte ranges in `styled` drawn in their own colour roles (a
    /// Note body's Bible references as links, #64; a copy's verse numbers, #154).
    pub fn get_styled(
        &mut self,
        text: &str,
        base: obs_layout::ColorRole,
        styled: &[(std::ops::Range<usize>, obs_layout::ColorRole)],
        rtl: bool,
        width: f32,
        settings: &obs_layout::TextSettings,
    ) -> Option<Arc<LaidBlock>> {
        use std::hash::{Hash, Hasher};
        if styled.is_empty() {
            return self.get(text, base, rtl, width, settings);
        }
        self.drain();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (text, base, styled, rtl, width.to_bits(), settings.key()).hash(&mut h);
        let key = h.finish();
        if let Some(l) = self.ready.get(&key) {
            return Some(Arc::clone(l));
        }
        if self.ready.len() > 2000 {
            self.ready.clear();
        }
        let para = obs_layout::styled_snippet_paragraph(text, base, styled, rtl, settings);
        self.client.submit(key, para, width);
        None
    }

    pub fn has_pending(&self) -> bool {
        self.client.pending() > 0
    }
}
