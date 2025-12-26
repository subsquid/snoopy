use alloy::{
    hex,
    primitives::{Address, Uint},
    providers::{ProviderBuilder, WsConnect},
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::anyhow;
use clickhouse::{Client, Row};
use eth_trie::{EthTrie, MemoryDB, Trie};
use flate2::read::GzDecoder;
use libp2p_identity::PeerId;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use sp1_sdk::{HashableKey, Prover, ProverClient, SP1Stdin};
use sqd_assignments::Assignment;
pub use sqd_messages::query_finished::Result as QueryFinishedResult;
use sqd_messages::{Query, QueryFinished, QueryOkSummary, Range};
use std::{cmp::Ordering, collections::HashMap, fs::File, io::Read, str::FromStr};
use tiny_keccak::{Hasher, Keccak};
use tracing::{debug, info};

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    CommitmentHolder,
    "abi/CommitmentHolder.json"
);

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    ProvingManager,
    "abi/ProvingManager.json"
);

pub async fn populate_trie(
    assignment_url: String,
    trie: &mut EthTrie<MemoryDB>,
) -> Result<(), anyhow::Error> {
    let buf = &mut Default::default();
    if assignment_url.starts_with("http") {
        let response_assignment = reqwest::get(assignment_url).await?;
        let compressed_assignment = response_assignment.bytes().await?;
        let mut decoder = GzDecoder::new(&compressed_assignment[..]);
        decoder.read_to_end(buf)?;
    } else {
        let file = File::open(assignment_url)?;
        let mut decoder = GzDecoder::new(file);
        decoder.read_to_end(buf)?;
    }

    let assignment = Assignment::from_owned_unchecked(buf.to_vec());

    let workers = assignment
        .workers()
        .iter()
        .map(|worker| bs58::encode(&worker.worker_id().0).into_string())
        .collect::<Vec<_>>();

    for dataset in assignment.datasets() {
        let prefix = &dataset.id();
        for chunk in dataset.chunks() {
            let id = &chunk.id();
            let key = format!("{prefix}|{id}");
            let mut workers = chunk
                .worker_indexes()
                .iter()
                .map(|idx| workers[idx as usize].clone())
                .collect::<Vec<String>>();
            workers.sort();
            let val = workers.join("|");

            let mut keccak = Keccak::v256();
            keccak.update(key.as_bytes());
            let mut bytes = [0u8; 32];
            keccak.finalize(&mut bytes);

            trie.insert(&bytes, val.as_bytes())?;
        }
    }
    Ok(())
}

pub fn make_mpt_proof(
    trie: &mut EthTrie<MemoryDB>,
    dataset_id: &String,
    chunk_id: &String,
    worker_id: &String,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let key = format!("{dataset_id}|{chunk_id}");
    let mut keccak = Keccak::v256();
    keccak.update(key.as_bytes());
    let mut bytes = [0u8; 32];
    keccak.finalize(&mut bytes);
    let trie_key = &bytes[0..8];
    let mpt_proof = trie.get_proof(trie_key)?;
    let leaf = mpt_proof.last().ok_or(anyhow!("Empty leaf in proof"))?;
    let rlp_leaf: rlp::Rlp<'_> = rlp::Rlp::new(leaf);
    let payload: String = rlp_leaf.val_at(1)?;
    let parts = payload.split("|").map(|v| v.to_owned()).collect::<Vec<_>>();
    if parts.contains(worker_id) {
        Ok(mpt_proof)
    } else {
        Err(anyhow!("Wrong assignment"))
    }
}

#[derive(Row, Debug, Clone, Serialize, Deserialize)]
pub struct QueryFinishedRow {
    query_id: String,
    worker_id: String,
    #[serde(with = "serde_bytes")]
    result_hash: Vec<u8>,
    #[serde(with = "serde_bytes")]
    worker_signature: Vec<u8>,
    total_time: u32,
    collector_timestamp: u64,
}

#[derive(Debug, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
enum QueryResult {
    Ok = 1,
    BadRequest = 2,
    ServerError = 3,
    NotFound = 4,
    ServerOverloaded = 5,
    TooManyRequests = 6,
}

#[derive(Row, Debug, Clone, Serialize, Deserialize)]
pub struct QueryExecutedRow {
    pub query_id: String,
    client_id: String,
    pub worker_id: String,
    pub dataset_id: String,
    from_block: Option<u64>,
    to_block: Option<u64>,
    pub chunk_id: String,
    query: String,
    #[serde(with = "serde_bytes")]
    query_hash: Vec<u8>,
    result: QueryResult,
    #[serde(with = "serde_bytes")]
    output_hash: Vec<u8>,
    last_block: Option<u64>,
    error_msg: String,
    #[serde(with = "serde_bytes")]
    client_signature: Vec<u8>,
    pub client_timestamp: u64,
    request_id: String,
}

