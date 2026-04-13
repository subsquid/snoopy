use alloy::primitives::Address;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use sqd_messages::{Query, QueryFinished};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
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

    #[clap(long, env, default_value = "sepolia")]
    pub blockchain_network: String,

    #[clap(long, env, default_value = "wss://ethereum-sepolia-rpc.publicnode.com")]
    pub rpc_url: String,

    #[clap(
        long,
        env,
        default_value = "0x46025D5d224e423c7B79AACE2c8cf8cf389069aC"
    )]
    pub commiter_address: Address,

    #[clap(
        long,
        env,
        default_value = "0x4d8d508267feB023aB937B3D503feb9Cc89e8Af9"
    )]
    pub manager_address: Address,

    #[clap(long, env, default_value = "std-long")]
    pub config_name: String,

    #[clap(long, env, default_value = "prove-query-result-program")]
    pub program_path: String,

    /// Skip actual ZK proof creation and generate random proof bytes instead.
    #[clap(long, env, default_value = "false")]
    pub fake_proof: bool,
}

// ---------------------------------------------------------------------------
// Task-related types (used by the REST API and the run-loop)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TaskStatus {
    NotFound,
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: Uuid,
    pub query_id: String,
    pub ts: u64,
    pub creation_ts: u64,
    pub status: TaskStatus,
    pub comment: Option<String>,
    pub proof_bytes: Option<Vec<u8>>,
    pub public_values: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub network: String,
    pub blockchain_network: String,
    pub rpc_url: String,
    pub commiter_address: String,
    pub manager_address: String,
    pub config_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Proof {
    pub proof_bytes: Vec<u8>,
    pub public_values: Vec<u8>,
    pub is_published: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProofEntry {
    pub query_id: String,
    pub proof_bytes: Vec<u8>,
    pub public_values: Vec<u8>,
    pub is_published: bool,
}

#[derive(Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct TaskDescription {
    pub query_id: String,
    pub ts: u64,
}

// ---------------------------------------------------------------------------
// ClickHouse row types
// ---------------------------------------------------------------------------

#[derive(clickhouse::Row, serde::Deserialize)]
pub struct HashRow {
    pub hash: String,
}

#[derive(clickhouse::Row, serde::Deserialize, Debug)]
pub struct InvestigationRow {
    pub hash: String,
    pub dataset: String,
    pub chunk_id: String,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub workers: u64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
pub struct QueryIdRow {
    pub query_id: String,
}

#[derive(Debug, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum QueryResult {
    Ok = 1,
    BadRequest = 2,
    ServerError = 3,
    NotFound = 4,
    ServerOverloaded = 5,
    TooManyRequests = 6,
}

#[derive(clickhouse::Row, Debug, Clone, Serialize, Deserialize)]
pub struct QueryFinishedRow {
    pub query_id: String,
    pub worker_id: String,
    #[serde(with = "serde_bytes")]
    pub result_hash: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub worker_signature: Vec<u8>,
    pub total_time: u32,
    pub collector_timestamp: u64,
}

#[derive(clickhouse::Row, Debug, Clone, Serialize, Deserialize)]
pub struct QueryExecutedRow {
    pub query_id: String,
    pub client_id: String,
    pub worker_id: String,
    pub dataset_id: String,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub chunk_id: String,
    pub query: String,
    #[serde(with = "serde_bytes")]
    pub query_hash: Vec<u8>,
    pub result: QueryResult,
    #[serde(with = "serde_bytes")]
    pub output_hash: Vec<u8>,
    pub last_block: Option<u64>,
    pub error_msg: String,
    #[serde(with = "serde_bytes")]
    pub client_signature: Vec<u8>,
    pub client_timestamp: u64,
    pub request_id: String,
}

#[derive(clickhouse::Row, Debug, Clone, Serialize, Deserialize)]
pub struct SignatureRow {
    pub query_id: String,
    #[serde(with = "serde_bytes")]
    pub worker_signature: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub result_hash: Vec<u8>,
}

// ---------------------------------------------------------------------------
// ZK proof input data
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct PrivateProofData {
    pub query: Query,
    pub query_result: QueryFinished,
    pub mpt_proof: Vec<Vec<u8>>,
    pub worker_id: String,
    pub client_id: String,
    pub tree_root: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Discovery-loop progress tracking
// ---------------------------------------------------------------------------

/// A single event emitted during discovery loop execution.
/// `level` indicates nesting depth:
///   0 = top-level (e.g. "found N suspicious hashes"),
///   1 = per investigation-row (e.g. "hash X has Y siblings"),
///   2 = per oddity inside a row (e.g. "odd query_id Z"),
///   3 = per proof-assembly step inside an oddity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    /// Informational message; corresponds to `tracing::info!` calls.
    Info {
        level: u8,
        message: String,
        /// Unix timestamp (seconds) when the event was recorded.
        ts: u64,
    },
    /// Error / setback; corresponds to `tracing::error!` calls.
    Error {
        level: u8,
        message: String,
        ts: u64,
    },
}

/// Accumulated state of the discovery loop that can be queried via HTTP.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscoveryLoopProgress {
    /// Monotonically-increasing counter of completed discovery iterations.
    pub iteration: u64,
    /// Unix timestamp (seconds) of when the current/last iteration started.
    pub iteration_started_at: u64,
    /// All events recorded in the current iteration (cleared at the start of
    /// each new iteration so the list stays bounded).
    pub events: Vec<DiscoveryEvent>,
}
