use crate::{
    CKB_MAINNET_RPC, CKB_TESTNET_RPC, RpcClient, get_pg_pool,
    ip_location::lookup_ipinfo,
    pg_write::{
        ChannelInfoDBSchema, Network, NodeInfoDBSchema, RelationCache, UdtInfos, UdtNodeRelation,
        UdtdepRelation, global_cache, global_cache_testnet,
    },
    rpc_client::{CKB_MAINNET_RPC_BEARER_TOKEN, CKB_TESTNET_RPC_BEARER_TOKEN},
    types::{
        CellType, IndexerScriptSearchMode, MAINNET_COMMITMENT_CODE_HASH, NodeInfo, Order,
        ScriptType, SearchKey, SearchKeyFilter, TESTNET_COMMITMENT_CODE_HASH, Tx,
        commitment_script, funding_script,
    },
};

use chrono::Duration;
use ckb_jsonrpc_types::{BlockNumber, DepType, JsonBytes};
use ckb_types::{H256, packed, prelude::*};
use faster_hex::{hex_decode, hex_string};
use futures::StreamExt;
use multiaddr::{Multiaddr, Protocol};
use serde::{Deserialize, Serialize};
use sqlx::{
    Pool, Postgres,
    types::chrono::{DateTime, Utc},
};

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    vec,
};