#[derive(Row, Debug, Clone, Serialize, Deserialize)]
pub struct SignatureRow {
    query_id: String,
    #[serde(with = "serde_bytes")]
    worker_signature: Vec<u8>,
    #[serde(with = "serde_bytes")]
    result_hash: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct PrivateProofData {
    pub query: Query,
    pub query_result: QueryFinished,
    pub mpt_proof: Vec<Vec<u8>>,
    pub worker_id: String,
    pub client_id: String,
    pub tree_root: Vec<u8>,
}

pub async fn get_siblings_queries(
    client: &Client,
    query_id: &str,
    ts: u64,
    ts_tolerance: u64,
    ts_search_range: u64,
) -> Result<Vec<QueryExecutedRow>, anyhow::Error> {
    info!(
        "Params: {} {} {}",
        query_id,
        ts - ts_tolerance,
        ts + ts_tolerance
    );
    let original_query = client
        .query("select query_id, client_id, worker_id, dataset_id, from_block, to_block, chunk_id, query, query_hash, result, output_hash, last_block, error_msg, client_signature, client_timestamp, request_id from worker_query_logs where worker_timestamp > ? AND worker_timestamp < ? AND query_id = ?")
        .bind((ts - ts_tolerance) as u32)
        .bind((ts + ts_tolerance) as u32)
        .bind(query_id)
        .fetch_one::<QueryExecutedRow>()
        .await?;
    info!(
        "Found query hash: {:?}",
        original_query
            .query_hash
            .iter()
            .map(|v| format!("{v:02X}"))
            .collect::<Vec<_>>()
            .join("")
    );

    let mut sibling_queries = client
        .query("select query_id, client_id, worker_id, dataset_id, from_block, to_block, chunk_id, query, query_hash, result, output_hash, last_block, error_msg, client_signature, client_timestamp, request_id from worker_query_logs where worker_timestamp > ? AND worker_timestamp < ? AND hex(query_hash) = ? AND from_block = ? AND to_block = ? AND result = 'ok'")
        .bind(ts - ts_search_range)
        .bind(ts + ts_search_range)
        .bind(original_query.query_hash.iter().map(|v| format!("{v:02X}")).collect::<Vec<_>>().join(""))
        .bind(original_query.from_block)
        .bind(original_query.to_block)
        .fetch_all::<QueryExecutedRow>()
        .await?;
    info!("Found {:?} queries with same hash", sibling_queries.len());

    sibling_queries.sort_by(|a, b| a.query_id.cmp(&b.query_id));
    sibling_queries.dedup_by(|a, b| a.query_id == b.query_id);

    info!(
        "After filtering got {:?} unique queries",
        sibling_queries.len()
    );

    Ok(sibling_queries)
}

pub async fn get_assignment_id_map(
    sibling_queries: &Vec<QueryExecutedRow>,
    rpc_url: &str,
    commiter_address: Address,
) -> Result<HashMap<String, String>, anyhow::Error> {
    let ws = WsConnect::new(rpc_url);
    let provider = ProviderBuilder::new().connect_ws(ws.clone()).await?;
    let mut assignment_id_map = HashMap::<String, String>::new();
    let commiter = CommitmentHolder::new(commiter_address, provider.clone());
    for row in sibling_queries {
        let ts = row.client_timestamp / 1000;
        let ts: [u64; 4] = [ts, 0, 0, 0];
        let ts = Uint::<256, 4>::from_limbs(ts);
        let res = commiter.get_id_by_timestamp(ts).call().await?;
        if !res.is_empty() {
            assignment_id_map.insert(row.query_id.clone(), res);
        }
    }
    Ok(assignment_id_map)
}

pub fn filter_eligible_queries(
    sibling_queries: &[QueryExecutedRow],
    assignment_id_map: &HashMap<String, String>,
    query_id: &str,
) -> Vec<QueryExecutedRow> {
    let mut eligible_queries = sibling_queries
        .iter()
        .filter(|row| assignment_id_map.contains_key(&row.query_id))
        .cloned()
        .collect::<Vec<_>>();
    info!("Found {:?} eligible queries", eligible_queries.len());
    eligible_queries.sort_by(|a, b| {
        if a.query_id == query_id {
            return Ordering::Less;
        }
        if b.query_id == query_id {
            return Ordering::Greater;
        }
        b.client_timestamp.cmp(&a.client_timestamp)
    });
    eligible_queries
}

pub async fn get_signatures(
    client: &Client,
    ts: u64,
    ts_search_range: u64,
    eligible_queries: &[QueryExecutedRow],
    original_query_id: &str,
) -> Result<HashMap<String, (Vec<u8>, Vec<u8>)>, anyhow::Error> {
    let signatures = client
        .query("select query_id, worker_signature, result_hash from portal_logs where collector_timestamp > ? AND collector_timestamp < ? AND query_id IN ?")
        .bind(ts - ts_search_range)
        .bind(ts + ts_search_range)
        .bind(eligible_queries.iter().map(|row| row.query_id.clone()).collect::<Vec<_>>())
        .fetch_all::<SignatureRow>()
        .await?;
    debug!("Signature rows: {signatures:?}");
    let mut result_count: HashMap<Vec<u8>, usize> = HashMap::new();
    for row in &signatures {
        *result_count.entry(row.result_hash.clone()).or_default() += 1;
    }

    let plurality = result_count
        .iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .ok_or(anyhow!("Plurality not found"))?;
    info!(
        "Most frequent hash: {:?} ({:?}/{:?})",
        plurality
            .iter()
            .map(|v| format!("{v:02X}"))
            .collect::<Vec<_>>()
            .join(""),
        result_count.get(plurality),
        signatures.len()
    );

    Ok(signatures
        .into_iter()
        .filter(|row| row.result_hash == *plurality || row.query_id == original_query_id)
        .map(|row| (row.query_id, (row.result_hash, row.worker_signature)))
        .collect::<HashMap<String, (Vec<u8>, Vec<u8>)>>())
}

pub async fn post_proof(
    proof_bytes: Vec<u8>,
    public_values: Vec<u8>,
    rpc_url: &str,
    signer: PrivateKeySigner,
    manager_address: Address,
    config_name: &str,
) -> Result<Vec<u8>, anyhow::Error> {
    let ws = WsConnect::new(rpc_url);
    let wallet_provider = ProviderBuilder::new().wallet(signer).connect_ws(ws).await?;
    let prover = ProvingManager::new(manager_address, wallet_provider.clone());
    let pending = prover
        .verifyAndEmit(
            config_name.to_owned(),
            public_values.into(),
            proof_bytes.into(),
        )
        .send()
        .await?;
    let res = pending
        .with_required_confirmations(2)
        .with_timeout(Some(std::time::Duration::from_secs(60)))
        .watch()
        .await?;
    Ok(res.to_vec())
}

pub async fn build_zk_proof(
    proofs: &Vec<PrivateProofData>,
    program_path: &str,
) -> Result<(Vec<u8>, Vec<u8>), anyhow::Error> {
    let buf = &mut Default::default();
    let prover_client = ProverClient::builder().network().build();
    File::open(program_path).unwrap().read_to_end(buf)?;
    let (pk, vk) = prover_client.setup(buf);
    let mut stdin = SP1Stdin::new();
    stdin.write(&proofs);
    let proof = prover_client
        .prove(&pk, &stdin)
        .groth16()
        .run_async()
        .await?;

    info!("Verification Key: {}", vk.bytes32().to_string());
    info!(
        "Public Values: {}",
        format!("0x{}", hex::encode(proof.public_values.as_slice()))
    );
    info!(
        "Proof Bytes: {}",
        format!("0x{}", hex::encode(proof.bytes()))
    );

    let public_values = proof.public_values.to_vec();
    let proof_bytes = proof.bytes();
    Ok((proof_bytes, public_values))
}

pub fn make_proof_data(
    row: &QueryExecutedRow,
    result_hash: &[u8],
    worker_signature: &[u8],
    tree_root: Vec<u8>,
    mpt_proof: Vec<Vec<u8>>,
) -> Result<PrivateProofData, anyhow::Error> {
    let query = Query {
        request_id: row.request_id.clone(),
        query_id: row.query_id.to_string(),
        dataset: row.dataset_id.clone(),
        query: row.query.clone(),
        block_range: Some(Range {
            begin: row
                .from_block
                .ok_or(anyhow!("fromBlock not found for query"))?,
            end: row.to_block.ok_or(anyhow!("toBlock not found for query"))?,
        }),
        chunk_id: row.chunk_id.clone(),
        timestamp_ms: row.client_timestamp,
        signature: row.client_signature.clone(),
    };
    let verify = query.verify_signature(
        PeerId::from_str(&row.client_id)?,
        PeerId::from_str(&row.worker_id)?,
    );

    if !verify {
        return Err(anyhow!("Query signature verification failed"));
    }

    let internal_result = QueryOkSummary {
        uncompressed_data_size: 0,
        data_hash: result_hash.to_vec(),
        last_block: row.last_block.unwrap(),
    };

    let query_result = QueryFinished {
        query_id: row.query_id.to_string(),
        result: Some(QueryFinishedResult::Ok(internal_result)),
        worker_id: row.worker_id.clone(),
        total_time_micros: 0,
        worker_signature: worker_signature.to_vec(),
    };
    let verify_result = query_result.verify_signature();

    if !verify_result {
        return Err(anyhow!("Query Result signature verification failed"));
    }

    let proof = PrivateProofData {
        query,
        query_result,
        mpt_proof,
        worker_id: row.worker_id.clone(),
        client_id: row.client_id.clone(),
        tree_root,
    };
    Ok(proof)
}
