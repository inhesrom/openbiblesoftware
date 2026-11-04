//! The engine job queue: Import, index builds, searches and Guide sections run here so the UI
//! thread never waits on SQLite or parsing (#12 rule 11).
//!
//! A [`Job`] does its work in bounded [`Job::step`]s. On desktop a pool of worker threads
//! steps jobs to completion ([`JobQueue::threaded`]). Where threads are unavailable (a later
//! wasm build), [`JobQueue::inline`] steps queued jobs round-robin on the caller's thread from
//! [`JobQueue::pump`]. Cancellation is checked between steps. Progress and results come back
//! as [`JobEvent`]s over a channel.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// Identifies one submitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// What a finished job produced; downcast to the type the job documents.
pub type JobOutput = Box<dyn Any + Send>;

/// The result of one [`Job::step`].
pub enum Step {
    /// More work remains; call `step` again.
    Continue,
    Done(JobOutput),
}

/// Unit of background work. Keep each step short (a few milliseconds) so cancellation and the
/// inline mode stay responsive.
pub trait Job: Send + 'static {
    fn label(&self) -> String;
    fn step(&mut self, ctx: &JobContext) -> Result<Step, String>;
}

/// Handed to every step: report progress, check cancellation.
pub struct JobContext {
    id: JobId,
    cancel: Arc<AtomicBool>,
    events: Sender<JobEvent>,
}

impl JobContext {
    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Report `done` of `total` units of work.
    /// Begin a named phase; progress belongs to this phase until the next one.
    pub fn phase(&self, name: impl Into<String>) {
        let _ = self.events.send(JobEvent {
            id: self.id,
            kind: JobEventKind::Phase(name.into()),
        });
    }

    pub fn progress(&self, done: u64, total: u64) {
        let _ = self.events.send(JobEvent {
            id: self.id,
            kind: JobEventKind::Progress { done, total },
        });
    }
}

/// Something that happened to a job.
#[derive(Debug)]
pub struct JobEvent {
    pub id: JobId,
    pub kind: JobEventKind,
}

pub enum JobEventKind {
    Started,
    Phase(String),
    Progress { done: u64, total: u64 },
    Finished(JobOutput),
    Failed(String),
    Cancelled,
}

impl fmt::Debug for JobEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobEventKind::Phase(name) => f.debug_tuple("Phase").field(name).finish(),
            JobEventKind::Started => f.write_str("Started"),
            JobEventKind::Progress { done, total } => write!(f, "Progress({done}/{total})"),
            JobEventKind::Finished(_) => f.write_str("Finished(..)"),
            JobEventKind::Failed(e) => write!(f, "Failed({e:?})"),
            JobEventKind::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl JobEventKind {
    /// Whether this is the last event the job will send.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobEventKind::Finished(_) | JobEventKind::Failed(_) | JobEventKind::Cancelled
        )
    }
}

/// A job made from a closure that runs in one step.
pub struct FnJob<F> {
    label: String,
    f: Option<F>,
}

impl<F, T> Job for FnJob<F>
where
    F: FnOnce(&JobContext) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    fn label(&self) -> String {
        self.label.clone()
    }

    fn step(&mut self, ctx: &JobContext) -> Result<Step, String> {
        match self.f.take() {
            Some(f) => f(ctx).map(|v| Step::Done(Box::new(v))),
            None => Err("job already ran".into()),
        }
    }
}

struct Queued {
    id: JobId,
    job: Box<dyn Job>,
    cancel: Arc<AtomicBool>,
    started: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<VecDeque<Queued>>,
    ready: Condvar,
    shutdown: AtomicBool,
    cancels: Mutex<Vec<(JobId, Arc<AtomicBool>)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs [`Job`]s and reports [`JobEvent`]s.
pub struct JobQueue {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    next_id: AtomicU64,
    events_tx: Sender<JobEvent>,
    events_rx: Receiver<JobEvent>,
}

impl fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobQueue")
            .field("workers", &self.workers.len())
            .finish_non_exhaustive()
    }
}

enum Outcome {
    Pending,
    /// The job is over; the caller forgets it, then sends this terminal event, so anyone who
    /// sees the event also sees the job gone (e.g. `cancel` returns false).
    Done(JobEventKind),
}

/// Run one step of `q`, sending non-terminal events.
fn step_once(q: &mut Queued, events: &Sender<JobEvent>) -> Outcome {
    let send = |kind| {
        let _ = events.send(JobEvent { id: q.id, kind });
    };
    if q.cancel.load(Ordering::Relaxed) {
        return Outcome::Done(JobEventKind::Cancelled);
    }
    if !q.started {
        q.started = true;
        send(JobEventKind::Started);
    }
    let ctx = JobContext {
        id: q.id,
        cancel: Arc::clone(&q.cancel),
        events: events.clone(),
    };
    let result = catch_unwind(AssertUnwindSafe(|| q.job.step(&ctx)));
    match result {
        Ok(Ok(Step::Continue)) => Outcome::Pending,
        Ok(Ok(Step::Done(out))) => Outcome::Done(JobEventKind::Finished(out)),
        Ok(Err(message)) => Outcome::Done(JobEventKind::Failed(message)),
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "job panicked".into());
            Outcome::Done(JobEventKind::Failed(format!(
                "{} panicked: {message}",
                q.job.label()
            )))
        }
    }
}

