use crate::{proof_storage::ProofStorage, types::{Args, Task}};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// Rocket-managed shared state.
pub struct InternalState {
    pub tasks: Arc<Mutex<HashMap<Uuid, Task>>>,
    pub proof_storage: Arc<Mutex<ProofStorage>>,
    pub config: Args,
}
