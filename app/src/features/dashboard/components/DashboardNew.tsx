import TimeSeriesChart from "@/shared/components/chart/TimeSeriesChart";
import {
  KpiCard,
  SectionHeader,
  GlassCardContainer,
  // RadioGroup, // 多资产切换已下线，先注释保留，便于后续新增代币时恢复
} from "@/shared/components/ui";
import { NodeTreeMap } from "@/shared/components/chart/NodeTreeMap";
// import { SUPPORTED_ASSETS } from "@/lib/config/assets"; // 多资产切换已下线，先注释保留
import { useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNetwork } from "@/features/networks/context/NetworkContext";
import { queryKeys } from "@/features/dashboard/hooks/useDashboard";
import { useRouter } from "next/navigation";

// 固定使用 hourly 时间范围
const TIME_RANGE = "hourly" as const;

// Mock data for TimeSeriesChart
const MOCK_TIME_SERIES_DATA = [
  {
    label: "Total liquidity",
    data: [
      { timestamp: "2024-10-17", value: 55000000 },
      { timestamp: "2024-10-18", value: 68000000 },
      { timestamp: "2024-10-19", value: 67000000 },
      { timestamp: "2024-10-20", value: 80000000 },
      { timestamp: "2024-10-21", value: 90000000 },
      { timestamp: "2024-10-22", value: 85000000 },
      { timestamp: "2024-10-23", value: 83000000 },
    ],
  },
  {
    label: "Total channels",
    data: [
      { timestamp: "2024-10-17", value: 2500 },
      { timestamp: "2024-10-18", value: 2400 },
      { timestamp: "2024-10-19", value: 2200 },
      { timestamp: "2024-10-20", value: 2100 },
      { timestamp: "2024-10-21", value: 1800 },
      { timestamp: "2024-10-22", value: 2000 },
      { timestamp: "2024-10-23", value: 2200 },
    ],
  },
];

const MOCK_TIME_SERIES_DATA2 = [
  {
    label: "Total active nodes",
    data: [
      { timestamp: "2024-10-17", value: 400 },
      { timestamp: "2024-10-18", value: 410 },
      { timestamp: "2024-10-19", value: 420 },
      { timestamp: "2024-10-20", value: 415 },
      { timestamp: "2024-10-21", value: 430 },
      { timestamp: "2024-10-22", value: 440 },
      { timestamp: "2024-10-23", value: 450 },
    ],
  }
];

