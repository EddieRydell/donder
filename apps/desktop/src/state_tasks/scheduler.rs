use camino::Utf8PathBuf;
use donder_project_io::{ProjectCheckReport, ProjectSession, SourceOverrides};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

struct Queue<P> {
    next: Option<P>,
    running: bool,
    closed: bool,
}

/// One running request and one replaceable pending request. Completion applies
/// its result before releasing the barrier; snapshots never drain tasks.
pub(crate) struct LatestScheduler<P> {
    queue: Arc<(Mutex<Queue<P>>, Condvar)>,
}

impl<P: Send + 'static> LatestScheduler<P> {
    pub(crate) fn new(worker: impl Fn(P) + Send + 'static) -> Self {
        let queue = Arc::new((
            Mutex::new(Queue {
                next: None,
                running: false,
                closed: false,
            }),
            Condvar::new(),
        ));
        let shared = Arc::clone(&queue);
        thread::spawn(move || {
            let (mutex, changed) = &*shared;
            loop {
                let request = {
                    let mut queue = mutex.lock().unwrap_or_else(|error| error.into_inner());
                    while queue.next.is_none() && !queue.closed {
                        queue = changed
                            .wait(queue)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if queue.closed {
                        return;
                    }
                    queue.running = true;
                    queue.next.take()
                };
                if let Some(request) = request {
                    worker(request);
                }
                let mut queue = mutex.lock().unwrap_or_else(|error| error.into_inner());
                queue.running = false;
                changed.notify_all();
            }
        });
        Self { queue }
    }

    pub(crate) fn schedule(&self, request: P) -> bool {
        let (mutex, changed) = &*self.queue;
        let mut queue = mutex.lock().unwrap_or_else(|error| error.into_inner());
        if queue.closed {
            return false;
        }
        queue.next = Some(request);
        changed.notify_all();
        true
    }

    pub(crate) fn invalidate_pending(&self) {
        let (mutex, changed) = &*self.queue;
        mutex.lock().unwrap_or_else(|error| error.into_inner()).next = None;
        changed.notify_all();
    }

    pub(crate) fn finish_pending(&self) {
        let (mutex, changed) = &*self.queue;
        let mut queue = mutex.lock().unwrap_or_else(|error| error.into_inner());
        while queue.running || queue.next.is_some() {
            queue = changed
                .wait(queue)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        let queue = self
            .queue
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        !queue.running && queue.next.is_none()
    }
}

impl<P> Drop for LatestScheduler<P> {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.queue;
        let mut queue = mutex.lock().unwrap_or_else(|error| error.into_inner());
        queue.closed = true;
        queue.next = None;
        changed.notify_all();
    }
}

pub(crate) struct WorkingCopyPayload {
    pub root: Utf8PathBuf,
    pub epoch: u32,
    pub revision: u32,
    pub sources: SourceOverrides,
    pub typed: Option<Arc<ProjectSession>>,
    pub save: bool,
    pub autosave_generation: u32,
}

pub(crate) fn analyze_working_copy(request: &WorkingCopyPayload) -> Option<ProjectCheckReport> {
    request
        .typed
        .is_none()
        .then(|| donder_project_io::check_package_with_overrides(&request.root, &request.sources))
}

pub(crate) struct RenderRefreshPayload {
    pub project: Arc<ProjectSession>,
    pub setup_id: donder_language::setup::SetupId,
    pub sequence_id: donder_language::sequence::SequenceId,
    pub project_epoch: u32,
    pub project_revision: u32,
}
