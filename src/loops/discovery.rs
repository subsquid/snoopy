//! Background loop that periodically scans ClickHouse for suspicious query hashes
//! and creates ZK fraud proofs automatically.

use crate::{
    contracts::{filter_eligible_queries, get_assignment_id_map},
    db::{find_odds_in_siblings, get_siblings_queries_by_investigate_row, get_suspicious_hashes, get_signatures, investigate_hash},
    mpt::{make_mpt_proof, populate_trie},
    state::InternalState,
    types::PrivateProofData,
    zk::{build_zk_proof, make_proof_data},
};
use clickhouse::Client;
use eth_trie::{EthTrie, MemoryDB, Trie};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Instant, UNIX_EPOCH},
};
use tracing::{error, info};

const NUMBER_OF_EVIDENCES_IN_ZK_PROOF: usize = 5;

pub fn start_discovery_loop(state: &InternalState) {
    let local_config = state.config.clone();
    let local_proof_storage = Arc::clone(&state.proof_storage);
    tokio::spawn(async move {
        loop {
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

            let range_end_sec = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32;
            let range_start_sec = range_end_sec - 24 * 3600 * 5;
            let start = Instant::now();

            let suspicious_hashes =
                match get_suspicious_hashes(&client, range_start_sec, range_end_sec).await {
                    Ok(hashes) => hashes,
                    Err(err) => {
                        error!("Got error while searching for suspicious hashes: {err:?}");
                        continue;
                    }
                };
            info!("Sus: {suspicious_hashes:?}");

            let res =
                match investigate_hash(&client, range_start_sec, range_end_sec, suspicious_hashes)
                    .await
                {
                    Ok(rows) => rows,
                    Err(err) => {
                        error!("Got error while investigating suspicious hashes: {err:?}");
                        continue;
                    }
                };
            for row in &res {
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
                        error!(
                            "Got error while searching for siblings for hash {:?}: {err:?}",
                            row.hash
                        );
                        continue;
                    }
                };
                info!("Siblings!: {:?}", siblings.len());
                let odds = match find_odds_in_siblings(&siblings) {
                    Ok(odds) => odds,
                    Err(err) => {
                        error!("Got error while finding oddities: {err:?}");
                        continue;
                    }
                };
                info!("Odd query id is: {odds:?}");
                for query_id in odds {
                    // Skip proof creation if a proof already exists for this query_id
                    {
                        let storage = local_proof_storage.lock().unwrap();
                        if storage.exists(&query_id) {
                            info!(
                                "Proof already exists for query_id {query_id}, skipping proof creation"
                            );
                            continue;
                        }
                    }

                    let assignment_id_map =
                        match get_assignment_id_map(&siblings, &rpc_url, commiter_address).await {
                            Ok(map) => map,
                            Err(err) => {
                                error!("Got {err:?} while quering contract");
                                continue;
                            }
                        };
                    let eligible_queries =
                        filter_eligible_queries(&siblings, &assignment_id_map, &query_id);
                    info!("Eligible queries: {:?}", eligible_queries.len());
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
                            error!("Got {err:?} while getting signatures");
                            continue;
                        }
                    };

                    if eligible_queries.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                        || signatures.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF
                    {
                        error!("Not enough evidence to create fraud proof");
                        continue;
                    };

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
                                error!("Failed to build MPT for {assignment_id}: {err}");
                                continue;
                            }
                        };
                        let tree_root = match trie.root_hash() {
                            Ok(root) => root.to_vec(),
                            Err(err) => {
                                error!(
                                    "Failed to calculate MPT root for {assignment_id}: {err}"
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
                                error!(
                                    "Failed to calculate MPT proof for {proof_row:?}: {err}"
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
                                error!(
                                    "Failed to generate proof data for {proof_row:?}: {err}"
                                );
                                continue;
                            }
                        };
                        used_keys.insert(proof_row.worker_id.clone());
                        proof_data_list.push(proof);
                    }

                    if proof_data_list.len() < NUMBER_OF_EVIDENCES_IN_ZK_PROOF {
                        error!("Could not assemble enough proof data entries");
                        continue;
                    }

                    let proof_result = if local_config.fake_proof {
                        info!(
                            "fake_proof enabled: generating random proof bytes for query_id {query_id}"
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
                            info!(
                                "Stored proof for query_id {query_id} in global proof storage"
                            );
                        }
                        Err(err) => {
                            error!("Failed to build ZK proof for query_id {query_id}: {err:?}");
                        }
                    }
                }
            }
            info!("Spun in {:?}", start.elapsed());
        }
    });
}
