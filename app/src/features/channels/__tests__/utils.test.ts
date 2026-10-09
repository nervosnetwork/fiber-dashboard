import { describe, expect, it } from "vitest";
import {
  parseLockArgsV2,
  parseWitnessV2,
  resolveChannelCommitmentFeatures,
  type CommitmentFeatures,
} from "../utils";

const HEADER = "10000000100000001000000010000000";
const VERSION = "0102030405060708";
const DELAY = (BigInt(5) << BigInt(40)) | (BigInt(2) << BigInt(24)) | BigInt(7);
const byte = (value: number) => value.toString(16).padStart(2, "0");
const repeated = (value: number, length: number) => byte(value).repeat(length);
const hex = (value: number, length: number) => `0x${repeated(value, length)}`;
const le = (value: bigint, length: number) =>
  Array.from({ length }, (_, i) =>
    byte(Number((value >> BigInt(i * 8)) & BigInt(255)))
  ).join("");

function lockArgs(features: CommitmentFeatures, settlementFlag = 0) {
  return (
    "0x" +
    repeated(0x11, 20) +
    le(DELAY, 8) +
    VERSION +
    repeated(0x22, 20) +
    byte(settlementFlag) +
    (features === 1 ? "01" : "")
  );
}

// Protocol-shaped parsing fixtures, not signed on-chain transactions.
function settlementWitness(features: CommitmentFeatures, pendingCount = 2) {
  const hashLength = features === 1 ? 32 : 20;
  const htlcs = Array.from({ length: pendingCount }, (_, i) => {
    const seconds = BigInt(1700000000 + i);
    return {
      htlc_type: i % 2,
      payment_amount: (BigInt(1) << BigInt(100)) + BigInt(i + 1),
      payment_hash: hex(0x30 + i, hashLength),
      remote_htlc_pubkey_hash: hex(0x50 + i, 20),
      local_htlc_pubkey_hash: hex(0x70 + i, 20),
      htlc_expiry_timestamp: seconds * BigInt(1000),
      htlc_expiry: new Date(Number(seconds * BigInt(1000))).toLocaleString(
        "zh-CN"
      ),
    };
  });
  const unlocks =
    pendingCount === 0
      ? [
          {
            unlock_type: 0xfe,
            with_preimage: 0,
            signature: hex(0xa0, 65),
            preimage: "N/A",
          },
        ]
      : [
          {
            unlock_type: 0,
            with_preimage: 1,
            signature: hex(0xa0, 65),
            preimage: hex(0xc0, 32),
          },
          {
            unlock_type: 1,
            with_preimage: 0,
            signature: hex(0xa1, 65),
            preimage: "N/A",
          },
        ];
  const remoteAmount = (BigInt(1) << BigInt(96)) + BigInt(17);
  const localAmount = (BigInt(1) << BigInt(80)) + BigInt(29);
  const htlcBytes = htlcs
    .map(
      htlc =>
        byte(htlc.htlc_type) +
        le(htlc.payment_amount, 16) +
        htlc.payment_hash.slice(2) +
        htlc.remote_htlc_pubkey_hash.slice(2) +
        htlc.local_htlc_pubkey_hash.slice(2) +
        le(
          (BigInt(1) << BigInt(62)) |
            (htlc.htlc_expiry_timestamp / BigInt(1000)),
          8
        )
    )
    .join("");
  const unlockBytes = unlocks
    .map(
      unlock =>
        byte(unlock.unlock_type) +
        byte(unlock.with_preimage) +
        unlock.signature.slice(2) +
        (unlock.with_preimage === 1 ? unlock.preimage.slice(2) : "")
    )
    .join("");

  return {
    witness:
      "0x" +
      HEADER +
      byte(unlocks.length) +
      byte(pendingCount) +
      htlcBytes +
      repeated(0xd0, 20) +
      le(remoteAmount, 16) +
      repeated(0xd1, 20) +
      le(localAmount, 16) +
      unlockBytes,
    expected: {
      payment_hash_len: hashLength,
      pending_htlc_count: pendingCount,
      htlcs,
      settlement_remote_pubkey_hash: hex(0xd0, 20),
      settlement_remote_amount: remoteAmount,
      settlement_local_pubkey_hash: hex(0xd1, 20),
      settlement_local_amount: localAmount,
      unlocks,
    },
  };
}

