import { hexToDecimal } from "@/lib/utils";

/**
 * 格式化区块高度为 10 进制字符串
 */
export const formatBlockNumber = (blockNumber: string): string => {
  if (blockNumber.startsWith("0x")) {
    // 16 进制转 10 进制
    return String(hexToDecimal(blockNumber));
  }
  // 已经是 10 进制
  return blockNumber;
};

/**
 * 格式化时间戳为可读日期字符串
 */
export const formatTimestamp = (timestamp: string | number) => {
  let date: Date;

  if (typeof timestamp === "string") {
    if (timestamp.startsWith("0x")) {
      // 十六进制格式，需要转换
      date = new Date(Number(hexToDecimal(timestamp)));
    } else if (/^\d+$/.test(timestamp)) {
      // 纯数字字符串（时间戳）
      date = new Date(Number(timestamp));
    } else {
      // ISO 字符串或其他日期格式
      date = new Date(timestamp);
    }
  } else {
    date = new Date(timestamp);
  }

  // 检查是否是有效日期
  if (isNaN(date.getTime())) {
    return "Invalid Date";
  }

  return date.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
};

/**
 * 小端序 Hex 转 BigInt
 */
export const littleEndianHexToBigInt = (hex: string): bigint => {
  let cleanHex = hex;
  if (cleanHex.length % 2 !== 0) {
    cleanHex = '0' + cleanHex;
  }
  const bytes: string[] = [];
  for (let i = 0; i < cleanHex.length; i += 2) {
    bytes.push(cleanHex.substring(i, i + 2));
  }
  return BigInt('0x' + bytes.reverse().join(''));
};

/**
 * 解析 Epoch
 */
export const parseEpoch = (epoch: bigint) => {
  const number = (epoch >> BigInt(0)) & ((BigInt(1) << BigInt(24)) - BigInt(1));
  const index = (epoch >> BigInt(24)) & ((BigInt(1) << BigInt(16)) - BigInt(1));
  const length = (epoch >> BigInt(40)) & ((BigInt(1) << BigInt(16)) - BigInt(1));
  return {
    number: number.toString(),
    index: index.toString(),
    length: length.toString(),
    value: epoch.toString()
  };
};

/**
 * commitment lock 合约的 feature 位（args 的最后一个字节）。
 * 见 fiber-scripts#30：bit0 表示链上提交完整 32 字节 payment hash。
 */
export const COMMITMENT_FEATURE_ONCHAIN_FULL_PAYMENT_HASH = 0b0000_0001;

/**
 * 解析 Lock Args V2
 *
 * Legacy 布局为 57 字节；开启 ONCHAIN_FULL_PAYMENT_HASH 的 v1 布局在此基础上
 * 追加 1 字节 feature 位，前 57 字节不变。
 */
export const parseLockArgsV2 = (hex: string) => {
  const data = hex.startsWith('0x') ? hex.substring(2) : hex;
  let offset = 0;

  const pubkeyHash = data.substring(offset, offset + 40);
  offset += 40;

  const delayEpochHex = data.substring(offset, offset + 16);
  const delayEpoch = littleEndianHexToBigInt(delayEpochHex);
  offset += 16;

  const versionHex = data.substring(offset, offset + 16);
  const version = BigInt('0x' + versionHex);
  offset += 16;

  const settlementHash = data.substring(offset, offset + 40);
  offset += 40;

  const settlementFlagHex = data.substring(offset, offset + 2);
  const settlement_flag = settlementFlagHex ? parseInt(settlementFlagHex, 16) : 0;
  offset += 2;

  // Legacy args 到此结束，取不到字节即视为 feature 为 0
  const featuresHex = data.substring(offset, offset + 2);
  const features = featuresHex ? parseInt(featuresHex, 16) : 0;

  return {
    pubkey_hash: `0x${pubkeyHash}`,
    delay_epoch: parseEpoch(delayEpoch),
    version: version.toString(),
    settlement_hash: settlementHash ? `0x${settlementHash}` : '',
    settlement_flag: settlement_flag,
    features: features,
    has_full_payment_hash:
      (features & COMMITMENT_FEATURE_ONCHAIN_FULL_PAYMENT_HASH) !== 0
  };
};

/**
 * settlement witness 中链上提交的 payment hash 长度（字节）。
 *
 * Legacy 合约只提交 hash 的 20 字节前缀，v1（ONCHAIN_FULL_PAYMENT_HASH）
 * 提交完整 32 字节，HTLC 记录由 85 字节变为 97 字节，其后所有字段随之偏移。
 */
const PAYMENT_HASH_LEN_LEGACY = 20;
const PAYMENT_HASH_LEN_V1 = 32;

/**
 * 按给定的 payment hash 长度解析 settlement witness 主体。
 *
 * 返回解析结果以及实际消耗到的偏移量，调用方据此判断布局是否匹配。
 */
