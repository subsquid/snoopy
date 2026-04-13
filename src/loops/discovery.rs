//! Background loop that periodically scans ClickHouse for suspicious query hashes
//! and creates ZK fraud proofs automatically.

use crate::{
    contracts::{filter_eligible_queries, get_assignment_id_map},
    db::{find_odds_in_siblings, get_siblings_queries_by_investigate_row, get_suspicious_hashes, get_signatures, investigate_hash},
    mpt::{make_mpt_proof, populate_trie},
    state::InternalState,
    types::{DiscoveryEvent, DiscoveryLoopProgress, PrivateProofData},
    zk::{build_zk_proof, make_proof_data},
};
use clickhouse::Client;
use eth_trie::{EthTrie, MemoryDB, Trie};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{error, info};

const NUMBER_OF_EVIDENCES_IN_ZK_PROOF: usize = 5;

// ---------------------------------------------------------------------------
// Stage definitions
//
// Every iteration of the discovery loop passes through these 8 sequential
// high-level stages.  The constant is `pub` so that `types::DiscoveryLoopProgress`
// can embed it in the `max_stages` field without a circular dependency.
// ---------------------------------------------------------------------------

/// Stage 1 – Fetch suspicious hashes from ClickHouse.
pub const STAGE_FETCH_SUSPICIOUS: u8 = 1;
/// Stage 2 – Investigate suspicious hashes (produce investigation rows).
pub const STAGE_INVESTIGATE: u8 = 2;
/// Stage 3 – Fetch sibling queries for each investigation row.
pub const STAGE_FETCH_SIBLINGS: u8 = 3;
/// Stage 4 – Find oddities (divergent query_ids) among siblings.
pub const STAGE_FIND_ODDS: u8 = 4;
/// Stage 5 – Resolve assignment-id map from the on-chain contract.
pub const STAGE_ASSIGNMENT_MAP: u8 = 5;
/// Stage 6 – Fetch worker signatures from ClickHouse.
pub const STAGE_FETCH_SIGNATURES: u8 = 6;
/// Stage 7 – Assemble proof data entries (MPT proofs + proof data structs).
pub const STAGE_ASSEMBLE_PROOF_DATA: u8 = 7;
/// Stage 8 – Build and store the ZK proof.
pub const STAGE_BUILD_ZK_PROOF: u8 = 8;

/// Total number of distinct stages – used by the progress-bar in the Web UI.
pub const DISCOVERY_MAX_STAGES: u8 = 8;

// ---------------------------------------------------------------------------
// Helper: current unix timestamp in seconds
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Helper: advance `current_stage` and emit an Info event with the stage name
// ---------------------------------------------------------------------------

fn push_stage(
    progress: &Arc<Mutex<DiscoveryLoopProgress>>,
    stage: u8,
    label: impl Into<String>,
) {
    let msg = label.into();
    info!("[stage {stage}/{DISCOVERY_MAX_STAGES}] {msg}");
    let mut p = progress.lock().unwrap();
    p.current_stage = stage;
    p.events.push(DiscoveryEvent::Info {
        level: 0,
        message: format!("[stage {stage}/{DISCOVERY_MAX_STAGES}] {msg}"),
        ts: now_secs(),
    });
}

// ---------------------------------------------------------------------------
// Helper: push an Info event and also emit a tracing log
// ---------------------------------------------------------------------------

fn push_info(
    progress: &Arc<Mutex<DiscoveryLoopProgress>>,
    level: u8,
    message: impl Into<String>,
) {
    let msg = message.into();
    info!("{msg}");
    let mut p = progress.lock().unwrap();
    p.events.push(DiscoveryEvent::Info {
        level,
        message: msg,
        ts: now_secs(),
    });
}

// ---------------------------------------------------------------------------
// Helper: push an Error event and also emit a tracing log
// ---------------------------------------------------------------------------

