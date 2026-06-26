import { PageHeader, SectionHeader, Table, Pagination, GlassCardContainer, StatusBadge, CopyButton } from "@/shared/components/ui";
import type { ColumnDef, SortState } from "@/shared/components/ui";
import { useState, useMemo, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { useNetwork } from "@/features/networks/context/NetworkContext";
import { RustNodeInfo } from "@/lib/types";
import { hexToDecimal } from "@/lib/utils";
import { NodeDetailCard } from "./NodeDetailCard";
import { getAssetColor as getAssetColorUtil } from "@/features/channels/utils/assetColors";

interface ChannelData extends Record<string, unknown> {
  channelId: string;
  asset: string; // 资产名称（大写）
  assetColor: string; // 资产对应的颜色
  assetLiquidity: string; // 资产流动性
  assetLiquidityUnit: string; // 资产流动性单位
  state: string; // 通道状态
  createdOn: string;
  lastCommitted: string;
}

interface AssetData extends Record<string, unknown> {
  assetName: string;
  autoAcceptValue: string;
}

// Channel Outpoint Cell 组件
const ChannelOutpointCell = ({ value }: { value: string }) => {
  // 中间省略显示：前12个字符 + "..." + 后12个字符
  const displayValue = value.length > 30 
    ? `${value.slice(0, 12)}...${value.slice(-12)}`
    : value;
  
  return (
    <div className="flex items-center gap-2 group min-w-0">
      <span 
        className="text-primary text-sm font-mono hover:underline cursor-pointer transition-colors truncate min-w-0 flex-1" 
        title={value}
        onMouseEnter={(e) => e.currentTarget.style.color = '#674BDC'}
        onMouseLeave={(e) => e.currentTarget.style.color = ''}
      >
        {displayValue}
      </span>
      <CopyButton text={value} className="opacity-0 group-hover:opacity-100 flex-shrink-0" />
    </div>
  );
};


export const NodeDetail = () => {
  const params = useParams();
  const nodeId = decodeURIComponent(params.nodeId as string);
  const { apiClient, currentNetwork } = useNetwork();
  const router = useRouter();

  const [currentPage, setCurrentPage] = useState(1);
  const [sortKey, setSortKey] = useState<string>('lastCommitted');
  const [sortState, setSortState] = useState<SortState>('descending');
  const PAGE_SIZE = 10;

  // 当排序条件改变时，自动重置到第一页
  useEffect(() => {
    setCurrentPage(1);
  }, [sortKey, sortState]);

  // 拉取节点信息
  const {
    data: nodeInfo,
    isLoading: nodeLoading,
    isFetched: nodeFetched,
  } = useQuery<RustNodeInfo | null>({
    queryKey: ["node-info", nodeId, currentNetwork],
    queryFn: () => apiClient.getNodeInfo(nodeId),
    enabled: !!nodeId,
    retry: 3,
  });

  // 拉取节点支持的 UDT 资产信息
  const { data: udtInfos } = useQuery({
    queryKey: ["node-udt-infos", nodeId, currentNetwork],
    queryFn: () => apiClient.getNodeUdtInfos(nodeId),
    enabled: !!nodeId,
  });

  // 使用新接口：直接拉取该节点的通道数据（支持分页和排序）
  // 只筛选 CKB 和 USDI 资产
  const { data: channelsResponse, isLoading: channelsLoading } = useQuery({
    queryKey: ["node-channels", nodeId, currentNetwork, currentPage, sortKey, sortState],
    queryFn: () => {
      // 将前端的 sortKey 映射到后端的 sort_by
      const sortByMapping: Record<string, "create_time" | "last_commit_time" | "capacity"> = {
        'createdOn': 'create_time',
        'lastCommitted': 'last_commit_time',
        'capacity': 'capacity',
      };
      const sortBy = sortByMapping[sortKey] || 'last_commit_time';
      const order = sortState === 'ascending' ? 'asc' : 'desc';
      console.log('[NodeDetail] Sort params - sortKey:', sortKey, 'sortState:', sortState);
      console.log('[NodeDetail] Backend params - sortBy:', sortBy, 'order:', order);
      return apiClient.getChannelsByNodeId(nodeId, currentPage - 1, sortBy, order, PAGE_SIZE, ['ckb']);
    },
    enabled: !!nodeId,
    staleTime: 0, // 关闭缓存，确保每次排序都重新请求
  });

  const nodeChannels = useMemo(() => channelsResponse?.channels || [], [channelsResponse?.channels]);
  const totalCount = channelsResponse?.total_count ?? 0;
  const totalPages = Math.ceil(totalCount / PAGE_SIZE);

  // 统计：优先使用 nodeInfo.channel_count，取不到再用分页接口的 total_count
  const totalChannels = nodeInfo?.channel_count || totalCount;

  const locationText = useMemo(() => {
    if (!nodeInfo) return "Unknown";
    const { city, country_or_region } = nodeInfo;
    return city && country_or_region ? `${city}, ${country_or_region}` : country_or_region || "Unknown";
  }, [nodeInfo]);

  const lastSeenText = useMemo(() => {
    if (!nodeInfo) return "";
    try {
      return new Date(nodeInfo.announce_timestamp).toLocaleDateString("en-US", {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
    } catch {
      return "";
    }
  }, [nodeInfo]);

  // 格式化资产流动性数值，动态选择最合适的单位
  const formatAssetLiquidity = (value: number): string => {
    const absValue = Math.abs(value);
    
    // 定义单位阶梯：K (千), M (百万), B (十亿), T (万亿)
    if (absValue >= 1_000_000_000_000) {
      // Trillion (万亿)
      return (value / 1_000_000_000_000).toLocaleString('en-US', { 
        minimumFractionDigits: 0,
        maximumFractionDigits: 2 
      }) + ' T';
    } else if (absValue >= 1_000_000_000) {
      // Billion (十亿)
      return (value / 1_000_000_000).toLocaleString('en-US', { 
        minimumFractionDigits: 0,
        maximumFractionDigits: 2 
      }) + ' B';
    } else if (absValue >= 1_000_000) {
      // Million (百万)
      return (value / 1_000_000).toLocaleString('en-US', { 
        minimumFractionDigits: 0,
        maximumFractionDigits: 2 
      }) + ' M';
    } else if (absValue >= 10_000) {
      // Thousand (千) - 只有超过 10,000 才使用 K
      return (value / 1_000).toLocaleString('en-US', { 
        minimumFractionDigits: 0,
        maximumFractionDigits: 2 
      }) + ' K';
    } else {
      // 小于 10,000，直接显示完整数字
      return value.toLocaleString('en-US', { 
        minimumFractionDigits: 0,
        maximumFractionDigits: 2 
      });
    }
  };

  // 将真实通道数据映射到表格所需结构
  const realChannelRows = useMemo(() => {
    return nodeChannels.map((ch) => {
      // 将容量从十六进制 Shannon 转换为 CKB
      const capacityInShannon = hexToDecimal(ch.capacity);
      const capacityInCKB = Number(capacityInShannon) / 100_000_000;
      
      // 格式化时间
      const formatDate = (isoString: string) => {
        if (!isoString) return "-";
        const date = new Date(isoString);
        return date.toLocaleString('en-US', {
          month: 'short',
          day: 'numeric',
          year: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
          hour12: false,
        });
      };
      
      // 获取资产名称和颜色
      const assetName = ch.name || 'ckb'; // 默认为 ckb
      const assetColor = getAssetColorUtil(assetName);
      
      // 计算资产流动性
      let assetLiquidity = '';
      let assetLiquidityUnit = 'CKB';
      
      if (assetName.toLowerCase() === 'ckb') {
        // CKB 资产：Asset liquidity 与 Capacity 相同
        assetLiquidity = formatAssetLiquidity(capacityInCKB);
        assetLiquidityUnit = 'CKB';
      } else {
        // 其他资产（如 USDI）：使用 asset 字段，金额本身就是实际金额
        if (ch.asset) {
          const udtValue = hexToDecimal(ch.asset);
          assetLiquidity = formatAssetLiquidity(Number(udtValue));
        } else {
          assetLiquidity = '0';
        }
        assetLiquidityUnit = assetName.toUpperCase();
      }
      
      return {
        channelId: ch.channel_outpoint,
        asset: assetName.toUpperCase(), // 转换为大写
        assetColor,
        assetLiquidity,
        assetLiquidityUnit,
        state: ch.state || 'open', // 通道状态，默认为 open
        createdOn: formatDate(ch.created_timestamp),
        lastCommitted: formatDate(ch.last_commit_time),
      };
    });
  }, [nodeChannels]);

  // 格式化资产数值，动态选择最合适的单位
  const formatAssetValue = (value: number): string => {
    const absValue = Math.abs(value);
    
    // 定义单位阶梯：K (千), M (百万), B (十亿), T (万亿)
    if (absValue >= 1_000_000_000_000) {
      // Trillion (万亿)
      return (value / 1_000_000_000_000).toLocaleString('en-US', { maximumFractionDigits: 2 }) + ' T';
    } else if (absValue >= 1_000_000_000) {
      // Billion (十亿)
      return (value / 1_000_000_000).toLocaleString('en-US', { maximumFractionDigits: 2 }) + ' B';
    } else if (absValue >= 1_000_000) {
      // Million (百万)
      return (value / 1_000_000).toLocaleString('en-US', { maximumFractionDigits: 2 }) + ' M';
    } else if (absValue >= 1_000) {
      // Thousand (千)
      return (value / 1_000).toLocaleString('en-US', { maximumFractionDigits: 2 }) + ' K';
    } else {
      // 小于 1000，直接显示
      return value.toLocaleString('en-US', { maximumFractionDigits: 2 });
    }
  };

  // 处理 Supporting Assets 数据
  const supportingAssets = useMemo((): AssetData[] => {
    const assets: AssetData[] = [];
    
    // 添加 CKB 数据（auto_accept_min_ckb_funding_amount 单位是 Shannon，需转换为 CKB）
    if (nodeInfo) {
      const ckbAmount = nodeInfo.auto_accept_min_ckb_funding_amount / 100_000_000;
      assets.push({
        assetName: 'CKB',
        autoAcceptValue: `${formatAssetValue(ckbAmount)} CKB`,
      });
    }
    
    // USDI 已下线，先注释保留，便于后续新增代币时恢复
    // if (udtInfos && udtInfos.length > 0) {
    //   const usdiInfo = udtInfos.find((udt) => udt.name.toLowerCase() === 'usdi');
    //   if (usdiInfo && usdiInfo.auto_accept_amount) {
    //     const amount = hexToDecimal(usdiInfo.auto_accept_amount);
    //     const numericAmount = Number(amount) / 100_000_000;
    //     assets.push({
    //       assetName: 'USDI',
    //       autoAcceptValue: `${formatAssetValue(numericAmount)} USDI`,
    //     });
    //   }
    // }
    
    return assets;
  }, [nodeInfo, udtInfos]);

  // 后端已经处理了排序和分页，前端直接展示
  const paginatedData = realChannelRows;

  // 表格列定义
  const columns: ColumnDef<ChannelData>[] = [
    {
      key: "channelId",
      label: "Channel Outpoint",
      width: "w-140 lg:flex-1 lg:min-w-94",
      render: (value) => <ChannelOutpointCell value={String(value)} />,
    },
    {
      key: "assetLiquidity",
      label: "Asset liquidity",
      width: "w-48",
      sortable: false,
      render: (value, row) => (
        <div className="text-purple font-semibold truncate">
          {value as string} {row.assetLiquidityUnit as string}
        </div>
      ),
    },
    {
      key: "state",
      label: "Status",
      width: "w-90",
      sortable: false,
      render: (value) => (
        <StatusBadge status={value as string} />
      ),
    },
    {
      key: "createdOn",
      label: "Created on",
      width: "w-60",
      sortable: true,
    },
    {
      key: "lastCommitted",
      label: "Last committed",
      width: "w-60",
      sortable: true,
    },
  ];

  // Supporting Assets 表格列定义
  const assetColumns: ColumnDef<AssetData>[] = [
    {
      key: "assetName",
      label: "Asset name",
      width: "w-1/2",
      render: (value) => {
        // 使用统一的资产颜色配置
        const color = getAssetColorUtil(String(value).toLowerCase());
        return (
          <div className="flex items-center gap-2">
            <div 
              className="w-3 h-3 flex-shrink-0 rounded-sm" 
              style={{ backgroundColor: color }}
            />
            <span className="text-primary text-sm font-medium">{value as string}</span>
          </div>
        );
      },
    },
    {
      key: "autoAcceptValue",
      label: "Auto accept value",
      width: "w-1/2",
      showInfo: true,
      infoTooltip: "The minimum amount that the node will automatically accept for opening a new channel",
    },
  ];
  return (
    <div>
      <PageHeader title="Node Details" />
      <NodeDetailCard
        name={nodeInfo?.node_name || "Unnamed"}
        status="Active"
        hash={nodeInfo?.node_id || nodeId}
        location={locationText}
        lastSeen={lastSeenText}
        isUnannounced={nodeFetched && !nodeInfo}
        isLoading={!nodeFetched || nodeLoading}
      />
      
      {/* KPI 卡片 */}
      {/* <div className="grid grid-cols-3 gap-4 mt-4 mb-5">
        <KpiCard
          label="TOTAL CHANNELS"
          value={String(totalChannels)}
        />
        <KpiCard
          label="CAPACITY"
          value={'N/A'}
          unit="CKB"
        />
        <KpiCard
          label="AUTO ACCEPT"
          value={String(autoAcceptCkb)}
          unit="CKB"
        />
      </div>
       */}
      {/* Supporting Assets Section */}
      {nodeFetched && !!nodeInfo && supportingAssets.length > 0 && (
        <>
          <div className="pt-8">
            <SectionHeader title={`Supporting Assets (${supportingAssets.length})`} />
          </div>
          
          <GlassCardContainer className="mt-4">
            <Table 
              columns={assetColumns} 
              data={supportingAssets}
              minTableWidth="100%"
            />
          </GlassCardContainer>
        </>
      )}
      
      <div className="pt-8">
        <SectionHeader title={`Channels (${totalChannels})`} />
      </div>
      
      {/* 表格和分页 */}
      <GlassCardContainer className="mt-4 relative min-h-[528px]">
        <Table 
          columns={columns} 
          data={paginatedData}
          onSort={(key, state) => {
            setSortKey(key);
            setSortState(state);
          }}
          defaultSortKey="lastCommitted"
          defaultSortState="descending"
          loading={channelsLoading}
          loadingText="Loading channels..."
          className="min-h-[528px]"
          onRowClick={(row) => router.push(`/channel/${row.channelId}`)}
        />
        
        {!channelsLoading && paginatedData.length > 0 && (
          <div className="mt-4">
            <Pagination
              currentPage={currentPage}
              totalPages={totalPages}
              onPageChange={setCurrentPage}
            />
          </div>
        )}

        {!channelsLoading && paginatedData.length === 0 && (
          <div className="absolute top-0 left-0 right-0 bottom-0 flex items-center justify-center">
            <div className="text-muted-foreground">
              No channels found
            </div>
          </div>
        )}
      </GlassCardContainer>
    </div>
  );
};
