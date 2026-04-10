//! SP1 ZK proof generation: `build_zk_proof` and `make_proof_data`.

use crate::types::{PrivateProofData, QueryExecutedRow};
use alloy::hex;
use anyhow::anyhow;
use libp2p_identity::PeerId;
use sp1_sdk::{HashableKey, Prover, ProverClient, SP1Stdin};
pub use sqd_messages::query_finished::Result as QueryFinishedResult;
use sqd_messages::{Query, QueryFinished, QueryOkSummary, Range};
use std::{fs::File, io::Read, str::FromStr};
use tracing::info;

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
