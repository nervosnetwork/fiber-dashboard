import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import type { ChannelStateInfo } from "@/lib/types";
import { ChannelLifecycle } from "../components/ChannelLifecycle";

// Keep the real lifecycle components, without loading unrelated barrel exports
// such as CustomSelect and its Next-specific PostCSS configuration.
vi.mock("@/shared/components/ui", async () => ({
  ...(await import("@/shared/components/ui/Timeline")),
  ...(await import("@/shared/components/ui/Dialog")),
  ...(await import("@/shared/components/ui/InfoBox")),
  ...(await import("@/shared/components/ui/TransactionOverview")),
  ...(await import("@/shared/components/ui/CollapsibleSection")),
}));

vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
let root: Root | undefined;
let container: HTMLDivElement;
afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = undefined;
});
afterAll(() => vi.unstubAllGlobals());

const findText = (text: string) =>
  Array.from(container.querySelectorAll("*")).find(
    element => element.textContent === text
  ) ?? null;
const findButton = (text: string) =>
  Array.from(container.querySelectorAll("button")).find(
    element => element.textContent === text
  ) ?? null;

const args = (fullHash: boolean) =>
  `0x${"11".repeat(20)}${"00".repeat(16)}${"22".repeat(20)}00${fullHash ? "01" : ""}`;

// One pending HTLC, one unlock without a preimage; synthetic signatures.
const witness = (fullHash: boolean) =>
  "0x" +
  [
    "10000000100000001000000010000000",
    "01",
    "01",
    "00",
    "01" + "00".repeat(15),
    "aa".repeat(fullHash ? 32 : 20),
    "bb".repeat(20),
    "cc".repeat(20),
    "00".repeat(8),
    "dd".repeat(20),
    "6f" + "00".repeat(15),
    "ee".repeat(20),
    "de" + "00".repeat(15),
    "00",
    "00",
    "ff".repeat(65),
  ].join("");

function channel(fullHash: boolean): ChannelStateInfo {
  const tx = (
    id: number,
    commitment_args: string | null,
    witness_args: string | null
  ) => ({
    tx_hash: `0x${id.toString(16).repeat(64)}`,
    block_number: "0x100",
    timestamp: "2026-09-30T00:00:00Z",
    commitment_args,
    witness_args,
  });
  return {
    channel_id: "test-channel",
    funding_args: "0x00",
    state: "closed_uncooperative",
    txs: [
      tx(1, null, null),
      tx(2, args(fullHash), null),
      tx(3, null, witness(fullHash)),
    ],
  };
}

function openFinalSettlement(state: ChannelStateInfo) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() =>
    root!.render(<ChannelLifecycle channelState={state} network="testnet" />)
  );
  const buttons = Array.from(container.querySelectorAll("button")).filter(
    button => button.textContent === "View details"
  );
  expect(buttons).toHaveLength(2);
  act(() => buttons[buttons.length - 1].click());
}

describe("channel lifecycle commitment decoding", () => {
  it.each([false, true])(
    "decodes a final settlement with null output args (fullHash=%s)",
    fullHash => {
      openFinalSettlement(channel(fullHash));
      expect(
        findText(
          fullHash ? "32 bytes (v1 full hash)" : "20 bytes (Legacy prefix)"
        )
      ).toBeInTheDocument();
      expect(findText("111")).toBeInTheDocument();
      expect(findText("222")).toBeInTheDocument();
      expect(
        findText("Unable to decode transaction details")
      ).not.toBeInTheDocument();
    }
  );

  it("shows an error instead of guessing when commitment history is missing", () => {
    const state = channel(true);
    state.txs[1].commitment_args = null;
    openFinalSettlement(state);
    expect(
      findText("Unable to decode transaction details")
    ).toBeInTheDocument();
    expect(findText("Settlement remote amount")).not.toBeInTheDocument();
    expect(findButton("View on Explorer")).toBeInTheDocument();
  });

  it("shows an error for conflicting commitment formats", () => {
    const state = channel(true);
    state.txs.splice(2, 0, {
      ...state.txs[1],
      tx_hash: `0x${"4".repeat(64)}`,
      commitment_args: args(false),
    });
    openFinalSettlement(state);
    expect(
      findText("Unable to decode transaction details")
    ).toBeInTheDocument();
    expect(findText("Settlement remote amount")).not.toBeInTheDocument();
  });
});
