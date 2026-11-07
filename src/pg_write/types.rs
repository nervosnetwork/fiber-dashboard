use std::collections::{HashMap, HashSet};

use ckb_jsonrpc_types::Script;
use ckb_types::bytes::Bytes;
use faster_hex::hex_string;
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::{PgConnection, QueryBuilder};

use crate::{
    pg_write::{Network, global_cache, global_cache_testnet},
    types::ChannelInfo,
};

pub const UDT_INFO_INSERT_SQL: &str =
    "insert into {} (id, name, code_hash, hash_type, args, auto_accept_amount) ";
pub const UDT_DEP_RELATION_INSERT_SQL: &str = "insert into {} (outpoint_tx_hash, outpoint_index, dep_type, code_hash, hash_type, args, udt_info_id) ";
pub const UDT_NODE_RELATION_INSERT_SQL: &str = "insert into {} (node_id, udt_info_id) ";
pub const NODE_INFO_INSERT_SQL: &str = "insert into {} (time, node_name, addresses, node_id, announce_timestamp, chain_hash, auto_accept_min_ckb_funding_amount, country_or_region, city, region, loc) ";
pub const CHANNEL_INFO_INSERT_SQL: &str = "insert into {} (
    time, channel_outpoint, node1, node2, capacity, chain_hash, udt_type_script, 
    created_timestamp, update_of_node1_timestamp, update_of_node1_enabled, 
    update_of_node1_outbound_liquidity, update_of_node1_tlc_expiry_delta, 
    update_of_node1_tlc_minimum_value, update_of_node1_fee_rate, 
    update_of_node2_timestamp, update_of_node2_enabled, 
    update_of_node2_outbound_liquidity, update_of_node2_tlc_expiry_delta, 
    update_of_node2_tlc_minimum_value, update_of_node2_fee_rate
) ";

/// Postgres accepts at most 65535 bind parameters per statement.
const PGBIND_LIMIT: usize = 65535;

#[derive(sqlx::FromRow)]
pub struct UdtInfoCache {
    pub id: i32,
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Default)]
pub struct RelationCache {
    pub udt: HashMap<Script, i32>,
    pub udt_node: HashMap<Bytes, HashSet<i32>>,
}

pub struct UdtInfos {
    pub id: i32,
    pub name: String,
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
    pub auto_accept_amount: String,
}

impl UdtInfos {
    pub async fn insert_batch(
        conn: &mut PgConnection,
        udts: &[UdtInfos],
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if udts.is_empty() {
            return Ok(());
        }
        let sql = UDT_INFO_INSERT_SQL.replace("{}", net.udt_infos());
        for chunk in udts.chunks(PGBIND_LIMIT / 6) {
            let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(&sql);

            query_builder.push_values(chunk, |mut b, udt| {
                b.push_bind(udt.id)
                    .push_bind(&udt.name)
                    .push_bind(&udt.code_hash)
                    .push_bind(&udt.hash_type)
                    .push_bind(&udt.args)
                    .push_bind(&udt.auto_accept_amount);
            });

            query_builder.build().execute(&mut *conn).await?;
        }
        Ok(())
    }
}

pub struct UdtdepRelation {
    pub outpoint_tx_hash: Option<String>,
    pub outpoint_index: Option<String>,
    pub dep_type: Option<String>,
    pub code_hash: Option<String>,
    pub hash_type: Option<String>,
    pub args: Option<String>,
    pub udt_info_id: i32,
}

impl UdtdepRelation {
    pub async fn use_sqlx(
        conn: &mut PgConnection,
        relations: &[UdtdepRelation],
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if relations.is_empty() {
            return Ok(());
        }
        let sql = UDT_DEP_RELATION_INSERT_SQL.replace("{}", net.udt_dep());
        for chunk in relations.chunks(PGBIND_LIMIT / 7) {
            let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(&sql);

            query_builder.push_values(chunk, |mut b, relation| {
                b.push_bind(&relation.outpoint_tx_hash)
                    .push_bind(&relation.outpoint_index)
                    .push_bind(&relation.dep_type)
                    .push_bind(&relation.code_hash)
                    .push_bind(&relation.hash_type)
                    .push_bind(&relation.args)
                    .push_bind(relation.udt_info_id);
            });

            query_builder.build().execute(&mut *conn).await?;
        }
        Ok(())
    }
}

