//! Long jobs that outlive the request that asked for them.
//!
//! Rebuilding the index takes half a minute; exporting a library took nine
//! seconds and importing one can take much longer. Holding the HTTP request
//! open for those means a client with nothing to show, a proxy free to time
//! out, and a reload that loses track of work still going on.
//!
//! So the request starts the job and returns a handle. The job reports where it
//! has got to, can be asked to stop, and its result waits to be collected.
//!
//! Deliberately not merged with `runs`, which tracks agent turns: a turn is one
//! per conversation, carries a transcript, and streams to the event bus. A task
//! is one of many, carries a result, and is polled. They look alike for about
//! as long as it takes to write the second one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use yk_agent::Cancel;

/// What a job is doing, from the outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    /// `export`, `import`, `backup`, `reindex`…
    pub kind: String,
    pub phase: Phase,
    /// What it is doing now, in the user's language.
    pub message: String,
    /// Steps finished, and how many there are — `total` is 0 when unknown,
    /// which is honest rather than pretending a spinner is a bar.
    pub done: u64,
    pub total: u64,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
    #[serde(rename = "finishedAt", skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    /// Counters only this kind of job has, updated as it goes.
    ///
    /// `done`/`total` answer "how far", which every job shares; harvesting
    /// also has to say how many reference lists it stored and how many
    /// publishers deposited none — numbers that mean nothing to an export.
    /// Putting them in the message as prose would make the interface parse
    /// English to draw a table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// Whatever the job produced, once it has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TaskState {
    fn new(id: String, kind: String, message: String) -> Self {
        Self {
            id,
            kind,
            phase: Phase::Running,
            message,
            done: 0,
            total: 0,
            started_at: now_secs(),
            finished_at: None,
            detail: None,
            result: None,
            error: None,
        }
    }
}

/// A handle the job itself holds, to say how it is getting on.
pub struct Task {
    state: Mutex<TaskState>,
    pub cancel: Cancel,
}

impl Task {
    /// Report progress. `total` of zero means "not countable".
    pub fn progress(&self, message: impl Into<String>, done: u64, total: u64) {
        let mut state = self.state.lock().expect("task state");
        state.message = message.into();
        state.done = done;
        state.total = total;
    }

    /// Report counters specific to this kind of job.
    pub fn detail(&self, detail: serde_json::Value) {
        self.state.lock().expect("task state").detail = Some(detail);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.stopped()
    }

    pub fn snapshot(&self) -> TaskState {
        self.state.lock().expect("task state").clone()
    }

}

/// How many finished tasks to remember.
///
/// Enough that a client which looked away can still collect its result;
/// not so many that a long-running server keeps every import it ever did.
const KEEP_FINISHED: usize = 50;

#[derive(Clone, Default)]
pub struct Tasks(Arc<Mutex<HashMap<String, Arc<Task>>>>, Option<yk_store::Db>);

impl Tasks {
    pub async fn open(db: yk_store::Db) -> yk_core::Result<Self> {
        let tasks = Self(Default::default(), Some(db.clone()));
        for record in db.task_records().await? {
            let mut state: TaskState = serde_json::from_value(record)?;
            if state.phase == Phase::Running {
                state.phase = Phase::Failed;
                state.finished_at = Some(now_secs());
                state.error = Some("The server stopped before this task finished. Review any partial changes before starting it again.".into());
                db.save_task_record(state.id.clone(), serde_json::to_value(&state)?).await?;
            }
            tasks.0.lock().expect("tasks").insert(state.id.clone(), Arc::new(Task {
                state: Mutex::new(state),
                cancel: Cancel::default(),
            }));
        }
        tasks.forget_old().await;
        Ok(tasks)
    }

