import { describe, it, expect } from "vitest";
import { parseWitnessV2, parseLockArgsV2 } from "../utils";

const byteHex = (value: number): string =>
  (value & 0xff).toString(16).padStart(2, "0");

const repeatHex = (byte: number, length: number): string =>
  byteHex(byte).repeat(length);

/** 按小端序编码整数，与链上 witness 的编码一致 */
const toLittleEndianHex = (value: bigint, byteLength: number): string => {
  const bytes: string[] = [];
  let remaining = value;
  for (let i = 0; i < byteLength; i++) {
    bytes.push(byteHex(Number(remaining & BigInt(0xff))));
    remaining >>= BigInt(8);
  }
  return bytes.join("");
};

const EMPTY_WITNESS_ARGS = repeatHex(0x10, 16);

interface HtlcFixture {
  htlcType: number;
  amount: bigint;
  paymentHash: string;
  remotePubkeyHash: string;
  localPubkeyHash: string;
  expiry: bigint;
}

interface UnlockFixture {
  unlockType: number;
  withPreimage: boolean;
  signature: string;
  preimage?: string;
}

interface WitnessFixture {
  htlcs: HtlcFixture[];
  settlementRemotePubkeyHash: string;
  settlementRemoteAmount: bigint;
  settlementLocalPubkeyHash: string;
  settlementLocalAmount: bigint;
  unlocks: UnlockFixture[];
}

/**
 * 构造 settlement witness。
 * htlc 记录的长度由各 fixture 字段自身的长度决定，因此调用方用 20 字节或
 * 32 字节的 paymentHash 即可得到 Legacy / v1 两种布局。
 */
const buildSettlementWitness = (fixture: WitnessFixture): string => {
  const parts: string[] = [
    EMPTY_WITNESS_ARGS,
    byteHex(fixture.unlocks.length),
    byteHex(fixture.htlcs.length),
  ];

  for (const htlc of fixture.htlcs) {
    parts.push(
      byteHex(htlc.htlcType),
      toLittleEndianHex(htlc.amount, 16),
      htlc.paymentHash,
      htlc.remotePubkeyHash,
      htlc.localPubkeyHash,
      toLittleEndianHex(htlc.expiry, 8)
    );
  }

  parts.push(
    fixture.settlementRemotePubkeyHash,
    toLittleEndianHex(fixture.settlementRemoteAmount, 16),
    fixture.settlementLocalPubkeyHash,
    toLittleEndianHex(fixture.settlementLocalAmount, 16)
  );

  for (const unlock of fixture.unlocks) {
    parts.push(
      byteHex(unlock.unlockType),
      byteHex(unlock.withPreimage ? 1 : 0),
      unlock.signature
    );
    if (unlock.withPreimage) {
      parts.push(unlock.preimage ?? "");
    }
  }

  return `0x${parts.join("")}`;
};

const buildRevocationWitness = (): string =>
  `0x${[
    EMPTY_WITNESS_ARGS,
    byteHex(0), // unlock_count = 0 表示 revocation
    toLittleEndianHex(BigInt(1), 8),
    repeatHex(0x42, 32),
    repeatHex(0x43, 65),
  ].join("")}`;

const SIGNATURE = repeatHex(0x11, 65);

