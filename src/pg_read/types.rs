use sqlx::types::chrono::{DateTime, Utc};
use sqlx::{FromRow, Pool, Postgres, Row, postgres::PgRow};

use ckb_jsonrpc_types::{JsonBytes, Script};
use ckb_types::H256;
use multiaddr::MultiAddr;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::http_server::{FuzzyNodeName, ListNodesHourlyParams, NodeByRegion, Page};
use crate::{
    Network,
    types::{ChannelUpdateInfo, U64Hex, U128Hex},
};

const SELECT_HOURLY_NODES_SQL: &str = "
SELECT
  n.node_id as node_id,
  n.bucket AS last_seen_hour,
  n.node_name,
  n.addresses,
  n.announce_timestamp,
  n.chain_hash,
  n.auto_accept_min_ckb_funding_amount,
  n.country_or_region,
  n.city,
  n.region,
  n.loc,
  n.channel_count,
  COUNT(*) OVER() as total_count
FROM {nodes} n
ORDER BY {sort_by} {order}";

const SELECT_HOURLY_CHANNELS_SQL: &str = "SELECT
  {1}.channel_outpoint,
  bucket AS last_seen_hour,
  node1,
  node2,
  {1}.capacity as asset,
  {3}.capacity as capacity,
  chain_hash,
  created_timestamp,
  update_of_node1_timestamp,
  update_of_node1_enabled,
  update_of_node1_outbound_liquidity,
  update_of_node1_tlc_expiry_delta,
  update_of_node1_tlc_minimum_value,
  update_of_node1_fee_rate,
  update_of_node2_timestamp,
  update_of_node2_enabled,
  update_of_node2_outbound_liquidity,
  update_of_node2_tlc_expiry_delta,
  update_of_node2_tlc_minimum_value,
  update_of_node2_fee_rate,
  COALESCE({2}.name, 'ckb') AS udt_name,
  {2}.code_hash AS udt_code_hash,
  {2}.hash_type AS udt_hash_type,
  {2}.args AS udt_args,
  {2}.auto_accept_amount AS udt_auto_accept_amount,
  COUNT(*) OVER() as total_count
FROM {1}
left join {2} on {1}.udt_type_script = {2}.id
left join {3} on {1}.channel_outpoint = {3}.channel_outpoint
WHERE bucket >= $1::timestamp
ORDER BY {1}.channel_outpoint, bucket DESC";

const SELECT_MONTHLY_NODES_SQL: &str = "
WITH latest_channels AS (
  SELECT DISTINCT ON (channel_outpoint) channel_outpoint, node1, node2
  FROM {channels}
  WHERE bucket >= $1::timestamp and bucket < $2::timestamp
  ORDER BY channel_outpoint, bucket DESC
),
channel_nodes AS (
  SELECT node1 AS node, channel_outpoint FROM latest_channels
  UNION ALL
  SELECT node2 AS node, channel_outpoint FROM latest_channels
),
channel_counts AS (
  SELECT node, COUNT(*) AS channel_count
  FROM channel_nodes
  GROUP BY node
)
SELECT DISTINCT ON (n.node_id)
  n.node_id as node_id,
  n.bucket AS last_seen_hour,
  n.node_name,
  n.addresses,
  n.announce_timestamp,
  n.chain_hash,
  n.auto_accept_min_ckb_funding_amount,
  n.country_or_region,
  n.city,
  n.region,
  n.loc,
  COALESCE(c.channel_count, 0) AS channel_count,
  COUNT(*) OVER() as total_count
FROM {nodes} n
LEFT JOIN channel_counts c ON n.node_id = c.node
WHERE n.bucket >= $1::timestamp and n.bucket < $2::timestamp
ORDER BY n.node_id, n.bucket DESC";

