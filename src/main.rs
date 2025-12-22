#[macro_use]
extern crate rocket;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use alloy::{primitives::Address, signers::local::PrivateKeySigner};
use clap::Parser;
use clickhouse::Client;
use eth_trie::{EthTrie, MemoryDB, Trie};
use rocket::{State, post, serde::json::Json, get, fs::NamedFile};
use serde::{Deserialize, Serialize};
use snoopy::{
    PrivateProofData, build_zk_proof, filter_eligible_queries, get_assignment_id_map,
    get_siblings_queries, get_signatures, make_mpt_proof, make_proof_data, populate_trie,
    post_proof,
};
pub use sqd_messages::query_finished::Result as QueryFinishedResult;
pub use sqd_messages::signatures;
use tokio::time::sleep;
use tracing::{error, info};
use uuid::Uuid;

const NUMBER_OF_EVIDENCES_IN_ZK_PROOF: usize = 5;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[clap(long, env, default_value = "http://localhost:8123")]
    pub db_url: String,

    #[clap(long, env, default_value = "mainnet")]
    pub db_database: String,

    #[clap(long, env, default_value = "subsqd_adm")]
    pub db_user: String,

    #[clap(long, env)]
    pub db_password: String,

    #[clap(long, env, default_value = "300")]
    pub ts_tolerance: u64,

    #[clap(long, env, default_value = "3600")]
    pub ts_search_range: u64,

    #[clap(long, env, default_value = "mainnet")]
    pub network: String,

    #[clap(long, env, default_value = "wss://ethereum-sepolia-rpc.publicnode.com")]
    pub rpc_url: String,

    #[clap(
        long,
        env,
        default_value = "0xD7092928Be395B318cDaeEAE0245b0a66ae357a3"
    )]
    pub commiter_address: Address,

    #[clap(
        long,
        env,
        default_value = "0x9f9d8535e8A2E503E034b142F136ABF3BeCF3CF2"
    )]
    pub manager_address: Address,

    #[clap(long, env, default_value = "std-long")]
    pub config_name: String,

    #[clap(long, env)]
    pub signer: PrivateKeySigner,

    #[clap(long, env, default_value = "prove-query-result-program")]
    pub program_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