pub struct UdtNodeRelation {
    pub node_id: String,
    pub udt_info_id: i32,
}

impl UdtNodeRelation {
    pub async fn use_sqlx(
        conn: &mut PgConnection,
        relations: &[UdtNodeRelation],
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if relations.is_empty() {
            return Ok(());
        }
        let sql = UDT_NODE_RELATION_INSERT_SQL.replace("{}", net.node_udt_relations());
        for chunk in relations.chunks(PGBIND_LIMIT / 2) {
            let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(&sql);

            query_builder.push_values(chunk, |mut b, relation| {
                b.push_bind(&relation.node_id)
                    .push_bind(relation.udt_info_id);
            });

            query_builder.build().execute(&mut *conn).await?;
        }
        Ok(())
    }
}

pub struct NodeInfoDBSchema {
    pub node_name: String,
    // json list
    pub addresses: String,
    // hex string
    pub node_id: String,
    pub announce_timestamp: DateTime<Utc>,
    // hex string
    pub chain_hash: String,
    pub auto_accept_min_ckb_funding_amount: String,
    pub country_or_region: String,
    pub city: String,
    pub region: String,
    pub loc: String,
}

impl NodeInfoDBSchema {
    pub async fn use_sqlx(
        conn: &mut PgConnection,
        nodes: &[NodeInfoDBSchema],
        time: &DateTime<Utc>,
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if nodes.is_empty() {
            return Ok(());
        }
        let sql = NODE_INFO_INSERT_SQL.replace("{}", net.node_infos());
        for chunk in nodes.chunks(PGBIND_LIMIT / 12) {
            let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(&sql);

            query_builder.push_values(chunk, |mut b, node| {
                b.push_bind(time)
                    .push_bind(&node.node_name)
                    .push_bind(&node.addresses)
                    .push_bind(&node.node_id)
                    .push_bind(node.announce_timestamp)
                    .push_bind(&node.chain_hash)
                    .push_bind(&node.auto_accept_min_ckb_funding_amount)
                    .push_bind(&node.country_or_region)
                    .push_bind(&node.city)
                    .push_bind(&node.region)
                    .push_bind(&node.loc);
            });

            query_builder.build().execute(&mut *conn).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ChannelInfoDBSchema {
    /// hex string
    pub channel_outpoint: String,
    /// hex string
    pub node1: String,
    /// hex string
    pub node2: String,

    /// hex string
    pub capacity: String,
    /// hex string
    pub chain_hash: String,

    pub udt_type_script: Option<i32>,

    pub created_timestamp: DateTime<Utc>,

    pub update_of_node1_timestamp: Option<DateTime<Utc>>,

    pub update_of_node1_enabled: Option<bool>,
    /// hex string
    pub update_of_node1_outbound_liquidity: Option<String>,

    pub update_of_node1_tlc_expiry_delta: Option<String>,
    /// hex string
    pub update_of_node1_tlc_minimum_value: Option<String>,

    pub update_of_node1_fee_rate: Option<String>,

    pub update_of_node2_timestamp: Option<DateTime<Utc>>,

    pub update_of_node2_enabled: Option<bool>,
    /// hex string
    pub update_of_node2_outbound_liquidity: Option<String>,

    pub update_of_node2_tlc_expiry_delta: Option<String>,
    /// hex string
    pub update_of_node2_tlc_minimum_value: Option<String>,

    pub update_of_node2_fee_rate: Option<String>,
}

impl ChannelInfoDBSchema {
    pub async fn use_sqlx(
        conn: &mut PgConnection,
        channels: &[ChannelInfoDBSchema],
        time: &DateTime<Utc>,
        net: Network,
    ) -> Result<(), sqlx::Error> {
        if channels.is_empty() {
            return Ok(());
        }
        let sql = CHANNEL_INFO_INSERT_SQL.replace("{}", net.channel_infos());
        for chunk in channels.chunks(PGBIND_LIMIT / 20) {
            let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(&sql);

            query_builder.push_values(chunk, |mut b, channel| {
                b.push_bind(time)
                    .push_bind(&channel.channel_outpoint)
                    .push_bind(&channel.node1)
                    .push_bind(&channel.node2)
                    .push_bind(&channel.capacity)
                    .push_bind(&channel.chain_hash)
                    .push_bind(channel.udt_type_script.as_ref())
                    .push_bind(channel.created_timestamp)
                    .push_bind(channel.update_of_node1_timestamp)
                    .push_bind(channel.update_of_node1_enabled)
                    .push_bind(&channel.update_of_node1_outbound_liquidity)
                    .push_bind(&channel.update_of_node1_tlc_expiry_delta)
                    .push_bind(&channel.update_of_node1_tlc_minimum_value)
                    .push_bind(&channel.update_of_node1_fee_rate)
                    .push_bind(channel.update_of_node2_timestamp)
                    .push_bind(channel.update_of_node2_enabled)
                    .push_bind(&channel.update_of_node2_outbound_liquidity)
                    .push_bind(&channel.update_of_node2_tlc_expiry_delta)
                    .push_bind(&channel.update_of_node2_tlc_minimum_value)
                    .push_bind(&channel.update_of_node2_fee_rate);
            });

            query_builder.build().execute(&mut *conn).await?;
        }
        Ok(())
    }
}

impl From<(ChannelInfo, Network)> for ChannelInfoDBSchema {
    fn from((channel_info, net): (ChannelInfo, Network)) -> Self {
        Self {
            channel_outpoint: hex_string(channel_info.channel_outpoint.as_bytes()),
            node1: String::from_utf8(channel_info.node1.to_vec()).unwrap(),
            node2: String::from_utf8(channel_info.node2.to_vec()).unwrap(),
            capacity: hex_string(channel_info.capacity.to_be_bytes().as_ref()),
            chain_hash: hex_string(channel_info.chain_hash.as_bytes()),
            udt_type_script: channel_info
                .udt_type_script
                .as_ref()
                .and_then(|script| match net {
                    Network::Mainnet => global_cache().load().udt.get(script).cloned(),
                    Network::Testnet => global_cache_testnet().load().udt.get(script).cloned(),
                }),
            created_timestamp: DateTime::from_timestamp_millis(
                channel_info.created_timestamp as i64,
            )
            .unwrap(),
            update_of_node1_timestamp: channel_info
                .update_info_of_node1
                .as_ref()
                .map(|info| DateTime::from_timestamp_millis(info.timestamp as i64).unwrap()),
            update_of_node1_enabled: channel_info
                .update_info_of_node1
                .as_ref()
                .map(|info| info.enabled),
            update_of_node1_outbound_liquidity: channel_info
                .update_info_of_node1
                .as_ref()
                .and_then(|info| {
                    info.outbound_liquidity
                        .map(|v| hex_string(&v.to_be_bytes()))
                }),
            update_of_node1_tlc_expiry_delta: channel_info
                .update_info_of_node1
                .as_ref()
                .map(|info| hex_string(&info.tlc_expiry_delta.to_be_bytes())),
            update_of_node1_tlc_minimum_value: channel_info
                .update_info_of_node1
                .as_ref()
                .map(|info| hex_string(&info.tlc_minimum_value.to_be_bytes())),
            update_of_node1_fee_rate: channel_info
                .update_info_of_node1
                .as_ref()
                .map(|info| hex_string(&info.fee_rate.to_be_bytes())),
            update_of_node2_timestamp: channel_info
                .update_info_of_node2
                .as_ref()
                .map(|info| DateTime::from_timestamp_millis(info.timestamp as i64).unwrap()),
            update_of_node2_enabled: channel_info
                .update_info_of_node2
                .as_ref()
                .map(|info| info.enabled),
            update_of_node2_outbound_liquidity: channel_info
                .update_info_of_node2
                .as_ref()
                .and_then(|info| {
                    info.outbound_liquidity
                        .map(|v| hex_string(&v.to_be_bytes()))
                }),
            update_of_node2_tlc_expiry_delta: channel_info
                .update_info_of_node2
                .as_ref()
                .map(|info| hex_string(&info.tlc_expiry_delta.to_be_bytes())),
            update_of_node2_tlc_minimum_value: channel_info
                .update_info_of_node2
                .as_ref()
                .map(|info| hex_string(&info.tlc_minimum_value.to_be_bytes())),
            update_of_node2_fee_rate: channel_info
                .update_info_of_node2
                .as_ref()
                .map(|info| hex_string(&info.fee_rate.to_be_bytes())),
        }
    }
}