const parseSettlementWitness = (
  data: string,
  start: number,
  unlockCount: number,
  paymentHashLen: number
): { settlement: SettlementData; end: number } => {
  let offset = start;

  const pendingHtlcCount = parseInt(data.substring(offset, offset + 2), 16);
  offset += 2;
  const htlcs: HTLCData[] = [];

  for (let i = 0; i < pendingHtlcCount; i++) {
    const htlc_type = parseInt(data.substring(offset, offset + 2), 16);
    offset += 2;

    const paymentAmountHex = data.substring(offset, offset + 32);
    const payment_amount = littleEndianHexToBigInt(paymentAmountHex);
    offset += 32;

    const payment_hash = `0x${data.substring(offset, offset + paymentHashLen * 2)}`;
    offset += paymentHashLen * 2;

    const remote_htlc_pubkey_hash = `0x${data.substring(offset, offset + 40)}`;
    offset += 40;

    const local_htlc_pubkey_hash = `0x${data.substring(offset, offset + 40)}`;
    offset += 40;

    const htlcExpiryHex = data.substring(offset, offset + 16);
    let htlc_expiry_timestamp = littleEndianHexToBigInt(htlcExpiryHex);
    htlc_expiry_timestamp = (htlc_expiry_timestamp & ((BigInt(1) << BigInt(56)) - BigInt(1))) * BigInt(1000);
    const htlc_expiry = new Date(Number(htlc_expiry_timestamp)).toLocaleString('zh-CN');
    offset += 16;

    htlcs.push({
      htlc_type,
      payment_amount,
      payment_hash,
      remote_htlc_pubkey_hash,
      local_htlc_pubkey_hash,
      htlc_expiry,
      htlc_expiry_timestamp
    });
  }

  const settlement_remote_pubkey_hash = `0x${data.substring(offset, offset + 40)}`;
  offset += 40;
  const settlement_remote_amount = littleEndianHexToBigInt(data.substring(offset, offset + 32));
  offset += 32;
  const settlement_local_pubkey_hash = `0x${data.substring(offset, offset + 40)}`;
  offset += 40;
  const settlement_local_amount = littleEndianHexToBigInt(data.substring(offset, offset + 32));
  offset += 32;

  const unlocks: UnlockData[] = [];
  for (let i = 0; i < unlockCount; i++) {
    const unlock_type = parseInt(data.substring(offset, offset + 2), 16);
    offset += 2;
    const with_preimage = parseInt(data.substring(offset, offset + 2), 16);
    offset += 2;
    const signature = `0x${data.substring(offset, offset + 130)}`;
    offset += 130;
    let preimage = 'N/A';
    if (with_preimage === 0x01) {
      preimage = `0x${data.substring(offset, offset + 64)}`;
      offset += 64;
    }
    unlocks.push({
      unlock_type,
      with_preimage,
      signature,
      preimage
    });
  }

  return {
    settlement: {
      pending_htlc_count: pendingHtlcCount,
      htlcs,
      settlement_remote_pubkey_hash,
      settlement_remote_amount,
      settlement_local_pubkey_hash,
      settlement_local_amount,
      payment_hash_len: paymentHashLen,
      unlocks
    },
    end: offset
  };
};

/**
 * 解析 Witness V2
 */
export const parseWitnessV2 = (hex: string): ParsedWitnessData => {
  const data = hex.startsWith('0x') ? hex.substring(2) : hex;
  let offset = 0;

  const emptyWitnessArgs = data.substring(offset, offset + 32);
  offset += 32;

  const unlockCount = parseInt(data.substring(offset, offset + 2), 16);
  offset += 2;

  const witnessData: ParsedWitnessData = {
    empty_witness_args: `0x${emptyWitnessArgs}`,
    unlock_count: unlockCount
  };

  if (unlockCount === 0x00) { // Revocation unlock
    witnessData.revocation = {
      version: BigInt('0x' + data.substring(offset, offset + 16)),
      pubkey: `0x${data.substring(offset + 16, offset + 16 + 64)}`,
      signature: `0x${data.substring(offset + 16 + 64)}`
    };
    return witnessData;
  }

  // Settlement unlock。
  // witness 本身不带布局标记，先按 Legacy 解析：Legacy 数据必然恰好消耗完整个
  // witness，因此只在解析不满时再尝试 v1 布局，避免影响既有数据。
  const legacy = parseSettlementWitness(
    data,
    offset,
    unlockCount,
    PAYMENT_HASH_LEN_LEGACY
  );
  let settlement = legacy.settlement;

  if (legacy.end !== data.length) {
    try {
      const v1 = parseSettlementWitness(
        data,
        offset,
        unlockCount,
        PAYMENT_HASH_LEN_V1
      );
      if (v1.end === data.length) {
        settlement = v1.settlement;
      }
    } catch {
      // 不是 v1 布局，保留 Legacy 的解析结果
    }
  }

  witnessData.settlement = settlement;
  return witnessData;
};

// Type definitions for parsed witness data
export interface HTLCData {
  htlc_type: number;
  payment_amount: bigint;
  payment_hash: string;
  remote_htlc_pubkey_hash: string;
  local_htlc_pubkey_hash: string;
  htlc_expiry: string;
  htlc_expiry_timestamp: bigint;
}

export interface UnlockData {
  unlock_type: number;
  with_preimage: number;
  signature: string;
  preimage: string;
}

export interface SettlementData {
  pending_htlc_count: number;
  htlcs: HTLCData[];
  settlement_remote_pubkey_hash: string;
  settlement_remote_amount: bigint;
  settlement_local_pubkey_hash: string;
  settlement_local_amount: bigint;
  /** 解析该 witness 时采用的 payment hash 长度：20 为 Legacy，32 为 v1 */
  payment_hash_len: number;
  unlocks: UnlockData[];
}

export interface RevocationData {
  version: bigint;
  pubkey: string;
  signature: string;
}

export interface ParsedWitnessData {
  empty_witness_args: string;
  unlock_count: number;
  settlement?: SettlementData;
  revocation?: RevocationData;
}
