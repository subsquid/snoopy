//! Background loop that subscribes to on-chain `FraudFound` events and marks
//! the corresponding proofs as published in the shared proof storage.

use crate::{
    contracts::ProvingManager,
    db::get_query_id_by_worker_and_ts,
    state::InternalState,
};
use clickhouse::Client;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tracing::{error, info};

pub fn start_fetch_loop(state: &InternalState) {
    use alloy::providers::{Provider, ProviderBuilder, WsConnect};
    use futures_util::StreamExt;

    let local_config = state.config.clone();
    let local_proof_storage = Arc::clone(&state.proof_storage);

    tokio::spawn(async move {
        loop {
            let rpc_url = local_config.rpc_url.clone();
            let manager_address = local_config.manager_address;
            let db_url = local_config.db_url.clone();
            let db_database = local_config.db_database.clone();
            let db_user = local_config.db_user.clone();
            let db_password = local_config.db_password.clone();
            let ws = WsConnect::new(rpc_url.clone());
            let provider = match ProviderBuilder::new().connect_ws(ws).await {
                Ok(p) => p,
                Err(err) => {
                    error!("fetch_loop: failed to connect to WS RPC: {err:?}");
                    sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };

            let proving_manager = ProvingManager::new(manager_address, provider);

            let client = Client::default()
                .with_url(db_url)
                .with_database(db_database)
                .with_user(db_user)
                .with_password(db_password)
                .with_option("max_execution_time", "60");

            // ------------------------------------------------------------------
            // Backfill: fetch recent FraudFound events by walking backwards in
            // 50 000-block pages (node limit), up to 5 pages (~last month).
            // ------------------------------------------------------------------
            const PAGE: u64 = 49_999;
            const MAX_PAGES: u64 = 5;
            info!("fetch_loop: fetching historical FraudFound events (up to {MAX_PAGES} pages)");
            let latest_block = match proving_manager.provider().get_block_number().await {
                Ok(n) => n,
                Err(err) => {
                    error!("fetch_loop: failed to get latest block number: {err:?}");
                    0
                }
            };
            let mut page_end = latest_block;
            'backfill: for _page in 0..MAX_PAGES {
                let page_start = page_end.saturating_sub(PAGE);
                info!("fetch_loop: querying FraudFound events blocks {page_start}..={page_end}");
                match proving_manager
                    .FraudFound_filter()
                    .from_block(page_start)
                    .to_block(page_end)
                    .query()
                    .await
                {
                    Ok(events) => {
                        info!(
                            "fetch_loop: got {} historical FraudFound events in range {page_start}..={page_end}",
                            events.len()
                        );
                        for (event, _log) in events {
                            let worker_id: String = event.peer_id.clone();
                            let ts_ms: u64 = event.timestamp.to::<u64>();
                            match get_query_id_by_worker_and_ts(&client, &worker_id, ts_ms).await {
                                Ok(Some(query_id)) => {
                                    info!(
                                        "fetch_loop: historical FraudFound – marking query_id={query_id} as published (worker_id={worker_id})"
                                    );
                                    let mut storage = local_proof_storage.lock().unwrap();
                                    storage.upsert_published(query_id);
                                }
                                Ok(None) => {
                                    error!(
                                        "fetch_loop: historical FraudFound – no query_id found for worker_id={worker_id} ts_ms={ts_ms}"
                                    );
                                }
                                Err(err) => {
                                    error!(
                                        "fetch_loop: historical FraudFound – clickhouse error for worker_id={worker_id} ts_ms={ts_ms}: {err:?}"
                                    );
                                }
                            }
                        }
                    }
                    Err(err) => {
                        error!(
                            "fetch_loop: failed to query historical FraudFound events ({page_start}..={page_end}): {err:?}"
                        );
                        break 'backfill;
                    }
                }
                if page_start == 0 {
                    break 'backfill;
                }
                page_end = page_start - 1;
            }

            // ------------------------------------------------------------------
            // Live subscription: receive new FraudFound events going forward.
            // ------------------------------------------------------------------
            let event_filter = proving_manager.FraudFound_filter();
            let mut stream = match event_filter.subscribe().await {
                Ok(s) => s.into_stream(),
                Err(err) => {
                    error!("fetch_loop: failed to subscribe to FraudFound events: {err:?}");
                    sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };

            info!("fetch_loop: subscribed to FraudFound events");

            loop {
                match stream.next().await {
                    None => {
                        info!("fetch_loop: FraudFound stream ended, reconnecting...");
                        break;
                    }
                    Some(Err(err)) => {
                        error!("fetch_loop: error receiving FraudFound event: {err:?}");
                        break;
                    }
                    Some(Ok((event, _log))) => {
                        let worker_id: String = event.peer_id.clone();
                        let ts_ms: u64 = event.timestamp.to::<u64>();
                        info!(
                            "fetch_loop: received FraudFound event for worker_id={worker_id} ts_ms={ts_ms}"
                        );

                        let query_id = match get_query_id_by_worker_and_ts(
                            &client,
                            &worker_id,
                            ts_ms,
                        )
                        .await
                        {
                            Ok(Some(qid)) => qid,
                            Ok(None) => {
                                error!(
                                    "fetch_loop: no query_id found for worker_id={worker_id} ts_ms={ts_ms}"
                                );
                                continue;
                            }
                            Err(err) => {
                                error!(
                                    "fetch_loop: clickhouse error while fetching query_id for worker_id={worker_id} ts_ms={ts_ms}: {err:?}"
                                );
                                continue;
                            }
                        };

                        info!(
                            "fetch_loop: marking query_id={query_id} as published (worker_id={worker_id})"
                        );
                        let mut storage = local_proof_storage.lock().unwrap();
                        storage.upsert_published(query_id);
                    }
                }
            }
        }
    });
}