pub async fn from_rpc_to_db_schema(
    node_info: NodeInfo,
    net: Network,
) -> (
    NodeInfoDBSchema,
    Vec<UdtInfos>,
    Vec<UdtdepRelation>,
    Vec<UdtNodeRelation>,
) {
    let node_id = String::from_utf8(node_info.node_id.to_vec()).unwrap();
    let announce_timestamp = DateTime::from_timestamp_millis(node_info.timestamp as i64).unwrap();
    let auto_accept_min_ckb_funding_amount =
        hex_string(&node_info.auto_accept_min_ckb_funding_amount.to_be_bytes());

    let mut udt_infos = vec![];
    let mut udt_dep_relations = vec![];
    let mut udt_node_relations = vec![];

    let global = match net {
        Network::Mainnet => global_cache().load(),
        Network::Testnet => global_cache_testnet().load(),
    };
    let mut new_udt_infos = RelationCache {
        udt: global.udt.clone(),
        udt_node: global.udt_node.clone(),
    };

    let mut need_update_global = false;

    for udt_cfg in node_info.udt_cfg_infos.0 {
        let len = new_udt_infos.udt.len() as i32;
        let udt_info_id = *new_udt_infos
            .udt
            .entry(udt_cfg.script.clone())
            .or_insert_with(|| len + 1);

        if len != new_udt_infos.udt.len() as i32 {
            need_update_global = true;
            let udt_info = UdtInfos {
                id: udt_info_id,
                name: udt_cfg.name,
                code_hash: hex_string(udt_cfg.script.code_hash.as_bytes()),
                hash_type: udt_cfg.script.hash_type.to_string(),
                args: hex_string(udt_cfg.script.args.as_bytes()),
                auto_accept_amount: udt_cfg
                    .auto_accept_amount
                    .map_or("NULL".to_string(), |v| hex_string(&v.to_be_bytes())),
            };
            udt_infos.push(udt_info);
            for dep in udt_cfg.cell_deps {
                if let Some(cell_dep) = dep.cell_dep {
                    let relation = UdtdepRelation {
                        outpoint_tx_hash: Some(hex_string(cell_dep.out_point.tx_hash.as_bytes())),
                        outpoint_index: Some(hex_string(
                            &cell_dep.out_point.index.value().to_be_bytes(),
                        )),
                        dep_type: Some({
                            match cell_dep.dep_type {
                                DepType::Code => "code".to_string(),
                                DepType::DepGroup => "dep_group".to_string(),
                            }
                        }),
                        code_hash: None,
                        hash_type: None,
                        args: None,
                        udt_info_id,
                    };
                    udt_dep_relations.push(relation);
                }
                if let Some(type_id) = dep.type_id {
                    let relation = UdtdepRelation {
                        outpoint_tx_hash: None,
                        outpoint_index: None,
                        dep_type: None,
                        code_hash: Some(hex_string(type_id.code_hash.as_bytes())),
                        hash_type: Some(type_id.hash_type.to_string()),
                        args: Some(hex_string(type_id.args.as_bytes())),
                        udt_info_id,
                    };
                    udt_dep_relations.push(relation);
                }
            }
        }

        match new_udt_infos.udt_node.entry(node_info.node_id.clone()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get_mut().insert(udt_info_id) {
                    need_update_global = true;
                    let relation = UdtNodeRelation {
                        node_id: node_id.clone(),
                        udt_info_id,
                    };
                    udt_node_relations.push(relation);
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(HashSet::from([udt_info_id]));
                need_update_global = true;
                let relation = UdtNodeRelation {
                    node_id: node_id.clone(),
                    udt_info_id,
                };
                udt_node_relations.push(relation);
            }
        }
    }

    let mut node_schema = NodeInfoDBSchema {
        node_name: node_info.node_name,
        addresses: serde_json::to_string(&node_info.addresses).unwrap(),
        node_id,
        announce_timestamp,
        chain_hash: hex_string(node_info.chain_hash.as_bytes()),
        auto_accept_min_ckb_funding_amount,
        country_or_region: Default::default(),
        city: Default::default(),
        region: Default::default(),
        loc: Default::default(),
    };

    for addr in node_info
        .addresses
        .iter()
        .filter_map(multiaddr_to_socketaddr)
    {
        if let Ok(ip_details) = lookup_ipinfo(&addr.ip().to_string()).await {
            node_schema.country_or_region = ip_details.country;
            node_schema.city = ip_details.city;
            node_schema.region = ip_details.region;
            node_schema.loc = ip_details.loc;
            break;
        }
    }
    // Update the global cache if there are new UDT infos or relations
    if need_update_global {
        match net {
            Network::Mainnet => global_cache().store(Arc::new(new_udt_infos)),
            Network::Testnet => global_cache_testnet().store(Arc::new(new_udt_infos)),
        }
    }
    (
        node_schema,
        udt_infos,
        udt_dep_relations,
        udt_node_relations,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_batch(
    pool: &Pool<Postgres>,
    udt_infos: &[UdtInfos],
    udt_dep_relations: &[UdtdepRelation],
    udt_node_relations: &[UdtNodeRelation],
    node_schemas: &[NodeInfoDBSchema],
    channel_schemas: &[ChannelInfoDBSchema],
    time: &DateTime<Utc>,
    net: Network,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    UdtInfos::insert_batch(&mut tx, udt_infos, net).await?;
    UdtdepRelation::use_sqlx(&mut tx, udt_dep_relations, net).await?;
    UdtNodeRelation::use_sqlx(&mut tx, udt_node_relations, net).await?;
    NodeInfoDBSchema::use_sqlx(&mut tx, node_schemas, time, net).await?;
    ChannelInfoDBSchema::use_sqlx(&mut tx, channel_schemas, time, net).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn daily_statistics(
    pool: &Pool<Postgres>,
    start_time: Option<DateTime<Utc>>,
    nets: impl Iterator<Item = &Network>,
) -> Result<(), sqlx::Error> {
    use chrono::Timelike;
    use sqlx::Row;

    let now = Utc::now();

    let end_time = now
        .with_hour(0)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    let start_time = start_time.unwrap_or(end_time - Duration::days(1));

    for net in nets {
        let nodes_count_sql = format!(
            "
    SELECT
        time_bucket('1 day', bucket) AS day_bucket,
        COUNT(DISTINCT node_id) AS nodes_count
    FROM {}
    WHERE bucket < $1::timestamp and bucket >= $2::timestamp
    GROUP BY day_bucket
    ORDER BY day_bucket DESC
    ",
            net.online_nodes_hourly()
        );
        let channels_data_sql = format!(
            "
    SELECT DISTINCT ON (time_bucket('1 day', bucket), n.channel_outpoint)
        time_bucket('1 day', bucket) AS day_bucket,
        n.capacity as asset,
        COALESCE(c.name, 'ckb') as name, r.capacity as capacity
    FROM {} n
    left join {} c on n.udt_type_script = c.id
    join {} r on n.channel_outpoint = r.channel_outpoint
    WHERE bucket < $1::timestamp and bucket >= $2::timestamp
    ORDER BY time_bucket('1 day', bucket), n.channel_outpoint, bucket DESC
    ",
            net.online_channels_hourly(),
            net.udt_infos(),
            net.channel_states()
        );
        let nodes_count: Vec<(DateTime<Utc>, i64)> = sqlx::query(&nodes_count_sql)
            .bind(end_time)
            .bind(start_time)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| {
                let day_bucket: DateTime<Utc> = row.get("day_bucket");
                let nodes_count: i64 = row.get("nodes_count");
                (day_bucket, nodes_count)
            })
            .collect();
        let channels_data = sqlx::query(&channels_data_sql)
            .bind(end_time)
            .bind(start_time)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| {
                let day_bucket: DateTime<Utc> = row.get("day_bucket");
                let asset: u128 = {
                    let raw: String = row.get("asset");
                    let mut buf = [0u8; 16];
                    faster_hex::hex_decode(raw.as_bytes(), &mut buf).unwrap();
                    u128::from_be_bytes(buf)
                };
                let capacity: u64 = {
                    let raw: String = row.get("capacity");
                    let mut buf = [0u8; 8];
                    faster_hex::hex_decode(raw.as_bytes(), &mut buf).unwrap();
                    u64::from_be_bytes(buf)
                };
                let name = row.get::<String, _>("name");
                (day_bucket, (name, asset, capacity))
            })
            .fold(
                HashMap::new(),
                |mut acc: HashMap<DateTime<Utc>, HashMap<String, Vec<(u128, u64)>>>,
                 (dt, (name, asset, capacity))| {
                    acc.entry(dt)
                        .or_default()
                        .entry(name)
                        .or_default()
                        .push((asset, capacity));
                    acc
                },
            );

        let summarized_data = summarize_data(channels_data, nodes_count);
        if summarized_data.is_empty() {
            continue;
        }
        let insert_sql = format!(
            "Insert into {} (day, channels_count, asset_analysis, capacity_analysis, nodes_count) ",
            net.daily_summarized_data()
        );
        let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new(&insert_sql);

        query_builder.push_values(summarized_data.iter().take(65535 / 5), |mut b, sd| {
            b.push_bind(sd.date)
                .push_bind(sqlx::types::Json(&sd.channels_count))
                .push_bind(sqlx::types::Json(&sd.asset_analysis))
                .push_bind(sqlx::types::Json(&sd.capacity_analysis))
                .push_bind(sd.nodes_count);
        });

        query_builder.push(" On Conflict (day) Do Nothing");
        query_builder.build().execute(pool).await?;
    }

    Ok(())
}

#[derive(Debug)]
pub struct DailySummary {
    pub date: DateTime<Utc>,
    pub channels_count: HashMap<String, i64>,
    pub nodes_count: i64,
    pub asset_analysis: Vec<DailySummaryInner>,
    pub capacity_analysis: Vec<DailySummaryInner>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DailySummaryInner {
    pub name: String,
    pub average: String,
    pub min: String,
    pub max: String,
    pub median: String,
    pub sum: String, // hex encoded
}

fn summarize_data(
    channels_data: HashMap<DateTime<Utc>, HashMap<String, Vec<(u128, u64)>>>,
    nodes_data: Vec<(DateTime<Utc>, i64)>,
) -> Vec<DailySummary> {
    use std::collections::HashMap;

    let nodes_by_date: HashMap<DateTime<Utc>, i64> = nodes_data.into_iter().collect();

    let mut all_dates: HashSet<DateTime<Utc>> = channels_data.keys().copied().collect();
    all_dates.extend(nodes_by_date.keys());
    all_dates
        .into_iter()
        .map(|dt| {
            let nodes_count = nodes_by_date.get(&dt).copied().unwrap_or(0);
            if let Some(values) = channels_data.get(&dt) {
                if values.is_empty() {
                    return DailySummary {
                        date: dt,
                        channels_count: HashMap::default(),
                        asset_analysis: Vec::default(),
                        capacity_analysis: Vec::default(),
                        nodes_count: 0,
                    };
                }
                let mut channel_count = HashMap::new();
                let mut asset_analysis = Vec::with_capacity(values.len());
                let mut capacity_analysis = Vec::with_capacity(values.len());
                for (name, caps) in values.iter() {
                    channel_count.insert(name.clone(), caps.len() as i64);

                    let assets: Vec<u128> = caps.iter().map(|(asset, _)| *asset).collect();

                    asset_analysis.push(calculate_u128_statistics(name.clone(), assets));

                    let capacities: Vec<u64> = caps.iter().map(|(_, capacity)| *capacity).collect();

                    capacity_analysis.push(calculate_u64_statistics(name.clone(), capacities));
                }

                DailySummary {
                    date: dt,
                    channels_count: channel_count,
                    asset_analysis,
                    capacity_analysis,
                    nodes_count,
                }
            } else {
                DailySummary {
                    date: dt,
                    channels_count: HashMap::default(),
                    asset_analysis: Vec::default(),
                    capacity_analysis: Vec::default(),
                    nodes_count,
                }
            }
        })
        .collect()
}

fn calculate_u128_statistics(name: String, mut values: Vec<u128>) -> DailySummaryInner {
    values.sort_unstable();
    let min = values[0];
    let max = values[values.len() - 1];
    let sum: u128 = values.iter().sum();
    let average = sum / values.len() as u128;
    let median = if values.len().is_multiple_of(2) {
        let mid1 = values[values.len() / 2 - 1];
        let mid2 = values[values.len() / 2];
        (mid1 + mid2) / 2
    } else {
        values[values.len() / 2]
    };

    DailySummaryInner {
        name,
        average: faster_hex::hex_string(average.to_be_bytes().as_ref()),
        min: faster_hex::hex_string(min.to_be_bytes().as_ref()),
        max: faster_hex::hex_string(max.to_be_bytes().as_ref()),
        median: faster_hex::hex_string(median.to_be_bytes().as_ref()),
        sum: faster_hex::hex_string(sum.to_be_bytes().as_ref()),
    }
}

fn calculate_u64_statistics(name: String, mut values: Vec<u64>) -> DailySummaryInner {
    values.sort_unstable();
    let min = values[0];
    let max = values[values.len() - 1];
    let sum: u64 = values.iter().sum();
    let average = sum / values.len() as u64;
    let median = if values.len().is_multiple_of(2) {
        let mid1 = values[values.len() / 2 - 1];
        let mid2 = values[values.len() / 2];
        (mid1 + mid2) / 2
    } else {
        values[values.len() / 2]
    };

    DailySummaryInner {
        name,
        average: faster_hex::hex_string(average.to_be_bytes().as_ref()),
        min: faster_hex::hex_string(min.to_be_bytes().as_ref()),
        max: faster_hex::hex_string(max.to_be_bytes().as_ref()),
        median: faster_hex::hex_string(median.to_be_bytes().as_ref()),
        sum: faster_hex::hex_string(sum.to_be_bytes().as_ref()),
    }
}

pub async fn channel_states_monitor(
    mut rpc: RpcClient,
    mut recv: tokio::sync::mpsc::Receiver<(Network, Vec<JsonBytes>)>,
) {
    let mut channel_states = {
        use sqlx::Row;
        let pool = get_pg_pool();
        let mainnet_sql = r#"
        SELECT channel_outpoint, funding_args, last_tx_hash, last_block_number,
            last_commitment_args, state
        FROM channel_states
        WHERE
            (state IN ('closed_cooperative', 'closed_uncooperative')
            AND last_commit_time >= now() - interval '30 days')
            OR
            state NOT IN ('closed_cooperative', 'closed_uncooperative')"#;
        let testnet_sql = r#"
        SELECT channel_outpoint, funding_args, last_tx_hash, last_block_number,
            last_commitment_args, state
        FROM channel_states_testnet
        WHERE
            (state IN ('closed_cooperative', 'closed_uncooperative')
            AND last_commit_time >= now() - interval '30 days')
            OR
            state NOT IN ('closed_cooperative', 'closed_uncooperative')"#;

        let mainnet_states = sqlx::query(mainnet_sql)
            .fetch_all(pool)
            .await
            .expect("failed to fetch mainnet channel states")
            .iter()
            .map(|row| {
                let raw_outpoint = row.get::<String, _>("channel_outpoint");
                let raw_funding_args = row.get::<String, _>("funding_args");
                let raw_last_block_number = row.get::<String, _>("last_block_number");
                let raw_last_commitment_args = row.get::<Option<String>, _>("last_commitment_args");
                let raw_tx_hash = row.get::<String, _>("last_tx_hash");
                let state = row.get::<String, _>("state");
                let outpoint = {
                    let mut buf = vec![0u8; raw_outpoint.len() / 2];
                    hex_decode(raw_outpoint.as_bytes(), &mut buf).unwrap();
                    JsonBytes::from_bytes(buf.into())
                };
                let funding_args = {
                    let mut buf = vec![0u8; raw_funding_args.len() / 2];
                    hex_decode(raw_funding_args.as_bytes(), &mut buf).unwrap();
                    JsonBytes::from_bytes(buf.into())
                };
                let last_block_number = {
                    let mut buf = [0u8; 8];
                    hex_decode(raw_last_block_number.as_bytes(), &mut buf).unwrap();
                    u64::from_be_bytes(buf)
                };
                let tx_hash = {
                    let mut buf = [0u8; 32];
                    hex_decode(raw_tx_hash.as_bytes(), &mut buf).unwrap();
                    H256::from(buf)
                };
                match state.as_str() {
                    "open" => (
                        outpoint,
                        ChannelState {
                            state: State::Funding {
                                funding_args,
                                tx_hash,
                                block_number: last_block_number.into(),
                            },
                            net: Network::Mainnet,
                        },
                    ),
                    "closed_waiting_onchain_settlement" => {
                        let last_commitment_args = raw_last_commitment_args.as_ref().map(|s| {
                            let mut buf = vec![0u8; s.len() / 2];
                            hex_decode(s.as_bytes(), &mut buf).unwrap();
                            JsonBytes::from_bytes(buf.into())
                        });
                        (
                            outpoint,
                            ChannelState {
                                state: State::ClosedWaitingOnchainSettlement {
                                    tx_hash,
                                    block_number: last_block_number.into(),
                                    commitment_args: last_commitment_args.unwrap(),
                                },
                                net: Network::Mainnet,
                            },
                        )
                    }
                    "closed_cooperative" => (
                        outpoint,
                        ChannelState {
                            state: State::ClosedCooperative,
                            net: Network::Mainnet,
                        },
                    ),
                    "closed_uncooperative" => (
                        outpoint,
                        ChannelState {
                            state: State::ClosedUncooperative,
                            net: Network::Mainnet,
                        },
                    ),
                    _ => panic!("Unknown state: {}", state),
                }
            })
            .collect::<Vec<_>>();
        let testnet_states = sqlx::query(testnet_sql)
            .fetch_all(pool)
            .await
            .expect("failed to fetch testnet channel states")
            .iter()
            .map(|row| {
                let raw_outpoint = row.get::<String, _>("channel_outpoint");
                let raw_funding_args = row.get::<String, _>("funding_args");
                let raw_last_block_number = row.get::<String, _>("last_block_number");
                let raw_last_commitment_args = row.get::<Option<String>, _>("last_commitment_args");
                let raw_tx_hash = row.get::<String, _>("last_tx_hash");
                let state = row.get::<String, _>("state");
                let outpoint = {
                    let mut buf = vec![0u8; raw_outpoint.len() / 2];
                    hex_decode(raw_outpoint.as_bytes(), &mut buf).unwrap();
                    JsonBytes::from_bytes(buf.into())
                };
                let funding_args = {
                    let mut buf = vec![0u8; raw_funding_args.len() / 2];
                    hex_decode(raw_funding_args.as_bytes(), &mut buf).unwrap();
                    JsonBytes::from_bytes(buf.into())
                };
                let last_block_number = {
                    let mut buf = [0u8; 8];
                    hex_decode(raw_last_block_number.as_bytes(), &mut buf).unwrap();
                    u64::from_be_bytes(buf)
                };
                let tx_hash = {
                    let mut buf = [0u8; 32];
                    hex_decode(raw_tx_hash.as_bytes(), &mut buf).unwrap();
                    H256::from(buf)
                };
                match state.as_str() {
                    "open" => (
                        outpoint,
                        ChannelState {
                            state: State::Funding {
                                funding_args,
                                tx_hash,
                                block_number: last_block_number.into(),
                            },
                            net: Network::Testnet,
                        },
                    ),
                    "closed_waiting_onchain_settlement" => {
                        let last_commitment_args = raw_last_commitment_args.as_ref().map(|s| {
                            let mut buf = vec![0u8; s.len() / 2];
                            hex_decode(s.as_bytes(), &mut buf).unwrap();
                            JsonBytes::from_bytes(buf.into())
                        });
                        (
                            outpoint,
                            ChannelState {
                                state: State::ClosedWaitingOnchainSettlement {
                                    tx_hash,
                                    block_number: last_block_number.into(),
                                    commitment_args: last_commitment_args.unwrap(),
                                },
                                net: Network::Testnet,
                            },
                        )
                    }
                    "closed_cooperative" => (
                        outpoint,
                        ChannelState {
                            state: State::ClosedCooperative,
                            net: Network::Testnet,
                        },
                    ),
                    "closed_uncooperative" => (
                        outpoint,
                        ChannelState {
                            state: State::ClosedUncooperative,
                            net: Network::Testnet,
                        },
                    ),
                    _ => panic!("Unknown state: {}", state),
                }
            })
            .collect::<Vec<_>>();
        ChannelStates {
            channels: mainnet_states.into_iter().chain(testnet_states).collect(),
        }
    };

    let mut internal = tokio::time::interval(std::time::Duration::from_secs(10 * 60));
    internal.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat_timer = tokio::time::interval(std::time::Duration::from_secs(60));
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = internal.tick() => {
                log::info!("channel states updated");
                channel_tx_update(&mut channel_states, &mut rpc).await;
            }
            _ = heartbeat_timer.tick() => {
                CHANNEL_MONITOR_HEARTBEAT.store(Utc::now().timestamp() as u64, std::sync::atomic::Ordering::Release);
            }
            Some((net, new)) = recv.recv() => {
                let new = new.into_iter().filter_map(|op| {
                    if channel_states.channels.contains_key(&op) {
                        None
                    } else {
                        Some(op)
                    }
                }).collect::<Vec<_>>();
                log::info!("{:?}, new channels received: {}", net, new.len());
                if !new.is_empty() {
                    let groups = new_channels(net, new, &rpc).await;
                    for group in groups {
                        let (outpoint, state) = group.into_state();
                        channel_states.channels.insert(outpoint, state);
                    }
                }
            }
        }
    }
}