describe("parseWitnessV2 settlement layout", () => {
  it("parses the Legacy layout (20-byte payment hash)", () => {
    const witness = buildSettlementWitness({
      htlcs: [
        {
          htlcType: 0,
          amount: BigInt(1000),
          paymentHash: repeatHex(0xaa, 20),
          remotePubkeyHash: repeatHex(0xbb, 20),
          localPubkeyHash: repeatHex(0xcc, 20),
          expiry: BigInt(1700000000),
        },
      ],
      settlementRemotePubkeyHash: repeatHex(0xdd, 20),
      settlementRemoteAmount: BigInt(111),
      settlementLocalPubkeyHash: repeatHex(0xee, 20),
      settlementLocalAmount: BigInt(222),
      unlocks: [
        { unlockType: 0, withPreimage: true, signature: SIGNATURE, preimage: repeatHex(0xff, 32) },
      ],
    });

    const parsed = parseWitnessV2(witness);
    const settlement = parsed.settlement!;

    expect(settlement.payment_hash_len).toBe(20);
    expect(settlement.htlcs[0].payment_hash).toBe(`0x${repeatHex(0xaa, 20)}`);
    expect(settlement.htlcs[0].remote_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xbb, 20)}`
    );
    expect(settlement.htlcs[0].local_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xcc, 20)}`
    );
    expect(settlement.htlcs[0].htlc_expiry_timestamp).toBe(BigInt(1700000000) * BigInt(1000));
    expect(settlement.settlement_remote_amount).toBe(BigInt(111));
    expect(settlement.settlement_local_amount).toBe(BigInt(222));
    expect(settlement.unlocks[0].preimage).toBe(`0x${repeatHex(0xff, 32)}`);
  });

  it("parses the v1 layout (32-byte full payment hash) without shifting later fields", () => {
    const witness = buildSettlementWitness({
      htlcs: [
        {
          htlcType: 0,
          amount: BigInt(1000),
          paymentHash: repeatHex(0xaa, 32),
          remotePubkeyHash: repeatHex(0xbb, 20),
          localPubkeyHash: repeatHex(0xcc, 20),
          expiry: BigInt(1700000000),
        },
        {
          htlcType: 1,
          amount: BigInt(2000),
          paymentHash: repeatHex(0xab, 32),
          remotePubkeyHash: repeatHex(0xcd, 20),
          localPubkeyHash: repeatHex(0xef, 20),
          expiry: BigInt(1700000100),
        },
      ],
      settlementRemotePubkeyHash: repeatHex(0xdd, 20),
      settlementRemoteAmount: BigInt(111),
      settlementLocalPubkeyHash: repeatHex(0xee, 20),
      settlementLocalAmount: BigInt(222),
      unlocks: [
        { unlockType: 0, withPreimage: true, signature: SIGNATURE, preimage: repeatHex(0xff, 32) },
      ],
    });

    const parsed = parseWitnessV2(witness);
    const settlement = parsed.settlement!;

    expect(settlement.pending_htlc_count).toBe(2);

    // payment hash 之后的字段会因布局误判而整体错位，是真正要守住的部分，
    // 因此先断言这些，避免被后面的布局标记断言掩盖
    expect(settlement.htlcs[0].remote_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xbb, 20)}`
    );
    expect(settlement.htlcs[0].local_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xcc, 20)}`
    );
    expect(settlement.htlcs[0].htlc_expiry_timestamp).toBe(BigInt(1700000000) * BigInt(1000));

    // 第二个 htlc 会累积偏移，错位最明显
    expect(settlement.htlcs[1].htlc_type).toBe(1);
    expect(settlement.htlcs[1].payment_amount).toBe(BigInt(2000));
    expect(settlement.htlcs[1].remote_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xcd, 20)}`
    );
    expect(settlement.htlcs[1].local_htlc_pubkey_hash).toBe(
      `0x${repeatHex(0xef, 20)}`
    );
    expect(settlement.htlcs[1].htlc_expiry_timestamp).toBe(BigInt(1700000100) * BigInt(1000));

    expect(settlement.settlement_remote_pubkey_hash).toBe(
      `0x${repeatHex(0xdd, 20)}`
    );
    expect(settlement.settlement_remote_amount).toBe(BigInt(111));
    expect(settlement.settlement_local_pubkey_hash).toBe(
      `0x${repeatHex(0xee, 20)}`
    );
    expect(settlement.settlement_local_amount).toBe(BigInt(222));
    expect(settlement.unlocks[0].preimage).toBe(`0x${repeatHex(0xff, 32)}`);

    // 布局标记与 payment hash 本身
    expect(settlement.payment_hash_len).toBe(32);
    expect(settlement.htlcs[0].payment_hash).toBe(`0x${repeatHex(0xaa, 32)}`);
    expect(settlement.htlcs[1].payment_hash).toBe(`0x${repeatHex(0xab, 32)}`);
  });

  it("parses a settlement with no pending htlc", () => {
    const witness = buildSettlementWitness({
      htlcs: [],
      settlementRemotePubkeyHash: repeatHex(0xdd, 20),
      settlementRemoteAmount: BigInt(111),
      settlementLocalPubkeyHash: repeatHex(0xee, 20),
      settlementLocalAmount: BigInt(222),
      unlocks: [
        { unlockType: 0, withPreimage: false, signature: SIGNATURE },
      ],
    });

    const parsed = parseWitnessV2(witness);
    const settlement = parsed.settlement!;

    expect(settlement.pending_htlc_count).toBe(0);
    expect(settlement.settlement_local_amount).toBe(BigInt(222));
    expect(settlement.unlocks[0].preimage).toBe("N/A");
    // 没有 pending htlc 时 Legacy 与 v1 的字节完全一致，无从区分也无妨，
    // 因为没有 payment hash 需要展示
    expect(settlement.payment_hash_len).toBe(20);
  });

  it("keeps parsing revocation witnesses", () => {
    const parsed = parseWitnessV2(buildRevocationWitness());

    expect(parsed.unlock_count).toBe(0);
    expect(parsed.settlement).toBeUndefined();
    expect(parsed.revocation?.pubkey).toBe(`0x${repeatHex(0x42, 32)}`);
    expect(parsed.revocation?.signature).toBe(`0x${repeatHex(0x43, 65)}`);
  });
});

describe("parseLockArgsV2 commitment contract features", () => {
  const argsPrefix =
    repeatHex(0x01, 20) + // pubkey hash
    toLittleEndianHex(BigInt(0), 8) + // delay epoch
    "00".repeat(8) + // protocol version
    repeatHex(0x02, 20) + // settlement hash
    "01"; // settlement flag

  it("treats 57-byte args as Legacy", () => {
    const parsed = parseLockArgsV2(`0x${argsPrefix}`);

    expect(parsed.features).toBe(0);
    expect(parsed.has_full_payment_hash).toBe(false);
    expect(parsed.settlement_flag).toBe(1);
  });

  it("reads the feature byte of 58-byte v1 args", () => {
    const parsed = parseLockArgsV2(`0x${argsPrefix}01`);

    expect(parsed.features).toBe(1);
    expect(parsed.has_full_payment_hash).toBe(true);
    // v1 只是追加 feature 字节，前 57 字节的语义不变
    expect(parsed.pubkey_hash).toBe(`0x${repeatHex(0x01, 20)}`);
    expect(parsed.settlement_hash).toBe(`0x${repeatHex(0x02, 20)}`);
    expect(parsed.settlement_flag).toBe(1);
  });
});
