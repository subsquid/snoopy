//! Rocket HTTP route handlers and task-state helpers.

use crate::{
    state::InternalState,
    types::{DiscoveryLoopProgress, Metadata, ProofEntry, Task, TaskDescription, TaskStatus},
};
use rocket::{State, get, post, serde::json::Json, fs::NamedFile};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Task state helpers
// ---------------------------------------------------------------------------

pub fn set_task_status(
    tasks: &Arc<Mutex<HashMap<Uuid, Task>>>,
    task_id: Uuid,
    status: TaskStatus,
    comment: Option<String>,
) {
    let mut tasks_lock = tasks.lock().unwrap();
    let task = tasks_lock.get_mut(&task_id).unwrap();
    task.status = status;
    task.comment = comment;
}

pub fn set_task_status_with_proof(
    tasks: &Arc<Mutex<HashMap<Uuid, Task>>>,
    task_id: Uuid,
    status: TaskStatus,
    comment: Option<String>,
    proof_bytes: Option<Vec<u8>>,
    public_values: Option<Vec<u8>>,
) {
    let mut tasks_lock = tasks.lock().unwrap();
    let task = tasks_lock.get_mut(&task_id).unwrap();
    task.status = status;
    task.comment = comment;
    task.proof_bytes = proof_bytes;
    task.public_values = public_values;
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

#[get("/proofs")]
pub async fn get_all_proofs(state: &State<InternalState>) -> Json<Vec<ProofEntry>> {
    let storage = state.proof_storage.lock().unwrap();
    let entries = storage
        .proofs
        .iter()
        .map(|(query_id, proof)| ProofEntry {
            query_id: query_id.clone(),
            proof_bytes: proof.proof_bytes.clone(),
            public_values: proof.public_values.clone(),
            is_published: proof.is_published,
        })
        .collect();
    Json(entries)
}

#[get("/tasks")]
pub async fn get_all_tasks(state: &State<InternalState>) -> Json<Vec<Task>> {
    let tasks_lock = state.tasks.lock().unwrap();
    let mut tasks: Vec<Task> = tasks_lock.values().cloned().collect();
    tasks.sort_by(|a, b| b.creation_ts.cmp(&a.creation_ts));
    Json(tasks)
}

#[get("/tasks/<task_id>")]
pub async fn get_task_status(task_id: String, state: &State<InternalState>) -> Json<Task> {
    let task_id = Uuid::parse_str(&task_id).unwrap();
    let tasks_lock = state.tasks.lock().unwrap();
    if let Some(task) = tasks_lock.get(&task_id) {
        Json(task.clone())
    } else {
        Json(Task {
            id: task_id,
            query_id: Default::default(),
            ts: 0,
            creation_ts: 0,
            status: TaskStatus::NotFound,
            comment: None,
            proof_bytes: None,
            public_values: None,
        })
    }
}

#[get("/metadata")]
pub async fn get_metadata(state: &State<InternalState>) -> Json<Metadata> {
    let config = &state.config;
    Json(Metadata {
        network: config.network.clone(),
        blockchain_network: config.blockchain_network.clone(),
        rpc_url: config.rpc_url.clone(),
        commiter_address: config.commiter_address.to_string(),
        manager_address: config.manager_address.to_string(),
        config_name: config.config_name.clone(),
    })
}

#[get("/")]
pub async fn index() -> NamedFile {
    NamedFile::open("templates/index.html").await.unwrap()
}

#[get("/styles.css")]
pub async fn styles() -> NamedFile {
    NamedFile::open("static/styles.css").await.unwrap()
}

#[get("/app.js")]
pub async fn app_js() -> NamedFile {
    NamedFile::open("static/app.js").await.unwrap()
}

#[post("/tasks", data = "<task>")]
pub async fn submit_task(
    task: Json<TaskDescription>,
    state: &State<InternalState>,
) -> Json<Uuid> {
    let task_id = Uuid::new_v4();
    let creation_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut tasks_lock = state.tasks.lock().unwrap();
    tasks_lock.insert(
        task_id,
        Task {
            id: task_id,
            query_id: task.query_id.clone(),
            ts: task.ts,
            creation_ts,
            status: TaskStatus::Pending,
            comment: None,
            proof_bytes: None,
            public_values: None,
        },
    );
    Json(task_id)
}

#[get("/discovery-progress")]
pub async fn get_discovery_progress(
    state: &State<InternalState>,
) -> Json<DiscoveryLoopProgress> {
    let progress = state.discovery_progress.lock().unwrap();
    Json(progress.clone())
}
