//! The storage worker: one thread that owns the connection and handles
//! writes and bounded reads in order, so the coordinator never waits on SQLite.

use std::sync::mpsc;
use std::thread;

use bukno_core::event::{LoadRequest, PersistRequest, StorageResult};

use crate::{Store, repository};

pub enum Job {
    Persist(Box<PersistRequest>),
    Load(LoadRequest),
    /// Reply on the channel once every earlier job is done.
    Flush(mpsc::Sender<()>),
}

/// Where results go: the coordinator's inbox.
pub type Reply = Box<dyn Fn(StorageResult) + Send>;

pub struct Worker {
    jobs: mpsc::Sender<Job>,
    thread: Option<thread::JoinHandle<Store>>,
}

impl Worker {
    pub fn spawn(store: Store, reply: Reply) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let thread = thread::Builder::new()
            .name("bukno-storage".into())
            .spawn(move || run(store, rx, reply))
            .expect("start the storage thread");
        Self { jobs, thread: Some(thread) }
    }

    pub fn send(&self, job: Job) {
        // The thread only ends when the worker is shut down.
        let _ = self.jobs.send(job);
    }

    /// Wait until every job sent so far has been handled.
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        self.send(Job::Flush(tx));
        let _ = rx.recv();
    }

    /// Finish pending work, checkpoint the log and close the database.
    pub fn shutdown(mut self) -> Result<(), String> {
        let thread = self.thread.take().expect("worker thread");
        drop(std::mem::replace(&mut self.jobs, mpsc::channel().0));
        let store = thread.join().map_err(|_| "the storage thread panicked".to_owned())?;
        store.checkpoint().map_err(|e| e.to_string())
    }
}

fn run(mut store: Store, jobs: mpsc::Receiver<Job>, reply: Reply) -> Store {
    for job in jobs {
        match job {
            Job::Persist(request) => {
                let result = repository::apply(store.connection(), &request);
                let request = *request;
                if let Some(out) = outcome(&request, result) {
                    reply(out);
                }
            }
            Job::Load(LoadRequest::History { task }) => match repository::load_history(store.connection(), task) {
                Ok((items, draft)) => reply(StorageResult::HistoryLoaded { task, items, draft }),
                Err(e) => reply(StorageResult::WriteFailed { reason: format!("could not read the chat: {e}") }),
            },
            Job::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
    store
}

/// What the coordinator hears back for a write.
fn outcome(request: &PersistRequest, result: rusqlite::Result<()>) -> Option<StorageResult> {
    let error = result.err().map(|e| e.to_string());
    Some(match (request, error) {
        (PersistRequest::RecordDelivery { message, .. } | PersistRequest::MarkAboutToSend { message }, None) => {
            StorageResult::DeliveryRecorded { message: *message }
        }
        (
            PersistRequest::RecordDelivery { message, .. } | PersistRequest::MarkAboutToSend { message },
            Some(reason),
        ) => StorageResult::DeliveryFailed { message: *message, reason },
        (PersistRequest::Chat(task), None) => StorageResult::ChatSaved { task: task.id },
        (PersistRequest::Project(project), None) => StorageResult::ProjectSaved { project: project.id },
        (PersistRequest::Draft { task, revision, .. }, None) => {
            StorageResult::DraftSaved { task: *task, revision: *revision }
        }
        (PersistRequest::Draft { task, .. }, Some(reason)) => StorageResult::DraftFailed { task: *task, reason },
        (_, None) => return None,
        (_, Some(reason)) => StorageResult::WriteFailed { reason },
    })
}
