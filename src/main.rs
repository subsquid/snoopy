#[macro_use]
extern crate rocket;

use clap::Parser;
use snoopy::{
    loops::{
        discovery::start_discovery_loop,
        fetch::start_fetch_loop,
        run::start_run_loop,
    },
    proof_storage::ProofStorage,
    routes::{
        app_js, get_all_proofs, get_all_tasks, get_metadata, get_task_status, index, styles,
        submit_task,
    },
    state::InternalState,
    types::Args,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
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
        tasks: Arc::new(Mutex::new(HashMap::new())),
        proof_storage: Arc::new(Mutex::new(ProofStorage::new())),
        config: args,
    };
    start_discovery_loop(&state);
    start_run_loop(&state);
    start_fetch_loop(&state);
    let _ = rocket::build()
        .manage(state)
        .mount(
            "/",
            routes![
                index,
                styles,
                app_js,
                submit_task,
                get_task_status,
                get_all_tasks,
                get_metadata,
                get_all_proofs
            ],
        )
        .launch()
        .await;
    Ok(())
}
