"use client";

import { useState } from "react";
import Image from "next/image";
import {
  Timeline,
  TimelineEvent,
  TimelineContentRow,
  Dialog,
  InfoBox,
  TransactionOverview,
  CollapsibleSection,
} from "@/shared/components/ui";
import {
  formatTimestamp,
  parseLockArgsV2,
  parseWitnessV2,
  resolveChannelCommitmentFeatures,
  formatBlockNumber,
  type CommitmentFeatures,
  type ParsedWitnessData,
} from "../utils";
import type { ChannelStateInfo } from "@/lib/types";
import { buildTransactionLinkUrl } from "@/lib/utils";

interface ChannelLifecycleProps {
  channelState: ChannelStateInfo;
  network: "mainnet" | "testnet";
}

export function ChannelLifecycle({
  channelState,
  network,
}: ChannelLifecycleProps) {
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isCommitmentDialogOpen, setIsCommitmentDialogOpen] = useState(false);
  const [selectedTxIndex, setSelectedTxIndex] = useState<number | null>(null);

  // Open state Timeline
  if (channelState.state === "open" && channelState.txs.length > 0) {
    return (
      <>
        <Timeline>
          <TimelineEvent
            status="success"
            isFirst={true}
            title="Channel Opened (Funding)"
            timestamp={formatTimestamp(channelState.txs[0].timestamp)}
            badges={[
              {
                text: `${channelState.txs.length} Confirmation${
                  channelState.txs.length > 1 ? "s" : ""
                }`,
                color: "success",
              },
            ]}
            footerLinks={[
              {
                text: "View details",
                onClick: () => setIsDialogOpen(true),
              },
              {
                text: "View on Explore",
                onClick: () => {
                  const explorerUrl = buildTransactionLinkUrl(channelState.txs[0].tx_hash, network);
                  window.open(explorerUrl, "_blank", "noopener,noreferrer");
                },
              },
            ]}
          >
            <TimelineContentRow
              label="Block #:"
              value={formatBlockNumber(channelState.txs[0].block_number)}
              showCopy={true}
            />
            <TimelineContentRow
              label="Tx hash:"
              value={channelState.txs[0].tx_hash}
              showCopy={true}
            />
          </TimelineEvent>

          <TimelineEvent
            status="info"
            isLast={true}
            title="Channel Active"
            subtitle="No on-chain commitment or settlement transactions detected."
          />
        </Timeline>

        {/* Open Channel Dialog */}
        <Dialog
          isOpen={isDialogOpen}
          onClose={() => setIsDialogOpen(false)}
          title="Transaction Details"
          secondaryButtonText="View on Explorer"
          onSecondaryClick={() => {
            const explorerUrl = buildTransactionLinkUrl(channelState.txs[0].tx_hash, network);
            window.open(explorerUrl, "_blank", "noopener,noreferrer");
          }}
          primaryButtonIcon={
            <Image
              src="/explorer.svg"
              alt="Explorer icon"
              width={16}
              height={16}
            />
          }
        >
          <InfoBox
            title="Summary"
            content="This transaction opens the payment channel and locks the initial on-chain capacity."
          />

          <TransactionOverview
            txHash={`${channelState.txs[0].tx_hash.slice(0, 10)}..${channelState.txs[0].tx_hash.slice(-6)}`}
            fullTxHash={channelState.txs[0].tx_hash}
            fields={[
              {
                label: "Transaction type",
                value: "Channel funding",
                badge: {
                  text: "Channel funding",
                  status: "open",
                },
              },
              {
                label: "Block height",
                value: formatBlockNumber(channelState.txs[0].block_number),
                copyable: true,
              },
              {
                label: "Timestamp",
                value: formatTimestamp(channelState.txs[0].timestamp),
              },
            ]}
          />
        </Dialog>
      </>
    );
  }

  // Closed waiting settlement / Closed state Timeline
  const isClosedState = channelState.state === "closed_cooperative" || channelState.state === "closed_uncooperative";
  if (
    (channelState.state === "closed_waiting_onchain_settlement" || isClosedState) &&
    channelState.txs.length > 0
  ) {
    let commitmentFeatures: CommitmentFeatures | undefined;
    let commitmentFormatError: unknown;
    try {
      commitmentFeatures = resolveChannelCommitmentFeatures(channelState.txs);
    } catch (error) {
      commitmentFormatError = error;
    }
    const parseChannelWitness = (witness: string) => {
      if (commitmentFormatError) throw commitmentFormatError;
      return parseWitnessV2(witness, commitmentFeatures);
    };

    // Calculate commitment transactions
    const commitmentTxs =
      isClosedState && channelState.txs.length > 2
        ? channelState.txs.slice(1, -1)
        : channelState.state === "closed_waiting_onchain_settlement" && channelState.txs.length > 1
        ? channelState.txs.slice(1)
        : [];

    return (
      <>
        <Timeline>
          {/* Channel Opened */}
          <TimelineEvent
            status="success"
            isFirst={true}
            title="Channel Opened (Funding)"
            timestamp={formatTimestamp(channelState.txs[0].timestamp)}
            footerLinks={[
              {
                text: "View details",
                onClick: () => setIsDialogOpen(true),
              },
              {
                text: "View on Explore",
                onClick: () => {
                  const explorerUrl = buildTransactionLinkUrl(channelState.txs[0].tx_hash, network);
                  window.open(explorerUrl, "_blank", "noopener,noreferrer");
                },
              },
            ]}
          >
            <TimelineContentRow
              label="Block #:"
              value={formatBlockNumber(channelState.txs[0].block_number)}
              showCopy={true}
            />
            <TimelineContentRow
              label="Tx hash:"
              value={channelState.txs[0].tx_hash}
              showCopy={true}
            />
          </TimelineEvent>

          {/* Commitment Updates */}
          {commitmentTxs.length > 0 && (
            <TimelineEvent
              status="warning"
              isLast={channelState.state === "closed_waiting_onchain_settlement"}
              title={`Commitment updates (${commitmentTxs.length} update${
                commitmentTxs.length > 1 ? "s" : ""
              })`}
              subtitle="Preparing the channel state for final settlement and safe fund distribution"
              badges={[
                {
                  text: "Watchtower protection detected",
                  color: "info",
                  icon: (
                    <Image
                      src="/protection.svg"
                      alt="Protection icon"
                      width={16}
                      height={16}
                    />
                  ),
                },
              ]}
              titleIcon={
                <Image
                  src="/expand.svg"
                  alt="Expand icon"
                  width={20}
                  height={20}
                />
              }
              tableColumns={[
                {
                  key: "tx_hash",
                  label: "Tx hash",
                  render: (value, row) => (
                    <div className="type-body text-primary">
                      {`${(row as { tx_hash: string }).tx_hash.slice(0, 10)}...${(row as { tx_hash: string }).tx_hash.slice(-8)}`}
                    </div>
                  ),
                },
                {
                  key: "commitment_args",
                  label: "Commitment args",
                  render: (value, row) => (
                    <div className="type-body text-primary">
                      {(row as { commitment_args: string | null }).commitment_args
                        ? `${(row as { commitment_args: string }).commitment_args.slice(0, 10)}...${(row as { commitment_args: string }).commitment_args.slice(-6)}`
                        : "-"}
                    </div>
                  ),
                },
                {
                  key: "block_number",
                  label: "Block",
                  render: (value, row) => (
                    <div className="type-body text-primary">
                      {formatBlockNumber((row as { block_number: string }).block_number)}
                    </div>
                  ),
                },
                {
                  key: "witness_args",
                  label: "Unlock count",
                  render: (value, row) => {
                    try {
                      const witnessArgs = (row as { witness_args: string | null })
                        .witness_args;
                      if (witnessArgs) {
                        const parsed = parseChannelWitness(witnessArgs);
                        return (
                          <div className="type-body text-primary">
                            {String(parsed.settlement?.unlocks.length ?? parsed.unlock_count)}
                          </div>
                        );
                      }
                    } catch (e) {
                      console.error("Failed to parse witness_args", e);
                    }
                    return <div className="type-body text-primary">-</div>;
                  },
                },
                {
                  key: "pending_htlcs",
                  label: "Pending HTLCs",
                  render: (value, row) => {
                    try {
                      const witnessArgs = (row as { witness_args: string | null })
                        .witness_args;
                      if (witnessArgs) {
                        const parsed = parseChannelWitness(witnessArgs);
                        const unlockCount = Number(parsed.unlock_count);
                        if (unlockCount > 0 && parsed.settlement) {
                          return (
                            <div className="type-body text-primary">
                              {String(parsed.settlement.pending_htlc_count)}
                            </div>
                          );
                        }
                      }
                    } catch (e) {
                      console.error("Failed to parse witness_args", e);
                    }
                    return <div className="type-body text-primary">-</div>;
                  },
                },
                {
                  key: "timestamp",
                  label: "Timestamp",
                  render: (value, row) => (
                    <div className="type-body text-primary">
                      {formatTimestamp((row as { timestamp: string }).timestamp)}
                    </div>
                  ),
                },
                {
                  key: "actions",
                  label: "",
                  render: (value, row) => (
                    <button
                      onClick={() => {
                        const txIndex = channelState.txs.findIndex(
                          (t) => t.tx_hash === (row as { tx_hash: string }).tx_hash
                        );
                        setSelectedTxIndex(txIndex);
                        setIsCommitmentDialogOpen(true);
                      }}
                      className="type-button2 text-purple hover:opacity-80 cursor-pointer"
                    >
                      View detail
                    </button>
                  ),
                },
              ]}
              tableData={commitmentTxs}
            />
          )}

          {/* Channel Closed (Final Settlement) */}
          {isClosedState && ((() => {
            // 判断关闭类型：cooperative 或 uncooperative
            const isCooperativeClose = channelState.state === "closed_cooperative";
            
            return (
            <TimelineEvent
              status={isCooperativeClose ? "purple" : "error"}
              isLast={true}
              title="Channel Closed (Final Settlement)"
              timestamp={formatTimestamp(
                channelState.txs[channelState.txs.length - 1].timestamp
              )}
              badges={[
                {
                  text: isCooperativeClose ? "Cooperative closed" : "Uncooperative closed",
                  color: isCooperativeClose ? "purple" : "error",
                },
              ]}
              footerLinks={[
                {
                  text: "View details",
                  onClick: () => {
                    setSelectedTxIndex(channelState.txs.length - 1);
                    setIsCommitmentDialogOpen(true);
                  },
                },
                {
                  text: "View on Explore",
                  onClick: () => {
                    const lastTx = channelState.txs[channelState.txs.length - 1];
                  const explorerUrl = buildTransactionLinkUrl(lastTx.tx_hash, network);
                    window.open(explorerUrl, "_blank", "noopener,noreferrer");
                  },
                },
              ]}
            >
              <TimelineContentRow
                label="Block #:"
                value={formatBlockNumber(channelState.txs[
                  channelState.txs.length - 1
                ].block_number)}
                showCopy={true}
              />
              <TimelineContentRow
                label="Tx hash:"
                value={channelState.txs[channelState.txs.length - 1].tx_hash}
                showCopy={true}
              />
            </TimelineEvent>
            );
          })())}
        </Timeline>

        {/* Open Channel Funding Dialog */}
        <Dialog
          isOpen={isDialogOpen}
          onClose={() => setIsDialogOpen(false)}
          title="Transaction Details"
          secondaryButtonText="View on Explorer"
          onSecondaryClick={() => {
            const explorerUrl = buildTransactionLinkUrl(channelState.txs[0].tx_hash, network);
            window.open(explorerUrl, "_blank", "noopener,noreferrer");
          }}
          primaryButtonIcon={
            <Image
              src="/explorer.svg"
              alt="Explorer icon"
              width={16}
              height={16}
            />
          }
        >
          <InfoBox
            title="Summary"
            content="This transaction opens the payment channel and locks the initial on-chain capacity."
          />

          <TransactionOverview
            txHash={`${channelState.txs[0].tx_hash.slice(0, 10)}..${channelState.txs[0].tx_hash.slice(-6)}`}
            fullTxHash={channelState.txs[0].tx_hash}
            fields={[
              {
                label: "Transaction type",
                value: "Channel funding",
                badge: {
                  text: "Channel funding",
                  status: "open",
                },
              },
              {
                label: "Block height",
                value: formatBlockNumber(channelState.txs[0].block_number),
                copyable: true,
              },
              {
                label: "Timestamp",
                value: formatTimestamp(channelState.txs[0].timestamp),
              },
            ]}
          />
        </Dialog>

        {/* Commitment/Settlement Transaction Dialog */}
        {selectedTxIndex !== null &&
          channelState.txs[selectedTxIndex] &&
          (() => {
            const tx = channelState.txs[selectedTxIndex];
            let parsedArgs = null;
            let parsedWitness: ParsedWitnessData | null = null;
            let decodeError: string | null = null;

            try {
              if (tx.commitment_args) {
                parsedArgs = parseLockArgsV2(tx.commitment_args);
              }
            } catch (e) {
              decodeError = e instanceof Error ? e.message : "Unsupported commitment data.";
            }
            try {
              if (tx.witness_args) {
                parsedWitness = parseChannelWitness(tx.witness_args);
              }
            } catch (e) {
              decodeError = e instanceof Error ? e.message : "Unsupported commitment data.";
            }

            const isLastTx = selectedTxIndex === channelState.txs.length - 1;
            const isClosedState = channelState.state === "closed_cooperative" || channelState.state === "closed_uncooperative";

            return (
              <Dialog
                isOpen={isCommitmentDialogOpen}
                onClose={() => {
                  setIsCommitmentDialogOpen(false);
                  setSelectedTxIndex(null);
                }}
                title="Transaction Details"
                secondaryButtonText="View on Explorer"
                onSecondaryClick={() => {
                  const explorerUrl = buildTransactionLinkUrl(tx.tx_hash, network);
                  window.open(explorerUrl, "_blank", "noopener,noreferrer");
                }}
                primaryButtonIcon={
                  <Image
                    src="/explorer.svg"
                    alt="Explorer icon"
                    width={16}
                    height={16}
                  />
                }
              >
                <InfoBox
                  title="Summary"
                  content={
                    isLastTx && isClosedState
                      ? "Final settlement executed. Channel closed and final balances distributed on-chain."
                      : "Further commitment updates may occur before final settlement."
                  }
                />

                <TransactionOverview
                  txHash={`${tx.tx_hash.slice(0, 10)}..${tx.tx_hash.slice(-6)}`}
                  fullTxHash={tx.tx_hash}
                  fields={[
                    {
                      label: "Transaction type",
                      value:
                        isLastTx && isClosedState
                          ? "Final settlement"
                          : "Committing update",
                      badge: {
                        text:
                          isLastTx && isClosedState
                            ? "Final settlement"
                            : "Committing update",
                        status: isLastTx && isClosedState ? "settled" : "commitment",
                      },
                    },
                    {
                      label: "Block height",
                      value: formatBlockNumber(tx.block_number),
                      copyable: true,
                    },
                    {
                      label: "Commitment args",
                      value: tx.commitment_args || "-",
                      copyable: true,
                    },
                    {
                      label: "Timestamp",
                      value: formatTimestamp(tx.timestamp),
                    },
                  ]}
                />

                {decodeError && (
                  <InfoBox
                    title="Unable to decode transaction details"
                    content={`${decodeError} View the transaction on the explorer for the original data.`}
                  />
                )}

                {(parsedArgs || parsedWitness) && (
                  <TransactionOverview
                    title="Channel state snapshot (Decoded from Witness data)"
                    txHash={
                      tx.witness_args
                        ? `${tx.witness_args.slice(0, 10)}..${tx.witness_args.slice(-6)}`
                        : ""
                    }
                    fullTxHash={tx.witness_args || ""}
                    fields={[
                      ...(parsedWitness?.empty_witness_args
                        ? [
                            {
                              label: "Empty witness",
                              value: String(parsedWitness.empty_witness_args),
                              copyable: true,
                            },
                          ]
                        : []),
                      ...(parsedArgs
                        ? [
                            {
                              label: "Pubkey hash",
                              value: parsedArgs.pubkey_hash,
                              copyable: true,
                            },
                            {
                              label: "Security delay",
                              value: `Number: ${parsedArgs.delay_epoch.number}, Index: ${parsedArgs.delay_epoch.index}, Length: ${parsedArgs.delay_epoch.length}`,
                            },
                            {
                              label: "Protocol version",
                              value: parsedArgs.version,
                            },
                            {
                              label: "",
                              value: "",
                              isSeparator: true,
                            },
                            {
                              label: "Settlement hash",
                              value: parsedArgs.settlement_hash,
                              copyable: true,
                            },
                            {
                              label: "Settlement flag",
                              value:
                                parsedArgs.settlement_flag === 0
                                  ? "0 (First settlement)"
                                  : "1 (Subsequent commitment update)",
                            },
                            {
                              label: "Commitment contract",
                              value: parsedArgs.has_full_payment_hash
                                ? "v1 (full payment hash)"
                                : "Legacy (20-byte hash prefix)",
                            },
                          ]
                        : []),
                      ...(parsedWitness &&
                      Number(parsedWitness.unlock_count) > 0 &&
                      parsedWitness.settlement
                        ? [
                            {
                              label: "Settlement local pubkey hash",
                              value: parsedWitness.settlement.settlement_local_pubkey_hash,
                              copyable: true,
                            },
                            {
                              label: "Settlement local amount",
                              value: String(parsedWitness.settlement.settlement_local_amount),
                            },
                            {
                              label: "Settlement remote pubkey hash",
                              value: parsedWitness.settlement.settlement_remote_pubkey_hash,
                              copyable: true,
                            },
                            {
                              label: "Settlement remote amount",
                              value: String(parsedWitness.settlement.settlement_remote_amount),
                            },
                            {
                              label: "Payment hash format",
                              value: parsedWitness.settlement.payment_hash_len === 32
                                ? "32 bytes (v1 full hash)"
                                : "20 bytes (Legacy prefix)",
                            },
                          ]
                        : []),
                    ]}
                  />
                )}

                {parsedWitness &&
                  Number(parsedWitness.unlock_count) > 0 &&
                  parsedWitness.settlement && (
                    <CollapsibleSection
                      title={`Unlocks (${parsedWitness.settlement.unlocks?.length || 0})`}
                      badge={{
                        text: "Settlement unlock",
                        status: "Active",
                      }}
                      tableColumns={[
                        {
                          key: "type",
                          label: "Type",
                          width: "w-48",
                          showInfo: true,
                          infoTooltip: "Unlock type information",
                        },
                        {
                          key: "withPreimage",
                          label: "With Preimage",
                          width: "w-40",
                          showInfo: true,
                          infoTooltip: "Indicates if preimage is provided",
                        },
                        {
                          key: "preimage",
                          label: "Preimage",
                          width: "w-32",
                        },
                        {
                          key: "signature",
                          label: "Signature",
                          width: "flex-1",
                        },
                      ]}
                      tableData={
                        parsedWitness.settlement.unlocks?.map((unlock) => ({
                          type: `${unlock.unlock_type} (${
                            unlock.unlock_type === 255 ? "Unlocks an HTLC" : "Other"
                          })`,
                          withPreimage: `${unlock.with_preimage} (${
                            unlock.with_preimage === 0 ? "No" : "Yes"
                          })`,
                          preimage:
                            unlock.with_preimage === 1 && unlock.preimage !== "N/A"
                              ? `${unlock.preimage.slice(0, 10)}...${unlock.preimage.slice(-6)}`
                              : "-",
                          signature: `${unlock.signature.slice(0, 10)}...${unlock.signature.slice(-6)}`,
                        })) || []
                      }
                      defaultExpanded={false}
                    />
                  )}

                {parsedWitness &&
                  Number(parsedWitness.unlock_count) > 0 &&
                  parsedWitness.settlement &&
                  parsedWitness.settlement.htlcs?.length > 0 && (
                    <CollapsibleSection
                      title={`Pending HTLCs (${parsedWitness.settlement.htlcs.length})`}
                      tableColumns={[
                        {
                          key: "type",
                          label: "Type",
                          width: "w-20",
                          showInfo: true,
                          infoTooltip: "HTLC type information",
                        },
                        {
                          key: "amount",
                          label: "Amount",
                          width: "w-32",
                        },
                        {
                          key: "paymentHash",
                          label: "Payment hash",
                          width: "w-40",
                        },
                        {
                          key: "localPubkeyHash",
                          label: "Local pubkey hash",
                          width: "w-40",
                        },
                        {
                          key: "remotePubkeyHash",
                          label: "Remote pubkey hash",
                          width: "w-40",
                        },
                        {
                          key: "expiresOn",
                          label: "Expires on",
                          width: "flex-1",
                        },
                      ]}
                      tableData={parsedWitness.settlement.htlcs.map((htlc) => ({
                        type: htlc.htlc_type.toString(),
                        amount: String(htlc.payment_amount),
                        paymentHash: `${htlc.payment_hash.slice(0, 10)}...${htlc.payment_hash.slice(-6)}`,
                        localPubkeyHash: `${htlc.local_htlc_pubkey_hash.slice(0, 10)}...${htlc.local_htlc_pubkey_hash.slice(-6)}`,
                        remotePubkeyHash: `${htlc.remote_htlc_pubkey_hash.slice(0, 10)}...${htlc.remote_htlc_pubkey_hash.slice(-6)}`,
                        expiresOn: formatTimestamp(String(htlc.htlc_expiry_timestamp)),
                      }))}
                      defaultExpanded={false}
                    />
                  )}

                {isClosedState &&
                  isLastTx &&
                  parsedWitness &&
                  Number(parsedWitness.unlock_count) > 0 &&
                  parsedWitness.settlement &&
                  (parsedWitness.settlement.htlcs?.length === 0 ||
                    !parsedWitness.settlement.htlcs) && (
                    <CollapsibleSection
                      title="All HTLCs resolved before settlement"
                      disableCollapse={true}
                    />
                  )}
              </Dialog>
            );
          })()}
      </>
    );
  }

  return null;
}