export const DashboardNew = () => {
  const timeRange = TIME_RANGE; // 固定使用 hourly
  const router = useRouter();
  const queryClient = useQueryClient();
  const { apiClient, currentNetwork } = useNetwork();

  // 资产固定为 CKB（USDI 已下线）
  // 后续新增代币时可恢复以下多资产切换逻辑：
  // const searchParams = useSearchParams();
  // const urlAsset = searchParams.get('asset') || 'ckb';
  // const [selectedAsset, setSelectedAsset] = useState<string>(urlAsset);
  // useEffect(() => { setSelectedAsset(urlAsset); }, [urlAsset]);
  // useEffect(() => { ...URL 同步... }, [selectedAsset]);
  // const assetLabel = SUPPORTED_ASSETS.find(a => a.value === selectedAsset)?.label.toUpperCase() || "CKB";
  const selectedAsset = 'ckb';
  const assetLabel = "CKB";

  // Fiber Network Snapshot 数据
  const { data: snapshotDataCurrent } = useQuery({
    queryKey: [...queryKeys.snapshot, currentNetwork, "current"],
    queryFn: () => apiClient.getActiveAnalysisHourly(),
    refetchInterval: 30000,
  });

  const { data: snapshotDataLastWeek } = useQuery({
    queryKey: [...queryKeys.snapshot, currentNetwork, "lastWeek"],
    queryFn: () => {
      const oneWeekAgo = new Date(Date.now() - 7 * 24 * 60 * 60 * 1000);
      return apiClient.getActiveAnalysisHourly(oneWeekAgo.toISOString());
    },
    refetchInterval: 30000,
  });

  // 计算 Total Channels (CKB)
  const totalChannelsData = useMemo(() => {
    if (!snapshotDataCurrent?.asset_analysis) return { current: 0, change: 0 };

    const currentCkb = snapshotDataCurrent.asset_analysis.find(
      a => a.name.toLowerCase() === 'ckb'
    );
    const current = Number(currentCkb?.channel_len || 0);

    if (!snapshotDataLastWeek?.asset_analysis) return { current, change: 0 };

    const lastWeekCkb = snapshotDataLastWeek.asset_analysis.find(
      a => a.name.toLowerCase() === 'ckb'
    );
    const lastWeek = Number(lastWeekCkb?.channel_len || 0);

    const change = lastWeek > 0 ? ((current - lastWeek) / lastWeek) * 100 : 0;

    return { current, change };
  }, [snapshotDataCurrent, snapshotDataLastWeek]);

  // 计算 Total Active Nodes
  const totalNodesData = useMemo(() => {
    if (!snapshotDataCurrent) return { current: 0, change: 0 };
    
    const current = Number(snapshotDataCurrent.total_nodes || 0);

    if (!snapshotDataLastWeek) return { current, change: 0 };

    const lastWeek = Number(snapshotDataLastWeek.total_nodes || 0);
    const change = lastWeek > 0 ? ((current - lastWeek) / lastWeek) * 100 : 0;

    return { current, change };
  }, [snapshotDataCurrent, snapshotDataLastWeek]);

  // Channels 区域数据 - 固定使用 liquidity
  const { data: kpi } = useQuery({
    queryKey: [...queryKeys.kpis, currentNetwork, timeRange, selectedAsset, "liquidity"],
    queryFn: () => apiClient.fetchKpiDataByTimeRange(timeRange, selectedAsset, "liquidity"),
    refetchInterval: 30000,
  });

  const { data: timeSeriesData } = useQuery({
    queryKey: [...queryKeys.timeSeries, currentNetwork, timeRange, selectedAsset, "liquidity"],
    queryFn: () => apiClient.fetchTimeSeriesDataByTimeRange(timeRange, selectedAsset, "liquidity"),
    refetchInterval: 30000,
  });

  // 获取节点数据用于 TreeMap
  const { data: nodesData, isLoading: nodesLoading } = useQuery({
    queryKey: [...queryKeys.nodes, currentNetwork, "all"],
    queryFn: () => apiClient.getActiveNodesByPage(0, "channel_count", "desc", 500),
    refetchInterval: 30000,
  });

  // 生成最后更新时间
  const lastUpdated = useMemo(() => {
    const now = new Date();
    const month = now.toLocaleString('en-US', { month: 'short' });
    const day = now.getDate();
    const time = now.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
    return `Last updated: ${month} ${day}, ${time}`;
  }, []);

  // 刷新所有接口数据
  const handleRefresh = () => {
    // 刷新 Fiber Network Snapshot 数据
    queryClient.invalidateQueries({ queryKey: [...queryKeys.snapshot, currentNetwork] });
    
    // 刷新 Channels 区域数据
    queryClient.invalidateQueries({ queryKey: [...queryKeys.kpis, currentNetwork] });
    queryClient.invalidateQueries({ queryKey: [...queryKeys.timeSeries, currentNetwork] });
    
    // 刷新 Nodes 数据
    queryClient.invalidateQueries({ queryKey: [...queryKeys.nodes, currentNetwork] });
  };

  return (
    <div className="flex flex-col gap-5">
      {/* 第一部分: Fiber Network Snapshot */}
      <div className="flex flex-col gap-4">
        <SectionHeader
          title="Fiber Network Snapshot"
          lastUpdated={lastUpdated}
          onRefresh={handleRefresh}
        />
        
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <KpiCard
            label="TOTAL CHANNELS"
            value={String(totalChannelsData.current)}
            changePercent={Number(totalChannelsData.change.toFixed(1))}
            trending={totalChannelsData.change >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
          <KpiCard
            label="TOTAL ACTIVE NODES"
            value={String(totalNodesData.current)}
            changePercent={Number(totalNodesData.change.toFixed(1))}
            trending={totalNodesData.change >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
        </div>
      </div>

      {/* 第二部分: Channels */}
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between gap-4">
          <SectionHeader title="Channels" />
          <button
            onClick={() => router.push('/channels')}
            className="type-button1 text-purple cursor-pointer"
          >
            View details
          </button>
        </div>
        {/* 多资产切换已下线，先注释保留，便于后续新增代币时恢复
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div className="flex items-center justify-between md:justify-start gap-4">
            <SectionHeader title="Channels" />
            <button
              onClick={() => router.push(`/channels?asset=${selectedAsset}`)}
              className="type-button1 text-purple cursor-pointer md:hidden"
            >
              View details
            </button>
            <div className="hidden md:block">
              <RadioGroup
                options={[
                  { value: "ckb", label: "CKB" },
                  { value: "usdi", label: "USDI" },
                ]}
                value={selectedAsset}
                onChange={setSelectedAsset}
              />
            </div>
          </div>
          <div className="md:hidden w-full">
            <RadioGroup
              options={[
                { value: "ckb", label: "CKB" },
                { value: "usdi", label: "USDI" },
              ]}
              value={selectedAsset}
              onChange={setSelectedAsset}
              className="h-[45px] w-full flex"
            />
          </div>
          <button
            onClick={() => router.push(`/channels?asset=${selectedAsset}`)}
            className="hidden md:block type-button1 text-purple cursor-pointer"
          >
            View details
          </button>
        </div>
        */}
        
        {/* 第一行: Liquidity 和 Channels 卡片 */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <KpiCard
            label={`${assetLabel} LIQUIDITY`}
            value={String(kpi?.totalCapacity ?? 0)}
            unit={kpi?.capacityUnit || "CKB"}
            changePercent={kpi?.totalCapacityChange ?? 0}
            trending={(kpi?.totalCapacityChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
            tooltip={`The total amount of ${assetLabel} currently available across ${assetLabel} channels.`}
          />
          <KpiCard
            label={`${assetLabel} CHANNELS`}
            value={String(kpi?.totalChannels ?? 0)}
            changePercent={kpi?.totalChannelsChange ?? 0}
            trending={(kpi?.totalChannelsChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
            // onViewDetails={() => router.push(`/channels?asset=${selectedAsset}`)}
          />
        </div>

        {/* 第二行: TimeSeriesChart 图表 */}
        <GlassCardContainer>
          <TimeSeriesChart
            data={timeSeriesData ? [timeSeriesData.capacity, timeSeriesData.channels] : MOCK_TIME_SERIES_DATA}
            height="321px"
            className="w-full"
            colors={["#674BDC", "#fab83d"]}
            timeRange={timeRange}
          />
        </GlassCardContainer>
        
        {/* 第三行: 4 个 Liquidity KPI 卡片 */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          <KpiCard
            label={`MIN ${assetLabel} LIQUIDITY`}
            value={String(kpi?.minChannelCapacity ?? 0)}
            unit={kpi?.capacityUnit || "CKB"}
            changePercent={kpi?.minChannelCapacityChange ?? 0}
            trending={(kpi?.minChannelCapacityChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
          <KpiCard
            label={`MAX ${assetLabel} LIQUIDITY`}
            value={String(kpi?.maxChannelCapacity ?? 0)}
            unit={kpi?.capacityUnit || "CKB"}
            changePercent={kpi?.maxChannelCapacityChange ?? 0}
            trending={(kpi?.maxChannelCapacityChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
          <KpiCard
            label={`AVG ${assetLabel} LIQUIDITY`}
            value={String(kpi?.averageChannelCapacity ?? 0)}
            unit={kpi?.capacityUnit || "CKB"}
            changePercent={kpi?.averageChannelCapacityChange ?? 0}
            trending={(kpi?.averageChannelCapacityChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
          <KpiCard
            label={`MEDIAN ${assetLabel} LIQUIDITY`}
            value={String(kpi?.medianChannelCapacity ?? 0)}
            unit={kpi?.capacityUnit || "CKB"}
            changePercent={kpi?.medianChannelCapacityChange ?? 0}
            trending={(kpi?.medianChannelCapacityChange ?? 0) >= 0 ? "up" : "down"}
            changeLabel="from last week"
          />
        </div>
      </div>

      {/* 第三部分: Nodes */}
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between gap-4">
          <SectionHeader
            title="Nodes"
          />
          <button 
            onClick={() => router.push('/nodes')}
            className="type-button1 text-purple cursor-pointer"
          >
            View details
          </button>
        </div>
        
        <div className="flex flex-col lg:flex-row gap-4">
          {/* 左侧: Total Active Nodes 时序图 */}
          <div className="flex-1">
            <GlassCardContainer className="h-[480px]">
              <TimeSeriesChart
                data={timeSeriesData ? [timeSeriesData.nodes] : MOCK_TIME_SERIES_DATA2}
                height="100%"
                className="w-full"
                colors={["#59ABE6"]}
                timeRange={timeRange}
              />
            </GlassCardContainer>
          </div>

          {/* 右侧: TreeMap */}
          <div className="flex-1">
            <GlassCardContainer className="h-[480px] lg:h-full flex flex-col">
              <div className="type-label text-secondary uppercase mb-4">
                Channel share by ACTIVE nodes
              </div>
              <div className="flex-1">
                <NodeTreeMap
                  data={nodesData?.nodes || []}
                  height="100%"
                  loading={nodesLoading}
                  onNodeClick={(nodeId) => router.push(`/node/${nodeId}`)}
                />
              </div>
            </GlassCardContainer>
          </div>
        </div>
      </div>
    </div>
  );
};