const SELECT_MONTHLY_CHANNELS_SQL: &str = "SELECT DISTINCT ON ({1}.channel_outpoint)
  {1}.channel_outpoint,
  bucket AS last_seen_hour,
  node1,
  node2,
  {1}.capacity as asset,
  {3}.capacity as capacity,
  chain_hash,
  created_timestamp,
  update_of_node1_timestamp,
  update_of_node1_enabled,
  update_of_node1_outbound_liquidity,
  update_of_node1_tlc_expiry_delta,
  update_of_node1_tlc_minimum_value,
  update_of_node1_fee_rate,
  update_of_node2_timestamp,
  update_of_node2_enabled,
  update_of_node2_outbound_liquidity,
  update_of_node2_tlc_expiry_delta,
  update_of_node2_tlc_minimum_value,
  update_of_node2_fee_rate,
  COALESCE({2}.name, 'ckb') AS udt_name,
  {2}.code_hash AS udt_code_hash,
  {2}.hash_type AS udt_hash_type,
  {2}.args AS udt_args,
  {2}.auto_accept_amount AS udt_auto_accept_amount,
  COUNT(*) OVER() as total_count
FROM {1}
left join {2} on {1}.udt_type_script = {2}.id
left join {3} on {1}.channel_outpoint = {3}.channel_outpoint
WHERE bucket >= $1::timestamp and bucket < $2::timestamp
ORDER BY {1}.channel_outpoint, bucket DESC";
pub const PAGE_SIZE: usize = 500;

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HourlyNodeInfo {
    pub node_id: String,
    pub node_name: String,
    pub addresses: Vec<MultiAddr>,
    pub commit_timestamp: String,
    /// The latest timestamp set by the owner for the node announcement.
    /// When a Node is online this timestamp will be updated to the latest value.
    pub announce_timestamp: u64,
    /// The chain hash of the node.
    pub chain_hash: H256,
    /// The minimum CKB funding amount for automatically accepting open channel requests.
    pub auto_accept_min_ckb_funding_amount: u64,
    pub country_or_region: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub loc: Option<String>,
    pub channel_count: usize,
    pub last_seen_hour: String,
}