pub static CHANNEL_MONITOR_HEARTBEAT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[derive(Clone)]
enum UpdateType {
    Nothing,
    Update((Network, JsonBytes, ChannelStateUpdate)),
}

async fn channel_tx_update(channel_states: &mut ChannelStates, rpc: &mut RpcClient) {
    let (testnet_tip, mainnet_tip) = loop {
        let testnet_tip = {
            rpc.set_bearer_token(CKB_TESTNET_RPC_BEARER_TOKEN.clone());
            rpc.get_indexer_tip(CKB_TESTNET_RPC.clone()).await
        };
        let mainnet_tip = {
            rpc.set_bearer_token(CKB_MAINNET_RPC_BEARER_TOKEN.clone());
            rpc.get_indexer_tip(CKB_MAINNET_RPC.clone()).await
        };
        if let (Ok(testnet_tip), Ok(mainnet_tip)) = (testnet_tip, mainnet_tip) {
            break (testnet_tip, mainnet_tip);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };

    let mut handles = Vec::with_capacity(channel_states.channels.len() / 3);
    for (outpoint, state) in channel_states.channels.iter() {
        if matches!(
            state.state,
            State::ClosedCooperative | State::ClosedUncooperative
        ) {
            continue;
        }
        let outpoint = outpoint.clone();
        let state = state.clone();
        let mut rpc = rpc.clone();
        let handle = tokio::spawn(async move {
            let mut csus = UpdateType::Nothing;
            match state.state {
                State::ClosedCooperative | State::ClosedUncooperative => {}
                State::Funding { funding_args, .. } => {
                    let url = match state.net {
                        Network::Mainnet => {
                            rpc.set_bearer_token(CKB_MAINNET_RPC_BEARER_TOKEN.clone());
                            CKB_MAINNET_RPC.clone()
                        }
                        Network::Testnet => {
                            rpc.set_bearer_token(CKB_TESTNET_RPC_BEARER_TOKEN.clone());
                            CKB_TESTNET_RPC.clone()
                        }
                    };
                    let txs = loop {
                        let txs = rpc
                            .get_transactions(
                                url.clone(),
                                SearchKey {
                                    script: funding_script(state.net, funding_args.clone()),
                                    script_type: ScriptType::Lock,
                                    script_search_mode: Some(IndexerScriptSearchMode::Exact),
                                    filter: None,
                                    with_data: Some(false),
                                    group_by_transaction: Some(true),
                                },
                                Order::Desc,
                                100.into(),
                                None,
                            )
                            .await;

                        if let Ok(txs) = txs {
                            break txs;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    };
                    let code_hash = match state.net {
                        Network::Mainnet => &*MAINNET_COMMITMENT_CODE_HASH,
                        Network::Testnet => &*TESTNET_COMMITMENT_CODE_HASH,
                    };
                    if txs.objects.len() == 2
                        && let Tx::Grouped(tc) = &txs.objects[0]
                    {
                        let new_tx = loop {
                            let tx = rpc.get_transaction(url.clone(), &tc.tx_hash).await;
                            if let Ok(tx) = tx {
                                break tx.unwrap();
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        };

                        let header = loop {
                            let header =
                                rpc.get_header_by_number(url.clone(), tc.block_number).await;
                            if let Ok(header) = header {
                                break header;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        };

                        let commitment_args: Option<JsonBytes> =
                            new_tx.inner.outputs.iter().find_map(|output| {
                                if &output.lock.code_hash == code_hash {
                                    Some(output.lock.args.clone())
                                } else {
                                    None
                                }
                            });
                        match commitment_args {
                            None => match csus {
                                UpdateType::Nothing => {
                                    csus = UpdateType::Update((
                                        state.net,
                                        outpoint.clone(),
                                        ChannelStateUpdate {
                                            outpoint: outpoint.clone(),
                                            state: DBState::ClosedCooperative,
                                            last_block_number: tc.block_number,
                                            last_commit: header.inner.timestamp.value(),
                                            last_commitment_args: None,
                                            txs: vec![(
                                                tc.tx_hash.clone(),
                                                tc.block_number,
                                                header.inner.timestamp.value(),
                                                None,
                                                None,
                                            )],
                                        },
                                    ));
                                }
                                UpdateType::Update((_, _, ref mut s)) => {
                                    s.state = DBState::ClosedCooperative;
                                    s.last_block_number = tc.block_number;
                                    s.last_commit = header.inner.timestamp.value();
                                    s.txs.push((
                                        tc.tx_hash.clone(),
                                        tc.block_number,
                                        header.inner.timestamp.value(),
                                        None,
                                        None,
                                    ));
                                }
                            },
                            Some(commitment_args) => match csus {
                                UpdateType::Nothing => {
                                    csus = UpdateType::Update((
                                        state.net,
                                        outpoint.clone(),
                                        ChannelStateUpdate {
                                            outpoint: outpoint.clone(),
                                            state: DBState::ClosedWaitingOnchainSettlement,
                                            last_block_number: tc.block_number,
                                            last_commit: header.inner.timestamp.value(),
                                            last_commitment_args: Some(commitment_args.clone()),
                                            txs: vec![(
                                                tc.tx_hash.clone(),
                                                tc.block_number,
                                                header.inner.timestamp.value(),
                                                None,
                                                Some(commitment_args.clone()),
                                            )],
                                        },
                                    ));
                                }
                                UpdateType::Update((_, _, ref mut s)) => {
                                    s.state = DBState::ClosedWaitingOnchainSettlement;
                                    s.last_block_number = tc.block_number;
                                    s.last_commitment_args = Some(commitment_args.clone());
                                    s.last_commit = header.inner.timestamp.value();
                                    s.txs.push((
                                        tc.tx_hash.clone(),
                                        tc.block_number,
                                        header.inner.timestamp.value(),
                                        None,
                                        Some(commitment_args.clone()),
                                    ));
                                }
                            },
                        }
                    }

                    let current_state = if let UpdateType::Update((_, _, ref s)) = csus {
                        s.state
                    } else {
                        DBState::Open
                    };
                    // Continue to retrieve commitment transactions
                    if current_state == DBState::ClosedWaitingOnchainSettlement
                        && let UpdateType::Update((_, _, csu)) = csus.clone()
                    {
                        let commitment_args = csu.last_commitment_args.unwrap();
                        let block_number = csu.last_block_number;
                        let tx_hash = csu.txs.last().unwrap().0.clone();
                        commitment_branch(
                            &rpc,
                            state.net,
                            &outpoint,
                            url,
                            commitment_args,
                            block_number,
                            match state.net {
                                Network::Mainnet => mainnet_tip.block_number,
                                Network::Testnet => testnet_tip.block_number,
                            },
                            tx_hash,
                            code_hash,
                            &mut csus,
                        )
                        .await;
                    }
                }
                State::ClosedWaitingOnchainSettlement {
                    commitment_args,
                    block_number,
                    tx_hash,
                } => {
                    let code_hash = match state.net {
                        Network::Mainnet => &MAINNET_COMMITMENT_CODE_HASH,
                        Network::Testnet => &TESTNET_COMMITMENT_CODE_HASH,
                    };
                    let url = match state.net {
                        Network::Mainnet => {
                            rpc.set_bearer_token(CKB_MAINNET_RPC_BEARER_TOKEN.clone());
                            CKB_MAINNET_RPC.clone()
                        }
                        Network::Testnet => {
                            rpc.set_bearer_token(CKB_TESTNET_RPC_BEARER_TOKEN.clone());
                            CKB_TESTNET_RPC.clone()
                        }
                    };
                    commitment_branch(
                        &rpc,
                        state.net,
                        &outpoint,
                        url,
                        commitment_args,
                        block_number,
                        match state.net {
                            Network::Mainnet => mainnet_tip.block_number,
                            Network::Testnet => testnet_tip.block_number,
                        },
                        tx_hash,
                        code_hash,
                        &mut csus,
                    )
                    .await;
                }
            }
            csus
        });
        handles.push(handle);
    }

    let mut testnet: HashMap<JsonBytes, ChannelStateUpdate> = HashMap::new();
    let mut mainnet: HashMap<JsonBytes, ChannelStateUpdate> = HashMap::new();

    futures::stream::iter(handles)
        .buffer_unordered(2048)
        .filter_map(|res| async move {
            match res {
                Ok(ut) => match ut {
                    UpdateType::Nothing => None,
                    UpdateType::Update((net, outpoint, csu)) => Some((net, outpoint, csu)),
                },
                Err(e) => {
                    log::error!("channel update task failed: {}", e);
                    None
                }
            }
        })
        .for_each(|(net, outpoint, csu)| {
            channel_states.channels.get_mut(&outpoint).unwrap().state = match csu.state {
                DBState::ClosedCooperative => State::ClosedCooperative,
                DBState::ClosedUncooperative => State::ClosedUncooperative,
                DBState::ClosedWaitingOnchainSettlement => State::ClosedWaitingOnchainSettlement {
                    tx_hash: csu.txs.last().unwrap().0.clone(),
                    block_number: csu.last_block_number,
                    commitment_args: csu.last_commitment_args.clone().unwrap(),
                },
                DBState::Open => panic!("Invalid state transition to Open"),
            };
            match net {
                Network::Mainnet => {
                    mainnet.insert(outpoint, csu);
                }
                Network::Testnet => {
                    testnet.insert(outpoint, csu);
                }
            }
            futures::future::ready(())
        })
        .await;

    if !mainnet.is_empty() || !testnet.is_empty() {
        log::info!(
            "channel states updated: testnet: {}, mainnet: {}",
            testnet.len(),
            mainnet.len()
        );
        let pool = get_pg_pool();
        let mut conn = pool.begin().await.unwrap();
        if !mainnet.is_empty() {
            let updates = mainnet.values().collect::<Vec<_>>();
            ChannelStateUpdate::state_sql(&updates, &mut conn, Network::Mainnet)
                .await
                .unwrap();
            ChannelStateUpdate::txs_sql(&updates, &mut conn, Network::Mainnet)
                .await
                .unwrap();
        }
        if !testnet.is_empty() {
            let updates = testnet.values().collect::<Vec<_>>();
            ChannelStateUpdate::state_sql(&updates, &mut conn, Network::Testnet)
                .await
                .unwrap();
            ChannelStateUpdate::txs_sql(&updates, &mut conn, Network::Testnet)
                .await
                .unwrap();
        }
        conn.commit().await.unwrap();
    }
}

#[allow(clippy::too_many_arguments)]
async fn commitment_branch(
    rpc: &RpcClient,
    net: Network,
    outpoint: &JsonBytes,
    url: reqwest::Url,
    mut commitment_args: JsonBytes,
    start: BlockNumber,
    end: BlockNumber,
    tx_hash: H256,
    code_hash: &H256,
    csus: &mut UpdateType,
) {
    let mut exist_tx = vec![tx_hash];
    let mut already_search_commitment = Vec::new();
    loop {
        if already_search_commitment.contains(&commitment_args) {
            break;
        }
        already_search_commitment.push(commitment_args.clone());
        let txs = loop {
            let txs = rpc
                .get_transactions(
                    url.clone(),
                    SearchKey {
                        script: commitment_script(net, commitment_args.clone()),
                        script_type: ScriptType::Lock,
                        script_search_mode: Some(IndexerScriptSearchMode::Exact),
                        filter: Some(SearchKeyFilter::block_range(start, end)),
                        with_data: Some(false),
                        group_by_transaction: Some(true),
                    },
                    Order::Asc,
                    100.into(),
                    None,
                )
                .await;
            if let Ok(txs) = txs {
                break txs;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        };

        for tx in txs.objects {
            if let Tx::Grouped(tc) = &tx {
                if exist_tx.contains(&tc.tx_hash) {
                    continue;
                }
                exist_tx.push(tc.tx_hash.clone());

                let new_tx = loop {
                    let tx = rpc.get_transaction(url.clone(), &tc.tx_hash).await;
                    if let Ok(tx) = tx {
                        break tx.unwrap();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                };
                let header = loop {
                    let header = rpc.get_header_by_number(url.clone(), tc.block_number).await;
                    if let Ok(header) = header {
                        break header;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                };
                let mut witness_args = None;
                for (ty, idx) in tc.cells.iter() {
                    if let CellType::Input = ty {
                        witness_args = new_tx.inner.witnesses.get(idx.value() as usize).cloned();
                    }
                }

                let next_commitment_args: Option<JsonBytes> =
                    new_tx.inner.outputs.iter().find_map(|output| {
                        if &output.lock.code_hash == code_hash {
                            Some(output.lock.args.clone())
                        } else {
                            None
                        }
                    });
                match next_commitment_args {
                    None => match csus {
                        UpdateType::Nothing => {
                            *csus = UpdateType::Update((
                                net,
                                outpoint.clone(),
                                ChannelStateUpdate {
                                    outpoint: outpoint.clone(),
                                    state: DBState::ClosedUncooperative,
                                    last_block_number: tc.block_number,
                                    last_commit: header.inner.timestamp.value(),
                                    last_commitment_args: None,
                                    txs: vec![(
                                        tc.tx_hash.clone(),
                                        tc.block_number,
                                        header.inner.timestamp.value(),
                                        witness_args.clone(),
                                        None,
                                    )],
                                },
                            ));
                        }
                        UpdateType::Update((_, _, s)) => {
                            s.state = DBState::ClosedUncooperative;
                            s.last_block_number = tc.block_number;
                            s.last_commit = header.inner.timestamp.value();
                            s.txs.push((
                                tc.tx_hash.clone(),
                                tc.block_number,
                                header.inner.timestamp.value(),
                                witness_args.clone(),
                                None,
                            ));
                        }
                    },

                    Some(next_commitment_args) => {
                        commitment_args = next_commitment_args.clone();

                        match csus {
                            UpdateType::Nothing => {
                                *csus = UpdateType::Update((
                                    net,
                                    outpoint.clone(),
                                    ChannelStateUpdate {
                                        outpoint: outpoint.clone(),
                                        state: DBState::ClosedWaitingOnchainSettlement,
                                        last_block_number: tc.block_number,
                                        last_commit: header.inner.timestamp.value(),
                                        last_commitment_args: Some(next_commitment_args.clone()),
                                        txs: vec![(
                                            tc.tx_hash.clone(),
                                            tc.block_number,
                                            header.inner.timestamp.value(),
                                            witness_args.clone(),
                                            Some(next_commitment_args.clone()),
                                        )],
                                    },
                                ));
                            }
                            UpdateType::Update((_, _, s)) => {
                                s.state = DBState::ClosedWaitingOnchainSettlement;
                                s.last_block_number = tc.block_number;
                                s.last_commit = header.inner.timestamp.value();
                                s.last_commitment_args = Some(next_commitment_args.clone());
                                s.txs.push((
                                    tc.tx_hash.clone(),
                                    tc.block_number,
                                    header.inner.timestamp.value(),
                                    witness_args.clone(),
                                    Some(next_commitment_args.clone()),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Funding {
        tx_hash: H256,
        block_number: BlockNumber,
        funding_args: JsonBytes,
    },
    ClosedWaitingOnchainSettlement {
        tx_hash: H256,
        block_number: BlockNumber,
        commitment_args: JsonBytes,
    },
    ClosedCooperative,
    ClosedUncooperative,
}

#[derive(Debug, Clone)]
struct ChannelState {
    net: Network,
    state: State,
}

struct ChannelStates {
    channels: HashMap<JsonBytes, ChannelState>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DBState {
    #[serde(alias = "open")]
    Open,
    #[serde(alias = "closed_waiting_onchain_settlement")]
    ClosedWaitingOnchainSettlement,
    #[serde(alias = "closed_cooperative")]
    ClosedCooperative,
    #[serde(alias = "closed_uncooperative")]
    ClosedUncooperative,
}
impl DBState {
    pub fn to_sql(&self) -> &str {
        match self {
            DBState::Open => "open",
            DBState::ClosedWaitingOnchainSettlement => "closed_waiting_onchain_settlement",
            DBState::ClosedCooperative => "closed_cooperative",
            DBState::ClosedUncooperative => "closed_uncooperative",
        }
    }
}

impl std::str::FromStr for DBState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(DBState::Open),
            "closed_waiting_onchain_settlement" => Ok(DBState::ClosedWaitingOnchainSettlement),
            "closed_cooperative" => Ok(DBState::ClosedCooperative),
            "closed_uncooperative" => Ok(DBState::ClosedUncooperative),
            _ => Err(format!("Invalid DBState string: {}", s)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChannelStateUpdate {
    outpoint: JsonBytes,
    state: DBState,
    last_commit: u64,
    last_block_number: BlockNumber,
    last_commitment_args: Option<JsonBytes>,
    txs: Vec<(H256, BlockNumber, u64, Option<JsonBytes>, Option<JsonBytes>)>, // (tx_hash, block_number, timestamp, witness_args, commitment_args)
}

impl ChannelStateUpdate {
    async fn state_sql(
        updates: &[&ChannelStateUpdate],
        conn: &mut sqlx::PgConnection,
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if updates.is_empty() {
            return Ok(());
        }

        let sql = format!(
            "UPDATE {} SET 
                last_tx_hash = $1,
                last_block_number = $2,
                last_commitment_args = $3,
                state = $4,
                last_commit_time = $5
            WHERE channel_outpoint = $6",
            net.channel_states()
        );

        for cu in updates {
            sqlx::query(&sql)
                .bind(hex_string(cu.txs.last().unwrap().0.as_bytes()))
                .bind(hex_string(
                    cu.last_block_number.value().to_be_bytes().as_ref(),
                ))
                .bind(
                    cu.last_commitment_args
                        .as_ref()
                        .map(|args| hex_string(args.as_bytes())),
                )
                .bind(cu.state.to_sql())
                .bind(chrono::DateTime::from_timestamp_millis(
                    cu.last_commit as i64,
                ))
                .bind(hex_string(cu.outpoint.as_bytes()))
                .execute(&mut *conn)
                .await?;
        }

        Ok(())
    }

    async fn txs_sql(
        updates: &[&ChannelStateUpdate],
        conn: &mut sqlx::PgConnection,
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if updates.is_empty() {
            return Ok(());
        }

        let sql = format!(
            "insert into {} (channel_outpoint, tx_hash, block_number, timestamp, witness_args, commitment_args) ",
            net.channel_txs()
        );
        let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new(sql);
        let combin = updates
            .iter()
            .flat_map(|cu| std::iter::repeat(cu.outpoint.clone()).zip(cu.txs.iter()))
            .map(
                |(outpoint, (tx_hash, block_number, timestamp, witness_args, commitment_args))| {
                    (
                        outpoint,
                        tx_hash,
                        block_number,
                        chrono::DateTime::from_timestamp_millis(*timestamp as i64),
                        witness_args,
                        commitment_args,
                    )
                },
            );
        query_builder.push_values(
            combin.take(65535 / 6),
            |mut b, (outpoint, tx_hash, block_number, timestamp, witness_args, commitment_args)| {
                b.push_bind(hex_string(outpoint.as_bytes()))
                    .push_bind(hex_string(tx_hash.as_bytes()))
                    .push_bind(hex_string(block_number.value().to_be_bytes().as_ref()))
                    .push_bind(timestamp)
                    .push_bind(witness_args.as_ref().map(|a| hex_string(a.as_bytes())))
                    .push_bind(commitment_args.as_ref().map(|a| hex_string(a.as_bytes())));
            },
        );
        let query = query_builder.build();
        let _ = query.execute(conn).await?;
        Ok(())
    }
}

pub struct ChannelGroup {
    net: Network,
    outpoint: JsonBytes,
    funding_args: JsonBytes,
    capacity: u64,
    create_time: u64,
    last_commit_time: u64,
    udt_value: Option<u128>,
    last_block_number: BlockNumber,
    last_commitment_args: Option<JsonBytes>,
    state: DBState,
    txs: Vec<(H256, BlockNumber, u64, Option<JsonBytes>, Option<JsonBytes>)>, // (tx_hash, block_number, commit_time, witness_args, commitment_args)
}

impl ChannelGroup {
    fn into_state(self) -> (JsonBytes, ChannelState) {
        (
            self.outpoint,
            ChannelState {
                net: self.net,
                state: match self.state {
                    DBState::Open => State::Funding {
                        tx_hash: self.txs[0].0.clone(),
                        block_number: self.txs[0].1,
                        funding_args: self.funding_args,
                    },
                    DBState::ClosedWaitingOnchainSettlement => {
                        State::ClosedWaitingOnchainSettlement {
                            tx_hash: self.txs.last().unwrap().0.clone(),
                            block_number: self.txs.last().unwrap().1,
                            commitment_args: self.txs.last().unwrap().4.clone().unwrap(),
                        }
                    }
                    DBState::ClosedUncooperative => State::ClosedUncooperative,
                    DBState::ClosedCooperative => State::ClosedCooperative,
                },
            },
        )
    }

    async fn state_sql(
        groups: &[ChannelGroup],
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), sqlx::Error> {
        let sql = format!(
            "insert into {} (channel_outpoint, funding_args, capacity, last_tx_hash, last_block_number, udt_value, create_time, last_commit_time, last_commitment_args, state) ",
            groups[0].net.channel_states()
        );

        let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new(sql);
        query_builder.push_values(groups.iter(), |mut b, cg| {
            b.push_bind(hex_string(cg.outpoint.as_bytes()))
                .push_bind(hex_string(cg.funding_args.as_bytes()))
                .push_bind(hex_string(cg.capacity.to_be_bytes().as_ref()))
                .push_bind(hex_string(cg.txs.last().unwrap().0.as_bytes()))
                .push_bind(hex_string(
                    cg.last_block_number.value().to_be_bytes().as_ref(),
                ))
                .push_bind(
                    cg.udt_value
                        .map(|v| v.to_be_bytes())
                        .as_ref()
                        .map(|b| hex_string(b.as_ref())),
                )
                .push_bind(chrono::DateTime::from_timestamp_millis(
                    cg.create_time as i64,
                ))
                .push_bind(chrono::DateTime::from_timestamp_millis(
                    cg.last_commit_time as i64,
                ))
                .push_bind(
                    cg.last_commitment_args
                        .as_ref()
                        .map(|args| hex_string(args.as_bytes())),
                )
                .push_bind(cg.state.to_sql());
        });
        let query = query_builder.build();
        query.execute(conn).await?;
        Ok(())
    }

    async fn txs_sql(
        groups: &[ChannelGroup],
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), sqlx::Error> {
        let sql = format!(
            "insert into {} (channel_outpoint, tx_hash, block_number, timestamp, witness_args, commitment_args) ",
            groups[0].net.channel_txs()
        );
        let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new(sql);
        let combin = groups
            .iter()
            .flat_map(|cg| std::iter::repeat(cg.outpoint.clone()).zip(cg.txs.clone()))
            .map(
                |(outpoint, (tx_hash, block_number, timestamp, witness_args, commitment_args))| {
                    (
                        outpoint,
                        tx_hash,
                        block_number,
                        chrono::DateTime::from_timestamp_millis(timestamp as i64),
                        witness_args,
                        commitment_args,
                    )
                },
            );
        query_builder.push_values(
            combin,
            |mut b, (outpoint, tx_hash, block_number, timestamp, witness_args, commitment_args)| {
                b.push_bind(hex_string(outpoint.as_bytes()))
                    .push_bind(hex_string(tx_hash.as_bytes()))
                    .push_bind(hex_string(block_number.value().to_be_bytes().as_ref()))
                    .push_bind(timestamp)
                    .push_bind(witness_args.as_ref().map(|a| hex_string(a.as_bytes())))
                    .push_bind(commitment_args.as_ref().map(|a| hex_string(a.as_bytes())));
            },
        );
        let query = query_builder.build();
        query.execute(conn).await?;
        Ok(())
    }
}

pub async fn new_channels(
    net: Network,
    channels: Vec<JsonBytes>,
    rpc: &RpcClient,
) -> Vec<ChannelGroup> {
    let url = match net {
        Network::Mainnet => CKB_MAINNET_RPC.clone(),
        Network::Testnet => CKB_TESTNET_RPC.clone(),
    };
    let code_hash = match net {
        Network::Mainnet => &*MAINNET_COMMITMENT_CODE_HASH,
        Network::Testnet => &*TESTNET_COMMITMENT_CODE_HASH,
    };
    let mut handles = Vec::with_capacity(channels.len());
    for outpoint in channels {
        let rpc = rpc.clone();
        let url = url.clone();
        let code_hash = code_hash.clone();
        let handle = tokio::spawn(async move {
            let raw_outpoint = packed::OutPoint::from_slice(outpoint.as_bytes()).unwrap();

            let funding_tx = loop {
                let tx = rpc
                    .get_transaction(url.clone(), &raw_outpoint.as_reader().tx_hash().into())
                    .await;
                if let Ok(tx) = tx {
                    break tx.unwrap();
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };
            let (funding_args, capacity) = funding_tx
                .inner
                .outputs
                .get(Into::<u32>::into(raw_outpoint.as_reader().index()) as usize)
                .map(|output| (output.lock.args.clone(), output.capacity.value()))
                .unwrap();
            let udt_value = funding_tx
                .inner
                .outputs_data
                .get(Into::<u32>::into(raw_outpoint.as_reader().index()) as usize)
                .and_then(|data| {
                    if data.len() >= 16 {
                        let mut buf = [0u8; 16];
                        buf.copy_from_slice(&data.as_bytes()[0..16]);
                        Some(u128::from_le_bytes(buf))
                    } else {
                        None
                    }
                });
            let txs = loop {
                let txs = rpc
                    .get_transactions(
                        url.clone(),
                        SearchKey {
                            script: funding_script(net, funding_args.clone()),
                            script_type: ScriptType::Lock,
                            script_search_mode: Some(IndexerScriptSearchMode::Exact),
                            filter: None,
                            with_data: Some(false),
                            group_by_transaction: Some(true),
                        },
                        Order::Asc,
                        100.into(),
                        None,
                    )
                    .await;

                if let Ok(txs) = txs {
                    break txs;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };

            let mut group = ChannelGroup {
                net,
                outpoint,
                funding_args: funding_args.clone(),
                last_block_number: 0.into(),
                capacity,
                create_time: 0,
                last_commit_time: 0,
                last_commitment_args: None,
                udt_value,
                state: DBState::Open,
                txs: vec![(funding_tx.hash.clone(), 0.into(), 0, None, None)],
            };
            for tx in txs.objects {
                if let Tx::Grouped(tc) = &tx {
                    if tc.tx_hash == funding_tx.hash {
                        let header = loop {
                            let header =
                                rpc.get_header_by_number(url.clone(), tc.block_number).await;
                            if let Ok(header) = header {
                                break header;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        };
                        group.create_time = header.inner.timestamp.value();
                        group.last_commit_time = header.inner.timestamp.value();
                        group.last_block_number = tc.block_number;
                        group.txs[0].1 = tc.block_number;
                        group.txs[0].2 = header.inner.timestamp.value();
                        continue;
                    }
                    let new_tx = loop {
                        let tx = rpc.get_transaction(url.clone(), &tc.tx_hash).await;
                        if let Ok(tx) = tx {
                            break tx.unwrap();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    };
                    let header = loop {
                        let header = rpc.get_header_by_number(url.clone(), tc.block_number).await;
                        if let Ok(header) = header {
                            break header;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    };
                    let commitment_args: Option<JsonBytes> =
                        new_tx.inner.outputs.iter().find_map(|output| {
                            if output.lock.code_hash == code_hash {
                                Some(output.lock.args.clone())
                            } else {
                                None
                            }
                        });
                    match commitment_args {
                        None => {
                            group.state = DBState::ClosedCooperative;
                            group.last_block_number = tc.block_number;
                            group.last_commit_time = header.inner.timestamp.value();
                            group.txs.push((
                                tc.tx_hash.clone(),
                                tc.block_number,
                                header.inner.timestamp.value(),
                                None,
                                None,
                            ));
                        }
                        Some(args) => {
                            group.last_commitment_args = Some(args.clone());
                            group.last_block_number = tc.block_number;
                            group.last_commit_time = header.inner.timestamp.value();
                            group.state = DBState::ClosedWaitingOnchainSettlement;
                            group.txs.push((
                                tc.tx_hash.clone(),
                                tc.block_number,
                                header.inner.timestamp.value(),
                                None,
                                Some(args),
                            ));
                        }
                    }
                }
            }
            let mut commitment_args = vec![];
            while let Some(args) = group.last_commitment_args.clone() {
                if commitment_args.contains(&Some(args.clone())) {
                    break;
                }
                commitment_args.push(Some(args.clone()));
                let txs = loop {
                    let txs = rpc
                        .get_transactions(
                            url.clone(),
                            SearchKey {
                                script: commitment_script(net, args.clone()),
                                script_type: ScriptType::Lock,
                                script_search_mode: Some(IndexerScriptSearchMode::Exact),
                                filter: None,
                                with_data: Some(false),
                                group_by_transaction: Some(true),
                            },
                            Order::Asc,
                            100.into(),
                            None,
                        )
                        .await;
                    if let Ok(txs) = txs {
                        break txs;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                };
                for tx in txs.objects {
                    if let Tx::Grouped(tc) = &tx {
                        if group
                            .txs
                            .iter()
                            .any(|(hash, _, _, _, _)| hash == &tc.tx_hash)
                        {
                            continue;
                        }
                        let new_tx = loop {
                            let tx = rpc.get_transaction(url.clone(), &tc.tx_hash).await;
                            if let Ok(tx) = tx {
                                break tx.unwrap();
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        };
                        let header = loop {
                            let header =
                                rpc.get_header_by_number(url.clone(), tc.block_number).await;
                            if let Ok(header) = header {
                                break header;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        };
                        let mut witness_args = None;
                        for (ty, idx) in tc.cells.iter() {
                            if let CellType::Input = ty {
                                witness_args =
                                    new_tx.inner.witnesses.get(idx.value() as usize).cloned();
                            }
                        }
                        let commitment_args: Option<JsonBytes> =
                            new_tx.inner.outputs.iter().find_map(|output| {
                                if output.lock.code_hash == code_hash {
                                    Some(output.lock.args.clone())
                                } else {
                                    None
                                }
                            });
                        match commitment_args {
                            None => {
                                group.state = DBState::ClosedUncooperative;
                                group.last_block_number = tc.block_number;
                                group.last_commit_time = header.inner.timestamp.value();
                                group.txs.push((
                                    tc.tx_hash.clone(),
                                    tc.block_number,
                                    header.inner.timestamp.value(),
                                    witness_args,
                                    None,
                                ));
                            }
                            Some(args) => {
                                group.last_commitment_args = Some(args.clone());
                                group.last_block_number = tc.block_number;
                                group.last_commit_time = header.inner.timestamp.value();
                                group.state = DBState::ClosedWaitingOnchainSettlement;
                                group.txs.push((
                                    tc.tx_hash.clone(),
                                    tc.block_number,
                                    header.inner.timestamp.value(),
                                    witness_args,
                                    Some(args),
                                ));
                            }
                        }
                    }
                }
            }
            group
        });
        handles.push(handle);
    }

    let groups: Vec<ChannelGroup> = futures::stream::iter(handles)
        .buffer_unordered(2048)
        .map(|x| x.unwrap())
        .collect()
        .await;

    log::info!("{:?}, new channels processed: {}", net, groups.len());
    if !groups.is_empty() {
        let pool = get_pg_pool();
        let mut conn = pool.begin().await.unwrap();
        ChannelGroup::state_sql(&groups, &mut conn).await.unwrap();
        ChannelGroup::txs_sql(&groups, &mut conn).await.unwrap();
        conn.commit().await.unwrap();
    }
    groups
}

pub fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter().peekable();

    while iter.peek().is_some() {
        match iter.peek() {
            Some(Protocol::Ip4(_)) | Some(Protocol::Ip6(_)) => (),
            _ => {
                // ignore is true
                let _ignore = iter.next();
                continue;
            }
        }

        let proto1 = iter.next()?;
        let proto2 = iter.next()?;

        match (proto1, proto2) {
            (Protocol::Ip4(ip), Protocol::Tcp(port)) => {
                return Some(SocketAddr::new(ip.into(), port));
            }
            (Protocol::Ip6(ip), Protocol::Tcp(port)) => {
                return Some(SocketAddr::new(ip.into(), port));
            }
            _ => (),
        }
    }

    None
}
