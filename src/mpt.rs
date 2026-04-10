//! Merkle Patricia Trie helpers: populate from assignment data and generate
//! inclusion proofs.

use anyhow::anyhow;
use eth_trie::{EthTrie, MemoryDB, Trie};
use flate2::read::GzDecoder;
use sqd_assignments::Assignment;
use std::{fs::File, io::Read};
use tiny_keccak::{Hasher, Keccak};

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
