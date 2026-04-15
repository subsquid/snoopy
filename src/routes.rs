//! Rocket HTTP route handlers.

use crate::{
    state::InternalState,
    types::{DiscoveryLoopProgress, Metadata, ProofEntry},
};
use rocket::{State, get, serde::json::Json, fs::NamedFile};

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

#[get("/discovery-progress")]
pub async fn get_discovery_progress(
    state: &State<InternalState>,
) -> Json<DiscoveryLoopProgress> {
    let progress = state.discovery_progress.lock().unwrap();
    Json(progress.clone())
}
