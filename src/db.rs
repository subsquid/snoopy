//! ClickHouse query functions.

use crate::types::{HashRow, InvestigationRow, QueryExecutedRow, QueryIdRow, SignatureRow};
use anyhow::anyhow;
use clickhouse::Client;
use std::collections::HashMap;
use tracing::{debug, info};

// ---------------------------------------------------------------------------
// Suspicious-hash discovery
// ---------------------------------------------------------------------------

pub async fn get_suspicious_hashes(
    client: &Client,
    range_start_sec: u32,
    range_end_sec: u32,
) -> Result<Vec<String>, anyhow::Error> {
    let mut cursor = client
        .query(
            "select hash from (
            select
            hex(query_hash) as hash, 
            count(distinct(chunk_id, from_block, to_block)) as A,
            count(distinct(chunk_id, from_block, to_block, output_hash)) as B
            from mainnet.worker_query_logs where
            worker_timestamp > ? and
            worker_timestamp < ? and
            result == 'ok'
            group by query_hash
            ) where A <> B",
        )
        .bind(range_start_sec)
        .bind(range_end_sec)
        .fetch::<HashRow>()?;

    let mut res = Vec::<String>::default();
    while let Some(row) = cursor.next().await? {
        res.push(row.hash);
    }
    Ok(res)
}

pub async fn investigate_hash(
    client: &Client,
    range_start_sec: u32,
    range_end_sec: u32,
    hashes: Vec<String>,
) -> Result<Vec<InvestigationRow>, anyhow::Error> {
    client
        .query(
            "select hash, dataset, chunk_id, from_block, to_block, workers from (
            select 
                hex(query_hash) as hash, 
                dataset, 
                chunk_id, 
                from_block, 
                to_block, 
                count(distinct(worker_id)) as workers, 
                count(distinct(output_hash)) as variants
            from mainnet.worker_query_logs 
            where 
                worker_timestamp > ? and
                worker_timestamp < ? and
                hex(query_hash) IN ? and
                result == 'ok'
            group by query_hash, dataset, chunk_id, from_block, to_block
            ) where variants > 1",
        )
        .bind(range_start_sec)
        .bind(range_end_sec)
        .bind(hashes)
        .fetch_all::<InvestigationRow>()
        .await
        .map_err(|err| anyhow!("{err:?}"))
}

pub async fn get_siblings_queries_by_investigate_row(
    client: &Client,
    range_start_sec: u32,
    range_end_sec: u32,
    row: &InvestigationRow,
) -> Result<Vec<QueryExecutedRow>, anyhow::Error> {
    let sibling_ids = client
        .query(
            "
            select 
                any(query_id)
            from mainnet.worker_query_logs
            where 
                worker_timestamp > ? and 
                worker_timestamp < ? and 
                hex(query_hash) = ? and
                dataset = ? and
                chunk_id = ? and
                from_block = ? and 
                to_block = ? and 
                result = 'ok'
            group by worker_id, output_hash",
        )
        .bind(range_start_sec)
        .bind(range_end_sec)
        .bind(row.hash.clone())
        .bind(row.dataset.clone())
        .bind(row.chunk_id.clone())
        .bind(row.from_block)
        .bind(row.to_block)
        .fetch_all::<QueryIdRow>()
        .await?;

    let mut sibling_queries = client
        .query(
            "
            select 
                query_id, 
                client_id, 
                worker_id, 
                dataset_id, 
                from_block, 
                to_block, 
                chunk_id, 
                query, 
                query_hash, 
                result, 
                output_hash, 
                last_block, 
                error_msg, 
                client_signature, 
                client_timestamp, 
                request_id
            from mainnet.worker_query_logs
            where 
                worker_timestamp > ? and 
                worker_timestamp < ? and
                query_id in ?",
        )
        .bind(range_start_sec)
        .bind(range_end_sec)
        .bind(
            sibling_ids
                .iter()
                .map(|v| v.query_id.clone())
                .collect::<Vec<_>>(),
        )
        .fetch_all::<QueryExecutedRow>()
        .await?;

    sibling_queries.sort_by(|a, b| a.query_id.cmp(&b.query_id));
    sibling_queries.dedup_by(|a, b| a.query_id == b.query_id);

    info!(
        "After filtering got {:?} unique queries",
        sibling_queries.len()
    );

    Ok(sibling_queries)
}

/// Return a stable identification of the odd-one-out query ids from a set of
/// siblings that produced different output hashes.
pub fn find_odds_in_siblings(
    siblings: &Vec<QueryExecutedRow>,
) -> Result<Vec<String>, anyhow::Error> {
    let mut map = HashMap::<String, Vec<String>>::default();
    for sibling in siblings {
        let key = sibling
            .output_hash
            .iter()
            .map(|v| format!("{v:02X}"))
            .collect::<Vec<_>>()
            .join("");
        let value = map.entry(key).or_insert(vec![]);
        (*value).push(sibling.query_id.clone());
    }
    let max_num = map
        .iter()
        .map(|(_, v)| v.len())
        .max()
        .ok_or(anyhow!("Empty input for odds finder"))?;
    let res = map
        .values()
        .filter(|v| v.len() < max_num)
        .cloned()
        .collect::<Vec<_>>()
        .concat();
    Ok(res)
}