fn push_error(
    progress: &Arc<Mutex<DiscoveryLoopProgress>>,
    level: u8,
    message: impl Into<String>,
) {
    let msg = message.into();
    error!("{msg}");
    let mut p = progress.lock().unwrap();
    p.events.push(DiscoveryEvent::Error {
        level,
        message: msg,
        ts: now_secs(),
    });
}

// ---------------------------------------------------------------------------
// Loop entry point
// ---------------------------------------------------------------------------

pub fn start_discovery_loop(state: &InternalState) {
    let local_config = state.config.clone();
    let local_proof_storage = Arc::clone(&state.proof_storage);
    let local_progress = Arc::clone(&state.discovery_progress);

    tokio::spawn(async move {
        loop {
            // ----------------------------------------------------------------
            // Start of a new iteration: reset events, current_stage; bump counter.
            // ----------------------------------------------------------------
            {
                let mut p = local_progress.lock().unwrap();
                p.iteration += 1;
                p.iteration_started_at = now_secs();
                p.current_stage = 0;
                p.events.clear();
            }

            let db_url = local_config.db_url.clone();
            let db_database = local_config.db_database.clone();
            let db_user = local_config.db_user.clone();
            let db_password = local_config.db_password.clone();
            let rpc_url = local_config.rpc_url.clone();
            let commiter_address = local_config.commiter_address;

            let client = Client::default()
                .with_url(db_url)
                .with_database(db_database)
                .with_user(db_user)
                .with_password(db_password)
                .with_option("max_execution_time", "240");

            let range_end_sec = now_secs() as u32;
            let range_start_sec = range_end_sec - 24 * 3600 * 5;
            let start = Instant::now();

            // ----------------------------------------------------------------
            // Stage 1: Fetch suspicious hashes
            // ----------------------------------------------------------------
            push_stage(&local_progress, STAGE_FETCH_SUSPICIOUS, "Fetching suspicious hashes");
            let suspicious_hashes =
                match get_suspicious_hashes(&client, range_start_sec, range_end_sec).await {
                    Ok(hashes) => hashes,
                    Err(err) => {
                        push_error(
                            &local_progress,
                            0,
                            format!("Got error while searching for suspicious hashes: {err:?}"),
                        );
                        continue;
                    }
                };
            push_info(
                &local_progress,
                0,
                format!("Suspicious hashes found: {suspicious_hashes:?}"),
            );

            // ----------------------------------------------------------------
            // Stage 2: Investigate suspicious hashes
            // ----------------------------------------------------------------
            push_stage(&local_progress, STAGE_INVESTIGATE, "Investigating suspicious hashes");
            let res =
                match investigate_hash(&client, range_start_sec, range_end_sec, suspicious_hashes)
                    .await
                {
                    Ok(rows) => rows,
                    Err(err) => {
                        push_error(
                            &local_progress,
                            0,
                            format!("Got error while investigating suspicious hashes: {err:?}"),
                        );
                        continue;
                    }
                };
            push_info(
                &local_progress,
                0,
                format!("Investigation produced {} row(s)", res.len()),
            );

            // ----------------------------------------------------------------
            // Per investigation row
            // ----------------------------------------------------------------
            for row in &res {
                // Stage 3: Fetch siblings --------------------------------
                push_stage(
                    &local_progress,
                    STAGE_FETCH_SIBLINGS,
                    format!("Fetching siblings for hash {:?}", row.hash),
                );
                let siblings = match get_siblings_queries_by_investigate_row(
                    &client,
                    range_start_sec,
                    range_end_sec,
                    row,
                )
                .await
                {
                    Ok(siblings) => siblings,
                    Err(err) => {
                        push_error(
                            &local_progress,
                            1,
                            format!(
                                "Got error while searching for siblings for hash {:?}: {err:?}",
                                row.hash
                            ),
                        );
                        continue;
                    }
                };
                push_info(
                    &local_progress,
                    1,
                    format!("Hash {:?}: found {} sibling(s)", row.hash, siblings.len()),
                );

                // Stage 4: Find oddities ---------------------------------
                push_stage(
                    &local_progress,
                    STAGE_FIND_ODDS,
                    format!("Finding oddities for hash {:?}", row.hash),
                );
                let odds = match find_odds_in_siblings(&siblings) {
                    Ok(odds) => odds,
                    Err(err) => {
                        push_error(
                            &local_progress,
                            1,
                            format!("Got error while finding oddities: {err:?}"),
                        );
                        continue;
                    }
                };
                push_info(
                    &local_progress,
                    1,
                    format!("Odd query id(s): {odds:?}"),
                );

                // --------------------------------------------------------
                // Per oddity (query_id)
                // --------------------------------------------------------
                for query_id in odds {
                    // Skip proof creation if a proof already exists for this query_id
                    {
                        let storage = local_proof_storage.lock().unwrap();
                        if storage.exists(&query_id) {
                            push_info(
                                &local_progress,
                                2,
                                format!(
                                    "Proof already exists for query_id {query_id}, skipping"
                                ),
                            );
                            continue;
                        }
                    }

                    // Stage 5: Assignment-id map -------------------------
                    push_stage(
                        &local_progress,
                        STAGE_ASSIGNMENT_MAP,
                        format!("Resolving assignment-id map for query_id {query_id}"),
                    );
                    let assignment_id_map =
                        match get_assignment_id_map(&siblings, &rpc_url, commiter_address).await {
                            Ok(map) => map,
                            Err(err) => {
                                push_error(
                                    &local_progress,
                                    2,
                                    format!(
                                        "query_id {query_id}: got {err:?} while querying contract"
                                    ),
                                );
                                continue;
                            }
                        };

                    let eligible_queries =
                        filter_eligible_queries(&siblings, &assignment_id_map, &query_id);
                    push_info(
                        &local_progress,
                        2,
                        format!(
                            "query_id {query_id}: {} eligible query/ies",
                            eligible_queries.len()
                        ),
                    );

                    // Stage 6: Fetch signatures ---------------------------
                    push_stage(
                        &local_progress,
                        STAGE_FETCH_SIGNATURES,
                        format!("Fetching signatures for query_id {query_id}"),
                    );
                    let signatures = match get_signatures(
                        &client,
                        range_start_sec,
                        range_end_sec,
                        &eligible_queries,
                        &query_id,
                    )
                    .await
                    {
                        Ok(signatures) => signatures,
                        Err(err) => {
                            push_error(
                                &local_progress,
                                2,
                                format!(
                                    "query_id {query_id}: got {err:?} while getting signatures"
                                ),
                            );
                            continue;
                        }
                    };

                    if eligible_queries.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                        || signatures.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                    {
                        push_error(
                            &local_progress,
                            2,
                            format!(
                                "query_id {query_id}: not enough evidence \
                                 (eligible={}, signatures={})",
                                eligible_queries.len(),
                                signatures.len()
                            ),
                        );
                        continue;
                    };

                    // Stage 7: Assemble proof data entries ----------------
                    push_stage(
                        &local_progress,
                        STAGE_ASSEMBLE_PROOF_DATA,
                        format!("Assembling proof data entries for query_id {query_id}"),
                    );
                    let program_path = local_config.program_path.clone();
                    let mut used_keys: HashSet<String> = Default::default();
                    let mut proof_data_list: Vec<PrivateProofData> = Default::default();

                    for proof_row in &eligible_queries {
                        if proof_data_list.len() >= NUMBER_OF_EVIDENCES_IN_ZK_PROOF {
                            break;
                        }
                        if used_keys.contains(&proof_row.worker_id) {
                            continue;
                        }
                        let (result_hash, worker_signature) =
                            match signatures.get(&proof_row.query_id) {
                                Some(res) => res,
                                None => continue,
                            };
                        let db = Arc::new(MemoryDB::new(true));
                        let mut trie = EthTrie::new(db);
                        let assignment_id = match assignment_id_map.get(&proof_row.query_id) {
                            Some(v) => v,
                            None => continue,
                        };
                        let network = local_config.network.clone();
                        let assignment_url = format!(
                            "https://metadata.sqd-datasets.io/assignments/{network}/{assignment_id}.fb.1.gz"
                        );
                        match populate_trie(assignment_url, &mut trie).await {
                            Ok(_) => {}
                            Err(err) => {
                                push_error(
                                    &local_progress,
                                    3,
                                    format!(
                                        "query_id {query_id}: failed to build MPT for \
                                         {assignment_id}: {err}"
                                    ),
                                );
                                continue;
                            }
                        };
                        let tree_root = match trie.root_hash() {
                            Ok(root) => root.to_vec(),
                            Err(err) => {
                                push_error(
                                    &local_progress,
                                    3,
                                    format!(
                                        "query_id {query_id}: failed to calculate MPT root for \
                                         {assignment_id}: {err}"
                                    ),
                                );
                                continue;
                            }
                        };
                        let mpt_proof = match make_mpt_proof(
                            &mut trie,
                            &proof_row.dataset_id,
                            &proof_row.chunk_id,
                            &proof_row.worker_id,
                        ) {
                            Ok(p) => p,
                            Err(err) => {
                                push_error(
                                    &local_progress,
                                    3,
                                    format!(
                                        "query_id {query_id}: failed to calculate MPT proof \
                                         for {proof_row:?}: {err}"
                                    ),
                                );
                                continue;
                            }
                        };
                        let proof = match make_proof_data(
                            proof_row,
                            result_hash,
                            worker_signature,
                            tree_root,
                            mpt_proof,
                        ) {
                            Ok(p) => p,
                            Err(err) => {
                                push_error(
                                    &local_progress,
                                    3,
                                    format!(
                                        "query_id {query_id}: failed to generate proof data \
                                         for {proof_row:?}: {err}"
                                    ),
                                );
                                continue;
                            }
                        };
                        used_keys.insert(proof_row.worker_id.clone());
                        proof_data_list.push(proof);
                    }

                    if proof_data_list.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF {
                        push_error(
                            &local_progress,
                            2,
                            format!(
                                "query_id {query_id}: could not assemble enough proof data \
                                 entries (got {})",
                                proof_data_list.len()
                            ),
                        );
                        continue;
                    }

                    push_info(
                        &local_progress,
                        2,
                        format!(
                            "query_id {query_id}: assembled {} proof data entries",
                            proof_data_list.len()
                        ),
                    );

                    // Stage 8: Build and store ZK proof ------------------
                    push_stage(
                        &local_progress,
                        STAGE_BUILD_ZK_PROOF,
                        format!("Building ZK proof for query_id {query_id}"),
                    );
                    let proof_result = if local_config.fake_proof {
                        push_info(
                            &local_progress,
                            2,
                            format!(
                                "fake_proof enabled: generating random proof bytes \
                                 for query_id {query_id}"
                            ),
                        );
                        let proof_bytes: Vec<u8> =
                            (0..128).map(|_x: u8| rand::random::<u8>()).collect();
                        let public_values: Vec<u8> =
                            (0..64).map(|_x: u8| rand::random::<u8>()).collect();
                        Ok((proof_bytes, public_values))
                    } else {
                        build_zk_proof(&proof_data_list, &program_path).await
                    };

                    match proof_result {
                        Ok((proof_bytes, public_values)) => {
                            let mut storage = local_proof_storage.lock().unwrap();
                            storage.add_proof(query_id.clone(), proof_bytes, public_values);
                            push_info(
                                &local_progress,
                                2,
                                format!(
                                    "Stored proof for query_id {query_id} in global proof storage"
                                ),
                            );
                        }
                        Err(err) => {
                            push_error(
                                &local_progress,
                                2,
                                format!(
                                    "Failed to build ZK proof for query_id {query_id}: {err:?}"
                                ),
                            );
                        }
                    }
                }
            }

            push_info(
                &local_progress,
                0,
                format!("Iteration completed in {:?}", start.elapsed()),
            );
        }
    });
}
