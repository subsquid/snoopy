//! Background loop that picks up pending tasks and builds ZK proofs for them.

use crate::{
    contracts::{filter_eligible_queries, get_assignment_id_map},
    db::{get_signatures, get_siblings_queries},
    mpt::{make_mpt_proof, populate_trie},
    routes::{set_task_status, set_task_status_with_proof},
    state::InternalState,
    types::{PrivateProofData, TaskStatus},
    zk::{build_zk_proof, make_proof_data},
};
use clickhouse::Client;
use eth_trie::{EthTrie, MemoryDB, Trie};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;
use tracing::{error, info};
use uuid::Uuid;

const NUMBER_OF_EVIDENCES_IN_ZK_PROOF: usize = 5;

pub fn start_run_loop(state: &InternalState) {
    let local_tasks = Arc::clone(&state.tasks);
    let local_proof_storage = Arc::clone(&state.proof_storage);
    let local_config = state.config.clone();
    tokio::spawn(async move {
        loop {
            let mut task_option = None;
            {
                let tasks_lock: std::sync::MutexGuard<'_, HashMap<Uuid, _>> =
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
            let ts = task.ts as u32;
            let rpc_url = local_config.rpc_url.clone();
            let commiter_address = local_config.commiter_address;
            let ts_tolerance = local_config.ts_tolerance as u32;
            let ts_search_range = local_config.ts_search_range as u32;
            let network = local_config.network.clone();
            let program_path = local_config.program_path.clone();

            let client = Client::default()
                .with_url(db_url)
                .with_database(db_database)
                .with_user(db_user)
                .with_password(db_password);

            let sibling_queries = match get_siblings_queries(
                &client,
                &query_id,
                ts,
                ts_tolerance,
                ts_search_range,
            )
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

            let signatures = match get_signatures(
                &client,
                ts - ts_search_range,
                ts + ts_search_range,
                &eligible_queries,
                &query_id,
            )
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

                let db = Arc::new(MemoryDB::new(true));
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

            let (proof_bytes, public_values) = if local_config.fake_proof {
                info!("fake_proof enabled: generating random proof bytes for task {task_id}");
                let proof_bytes: Vec<u8> = (0..128).map(|_x: u8| rand::random::<u8>()).collect();
                let public_values: Vec<u8> = (0..64).map(|_x: u8| rand::random::<u8>()).collect();
                (proof_bytes, public_values)
            } else {
                match build_zk_proof(&proofs, &program_path).await {
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
                }
            };

            // Store proof in global proof storage
            {
                let mut storage = local_proof_storage.lock().unwrap();
                storage.add_proof(query_id.clone(), proof_bytes.clone(), public_values.clone());
                info!("Stored proof for query_id {query_id} in global proof storage");
            }

            set_task_status_with_proof(
                &local_tasks,
                task_id,
                TaskStatus::Completed,
                Some("Got zk proof".to_owned()),
                Some(proof_bytes.clone()),
                Some(public_values.clone()),
            );
        }
    });
}
