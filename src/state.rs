use crate::{proof_storage::ProofStorage, types::{Args, DiscoveryLoopProgress}};
use std::sync::{Arc, Mutex};

/// Rocket-managed shared state.
pub struct InternalState {
    pub proof_storage: Arc<Mutex<ProofStorage>>,
    pub discovery_progress: Arc<Mutex<DiscoveryLoopProgress>>,
    pub config: Args,
}