    /// Register a job and hand back the handle it reports through.
    pub async fn start(&self, kind: &str, message: &str) -> yk_core::Result<Arc<Task>> {
        let id = next_id();
        let task = Arc::new(Task {
            state: Mutex::new(TaskState::new(id.clone(), kind.into(), message.into())),
            cancel: Cancel::default(),
        });
        if let Some(db) = &self.1 {
            db.save_task_record(id.clone(), serde_json::to_value(task.snapshot())?).await?;
        }
        self.0.lock().expect("tasks").insert(id, task.clone());
        Ok(task)
    }

    /// Jobs that write in bulk, and must not be interrupted by anything that
    /// takes the database exclusively.
    ///
    /// Named rather than inferred: a task knows its kind at birth, and the
    /// alternative — a flag every caller remembers to set — is the flag that
    /// gets forgotten on the job added next year.
    pub const BULK_WRITERS: [&str; 3] = ["reindex", "import", "zotero"];

    /// Whether any bulk write is going.
    pub fn bulk_write_running(&self) -> bool {
        Self::BULK_WRITERS.iter().any(|kind| self.running(kind))
    }

    /// Whether a job of this kind is already going.
    ///
    /// Some jobs must not overlap: two harvests talk to the same service and
    /// would only get the client throttled. The registry is the one place that
    /// knows what is running, so it is the one place that can answer this.
    pub fn running(&self, kind: &str) -> bool {
        self.0
            .lock()
            .expect("tasks")
            .values()
            .any(|t| {
                let s = t.snapshot();
                s.kind == kind && s.phase == Phase::Running
            })
    }

    pub fn get(&self, id: &str) -> Option<TaskState> {
        self.0.lock().expect("tasks").get(id).map(|t| t.snapshot())
    }

    /// Newest first, because what somebody wants is the one they just started.
    pub fn list(&self) -> Vec<TaskState> {
        let mut all: Vec<TaskState> =
            self.0.lock().expect("tasks").values().map(|t| t.snapshot()).collect();
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
        all
    }

    /// Ask a job to stop. Whether it does is up to the job: cancellation is a
    /// flag checked between steps, not a killed thread, so that whatever it was
    /// half way through finishes or unwinds cleanly.
    pub fn cancel(&self, id: &str) -> bool {
        match self.0.lock().expect("tasks").get(id) {
            Some(task) => {
                task.cancel.stop();
                true
            }
            None => false,
        }
    }

    /// Record that a job finished its work.
    ///
    /// Deliberately not "Done unless the cancel flag is set": whether a job
    /// stopped early is something only the job knows. Reading the flag here
    /// reported a rebuild that ran to completion as cancelled, because asking
    /// it to stop is not the same as it stopping — see [`Tasks::stopped`].
    pub async fn finish(&self, task: &Arc<Task>, result: serde_json::Value) {
        self.complete(task, Phase::Done, Some(result), None).await;
    }

    /// Record that a job noticed the cancel flag and stopped early.
    ///
    /// `partial` is whatever it managed, which is worth reporting: an import
    /// that stopped after four hundred items has added four hundred items.
    pub async fn stopped(&self, task: &Arc<Task>, partial: serde_json::Value) {
        self.complete(task, Phase::Cancelled, Some(partial), None).await;
    }

    pub async fn fail(&self, task: &Arc<Task>, error: impl std::fmt::Display) {
        self.complete(task, Phase::Failed, None, Some(error.to_string())).await;
    }

    async fn complete(
        &self,
        task: &Arc<Task>,
        phase: Phase,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) {
        let mut state = task.snapshot();
        state.phase = phase;
        state.finished_at = Some(now_secs());
        state.result = result;
        state.error = error;
        if let Some(db) = &self.1 {
            let saved = match serde_json::to_value(&state) {
                Ok(value) => db.save_task_record(state.id.clone(), value).await,
                Err(error) => Err(error.into()),
            };
            if let Err(error) = saved {
                tracing::error!(task = %state.id, %error, "could not persist task completion");
                state.phase = Phase::Failed;
                state.error = Some(format!("Could not persist task completion: {error}"));
            }
        }
        *task.state.lock().expect("task state") = state;
        self.forget_old().await;
    }