const replaceByte = (value: string, offset: number, replacement: number) =>
  value.slice(0, 2 + offset * 2) +
  byte(replacement) +
  value.slice(2 + (offset + 1) * 2);

function ambiguousWitness() {
  const start = 18 + 8 * 97 + 72;
  const bytes = new Uint8Array(start + 3 * 67).fill(0x22);
  bytes.set(HEADER.match(/../g)!.map(value => parseInt(value, 16)));
  bytes[16] = 3;
  bytes[17] = 8;
  for (let i = 0; i < 8; i++) {
    const offset = 18 + i * 97;
    bytes[offset] = 0;
    bytes.fill(0, offset + 89, offset + 97);
  }
  for (let i = 0; i < 3; i++) {
    bytes[start + i * 67] = i;
    bytes[start + i * 67 + 1] = 0;
  }
  // Legacy starts unlocks 8 * 12 = 96 bytes early. Three false preimage
  // flags then consume 3 * 32 = 96 bytes, so total length alone is ambiguous.
  for (const offset of [start - 95, start + 4, start + 103]) bytes[offset] = 1;
  return "0x" + Array.from(bytes, byte).join("");
}

describe("commitment lock args", () => {
  it.each([0, 1] as const)("reads fields with features=%i", features => {
    expect(parseLockArgsV2(lockArgs(features, 1))).toEqual({
      pubkey_hash: hex(0x11, 20),
      delay_epoch: {
        number: "7",
        index: "2",
        length: "5",
        value: DELAY.toString(),
      },
      version: BigInt(`0x${VERSION}`).toString(),
      settlement_hash: hex(0x22, 20),
      settlement_flag: 1,
      features,
      has_full_payment_hash: features === 1,
    });
  });

  it.each([
    "",
    "0x",
    "0x0",
    lockArgs(0).slice(0, -2),
    lockArgs(0) + "00",
    lockArgs(0) + "02",
    lockArgs(0) + "03",
    lockArgs(1) + "00",
    lockArgs(0).slice(0, -2) + "gg",
  ])("rejects invalid args: %s", args => {
    expect(() => parseLockArgsV2(args)).toThrow();
  });
});

describe("channel commitment features", () => {
  it("does not default missing args to legacy", () => {
    expect(resolveChannelCommitmentFeatures([])).toBeUndefined();
    expect(
      resolveChannelCommitmentFeatures([{ commitment_args: null }])
    ).toBeUndefined();
  });

  it.each([0, 1] as const)(
    "handles final null args and same-block reordering for features=%i",
    features => {
      const final = { block_number: "0x100", commitment_args: null };
      const initial = {
        block_number: "0x100",
        commitment_args: lockArgs(features),
      };
      const partial = {
        block_number: "0x100",
        commitment_args: lockArgs(features, 1),
      };
      for (const txs of [
        [initial, partial, final],
        [final, partial, initial],
        [partial, final, initial],
      ]) {
        const resolved = resolveChannelCommitmentFeatures(txs);
        expect(resolved).toBe(features);
        expect(
          parseWitnessV2(settlementWitness(features).witness, resolved)
            .settlement?.payment_hash_len
        ).toBe(features === 1 ? 32 : 20);
      }
    }
  );

  it("rejects mixed formats and unknown features", () => {
    expect(() =>
      resolveChannelCommitmentFeatures([
        { commitment_args: lockArgs(0) },
        { commitment_args: lockArgs(1) },
      ])
    ).toThrow();
    expect(() =>
      resolveChannelCommitmentFeatures([
        { commitment_args: lockArgs(1) },
        { commitment_args: lockArgs(0) + "03" },
      ])
    ).toThrow();
  });
});

