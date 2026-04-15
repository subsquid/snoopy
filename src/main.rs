#[macro_use]
extern crate rocket;

use clap::Parser;
use snoopy::{
    loops::{
        discovery::start_discovery_loop,
        fetch::start_fetch_loop,
    },
    proof_storage::ProofStorage,
    routes::{
        app_js, get_all_proofs, get_discovery_progress, get_metadata, index, styles,
    },
    state::InternalState,
    types::{Args, DiscoveryLoopProgress},
};
use std::sync::{Arc, Mutex};
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[rocket::main]
async fn main() -> Result<(), Box<rocket::Error>> {
    sp1_sdk::utils::setup_logger();
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("should be able to install the default crypto provider");
    let args = Args::parse();
    let state = InternalState {
        proof_storage: Arc::new(Mutex::new(ProofStorage::new())),
        discovery_progress: Arc::new(Mutex::new(DiscoveryLoopProgress::default())),
        config: args,
    };
    start_discovery_loop(&state);
    start_fetch_loop(&state);
    let _ = rocket::build()
        .manage(state)
        .mount(
            "/",
            routes![
                index,
                styles,
                app_js,
                get_metadata,
                get_all_proofs,
                get_discovery_progress
            ],
        )
        .launch()
        .await;
    Ok(())
}
