//! On-chain contract interactions: ABI bindings, `get_assignment_id_map`,
//! `filter_eligible_queries`, and `post_proof`.

use crate::types::QueryExecutedRow;
use alloy::{
    primitives::{Address, Uint},
    providers::{ProviderBuilder, WsConnect},
    signers::local::PrivateKeySigner,
    sol,
};
use std::{cmp::Ordering, collections::HashMap};
use tracing::info;

// ---------------------------------------------------------------------------
// ABI code-gen
// ---------------------------------------------------------------------------

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    CommitmentHolder,
    "abi/CommitmentHolder.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    ProvingManager,
    "abi/ProvingManager.json"
);

// ---------------------------------------------------------------------------
// Contract helpers
// ---------------------------------------------------------------------------

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