enum TaskStatus {
    NotFound,
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Task {
    id: Uuid,
    query_id: String,
    ts: u64,
    status: TaskStatus,
    comment: Option<String>,
}

// Shared state to store tasks
struct InternalState {
    tasks: Arc<Mutex<HashMap<Uuid, Task>>>,
    config: Args,
}

#[get("/tasks")]
async fn get_all_tasks(state: &State<InternalState>) -> Json<Vec<Task>> {
    let tasks_lock = state.tasks.lock().unwrap();
    let tasks: Vec<Task> = tasks_lock.values().cloned().collect();
    Json(tasks)
}

#[get("/tasks/<task_id>")]
async fn get_task_status(task_id: String, state: &State<InternalState>) -> Json<Task> {
    let task_id = Uuid::parse_str(&task_id).unwrap();
    let tasks_lock = state.tasks.lock().unwrap();
    if let Some(task) = tasks_lock.get(&task_id) {
        Json(task.clone())
    } else {
        Json(Task {
            id: task_id,
            query_id: Default::default(),
            ts: 0,
            status: TaskStatus::NotFound,
            comment: None,
        })
    }
}

#[get("/")]
async fn index() -> NamedFile {
    NamedFile::open("templates/index.html").await.unwrap()
}

#[get("/styles.css")]
async fn styles() -> NamedFile {
    NamedFile::open("static/styles.css").await.unwrap()
}

#[get("/app.js")]
async fn app_js() -> NamedFile {
    NamedFile::open("static/app.js").await.unwrap()
}

fn set_task_status(
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

#[derive(Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
struct TaskDescription {
    query_id: String,
    ts: u64,
}

#[post("/tasks", data = "<task>")]
async fn submit_task(task: Json<TaskDescription>, state: &State<InternalState>) -> Json<Uuid> {
    let task_id = Uuid::new_v4();
    let mut tasks_lock = state.tasks.lock().unwrap();
    tasks_lock.insert(
        task_id,
        Task {
            id: task_id,
            query_id: task.query_id.clone(),
            ts: task.ts,
            status: TaskStatus::Pending,
            comment: None,
        },
    );
    Json(task_id)
}

fn run_loop(state: &InternalState) {
    let local_tasks = Arc::clone(&state.tasks);
    let local_config = state.config.clone();
    tokio::spawn(async move {
        loop {
            let mut task_option: Option<Task> = None;
            {
                let tasks_lock: std::sync::MutexGuard<'_, HashMap<Uuid, Task>> =
                    local_tasks.lock().unwrap();
                for (_, value) in tasks_lock.iter() {
                    if value.status == TaskStatus::Pending {
                        task_option = Some(value.clone());
                        break;
                    }
                }
            }

            let task = match task_option {
                Some(task) => task,
                None => {
                    sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            let task_id = task.id;

            set_task_status(&local_tasks, task_id, TaskStatus::Running, None);

            let db_url = local_config.db_url.clone();
            let db_database = local_config.db_database.clone();
            let db_user = local_config.db_user.clone();
            let db_password = local_config.db_password.clone();
            let query_id = task.query_id.clone();
            let ts = task.ts;
            let rpc_url = local_config.rpc_url.clone();
            let commiter_address = local_config.commiter_address;
            let ts_tolerance = local_config.ts_tolerance;
            let ts_search_range = local_config.ts_search_range;
            let manager_address = local_config.manager_address;
            let config_name = local_config.config_name.clone();
            let signer: PrivateKeySigner = local_config.signer.clone();
            let network = local_config.network.clone();
            let program_path = local_config.program_path.clone();

            let client = Client::default()
                .with_url(db_url)
                .with_database(db_database)
                .with_user(db_user)
                .with_password(db_password);

            let sibling_queries =
                match get_siblings_queries(&client, &query_id, ts, ts_tolerance, ts_search_range)
                    .await
                {
                    Ok(siblings) => siblings,
                    Err(err) => {
                        set_task_status(
                            &local_tasks,
                            task_id,
                            TaskStatus::Failed,
                            Some(format!("Got {err:?} while searching for siblings")),
                        );
                        continue;
                    }
                };
            set_task_status(
                &local_tasks,
                task_id,
                TaskStatus::Running,
                Some("Got siblings".to_owned()),
            );

            let assignment_id_map =
                match get_assignment_id_map(&sibling_queries, &rpc_url, commiter_address).await {
                    Ok(map) => map,
                    Err(err) => {
                        set_task_status(
                            &local_tasks,
                            task_id,
                            TaskStatus::Failed,
                            Some(format!("Got {err:?} while quering contract")),
                        );
                        continue;
                    }
                };
            set_task_status(
                &local_tasks,
                task_id,
                TaskStatus::Running,
                Some("Got assignment id map".to_owned()),
            );

            let eligible_queries =
                filter_eligible_queries(&sibling_queries, &assignment_id_map, &query_id);

            let signatures =
                match get_signatures(&client, ts, ts_search_range, &eligible_queries, &query_id)
                    .await
                {
                    Ok(signatures) => signatures,
                    Err(err) => {
                        set_task_status(
                            &local_tasks,
                            task_id,
                            TaskStatus::Failed,
                            Some(format!("Got {err:?} while getting signatures")),
                        );
                        continue;
                    }
                };

            set_task_status(
                &local_tasks,
                task_id,
                TaskStatus::Running,
                Some("Got signatures".to_owned()),
            );

            if eligible_queries.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                || signatures.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
            {
                set_task_status(
                    &local_tasks,
                    task_id,
                    TaskStatus::Failed,
                    Some("Not enough evidence to create fraud proof".to_owned()),
                );
                continue;
            };

            let mut used_keys: HashSet<String> = Default::default();
            let mut proofs: Vec<PrivateProofData> = Default::default();
            for row in &eligible_queries {
                if proofs.len() >= NUMBER_OF_EVIDENCES_IN_ZK_PROOF {
                    break;
                }
                if used_keys.contains(&row.worker_id) {
                    continue;
                }

                let (result_hash, worker_signature) = match signatures.get(&row.query_id) {
                    Some(res) => res,
                    None => continue,
                };
                info!("Trying Query ID: {:?}", row.query_id);

                let db = Arc::new(MemoryDB::new(false));
                let mut trie = EthTrie::new(db);
                let assignment_id = match assignment_id_map.get(&row.query_id) {
                    Some(v) => v,
                    None => continue,
                };
                let assignment_url = format!(
                    "https://metadata.sqd-datasets.io/assignments/{network}/{assignment_id}.fb.1.gz"
                );
                match populate_trie(assignment_url, &mut trie).await {
                    Ok(_) => {}
                    Err(err) => {
                        error!("Failed to build MPT for {assignment_id}: {err}");
                        continue;
                    }
                };
                let tree_root = match trie.root_hash() {
                    Ok(root) => root.to_vec(),
                    Err(err) => {
                        error!("Failed to calculate MPT root for {assignment_id}: {err}");
                        continue;
                    }
                };
                info!(
                    "Assignment commitment: {:?}",
                    tree_root
                        .iter()
                        .map(|v| format!("{v:02x}"))
                        .collect::<Vec<_>>()
                        .join("")
                );
                let mpt_proof =
                    match make_mpt_proof(&mut trie, &row.dataset_id, &row.chunk_id, &row.worker_id)
                    {
                        Ok(proof) => proof,
                        Err(err) => {
                            error!("Failed to calculate MPT proof for {row:?}: {err}");
                            continue;
                        }
                    };

                let proof =
                    match make_proof_data(row, result_hash, worker_signature, tree_root, mpt_proof)
                    {
                        Ok(proof_data) => proof_data,
                        Err(err) => {
                            error!("Failed to generate proof data for {row:?}: {err}");
                            continue;
                        }
                    };

                used_keys.insert(row.worker_id.clone());
                proofs.push(proof);
                set_task_status(
                    &local_tasks,
                    task_id,
                    TaskStatus::Running,
                    Some(format!(
                        "Got proofs {}/{}",
                        proofs.len(),
                        NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                    )),
                );
            }

            let (proof_bytes, public_values) = match build_zk_proof(&proofs, &program_path).await {
                Ok(proof) => proof,
                Err(err) => {
                    set_task_status(
                        &local_tasks,
                        task_id,
                        TaskStatus::Failed,
                        Some(format!("Failed to create zk proof: {err}")),
                    );
                    continue;
                }
            };
            set_task_status(
                &local_tasks,
                task_id,
                TaskStatus::Running,
                Some("Got zk proof".to_owned()),
            );

            let res = match post_proof(
                proof_bytes,
                public_values,
                &rpc_url,
                signer,
                manager_address,
                &config_name,
            )
            .await
            {
                Ok(tx) => tx,
                Err(err) => {
                    set_task_status(
                        &local_tasks,
                        task_id,
                        TaskStatus::Failed,
                        Some(format!("Failed to post proof: {err}")),
                    );
                    continue;
                }
            };

            let tx = res
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<Vec<_>>()
                .join("");

            set_task_status(
                &local_tasks,
                task_id,
                TaskStatus::Completed,
                Some(format!("Transaction: {tx}")),
            );
        }
    });
}

#[rocket::main]
async fn main() -> Result<(), Box<rocket::Error>> {
    sp1_sdk::utils::setup_logger();
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("should be able to install the default crypto provider");
    let args = Args::parse();
    let state = InternalState {
        tasks: Arc::new(Mutex::new(HashMap::new())),
        config: args,
    };
    run_loop(&state);
    let _ = rocket::build()
        .manage(state)
        .mount("/", routes![index, styles, app_js, submit_task, get_task_status, get_all_tasks])
        .launch()
        .await;
    Ok(())
}
