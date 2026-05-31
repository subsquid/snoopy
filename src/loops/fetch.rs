//! Background loop that subscribes to on-chain `FraudFound` events and marks
//! the corresponding proofs as published in the shared proof storage.
//!
//! **Backfill strategy**
//! 1. Try to fetch all historical `FraudFound` events from the GraphQL squid
//!    (`graphql_url`).  The squid indexes every event and supports rich
//!    filtering, so a single paginated query covers the full history without
//!    block-range limitations.
//! 2. If the GraphQL request fails for any reason (network error, unexpected
//!    response, parse error …), fall back to the legacy approach: walk
//!    backwards through the last `MAX_PAGES` × `PAGE` blocks via the Ethereum
//!    WebSocket RPC.
//!
//! A watermark (`last_processed_block`) is persisted across reconnects so
//! that each reconnect only re-fetches events that are genuinely new — this
//! prevents request storms against the GraphQL squid and ClickHouse on
//! frequent WS reconnects.
//!
//! **Live subscription**
//! The GraphQL squid does not expose a WebSocket subscription endpoint, so
//! the live phase always uses the RPC WebSocket stream.

use crate::{
    contracts::ProvingManager,
    db::get_query_id_by_worker_and_ts,
    proof_storage::ProofStorage,
    state::InternalState,
    types::{GraphQlFraudFound, GraphQlFraudFoundsResponse},
};
use clickhouse::Client;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::sleep;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Shared event-processing helper
// ---------------------------------------------------------------------------

