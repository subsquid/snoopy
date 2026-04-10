use crate::types::Proof;
use std::collections::HashMap;

/// In-memory store for ZK proofs, keyed by `query_id`.
pub struct ProofStorage {
    pub proofs: HashMap<String, Proof>,
}

impl ProofStorage {
    pub fn new() -> Self {
        ProofStorage {
            proofs: HashMap::new(),
        }
    }

    /// Store a new proof for the given `query_id` (overwrites if already present).
    pub fn add_proof(&mut self, query_id: String, proof_bytes: Vec<u8>, public_values: Vec<u8>) {
        self.proofs.insert(
            query_id,
            Proof {
                proof_bytes,
                public_values,
                is_published: false,
            },
        );
    }

    /// Mark an existing proof as published.  Returns `true` if the entry existed.
    pub fn mark_published(&mut self, query_id: &str) -> bool {
        if let Some(proof) = self.proofs.get_mut(query_id) {
            proof.is_published = true;
            true
        } else {
            false
        }
    }

    /// If the proof already exists mark it published; otherwise insert a
    /// placeholder entry (empty proof / public-values) with `is_published = true`.
    pub fn upsert_published(&mut self, query_id: String) {
        if !self.mark_published(&query_id) {
            self.proofs.insert(
                query_id,
                Proof {
                    proof_bytes: vec![],
                    public_values: vec![],
                    is_published: true,
                },
            );
        }
    }

    /// Returns `query_id`s of **all** known proofs.
    pub fn list_all(&self) -> Vec<String> {
        self.proofs.keys().cloned().collect()
    }

    /// Returns `query_id`s of proofs that have been published.
    pub fn list_published(&self) -> Vec<String> {
        self.proofs
            .iter()
            .filter(|(_, p)| p.is_published)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Returns `true` if a proof for `query_id` exists.
    pub fn exists(&self, query_id: &str) -> bool {
        self.proofs.contains_key(query_id)
    }

    /// Returns a clone of the proof for `query_id`, or `None`.
    pub fn get(&self, query_id: &str) -> Option<Proof> {
        self.proofs.get(query_id).cloned()
    }
}