describe("commitment witnesses", () => {
  it.each([0, 1] as const)(
    "reads every settlement field with features=%i",
    features => {
      const fixture = settlementWitness(features);
      expect(parseWitnessV2(fixture.witness, features)).toEqual({
        empty_witness_args: `0x${HEADER}`,
        unlock_count: 2,
        settlement: fixture.expected,
      });
    }
  );

  it.each([0, 1] as const)(
    "reads zero pending HTLCs with features=%i",
    features => {
      const fixture = settlementWitness(features, 0);
      expect(parseWitnessV2(fixture.witness, features).settlement).toEqual(
        fixture.expected
      );
    }
  );

  it("requires features even with zero pending HTLCs", () => {
    expect(() => parseWitnessV2(settlementWitness(1).witness)).toThrow();
    expect(() => parseWitnessV2(settlementWitness(1, 0).witness)).toThrow();
  });

  it("reads all unlock records when the nonzero header count differs", () => {
    const twoUnlocks = settlementWitness(1).witness;
    expect(
      parseWitnessV2(replaceByte(twoUnlocks, 16, 1), 1).settlement?.unlocks
    ).toHaveLength(2);
    const oneUnlock = settlementWitness(1, 0).witness;
    expect(
      parseWitnessV2(replaceByte(oneUnlock, 16, 2), 1).settlement?.unlocks
    ).toHaveLength(1);
    expect(() => parseWitnessV2(oneUnlock.slice(0, -(67 * 2)), 1)).toThrow();
  });

  it.each([0, 1] as const)(
    "rejects the wrong layout for unambiguous features=%i data",
    features => {
      expect(() =>
        parseWitnessV2(
          settlementWitness(features).witness,
          features === 1 ? 0 : 1
        )
      ).toThrow();
    }
  );

  it.each([0, 1] as const)(
    "rejects malformed witness fields with features=%i",
    features => {
      const { witness } = settlementWitness(features);
      const unlockStart = 18 + 2 * (features === 1 ? 97 : 85) + 72;
      for (const malformed of [
        witness.slice(0, -2),
        witness.slice(0, 2 + 34 * 2),
        witness.slice(0, 2 + 16 * 2),
        witness + "00",
        replaceByte(witness, unlockStart + 1, 2),
        replaceByte(witness, 0, 0),
        replaceByte(witness, unlockStart, 10),
      ]) {
        expect(() => parseWitnessV2(malformed, features)).toThrow();
      }
    }
  );

  it.each(["", "0x", "0x0", "0xgg"])("rejects invalid hex: %s", witness => {
    expect(() => parseWitnessV2(witness, 1)).toThrow();
  });

  it("uses lock features for the 8-HTLC / 3-unlock length collision", () => {
    const witness = ambiguousWitness();
    const features = resolveChannelCommitmentFeatures([
      { commitment_args: null },
      { commitment_args: lockArgs(1) },
    ]);
    const parsed = parseWitnessV2(witness, features).settlement!;
    // Assert decoded data before metadata: this must catch the original offset bug.
    expect(parsed.unlocks.map(unlock => unlock.with_preimage)).toEqual([0, 0, 0]);
    expect(parsed.payment_hash_len).toBe(32);
    expect(parsed.pending_htlc_count).toBe(8);
    expect(parsed.htlcs).toHaveLength(8);
    expect(parsed.htlcs.every(htlc => htlc.payment_hash.length === 66)).toBe(
      true
    );
    expect(parsed.unlocks.map(unlock => unlock.unlock_type)).toEqual([0, 1, 2]);
    expect(parsed.unlocks.map(unlock => unlock.preimage)).toEqual([
      "N/A",
      "N/A",
      "N/A",
    ]);
    expect(() => parseWitnessV2(witness)).toThrow();
  });

  it("reads revocation without requiring commitment features", () => {
    const witness =
      "0x" + HEADER + "00" + VERSION + repeated(0xe0, 32) + repeated(0xf0, 64);
    expect(parseWitnessV2(witness)).toEqual({
      empty_witness_args: `0x${HEADER}`,
      unlock_count: 0,
      revocation: {
        version: BigInt(`0x${VERSION}`),
        pubkey: hex(0xe0, 32),
        signature: hex(0xf0, 64),
      },
    });
    expect(() => parseWitnessV2(witness.slice(0, -2))).toThrow();
    expect(() => parseWitnessV2(witness + "00")).toThrow();
  });
});
