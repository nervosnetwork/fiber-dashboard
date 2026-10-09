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

export type CommitmentFeatures = 0 | 1;

const EMPTY_WITNESS_ARGS = "10000000100000001000000010000000";

// substring silently returns short data on overrun. Every field must be complete.
const hexReader = (hex: string) => {
  const data = hex.replace(/^0x/i, "").toLowerCase();
  if (!/^(?:[0-9a-f]{2})+$/.test(data)) {
    throw new Error("Invalid hexadecimal transaction data.");
  }
  let offset = 0;
  return {
    data,
    get remainingBytes() {
      return (data.length - offset) / 2;
    },
    read(bytes: number): string {
      const end = offset + bytes * 2;
      if (end > data.length) {
        throw new Error("Transaction data is truncated.");
      }
      const value = data.slice(offset, end);
      offset = end;
      return value;
    },
    assertEnd() {
      if (offset !== data.length) {
        throw new Error("Transaction data does not match the commitment format.");
      }
    },
  };
};

/** Legacy: 57 bytes. Full payment hash: 58 bytes with features exactly 0x01. */
export const parseLockArgsV2 = (hex: string) => {
  const reader = hexReader(hex);
  const { data } = reader;
  let features: CommitmentFeatures;
  if (data.length === 57 * 2) {
    features = 0;
  } else if (data.length === 58 * 2 && data.slice(57 * 2) === "01") {
    features = 1;
  } else {
    throw new Error("Unsupported commitment lock args length or features.");
  }

  const pubkey_hash = `0x${reader.read(20)}`;
  const delay_epoch = parseEpoch(littleEndianHexToBigInt(reader.read(8)));
  const version = BigInt(`0x${reader.read(8)}`).toString();
  const settlement_hash = `0x${reader.read(20)}`;
  const settlement_flag = parseInt(reader.read(1), 16);

  return {
    pubkey_hash,
    delay_epoch,
    version,
    settlement_hash,
    settlement_flag,
    features,
    has_full_payment_hash: features === 1,
  };
};

/**
 * The API returns output args, including null on final settlement, and does not
 * order transactions within a block. The contract preserves features across
 * derived settlement cells, so all known args in a channel must agree. Do not
 * infer a layout from witness length or assume the previous row is the input.
 */
export const resolveChannelCommitmentFeatures = (
  transactions: readonly { commitment_args: string | null }[]
): CommitmentFeatures | undefined => {
  let features: CommitmentFeatures | undefined;
  for (const transaction of transactions) {
    if (transaction.commitment_args === null) continue;
    const current = parseLockArgsV2(transaction.commitment_args).features;
    if (features !== undefined && current !== features) {
      throw new Error("Conflicting commitment formats in this channel's history.");
    }
    features = current;
  }
  return features;
};

/**
 * Decode a commitment witness with features from validated channel args.
 * Only revocation witnesses can be decoded without those features.
 */
export const parseWitnessV2 = (
  hex: string,
  features?: CommitmentFeatures
): ParsedWitnessData => {
  const reader = hexReader(hex);
  const emptyWitnessArgs = reader.read(16);
  if (emptyWitnessArgs !== EMPTY_WITNESS_ARGS) {
    throw new Error("Unsupported commitment witness header.");
  }
  const unlockCount = parseInt(reader.read(1), 16);

  const witnessData: ParsedWitnessData = { 
    empty_witness_args: `0x${emptyWitnessArgs}`, 
    unlock_count: unlockCount 
  };

  if (unlockCount === 0x00) { // Revocation unlock
    witnessData.revocation = {
      version: BigInt(`0x${reader.read(8)}`),
      pubkey: `0x${reader.read(32)}`,
      signature: `0x${reader.read(64)}`
    };
  } else { // Settlement unlock
    if (features !== 0 && features !== 1) {
      throw new Error("Commitment format is unavailable in this channel's history.");
    }
    const paymentHashLength = features === 1 ? 32 : 20;
    const pendingHtlcCount = parseInt(reader.read(1), 16);
    const htlcs = [];
    
    for (let i = 0; i < pendingHtlcCount; i++) {
      const htlc_type = parseInt(reader.read(1), 16);
      const payment_amount = littleEndianHexToBigInt(reader.read(16));
      const payment_hash = `0x${reader.read(paymentHashLength)}`;
      const remote_htlc_pubkey_hash = `0x${reader.read(20)}`;
      const local_htlc_pubkey_hash = `0x${reader.read(20)}`;
      let htlc_expiry_timestamp = littleEndianHexToBigInt(reader.read(8));
      htlc_expiry_timestamp = (htlc_expiry_timestamp & ((BigInt(1) << BigInt(56)) - BigInt(1))) * BigInt(1000);
      const htlc_expiry = new Date(Number(htlc_expiry_timestamp)).toLocaleString('zh-CN');

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

    const settlement_remote_pubkey_hash = `0x${reader.read(20)}`;
    const settlement_remote_amount = littleEndianHexToBigInt(reader.read(16));
    const settlement_local_pubkey_hash = `0x${reader.read(20)}`;
    const settlement_local_amount = littleEndianHexToBigInt(reader.read(16));

    const unlocks = [];
    // The contract only uses the header count to distinguish revocation (0)
    // from settlement. Decode all unlock records, as the contract does.
    while (reader.remainingBytes > 0) {
      const unlock_type = parseInt(reader.read(1), 16);
      if (unlock_type >= pendingHtlcCount && unlock_type !== 0xfe && unlock_type !== 0xff) {
        throw new Error("Invalid settlement unlock type.");
      }
      const with_preimage = parseInt(reader.read(1), 16);
      if (with_preimage !== 0 && with_preimage !== 1) {
        throw new Error("Invalid settlement preimage flag.");
      }
      const signature = `0x${reader.read(65)}`;
      let preimage = 'N/A';
      if (with_preimage === 0x01) {
        preimage = `0x${reader.read(32)}`;
      }
      unlocks.push({
        unlock_type,
        with_preimage,
        signature,
        preimage
      });
    }

    if (unlocks.length === 0) {
      throw new Error("Settlement witness has no unlock records.");
    }

    witnessData.settlement = {
      pending_htlc_count: pendingHtlcCount,
      htlcs,
      settlement_remote_pubkey_hash,
      settlement_remote_amount,
      settlement_local_pubkey_hash,
      settlement_local_amount,
      payment_hash_len: paymentHashLength,
      unlocks
    };
  }

  reader.assertEnd();
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
  payment_hash_len: 20 | 32;
  pending_htlc_count: number;
  htlcs: HTLCData[];
  settlement_remote_pubkey_hash: string;
  settlement_remote_amount: bigint;
  settlement_local_pubkey_hash: string;
  settlement_local_amount: bigint;
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
