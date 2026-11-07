use serde::{Deserialize, Serialize};
use serde_with::{serde_as, serde_conv};

use ckb_jsonrpc_types::{
    BlockNumber, CellOutput, DepType, JsonBytes, OutPoint, Script, ScriptHashType, Uint32, Uint64,
};
use ckb_types::{H256, bytes::Bytes, h256};
use multiaddr::MultiAddr;

use crate::Network;

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GraphNodesParams {
    #[serde_as(as = "Option<U64Hex>")]
    /// The maximum number of nodes to return.
    pub limit: Option<u64>,
    /// The cursor to start returning nodes from.
    pub after: Option<JsonBytes>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GraphNodesResult {
    /// The list of nodes.
    pub nodes: Vec<NodeInfo>,
    /// The last cursor.
    pub last_cursor: JsonBytes,
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NodeInfo {
    /// The name of the node.
    pub node_name: String,
    /// The addresses of the node.
    pub addresses: Vec<MultiAddr>,
    /// The identity public key of the node.
    #[serde(alias = "pubkey")]
    pub node_id: Bytes,
    #[serde_as(as = "U64Hex")]
    /// The latest timestamp set by the owner for the node announcement.
    /// When a Node is online this timestamp will be updated to the latest value.
    pub timestamp: u64,
    /// The chain hash of the node.
    pub chain_hash: H256,
    #[serde_as(as = "U64Hex")]
    /// The minimum CKB funding amount for automatically accepting open channel requests.
    pub auto_accept_min_ckb_funding_amount: u64,
    /// The UDT configuration infos of the node.
    pub udt_cfg_infos: UdtCfgInfos,
}

#[derive(Serialize, Clone, Debug)]
pub struct UdtCfgInfos(
    /// The list of UDT configuration infos.
    pub Vec<UdtArgInfo>,
);

impl<'de> Deserialize<'de> for UdtCfgInfos {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
        let mut infos = Vec::with_capacity(raw.len());
        for value in raw {
            match serde_json::from_value::<UdtArgInfo>(value) {
                Ok(info) => infos.push(info),
                Err(err) => log::warn!("Skipping invalid UDT config: {}", err),
            }
        }
        Ok(UdtCfgInfos(infos))
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UdtArgInfo {
    /// The name of the UDT.
    pub name: String,
    /// The script of the UDT.
    pub script: Script,
    #[serde_as(as = "Option<U128Hex>")]
    /// The minimum amount of the UDT that can be automatically accepted.
    pub auto_accept_amount: Option<u128>,
    /// The cell deps of the UDT.
    pub cell_deps: Vec<UdtDep>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UdtDep {
    /// cell dep described by out_point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_dep: Option<UdtCellDep>,
    /// cell dep described by type ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<Script>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UdtCellDep {
    /// The out point of the cell dep.
    pub out_point: OutPoint,
    /// The type of the cell dep.
    pub dep_type: DepType,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GraphChannelsParams {
    /// The maximum number of channels to return.
    #[serde_as(as = "Option<U64Hex>")]
    pub limit: Option<u64>,
    /// The cursor to start returning channels from.
    pub after: Option<JsonBytes>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GraphChannelsResult {
    /// A list of channels.
    pub channels: Vec<ChannelInfo>,
    /// The last cursor for pagination.
    pub last_cursor: JsonBytes,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelInfo {
    /// The outpoint of the channel.
    pub channel_outpoint: JsonBytes,
    /// The identity public key of the first node.
    pub node1: Bytes,
    /// The identity public key of the second node.
    pub node2: Bytes,
    /// The created timestamp of the channel, which is the block header timestamp of the block
    /// that contains the channel funding transaction.
    #[serde_as(as = "U64Hex")]
    pub created_timestamp: u64,

    /// The update info from node1 to node2, e.g. timestamp, fee_rate, tlc_expiry_delta, tlc_minimum_value
    pub update_info_of_node1: Option<ChannelUpdateInfo>,

    /// The update info from node2 to node1, e.g. timestamp, fee_rate, tlc_expiry_delta, tlc_minimum_value
    pub update_info_of_node2: Option<ChannelUpdateInfo>,

    /// The capacity of the channel.
    #[serde_as(as = "U128Hex")]
    pub capacity: u128,
    /// The chain hash of the channel.
    pub chain_hash: H256,
    /// The UDT type script of the channel.
    pub udt_type_script: Option<Script>,
}

#[serde_as]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelUpdateInfo {
    /// The timestamp is the time when the channel update was received by the node.
    #[serde_as(as = "U64Hex")]
    pub timestamp: u64,
    /// Whether the channel can be currently used for payments (in this one direction).
    pub enabled: bool,
    /// The exact amount of balance that we can send to the other party via the channel.
    #[serde_as(as = "Option<U128Hex>")]
    pub outbound_liquidity: Option<u128>,
    /// The difference in htlc expiry values that you must have when routing through this channel (in milliseconds).
    #[serde_as(as = "U64Hex")]
    pub tlc_expiry_delta: u64,
    /// The minimum value, which must be relayed to the next hop via the channel
    #[serde_as(as = "U128Hex")]
    pub tlc_minimum_value: u128,
    /// The forwarding fee rate for the channel.
    #[serde_as(as = "U64Hex")]
    pub fee_rate: u64,
}

macro_rules! uint_as_hex {
    ($name:ident, $ty:ty) => {
        serde_conv!(
            pub $name,
            $ty,
            |u: &$ty| format!("0x{:x}", u),
            |hex: String| -> Result<$ty, String> {
                let bytes = hex.as_bytes();
                if bytes.len() < 3 || &bytes[..2] != b"0x" {
                    return Err(format!("uint hex string does not start with 0x: {}", hex));
                }
                if bytes.len() > 3 && &bytes[2..3] == b"0" {
                    return Err(format!("uint hex string starts with redundant leading zeros: {}", hex));
                };
                <$ty>::from_str_radix(&hex[2..], 16)
                    .map_err(|err| format!("failed to parse uint hex {}: {:?}", hex, err))
            }
        );
    };
}

uint_as_hex!(U128Hex, u128);
uint_as_hex!(U64Hex, u64);
uint_as_hex!(U32Hex, u32);
uint_as_hex!(U16Hex, u16);

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Desc,
    Asc,
}

#[derive(Serialize, Deserialize)]
pub struct Pagination<T> {
    pub objects: Vec<T>,
    pub last_cursor: JsonBytes,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CellType {
    Input,
    Output,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCell {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub io_index: Uint32,
    pub io_type: CellType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCells {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub cells: Vec<(CellType, Uint32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Tx {
    Ungrouped(TxWithCell),
    Grouped(TxWithCells),
}

impl Tx {
    pub fn tx_hash(&self) -> H256 {
        match self {
            Tx::Ungrouped(tx) => tx.tx_hash.clone(),
            Tx::Grouped(tx) => tx.tx_hash.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum IndexerScriptSearchMode {
    /// Mode `prefix` search script with prefix
    #[default]
    Prefix,
    /// Mode `exact` search script with exact match
    Exact,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchKey {
    pub script: Script,
    pub script_type: ScriptType,
    pub script_search_mode: Option<IndexerScriptSearchMode>,
    pub filter: Option<SearchKeyFilter>,
    pub with_data: Option<bool>,
    pub group_by_transaction: Option<bool>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SearchKeyFilter {
    pub script: Option<Script>,
    pub script_len_range: Option<[Uint64; 2]>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
    pub block_range: Option<[BlockNumber; 2]>,
}

impl SearchKeyFilter {
    pub fn block_range(start: BlockNumber, end: BlockNumber) -> Self {
        Self {
            script: None,
            script_len_range: None,
            output_data_len_range: None,
            output_capacity_range: None,
            block_range: Some([start, end]),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Lock,
    Type,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cell {
    pub output: CellOutput,
    pub output_data: Option<JsonBytes>,
    pub out_point: OutPoint,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexerTip {
    pub block_hash: H256,
    pub block_number: BlockNumber,
}

pub fn funding_script(net: Network, args: JsonBytes) -> Script {
    Script {
        code_hash: match net {
            Network::Mainnet => MAINNET_FUNDING_CODE_HASH.clone(),
            Network::Testnet => TESTNET_FUNDING_CODE_HASH.clone(),
        },
        hash_type: ScriptHashType::Type,
        args,
    }
}

pub fn commitment_script(net: Network, args: JsonBytes) -> Script {
    Script {
        code_hash: match net {
            Network::Mainnet => MAINNET_COMMITMENT_CODE_HASH.clone(),
            Network::Testnet => TESTNET_COMMITMENT_CODE_HASH.clone(),
        },
        hash_type: ScriptHashType::Type,
        args,
    }
}

use std::{str::FromStr, sync::LazyLock};

pub static MAINNET_FUNDING_CODE_HASH: LazyLock<H256> = LazyLock::new(|| {
    std::env::var("MAINNET_FUNDING_CODE_HASH")
        .ok()
        .and_then(|s| {
            let s = if s.len() < 2 {
                &s
            } else if &s[..2] == "0x" {
                &s[2..]
            } else {
                &s
            };
            H256::from_str(s).ok()
        })
        .unwrap_or(h256!(
            "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1"
        ))
});
pub static TESTNET_FUNDING_CODE_HASH: LazyLock<H256> = LazyLock::new(|| {
    std::env::var("TESTNET_FUNDING_CODE_HASH")
        .ok()
        .and_then(|s| {
            let s = if s.len() < 2 {
                &s
            } else if &s[..2] == "0x" {
                &s[2..]
            } else {
                &s
            };
            H256::from_str(s).ok()
        })
        .unwrap_or(h256!(
            "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c"
        ))
});
pub static MAINNET_COMMITMENT_CODE_HASH: LazyLock<H256> = LazyLock::new(|| {
    std::env::var("MAINNET_COMMITMENT_CODE_HASH")
        .ok()
        .and_then(|s| {
            let s = if s.len() < 2 {
                &s
            } else if &s[..2] == "0x" {
                &s[2..]
            } else {
                &s
            };
            H256::from_str(s).ok()
        })
        .unwrap_or(h256!(
            "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c"
        ))
});
pub static TESTNET_COMMITMENT_CODE_HASH: LazyLock<H256> = LazyLock::new(|| {
    std::env::var("TESTNET_COMMITMENT_CODE_HASH")
        .ok()
        .and_then(|s| {
            let s = if s.len() < 2 {
                &s
            } else if &s[..2] == "0x" {
                &s[2..]
            } else {
                &s
            };
            H256::from_str(s).ok()
        })
        .unwrap_or(h256!(
            "0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8"
        ))
});

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_udt_cfg_infos_are_skipped() {
        let valid = json!({
            "name": "RUSD",
            "script": {
                "code_hash": "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a",
                "hash_type": "type",
                "args": "0x878fcc6f1f08d48e87bb1c3b3d5083f23f8a39c5d5c764f253b55b998526439b"
            },
            "auto_accept_amount": "0x3b9aca00",
            "cell_deps": []
        });
        let invalid = json!({
            "name": "BAD",
            "script": {
                "code_hash": "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a",
                "hash_type": "type",
                "args": "^0x878fcc6f1f08d48e87bb1c3b3d5083f23f8a39c5d5c764f253b55b998526439b$"
            },
            "auto_accept_amount": null,
            "cell_deps": []
        });

        let infos: UdtCfgInfos = serde_json::from_value(json!([valid, invalid])).unwrap();
        assert_eq!(infos.0.len(), 1);
        assert_eq!(infos.0[0].name, "RUSD");
    }
}