impl JobQueue {
    fn with_workers(threads: usize) -> Self {
        let (events_tx, events_rx) = channel();
        let shared = Arc::new(Shared::default());
        let workers = (0..threads)
            .filter_map(|i| {
                let shared = Arc::clone(&shared);
                let events = events_tx.clone();
                std::thread::Builder::new()
                    .name(format!("obs-job-{i}"))
                    .spawn(move || worker(&shared, &events))
                    .ok()
            })
            .collect();
        Self {
            shared,
            workers,
            next_id: AtomicU64::new(1),
            events_tx,
            events_rx,
        }
    }

    /// A pool of `threads` worker threads (at least one).
    pub fn threaded(threads: usize) -> Self {
        Self::with_workers(threads.max(1))
    }

    /// No threads: jobs run only when [`pump`](Self::pump) is called.
    pub fn inline() -> Self {
        Self::with_workers(0)
    }

    /// Whether jobs run on worker threads.
    pub fn is_threaded(&self) -> bool {
        !self.workers.is_empty()
    }

    pub fn submit(&self, job: impl Job) -> JobId {
        let id = JobId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&self.shared.cancels).push((id, Arc::clone(&cancel)));
        lock(&self.shared.queue).push_back(Queued {
            id,
            job: Box::new(job),
            cancel,
            started: false,
        });
        self.shared.ready.notify_one();
        id
    }

    /// Submit a closure that runs in one step; its result arrives as `Finished(Box<T>)`.
    pub fn submit_fn<T, F>(&self, label: impl Into<String>, f: F) -> JobId
    where
        F: FnOnce(&JobContext) -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        self.submit(FnJob {
            label: label.into(),
            f: Some(f),
        })
    }

    /// Ask a job to stop; it ends with `Cancelled` at its next step boundary.
    pub fn cancel(&self, id: JobId) -> bool {
        let cancels = lock(&self.shared.cancels);
        match cancels.iter().find(|(j, _)| *j == id) {
            Some((_, flag)) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Inline mode: run up to `max_steps` steps round-robin. Returns the steps run. In
    /// threaded mode this does nothing.
    pub fn pump(&self, max_steps: usize) -> usize {
        if self.is_threaded() {
            return 0;
        }
        let mut ran = 0;
        while ran < max_steps {
            let Some(mut q) = lock(&self.shared.queue).pop_front() else {
                break;
            };
            ran += 1;
            match step_once(&mut q, &self.events_tx) {
                Outcome::Pending => lock(&self.shared.queue).push_back(q),
                Outcome::Done(kind) => finish(&self.shared, &self.events_tx, q.id, kind),
            }
        }
        ran
    }

    /// The next event, if one is waiting.
    pub fn try_event(&self) -> Option<JobEvent> {
        self.events_rx.try_recv().ok()
    }

    /// Wait up to `timeout` for the next event.
    pub fn wait_event(&self, timeout: Duration) -> Option<JobEvent> {
        self.events_rx.recv_timeout(timeout).ok()
    }

    /// Every event waiting now.
    pub fn drain_events(&self) -> Vec<JobEvent> {
        self.events_rx.try_iter().collect()
    }
}

fn finish(shared: &Shared, events: &Sender<JobEvent>, id: JobId, kind: JobEventKind) {
    lock(&shared.cancels).retain(|(j, _)| *j != id);
    let _ = events.send(JobEvent { id, kind });
}

fn worker(shared: &Shared, events: &Sender<JobEvent>) {
    loop {
        let next = {
            let mut queue = lock(&shared.queue);
            loop {
                if shared.shutdown.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(q) = queue.pop_front() {
                    break q;
                }
                queue = shared
                    .ready
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let mut q = next;
        loop {
            if shared.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if let Outcome::Done(kind) = step_once(&mut q, events) {
                finish(shared, events, q.id, kind);
                break;
            }
        }
    }
}

impl Drop for JobQueue {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        for (_, flag) in lock(&self.shared.cancels).iter() {
            flag.store(true, Ordering::Relaxed);
        }
        self.shared.ready.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    /// Counts to `total`, one per step, reporting progress; never ends if `total` is None.
    struct Counter {
        done: u64,
        total: Option<u64>,
    }

    impl Job for Counter {
        fn label(&self) -> String {
            "counter".into()
        }

        fn step(&mut self, ctx: &JobContext) -> Result<Step, String> {
            self.done += 1;
            ctx.progress(self.done, self.total.unwrap_or(0));
            match self.total {
                Some(t) if self.done >= t => Ok(Step::Done(Box::new(self.done))),
                _ => {
                    std::thread::sleep(Duration::from_millis(1));
                    Ok(Step::Continue)
                }
            }
        }
    }

    /// Events for `id` until its terminal event.
    fn events_until_done(q: &JobQueue, id: JobId) -> Vec<JobEventKind> {
        let mut out = Vec::new();
        while let Some(e) = q.wait_event(WAIT) {
            if e.id != id {
                continue;
            }
            let terminal = e.kind.is_terminal();
            out.push(e.kind);
            if terminal {
                return out;
            }
        }
        panic!("job {id:?} did not finish; got {out:?}");
    }

    #[test]
    fn threaded_job_reports_progress_and_result() {
        let q = JobQueue::threaded(2);
        let id = q.submit(Counter {
            done: 0,
            total: Some(5),
        });
        let events = events_until_done(&q, id);
        assert!(matches!(events[0], JobEventKind::Started));
        let progress: Vec<u64> = events
            .iter()
            .filter_map(|e| match e {
                JobEventKind::Progress { done, .. } => Some(*done),
                _ => None,
            })
            .collect();
        assert_eq!(progress, [1, 2, 3, 4, 5]);
        match events.last() {
            Some(JobEventKind::Finished(out)) => assert_eq!(out.downcast_ref::<u64>(), Some(&5)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn long_job_can_be_cancelled() {
        let q = JobQueue::threaded(1);
        let id = q.submit(Counter {
            done: 0,
            total: None,
        });
        // Wait until it is really running, then cancel.
        loop {
            match q.wait_event(WAIT).map(|e| e.kind) {
                Some(JobEventKind::Progress { done, .. }) if done >= 3 => break,
                Some(_) => {}
                None => panic!("no progress"),
            }
        }
        assert!(q.cancel(id));
        let rest = events_until_done(&q, id);
        assert!(
            matches!(rest.last(), Some(JobEventKind::Cancelled)),
            "{rest:?}"
        );
        assert!(!q.cancel(id), "finished jobs are forgotten");
    }

    #[test]
    fn many_jobs_all_finish_on_the_pool() {
        let q = JobQueue::threaded(4);
        let ids: Vec<JobId> = (0..50u64)
            .map(|i| q.submit_fn(format!("square {i}"), move |_| Ok(i * i)))
            .collect();
        let mut sum = 0u64;
        let mut finished = 0;
        while finished < ids.len() {
            let e = q.wait_event(WAIT).expect("event");
            if let JobEventKind::Finished(out) = e.kind {
                sum += *out.downcast::<u64>().expect("u64");
                finished += 1;
            }
        }
        assert_eq!(sum, (0..50u64).map(|i| i * i).sum::<u64>());
    }

    #[test]
    fn failures_and_panics_become_failed_events() {
        let q = JobQueue::threaded(1);
        let a = q.submit_fn::<(), _>("fails", |_| Err("bad input".into()));
        let events = events_until_done(&q, a);
        assert!(matches!(events.last(), Some(JobEventKind::Failed(m)) if m == "bad input"));
        let b = q.submit_fn::<(), _>("panics", |_| panic!("boom"));
        let events = events_until_done(&q, b);
        assert!(
            matches!(events.last(), Some(JobEventKind::Failed(m)) if m.contains("boom")),
            "{events:?}"
        );
        // The worker survived the panic.
        let c = q.submit_fn("after", |_| Ok(1u8));
        assert!(matches!(
            events_until_done(&q, c).last(),
            Some(JobEventKind::Finished(_))
        ));
    }

    #[test]
    fn inline_mode_runs_only_when_pumped_and_interleaves() {
        let q = JobQueue::inline();
        assert!(!q.is_threaded());
        let a = q.submit(Counter {
            done: 0,
            total: Some(3),
        });
        let b = q.submit(Counter {
            done: 0,
            total: Some(2),
        });
        assert!(q.try_event().is_none(), "nothing runs before pump");
        assert_eq!(q.pump(2), 2);
        let first: Vec<JobId> = q.drain_events().iter().map(|e| e.id).collect();
        assert!(first.contains(&a) && first.contains(&b), "round-robin");
        q.pump(100);
        let done: Vec<JobId> = q
            .drain_events()
            .into_iter()
            .filter(|e| matches!(e.kind, JobEventKind::Finished(_)))
            .map(|e| e.id)
            .collect();
        assert_eq!(done, [b, a]);
        assert_eq!(q.pump(10), 0);
    }

    #[test]
    fn inline_cancel_before_start() {
        let q = JobQueue::inline();
        let id = q.submit(Counter {
            done: 0,
            total: None,
        });
        q.cancel(id);
        q.pump(10);
        let kinds: Vec<_> = q.drain_events().into_iter().map(|e| e.kind).collect();
        assert!(
            matches!(kinds.as_slice(), [JobEventKind::Cancelled]),
            "{kinds:?}"
        );
    }

    #[test]
    fn dropping_the_queue_stops_running_jobs() {
        let q = JobQueue::threaded(2);
        q.submit(Counter {
            done: 0,
            total: None,
        });
        std::thread::sleep(Duration::from_millis(5));
        drop(q); // must not hang
    }
}