    /// Pruned when a task *finishes*, which is the moment the number of
    /// finished tasks grows. Doing it only on start leaves whatever finished
    /// since the last start hanging around — correct, but a bound nobody can
    /// state in one sentence.
    async fn forget_old(&self) {
        let removed = prune(&mut self.0.lock().expect("tasks"));
        if let Some(db) = &self.1 {
            if let Err(error) = db.forget_task_records(removed).await {
                tracing::warn!(%error, "could not prune task history");
            }
        }
    }
}

/// Forget the oldest finished tasks, never a running one.
fn prune(tasks: &mut HashMap<String, Arc<Task>>) -> Vec<String> {
    let mut finished: Vec<(String, i64)> = tasks
        .iter()
        .filter_map(|(id, t)| {
            let s = t.snapshot();
            (s.phase != Phase::Running).then(|| (id.clone(), s.finished_at.unwrap_or(0)))
        })
        .collect();
    if finished.len() <= KEEP_FINISHED {
        return Vec::new();
    }
    finished.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut removed = Vec::new();
    for (id, _) in finished.iter().take(finished.len() - KEEP_FINISHED) {
        tasks.remove(id);
        removed.push(id.clone());
    }
    removed
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Ordered within a process and unique across restarts.
fn next_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "t{:020}-{:016}-{}",
        yk_core::now_ms(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        uuid::Uuid::new_v4()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_started_task_is_running_and_findable() {
        let tasks = Tasks::default();
        let task = tasks.start("export", "Packing…").await.unwrap();
        let id = task.snapshot().id;

        let found = tasks.get(&id).expect("the task");
        assert_eq!(found.phase, Phase::Running);
        assert_eq!(found.kind, "export");
        assert!(found.finished_at.is_none());
    }

    #[tokio::test]
    async fn progress_is_visible_while_it_runs() {
        // The whole point: a client that polls must see movement, not a
        // spinner that could equally mean "stuck".
        let tasks = Tasks::default();
        let task = tasks.start("import", "Reading…").await.unwrap();
        task.progress("task.writingItems", 40, 120);

        let seen = tasks.get(&task.snapshot().id).unwrap();
        assert_eq!((seen.done, seen.total), (40, 120));
        assert_eq!(seen.message, "task.writingItems");
    }

    #[tokio::test]
    async fn a_result_waits_to_be_collected() {
        // A client that reloaded during the job must still be able to find out
        // how it went.
        let tasks = Tasks::default();
        let task = tasks.start("export", "Packing…").await.unwrap();
        tasks.finish(&task, serde_json::json!({ "name": "out.yinkote" })).await;

        let done = tasks.get(&task.snapshot().id).unwrap();
        assert_eq!(done.phase, Phase::Done);
        assert_eq!(done.result.unwrap()["name"], "out.yinkote");
        assert!(done.finished_at.is_some());
    }

    #[tokio::test]
    async fn a_job_that_stopped_early_says_so_and_keeps_what_it_did() {
        let tasks = Tasks::default();
        let task = tasks.start("import", "Reading…").await.unwrap();
        assert!(tasks.cancel(&task.snapshot().id));
        assert!(task.cancelled());

        tasks.stopped(&task, serde_json::json!({ "items": 12 })).await;
        let state = tasks.get(&task.snapshot().id).unwrap();
        assert_eq!(state.phase, Phase::Cancelled);
        assert_eq!(state.result.unwrap()["items"], 12, "what it managed still counts");
    }

    #[tokio::test]
    async fn asking_a_job_to_stop_is_not_the_same_as_it_stopping() {
        // Not every job can stop: rebuilding the index is two passes inside a
        // library that does not check the flag. Reporting it as cancelled
        // because somebody asked would be a lie about work that was done.
        let tasks = Tasks::default();
        let task = tasks.start("reindex", "Rebuilding…").await.unwrap();
        tasks.cancel(&task.snapshot().id);

        tasks.finish(&task, serde_json::json!({ "reindexed": 100 })).await;
        assert_eq!(
            tasks.get(&task.snapshot().id).unwrap().phase,
            Phase::Done,
            "it finished the work, whatever was asked of it"
        );
    }

    #[tokio::test]
    async fn a_bulk_write_is_recognised_whatever_its_kind() {
        // The checkpoint worker asks this before taking the database
        // exclusively. Getting it wrong is not a wrong answer on screen; it is
        // the program refusing writes for as long as the timeout allows.
        let tasks = Tasks::default();
        assert!(!tasks.bulk_write_running());

        let importing = tasks.start("import", "Reading").await.unwrap();
        assert!(tasks.bulk_write_running());
        tasks.finish(&importing, serde_json::json!({})).await;
        assert!(!tasks.bulk_write_running());

        // An export copies the database but does not write to it.
        let exporting = tasks.start("export", "Packing").await.unwrap();
        assert!(!tasks.bulk_write_running(), "an export is not a bulk write");
        tasks.finish(&exporting, serde_json::json!({})).await;
    }

    #[tokio::test]
    async fn a_kind_that_must_not_overlap_can_ask_whether_it_is_going() {
        let tasks = Tasks::default();
        assert!(!tasks.running("harvest"));

        let task = tasks.start("harvest", "Fetching").await.unwrap();
        assert!(tasks.running("harvest"));
        assert!(!tasks.running("export"), "a different kind is a different question");

        tasks.finish(&task, serde_json::json!({})).await;
        assert!(!tasks.running("harvest"), "a finished run does not block the next one");
    }

    #[tokio::test]
    async fn a_job_can_report_counters_only_it_understands() {
        // Harvesting has to say how many reference lists it stored and how
        // many publishers deposited none. Neither means anything to an export,
        // and putting them in the message would make the interface read prose.
        let tasks = Tasks::default();
        let task = tasks.start("harvest", "Fetching").await.unwrap();
        task.detail(serde_json::json!({ "stored": 40, "empty": 3 }));

        let seen = tasks.get(&task.snapshot().id).unwrap();
        assert_eq!(seen.detail.unwrap()["stored"], 40);
    }

    #[tokio::test]
    async fn a_failure_keeps_its_reason() {
        let tasks = Tasks::default();
        let task = tasks.start("backup", "Copying…").await.unwrap();
        tasks.fail(&task, "the disk is full").await;

        let state = tasks.get(&task.snapshot().id).unwrap();
        assert_eq!(state.phase, Phase::Failed);
        assert_eq!(state.error.as_deref(), Some("the disk is full"));
        assert!(state.result.is_none(), "a failure has no result to offer");
    }

    #[test]
    fn cancelling_something_that_is_not_there_is_not_an_error() {
        assert!(!Tasks::default().cancel("t999999"));
    }

    #[tokio::test]
    async fn old_tasks_are_forgotten_and_running_ones_never_are() {
        let tasks = Tasks::default();
        let long = tasks.start("import", "Reading…").await.unwrap();

        for _ in 0..(KEEP_FINISHED + 20) {
            let t = tasks.start("backup", "Copying…").await.unwrap();
            tasks.finish(&t, serde_json::json!({})).await;
        }

        let all = tasks.list();
        let finished = all.iter().filter(|t| t.phase != Phase::Running).count();
        assert_eq!(finished, KEEP_FINISHED, "finished tasks accumulate: {finished}");
        assert!(
            tasks.get(&long.snapshot().id).is_some(),
            "a job still going was forgotten while it was going"
        );
    }

    #[tokio::test]
    async fn the_newest_task_is_first() {
        let tasks = Tasks::default();
        let first = tasks.start("backup", "one").await.unwrap();
        let second = tasks.start("export", "two").await.unwrap();
        let listed = tasks.list();
        assert_eq!(listed[0].id, second.snapshot().id);
        assert_eq!(listed[1].id, first.snapshot().id);
    }
}