/// Look up a `query_id` in ClickHouse by `(worker_id, client_timestamp_ms)`.
pub async fn get_query_id_by_worker_and_ts(
    client: &Client,
    worker_id: &str,
    ts_ms: u64,
) -> Result<Option<String>, anyhow::Error> {
    // worker_timestamp in worker_query_logs is in seconds; the event timestamp is in milliseconds.
    let ts_sec = (ts_ms as f64 / 1000.0) as f64;
    let mut cursor = client
        .query(
            "SELECT any(query_id) as query_id
             FROM mainnet.worker_query_logs
             WHERE client_timestamp = ?
               AND worker_id = ?
             HAVING count() > 1
            ",
        )
        .bind(format!("{:0.3}", ts_sec))
        .bind(worker_id)
        .fetch::<QueryIdRow>()?;

    if let Some(row) = cursor.next().await? {
        Ok(Some(row.query_id))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Sibling-query lookup (used by the run-loop / manual task API)
// ---------------------------------------------------------------------------

pub async fn get_siblings_queries(
    client: &Client,
    query_id: &str,
    ts: u32,
    ts_tolerance: u32,
    ts_search_range: u32,
) -> Result<Vec<QueryExecutedRow>, anyhow::Error> {
    info!(
        "Params: {} {} {}",
        query_id,
        ts - ts_tolerance,
        ts + ts_tolerance
    );
    let original_query = client
        .query("select query_id, client_id, worker_id, dataset_id, from_block, to_block, chunk_id, query, query_hash, result, output_hash, last_block, error_msg, client_signature, client_timestamp, request_id from worker_query_logs where worker_timestamp > ? AND worker_timestamp < ? AND query_id = ?")
        .bind(ts - ts_tolerance)
        .bind(ts + ts_tolerance)
        .bind(query_id)
        .fetch_one::<QueryExecutedRow>()
        .await?;
    info!(
        "Found query hash: {:?}",
        original_query
            .query_hash
            .iter()
            .map(|v| format!("{v:02X}"))
            .collect::<Vec<_>>()
            .join("")
    );

    let mut sibling_queries = client
        .query("select query_id, client_id, worker_id, dataset_id, from_block, to_block, chunk_id, query, query_hash, result, output_hash, last_block, error_msg, client_signature, client_timestamp, request_id from worker_query_logs where worker_timestamp > ? AND worker_timestamp < ? AND hex(query_hash) = ? AND from_block = ? AND to_block = ? AND result = 'ok'")
        .bind(ts - ts_search_range)
        .bind(ts + ts_search_range)
        .bind(original_query.query_hash.iter().map(|v| format!("{v:02X}")).collect::<Vec<_>>().join(""))
        .bind(original_query.from_block)
        .bind(original_query.to_block)
        .fetch_all::<QueryExecutedRow>()
        .await?;
    info!("Found {:?} queries with same hash", sibling_queries.len());

    sibling_queries.sort_by(|a, b| a.query_id.cmp(&b.query_id));
    sibling_queries.dedup_by(|a, b| a.query_id == b.query_id);

    info!(
        "After filtering got {:?} unique queries",
        sibling_queries.len()
    );

    Ok(sibling_queries)
}

// ---------------------------------------------------------------------------
// Signature lookup
// ---------------------------------------------------------------------------

pub async fn get_signatures(
    client: &Client,
    range_start_sec: u32,
    range_end_sec: u32,
    eligible_queries: &[QueryExecutedRow],
    original_query_id: &str,
) -> Result<HashMap<String, (Vec<u8>, Vec<u8>)>, anyhow::Error> {
    let signatures = client
        .query("select query_id, worker_signature, result_hash from portal_logs where collector_timestamp > ? AND collector_timestamp < ? AND query_id IN ?")
        .bind(range_start_sec)
        .bind(range_end_sec)
        .bind(eligible_queries.iter().map(|row| row.query_id.clone()).collect::<Vec<_>>())
        .fetch_all::<SignatureRow>()
        .await?;
    debug!("Signature rows: {signatures:?}");
    let mut result_count: HashMap<Vec<u8>, usize> = HashMap::new();
    for row in &signatures {
        *result_count.entry(row.result_hash.clone()).or_default() += 1;
    }

    let plurality = result_count
        .iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .ok_or(anyhow!("Plurality not found"))?;
    info!(
        "Most frequent hash: {:?} ({:?}/{:?})",
        plurality
            .iter()
            .map(|v| format!("{v:02X}"))
            .collect::<Vec<_>>()
            .join(""),
        result_count.get(plurality),
        signatures.len()
    );

    Ok(signatures
        .into_iter()
        .filter(|row| row.result_hash == *plurality || row.query_id == original_query_id)
        .map(|row| (row.query_id, (row.result_hash, row.worker_signature)))
        .collect::<HashMap<String, (Vec<u8>, Vec<u8>)>>())
}
