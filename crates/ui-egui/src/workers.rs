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
/// happens off the UI thread. `None`: its requester no longer wants it, so nothing was done.
type Work = Box<dyn FnOnce() -> Option<LaidBlock> + Send>;

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
                    run(job);
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
    /// Bumped when pending work is forgotten: queued jobs from before skip their layout, so a
    /// burst of relayouts (Ctrl+Plus held, #299) never queues stale work ahead of the new.
    epoch: Arc<AtomicU64>,
    /// Layouts the workers did for this client.
    ran: Arc<AtomicU64>,
    /// Jobs kept back from the pool until [`Self::release`] (tests: a pool busy elsewhere).
    #[cfg(test)]
    held: Option<Vec<Job>>,
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
            epoch: Arc::default(),
            ran: Arc::default(),
            #[cfg(test)]
            held: None,
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
        let (epoch, ran) = (Arc::clone(&self.epoch), Arc::clone(&self.ran));
        let submitted = epoch.load(Ordering::Relaxed);
        let job = Job {
            key,
            work: Box::new(move || {
                if epoch.load(Ordering::Relaxed) != submitted {
                    return None;
                }
                ran.fetch_add(1, Ordering::Relaxed);
                Some(work())
            }),
            reply: self.reply_tx.clone(),
        };
        #[cfg(test)]
        if let Some(held) = self.held.as_mut() {
            held.push(job);
            return;
        }
        send(job);
    }

    /// Keep jobs back from the pool until [`Self::release`], as a pool busy with other work
    /// would (tests).
    #[cfg(test)]
    pub fn hold(&mut self) {
        self.held.get_or_insert_with(Vec::new);
    }

    /// Send the jobs kept back since [`Self::hold`] to the pool, in order.
    #[cfg(test)]
    pub fn release(&mut self) {
        for job in self.held.take().unwrap_or_default() {
            send(job);
        }
    }

    /// Layouts the workers did for this client (jobs skipped as forgotten are not counted).
    pub fn ran(&self) -> u64 {
        self.ran.load(Ordering::Relaxed)
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

    /// Forget pending work: queued jobs skip their layout, and the results of those already
    /// running are ignored by key.
    pub fn forget_pending(&mut self) {
        self.pending.clear();
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }
}

/// Queue a job for the workers; without any (wasm), run it now.
fn send(job: Job) {
    let tx = pool().tx.lock().unwrap_or_else(|p| p.into_inner()).clone();
    match tx {
        Some(tx) => {
            if let Err(e) = tx.send(job) {
                run(e.0);
            }
        }
        None => run(job),
    }
}

/// Do a job and send its layout back, unless its requester no longer wants it.
fn run(job: Job) {
    if let Some(laid) = (job.work)() {
        let _ = job.reply.send((job.key, Arc::new(laid)));
    }
}

/// Inline-snippet layouts (cross-reference previews, popups, Word Info, search results), laid
/// out by the workers and ready a frame or two after first being asked for (#19 rule 3).
#[derive(Debug, Default)]
pub struct SnippetCache {
    client: LayoutClient,
    ready: std::collections::HashMap<u64, Arc<LaidBlock>>,
}

impl SnippetCache {
    /// Forget the layouts still queued: new text settings (#299) want every snippet at the
    /// new size, so those at the old one are skipped rather than done first.
    pub fn forget_pending(&mut self) {
        self.client.forget_pending();
    }

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

    /// A Resource's own text unit (a Footnote's body, #208) as one paragraph, styled by its
    /// spans as the reading pane styles them.
    pub fn get_unit(
        &mut self,
        unit: &obs_model::TextUnit,
        direction: obs_model::Direction,
        width: f32,
        settings: &obs_layout::TextSettings,
    ) -> Option<Arc<LaidBlock>> {
        use std::hash::{Hash, Hasher};
        self.drain();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            "unit",
            &unit.text,
            &unit.spans,
            direction == obs_model::Direction::Rtl,
            width.to_bits(),
            settings.key(),
        )
            .hash(&mut h);
        let key = h.finish();
        if let Some(l) = self.ready.get(&key) {
            return Some(Arc::clone(l));
        }
        if self.ready.len() > 2000 {
            self.ready.clear();
        }
        let unit = unit.clone();
        let settings = *settings;
        self.client.submit_with(key, move || {
            let block = obs_layout::BlockSource {
                kind: obs_layout::BlockKind::Plain,
                range: 0..unit.text.len(),
            };
            let para = obs_layout::block_paragraph(&unit, &block, direction, &settings);
            lay_out(&para, width)
        });
        None
    }

    /// Like [`Self::get`], with Note formatting (#186): byte ranges bold, italic or in their
    /// own colour role, at `scale` times the text size.
    pub fn get_formatted(
        &mut self,
        text: &str,
        base: obs_layout::ColorRole,
        styled: &[(std::ops::Range<usize>, obs_layout::SnippetStyle)],
        scale: f32,
        width: f32,
        settings: &obs_layout::TextSettings,
    ) -> Option<Arc<LaidBlock>> {
        use std::hash::{Hash, Hasher};
        self.drain();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            "formatted",
            text,
            base,
            styled,
            scale.to_bits(),
            width.to_bits(),
            settings.key(),
        )
            .hash(&mut h);
        let key = h.finish();
        if let Some(l) = self.ready.get(&key) {
            return Some(Arc::clone(l));
        }
        if self.ready.len() > 2000 {
            self.ready.clear();
        }
        let para = obs_layout::formatted_snippet_paragraph(text, base, styled, scale, settings);
        self.client.submit(key, para, width);
        None
    }

    pub fn has_pending(&self) -> bool {
        self.client.pending() > 0
    }
}