/// Look up the `query_id` for the given worker/timestamp pair and, if found,
/// mark the corresponding proof as published.
///
/// Called from all three event sources: GraphQL backfill, RPC backfill, and
/// the live WS subscription.
async fn process_fraud_event(
    client: &Client,
    storage: &Arc<Mutex<ProofStorage>>,
    worker_id: &str,
    ts_ms: u64,
    ctx: &str,
) {
    match get_query_id_by_worker_and_ts(client, worker_id, ts_ms).await {
        Ok(Some(query_id)) => {
            info!(
                "fetch_loop: {ctx} – marking query_id={query_id} as published \
                 (worker_id={worker_id})"
            );
            storage.lock().unwrap().upsert_published(query_id);
        }
        Ok(None) => {
            error!(
                "fetch_loop: {ctx} – no query_id for worker_id={worker_id} \
                 ts_ms={ts_ms}"
            );
        }
        Err(err) => {
            error!(
                "fetch_loop: {ctx} – clickhouse error for \
                 worker_id={worker_id} ts_ms={ts_ms}: {err:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// GraphQL helpers
// ---------------------------------------------------------------------------

/// Page size used when paginating `contractEventFraudFounds`.
const GRAPHQL_PAGE_SIZE: usize = 1_000;

/// Stream `FraudFound` events from the GraphQL squid whose `blockNumber` is
/// ≥ `from_block`, processing each page immediately (one ClickHouse query per
/// event) rather than buffering all pages in memory.
///
/// Returns `Ok(())` on success, or an error if *any* HTTP / parse step fails
/// (the caller falls back to RPC on error).
async fn fetch_and_process_fraud_events_graphql(
    http_client: &reqwest::Client,
    graphql_url: &str,
    from_block: u64,
    client: &Client,
    storage: &Arc<Mutex<ProofStorage>>,
) -> anyhow::Result<()> {
    let mut offset = 0usize;

    loop {
        let query = format!(
            r#"{{
  contractEventFraudFounds(
    where: {{ blockNumber_gte: {from_block} }}
    orderBy: blockNumber_ASC
    limit: {limit}
    offset: {offset}
  ) {{
    peerId
    timestamp
  }}
}}"#,
            from_block = from_block,
            limit = GRAPHQL_PAGE_SIZE,
            offset = offset,
        );

        let body = serde_json::json!({ "query": query });

        let resp = http_client
            .post(graphql_url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let parsed: GraphQlFraudFoundsResponse = resp.json().await?;
        let page: Vec<GraphQlFraudFound> =
            parsed.data.contract_event_fraud_founds;
        let page_len = page.len();

        for event in page {
            process_fraud_event(
                client,
                storage,
                &event.peer_id,
                event.timestamp,
                "historical (GraphQL)",
            )
            .await;
        }

        if page_len < GRAPHQL_PAGE_SIZE {
            // Last (or only) page — done.
            break;
        }
        offset += page_len;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

pub fn start_fetch_loop(state: &InternalState) {
    use alloy::providers::{Provider, ProviderBuilder, WsConnect};
    use futures_util::StreamExt;

    let local_config = state.config.clone();
    let local_proof_storage = Arc::clone(&state.proof_storage);

    tokio::spawn(async move {
        // Build the HTTP client once, with explicit timeouts so a hung GraphQL
        // endpoint cannot stall the task indefinitely.
        let http_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        // Watermark: the highest block whose events have already been
        // processed.  Persisted across reconnects so each outer-loop iteration
        // only fetches genuinely new events, preventing request storms.
        let mut last_processed_block: u64 = 0;

        loop {
            let rpc_url = local_config.rpc_url.clone();
            let graphql_url = local_config.graphql_url.clone();
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

            let proving_manager = ProvingManager::new(manager_address, &provider);

            let client = Client::default()
                .with_url(db_url)
                .with_database(db_database)
                .with_user(db_user)
                .with_password(db_password)
                .with_option("max_execution_time", "60");

            // ------------------------------------------------------------------
            // Backfill: fetch FraudFound events newer than the watermark.
            //
            // Strategy:
            //   1. Try the GraphQL squid (covers full history, no block-range cap).
            //   2. On failure, fall back to walking backwards in 50 000-block
            //      pages via the Ethereum WS RPC (up to MAX_PAGES pages).
            // ------------------------------------------------------------------
            const PAGE: u64 = 49_999;
            const MAX_PAGES: u64 = 5;

            let latest_block = match provider.get_block_number().await {
                Ok(n) => n,
                Err(err) => {
                    error!("fetch_loop: failed to get latest block number: {err:?}");
                    0
                }
            };

            // On the very first iteration use the rolling window; on subsequent
            // reconnects use the watermark so we only fetch new events.
            let default_from = latest_block.saturating_sub(PAGE * MAX_PAGES);
            let backfill_from_block = if last_processed_block == 0 {
                default_from
            } else {
                last_processed_block.saturating_add(1)
            };

            info!(
                "fetch_loop: backfilling FraudFound events from block \
                 {backfill_from_block} (latest={latest_block}, \
                 watermark={last_processed_block})"
            );

            // --- Attempt 1: GraphQL squid -----------------------------------
            info!(
                "fetch_loop: trying GraphQL endpoint {graphql_url} for backfill"
            );
            let graphql_result = fetch_and_process_fraud_events_graphql(
                &http_client,
                &graphql_url,
                backfill_from_block,
                &client,
                &local_proof_storage,
            )
            .await;

            match graphql_result {
                Ok(()) => {
                    info!(
                        "fetch_loop: GraphQL backfill completed up to block \
                         {latest_block}"
                    );
                    last_processed_block = latest_block;
                }

                Err(graphql_err) => {
                    // --- Attempt 2: RPC fallback ----------------------------
                    warn!(
                        "fetch_loop: GraphQL backfill failed ({graphql_err:?}), \
                         falling back to RPC block-range queries"
                    );

                    let mut page_end = latest_block;
                    'backfill: for _page in 0..MAX_PAGES {
                        let page_start = page_end.saturating_sub(PAGE);
                        // Skip pages entirely below the watermark.
                        if page_end < backfill_from_block {
                            break 'backfill;
                        }
                        let effective_start =
                            page_start.max(backfill_from_block);
                        info!(
                            "fetch_loop: querying FraudFound events via RPC \
                             blocks {effective_start}..={page_end}"
                        );
                        match proving_manager
                            .FraudFound_filter()
                            .from_block(effective_start)
                            .to_block(page_end)
                            .query()
                            .await
                        {
                            Ok(events) => {
                                info!(
                                    "fetch_loop: got {} historical FraudFound \
                                     events in range \
                                     {effective_start}..={page_end}",
                                    events.len()
                                );
                                for (event, _log) in events {
                                    process_fraud_event(
                                        &client,
                                        &local_proof_storage,
                                        &event.peer_id,
                                        event.timestamp.to::<u64>(),
                                        "historical (RPC)",
                                    )
                                    .await;
                                }
                                last_processed_block = page_end;
                            }
                            Err(err) => {
                                error!(
                                    "fetch_loop: failed to query historical \
                                     FraudFound events via RPC \
                                     ({effective_start}..={page_end}): {err:?}"
                                );
                                break 'backfill;
                            }
                        }
                        if page_start == 0 || page_start < backfill_from_block
                        {
                            break 'backfill;
                        }
                        page_end = page_start - 1;
                    }
                }
            }

            // ------------------------------------------------------------------
            // Live subscription: receive new FraudFound events going forward.
            // The GraphQL squid has no subscription endpoint, so we always use
            // the RPC WebSocket stream here.
            // ------------------------------------------------------------------
            let event_filter = proving_manager.FraudFound_filter();
            let mut stream = match event_filter.subscribe().await {
                Ok(s) => s.into_stream(),
                Err(err) => {
                    error!(
                        "fetch_loop: failed to subscribe to FraudFound \
                         events: {err:?}"
                    );
                    sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };

            info!("fetch_loop: subscribed to FraudFound events");

            loop {
                match stream.next().await {
                    None => {
                        info!(
                            "fetch_loop: FraudFound stream ended, \
                             reconnecting..."
                        );
                        break;
                    }
                    Some(Err(err)) => {
                        error!(
                            "fetch_loop: error receiving FraudFound \
                             event: {err:?}"
                        );
                        break;
                    }
                    Some(Ok((event, log))) => {
                        let worker_id: String = event.peer_id.clone();
                        let ts_ms: u64 = event.timestamp.to::<u64>();
                        info!(
                            "fetch_loop: received FraudFound event for \
                             worker_id={worker_id} ts_ms={ts_ms}"
                        );
                        process_fraud_event(
                            &client,
                            &local_proof_storage,
                            &worker_id,
                            ts_ms,
                            "live",
                        )
                        .await;
                        // Advance watermark so the next reconnect's backfill
                        // starts from here rather than the original window.
                        if let Some(block_num) = log.block_number {
                            if block_num > last_processed_block {
                                last_processed_block = block_num;
                            }
                        }
                    }
                }
            }
        }
    });
}