impl From<HourlyNodeInfoDBRead> for HourlyNodeInfo {
    fn from(info: HourlyNodeInfoDBRead) -> Self {
        HourlyNodeInfo {
            node_name: info.node_name,
            addresses: serde_json::from_str(&info.addresses).unwrap(),
            node_id: format!("0x{}", info.node_id),
            commit_timestamp: info.last_seen_hour.to_rfc3339(),
            announce_timestamp: info.announce_timestamp.timestamp_millis() as u64,
            chain_hash: {
                let mut hash_bytes = [0u8; 32];
                faster_hex::hex_decode(info.chain_hash.as_bytes(), &mut hash_bytes).unwrap();
                H256::from(hash_bytes)
            },
            auto_accept_min_ckb_funding_amount: {
                let mut amount_bytes = [0u8; 8];
                faster_hex::hex_decode(
                    info.auto_accept_min_ckb_funding_amount.as_bytes(),
                    &mut amount_bytes,
                )
                .unwrap();
                u64::from_be_bytes(amount_bytes)
            },
            country_or_region: info.country_or_region,
            city: info.city,
            region: info.region,
            loc: info.loc,
            channel_count: info.channel_count as usize,
            last_seen_hour: info.last_seen_hour.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct HourlyNodeInfoDBRead {
    pub node_id: String,
    pub last_seen_hour: DateTime<Utc>,
    pub node_name: String,
    pub addresses: String,
    pub announce_timestamp: DateTime<Utc>,
    pub chain_hash: String,
    pub auto_accept_min_ckb_funding_amount: String,
    pub country_or_region: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub loc: Option<String>,
    pub channel_count: i64,
}

impl HourlyNodeInfoDBRead {
    pub async fn fetch_by_id(
        pool: &Pool<Postgres>,
        node_id: JsonBytes,
        net: Network,
    ) -> Result<Option<Self>, sqlx::Error> {
        let sql = format!(
            "
            SELECT 
                n.node_id as node_id,
                n.bucket AS last_seen_hour,
                n.node_name,
                n.addresses,
                n.announce_timestamp,
                n.chain_hash,
                n.auto_accept_min_ckb_funding_amount,
                n.country_or_region,
                n.city,
                n.region,
                n.loc,
                n.channel_count
            FROM {} n
            WHERE node_id = $1
            ORDER BY last_seen_hour DESC
            LIMIT 1",
            net.mv_online_nodes()
        );
        let hour_bucket = Utc::now() - chrono::Duration::hours(3);
        let res = sqlx::query_as::<_, Self>(&sql)
            .bind(faster_hex::hex_string(node_id.as_bytes()))
            .bind(hour_bucket)
            .fetch_optional(pool)
            .await?;

        Ok(res)
    }

    pub(crate) async fn fetch_node_by_region(
        pool: &Pool<Postgres>,
        params: NodeByRegion,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let hour_bucket = Utc::now() - chrono::Duration::hours(3);
        let sql = format!(
            r#"
        SELECT
            node_id,
            bucket AS last_seen_hour,
            node_name,
            addresses,
            announce_timestamp,
            chain_hash,
            auto_accept_min_ckb_funding_amount,
            country_or_region,
            city,
            region,
            loc,
            channel_count,
            COUNT(*) OVER() as total_count
        FROM {}
        WHERE bucket >= $1::timestamp and country_or_region = $2
        ORDER BY {} {}
        LIMIT {} OFFSET {}
    "#,
            params.net.mv_online_nodes(),
            params.sort_by.as_str(),
            params.order.as_str(),
            page_size,
            offset
        );
        let rows = sqlx::query(&sql)
            .bind(hour_bucket)
            .bind(params.region)
            .fetch_all(pool)
            .await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }

    pub(crate) async fn fetch_node_fuzzy_by_name_or_id(
        pool: &Pool<Postgres>,
        params: FuzzyNodeName,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let hour_bucket = Utc::now() - chrono::Duration::hours(3);
        let node_name = if params.node_name.starts_with("0x") {
            &params.node_name[2..]
        } else {
            &params.node_name
        };
        let sql = format!(
            r#"
        SELECT
            node_id,
            bucket AS last_seen_hour,
            node_name,
            addresses,
            announce_timestamp,
            chain_hash,
            auto_accept_min_ckb_funding_amount,
            country_or_region,
            city,
            region,
            loc,
            channel_count,
            COUNT(*) OVER() as total_count
        FROM {} n
        WHERE n.bucket >= $1::timestamp AND ((POSITION($2 IN n.node_id) > 0) OR (POSITION($2 IN n.node_name) > 0))
        ORDER BY {} {}
        LIMIT {} OFFSET {}"#,
            params.net.mv_online_nodes(),
            params.sort_by.as_str(),
            params.order.as_str(),
            page_size,
            offset
        );
        let rows = sqlx::query(&sql)
            .bind(hour_bucket)
            .bind(node_name)
            .fetch_all(pool)
            .await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }

    pub(crate) async fn fetch_by_page_hourly(
        pool: &Pool<Postgres>,
        params: ListNodesHourlyParams,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let hour_bucket = Utc::now() - chrono::Duration::hours(3);
        let sql = SELECT_HOURLY_NODES_SQL
            .replace("{nodes}", params.net.mv_online_nodes())
            .replace("{sort_by}", params.sort_by.as_str())
            .replace("{order}", params.order.as_str());
        let sql = format!("{} LIMIT {} OFFSET {}", sql, page_size, offset);
        let rows = sqlx::query(&sql).bind(hour_bucket).fetch_all(pool).await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }

    pub async fn fetch_by_page_monthly(
        pool: &Pool<Postgres>,
        params: Page,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let now = Utc::now().date_naive();
        let start = params.start.unwrap_or(now - chrono::Duration::days(30));
        let mut end: chrono::NaiveDate = params.end.unwrap_or(now);
        if end - start > chrono::Duration::days(30) || start > end {
            end = start + chrono::Duration::days(30);
        }
        let base_sql = SELECT_MONTHLY_NODES_SQL
            .replace("{nodes}", params.net.online_nodes_hourly())
            .replace("{channels}", params.net.online_channels_hourly());
        let sql = format!("{} LIMIT {} OFFSET {}", base_sql, page_size, offset);
        let rows = sqlx::query(&sql)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }
}

fn rows_with_total<T>(rows: Vec<PgRow>) -> Result<(Vec<T>, usize), sqlx::Error>
where
    for<'r> T: FromRow<'r, PgRow>,
{
    let total_count = rows
        .first()
        .map(|row| row.get::<i64, _>("total_count"))
        .unwrap_or(0);
    let items = rows
        .iter()
        .map(|row| T::from_row(row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total_count as usize))
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelInfo {
    /// The outpoint of the channel.
    pub channel_outpoint: String,
    /// The identity public key of the first node.
    pub node1: String,
    /// The identity public key of the second node.
    pub node2: String,
    pub commit_timestamp: String,
    #[serde_as(as = "U64Hex")]
    pub created_timestamp: u64,

    /// The update info from node1 to node2, e.g. timestamp, fee_rate, tlc_expiry_delta, tlc_minimum_value
    pub update_info_of_node1: Option<ChannelUpdateInfo>,

    /// The update info from node2 to node1, e.g. timestamp, fee_rate, tlc_expiry_delta, tlc_minimum_value
    pub update_info_of_node2: Option<ChannelUpdateInfo>,

    /// The capacity of the channel.
    #[serde_as(as = "U64Hex")]
    pub capacity: u64,
    #[serde_as(as = "U128Hex")]
    pub asset: u128,
    /// The chain hash of the channel.
    pub chain_hash: H256,
    /// The UDT type script of the channel.
    pub udt_type_script: Option<Script>,
    pub udt_name: Option<String>,
    pub udt_auto_accept_amount: Option<String>,
}

impl From<HourlyChannelInfoDBRead> for ChannelInfo {
    fn from(info: HourlyChannelInfoDBRead) -> Self {
        ChannelInfo {
            channel_outpoint: format!("0x{}", info.channel_outpoint),
            node1: format!("0x{}", info.node1),
            node2: format!("0x{}", info.node2),
            asset: {
                let mut asset_bytes = [0u8; 16];
                faster_hex::hex_decode(info.asset.as_bytes(), &mut asset_bytes).unwrap();
                u128::from_be_bytes(asset_bytes)
            },
            capacity: {
                let mut capacity_bytes = [0u8; 8];
                faster_hex::hex_decode(info.capacity.as_bytes(), &mut capacity_bytes).unwrap();
                u64::from_be_bytes(capacity_bytes)
            },
            chain_hash: {
                let mut hash_bytes = [0u8; 32];
                faster_hex::hex_decode(info.chain_hash.as_bytes(), &mut hash_bytes).unwrap();
                H256::from(hash_bytes)
            },
            commit_timestamp: info.last_seen_hour.to_rfc3339(),
            created_timestamp: info.created_timestamp.timestamp_millis() as u64,
            update_info_of_node1: info.update_of_node1_timestamp.map(|timestamp| {
                ChannelUpdateInfo {
                    timestamp: timestamp.timestamp_millis() as u64,
                    enabled: info.update_of_node1_enabled.unwrap_or(false),
                    outbound_liquidity: info.update_of_node1_outbound_liquidity.map(|ol| {
                        let mut ol_bytes = [0u8; 16];
                        faster_hex::hex_decode(ol.as_bytes(), &mut ol_bytes).unwrap();
                        u128::from_be_bytes(ol_bytes)
                    }),
                    tlc_expiry_delta: {
                        let mut delta_bytes = [0u8; 8];
                        faster_hex::hex_decode(
                            info.update_of_node1_tlc_expiry_delta
                                .as_ref()
                                .unwrap()
                                .as_bytes(),
                            &mut delta_bytes,
                        )
                        .unwrap();
                        u64::from_be_bytes(delta_bytes)
                    },
                    tlc_minimum_value: {
                        let mut min_value_bytes = [0u8; 16];
                        faster_hex::hex_decode(
                            info.update_of_node1_tlc_minimum_value
                                .as_ref()
                                .unwrap()
                                .as_bytes(),
                            &mut min_value_bytes,
                        )
                        .unwrap();
                        u128::from_be_bytes(min_value_bytes)
                    },
                    fee_rate: {
                        let mut fee_rate_bytes = [0u8; 8];
                        faster_hex::hex_decode(
                            info.update_of_node1_fee_rate.as_ref().unwrap().as_bytes(),
                            &mut fee_rate_bytes,
                        )
                        .unwrap();
                        u64::from_be_bytes(fee_rate_bytes)
                    },
                }
            }),
            update_info_of_node2: info.update_of_node2_timestamp.map(|timestamp| {
                ChannelUpdateInfo {
                    timestamp: timestamp.timestamp_millis() as u64,
                    enabled: info.update_of_node2_enabled.unwrap_or(false),
                    outbound_liquidity: info.update_of_node2_outbound_liquidity.map(|ol| {
                        let mut ol_bytes = [0u8; 16];
                        faster_hex::hex_decode(ol.as_bytes(), &mut ol_bytes).unwrap();
                        u128::from_be_bytes(ol_bytes)
                    }),
                    tlc_expiry_delta: {
                        let mut delta_bytes = [0u8; 8];
                        faster_hex::hex_decode(
                            info.update_of_node2_tlc_expiry_delta
                                .as_ref()
                                .unwrap()
                                .as_bytes(),
                            &mut delta_bytes,
                        )
                        .unwrap();
                        u64::from_be_bytes(delta_bytes)
                    },
                    tlc_minimum_value: {
                        let mut min_value_bytes = [0u8; 16];
                        faster_hex::hex_decode(
                            info.update_of_node2_tlc_minimum_value
                                .as_ref()
                                .unwrap()
                                .as_bytes(),
                            &mut min_value_bytes,
                        )
                        .unwrap();
                        u128::from_be_bytes(min_value_bytes)
                    },
                    fee_rate: {
                        let mut fee_rate_bytes = [0u8; 8];
                        faster_hex::hex_decode(
                            info.update_of_node2_fee_rate.as_ref().unwrap().as_bytes(),
                            &mut fee_rate_bytes,
                        )
                        .unwrap();
                        u64::from_be_bytes(fee_rate_bytes)
                    },
                }
            }),
            udt_type_script: info.udt_hash_type.map(|hash_type| Script {
                code_hash: {
                    let mut code_hash_bytes = [0u8; 32];
                    faster_hex::hex_decode(
                        info.udt_code_hash.as_ref().unwrap().as_bytes(),
                        &mut code_hash_bytes,
                    )
                    .unwrap();
                    H256::from(code_hash_bytes)
                },
                hash_type: match hash_type.as_str() {
                    "type" => ckb_jsonrpc_types::ScriptHashType::Type,
                    "data" => ckb_jsonrpc_types::ScriptHashType::Data,
                    "data1" => ckb_jsonrpc_types::ScriptHashType::Data1,
                    "data2" => ckb_jsonrpc_types::ScriptHashType::Data2,
                    _ => unreachable!("Unknown hash type: {}", hash_type),
                },
                args: {
                    let mut args_bytes = vec![0; info.udt_args.as_ref().unwrap().len() / 2];
                    faster_hex::hex_decode(
                        info.udt_args.as_ref().unwrap().as_bytes(),
                        &mut args_bytes,
                    )
                    .unwrap();
                    JsonBytes::from_vec(args_bytes)
                },
            }),
            udt_name: info.udt_name,
            udt_auto_accept_amount: info
                .udt_auto_accept_amount
                .map(|amount| format!("0x{}", amount)),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct HourlyChannelInfoDBRead {
    pub channel_outpoint: String,
    pub last_seen_hour: DateTime<Utc>,
    pub node1: String,
    pub node2: String,
    pub capacity: String,
    pub asset: String,
    pub chain_hash: String,
    pub created_timestamp: DateTime<Utc>,

    // Node1 updates
    pub update_of_node1_timestamp: Option<DateTime<Utc>>,
    pub update_of_node1_enabled: Option<bool>,
    pub update_of_node1_outbound_liquidity: Option<String>,
    pub update_of_node1_tlc_expiry_delta: Option<String>,
    pub update_of_node1_tlc_minimum_value: Option<String>,
    pub update_of_node1_fee_rate: Option<String>,

    // Node2 updates
    pub update_of_node2_timestamp: Option<DateTime<Utc>>,
    pub update_of_node2_enabled: Option<bool>,
    pub update_of_node2_outbound_liquidity: Option<String>,
    pub update_of_node2_tlc_expiry_delta: Option<String>,
    pub update_of_node2_tlc_minimum_value: Option<String>,
    pub update_of_node2_fee_rate: Option<String>,

    // UDT info (from JOIN)
    pub udt_name: Option<String>,
    pub udt_code_hash: Option<String>,
    pub udt_hash_type: Option<String>,
    pub udt_args: Option<String>,
    pub udt_auto_accept_amount: Option<String>,
}

impl HourlyChannelInfoDBRead {
    pub async fn fetch_by_id(
        pool: &Pool<Postgres>,
        outpoint: JsonBytes,
        net: Network,
    ) -> Result<Option<Self>, sqlx::Error> {
        let channel_info = net.channel_infos();
        let udt_info = net.udt_infos();
        let channel_state = net.channel_states();
        let sql = format!(
            "SELECT {channel_info}.channel_outpoint,
                time as last_seen_hour,
                node1,
                node2,
                {channel_state}.capacity as capacity,
                {channel_info}.capacity as asset,
                chain_hash,
                created_timestamp,
                update_of_node1_timestamp,
                update_of_node1_enabled,
                update_of_node1_outbound_liquidity,
                update_of_node1_tlc_expiry_delta,
                update_of_node1_tlc_minimum_value,
                update_of_node1_fee_rate,
                update_of_node2_timestamp,
                update_of_node2_enabled,
                update_of_node2_outbound_liquidity,
                update_of_node2_tlc_expiry_delta,
                update_of_node2_tlc_minimum_value,
                update_of_node2_fee_rate,
                COALESCE({udt_info}.name, 'ckb') AS udt_name,
                {udt_info}.code_hash AS udt_code_hash,
                {udt_info}.hash_type AS udt_hash_type,
                {udt_info}.args AS udt_args,
                {udt_info}.auto_accept_amount AS udt_auto_accept_amount
            FROM {channel_info}
            left join {udt_info} on {channel_info}.udt_type_script = {udt_info}.id
            left join {channel_state} on {channel_info}.channel_outpoint = {channel_state}.channel_outpoint
            WHERE {channel_info}.channel_outpoint = $1
            ORDER BY last_seen_hour DESC
            LIMIT 1",
        );

        let res = sqlx::query_as::<_, Self>(&sql)
            .bind(faster_hex::hex_string(outpoint.as_bytes()))
            .fetch_optional(pool)
            .await?;

        Ok(res)
    }

    pub async fn fetch_by_page_hourly(
        pool: &Pool<Postgres>,
        params: Page,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let hour_bucket = Utc::now() - chrono::Duration::hours(3);
        let sql = SELECT_HOURLY_CHANNELS_SQL
            .replace("{1}", params.net.mv_online_channels())
            .replace("{2}", params.net.udt_infos())
            .replace("{3}", params.net.channel_states());
        let sql = format!("{} LIMIT {} OFFSET {}", sql, page_size, offset);
        let rows = sqlx::query(&sql).bind(hour_bucket).fetch_all(pool).await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }

    pub async fn fetch_by_page_monthly(
        pool: &Pool<Postgres>,
        params: Page,
    ) -> Result<(Vec<Self>, usize, usize), sqlx::Error> {
        let page_size = std::cmp::min(params.page_size.unwrap_or(PAGE_SIZE), PAGE_SIZE);
        let offset = params.page.saturating_mul(page_size);
        let now = Utc::now().date_naive();
        let start = params.start.unwrap_or(now - chrono::Duration::days(30));
        let mut end = params.end.unwrap_or(now);
        if end - start > chrono::Duration::days(30) || start > end {
            end = start + chrono::Duration::days(30);
        }
        let sql = SELECT_MONTHLY_CHANNELS_SQL
            .replace("{1}", params.net.online_channels_hourly())
            .replace("{2}", params.net.udt_infos())
            .replace("{3}", params.net.channel_states());
        let sql = format!("{} LIMIT {} OFFSET {}", sql, page_size, offset);
        let rows = sqlx::query(&sql)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await?;
        let (rows, total_count) = rows_with_total::<Self>(rows)?;
        Ok((rows, params.page.saturating_add(1), total_count))
    }
}
