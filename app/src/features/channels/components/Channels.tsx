"use client";

import { useState, useEffect, useMemo, useCallback } from "react";
import { useRouter } from "next/navigation";
import {
  Table,
  Pagination,
  ColumnDef,
  SortState,
  GlassCardContainer,
  StatusSelect,
  StatusBadge,
  CopyButton,
} from "@/shared/components/ui";
import BarChart from "@/shared/components/chart/BarChart";
import PieChart from "@/shared/components/chart/PieChart";
import { useChannelsByState } from "@/features/channels/hooks/useChannels";
import { ChannelState, BasicChannelInfo } from "@/lib/types";
import { hexToDecimal } from "@/lib/utils";
import { useNetwork } from "@/features/networks/context/NetworkContext";
import { useQuery } from "@tanstack/react-query";
import { getAssetColor as getAssetColorUtil } from "../utils/assetColors";

// 通道数据类型
interface ChannelData extends Record<string, unknown> {
  channelId: string;
  asset: string; // 资产名称（大写）
  assetColor: string; // 资产对应的颜色
  transactions: number;
  capacity: string;
  assetLiquidity: string; // 资产流动性
  assetLiquidityUnit: string; // 资产流动性单位
  createdOn: string;
  lastCommitted: string;
  state: string; // 通道状态
}

// 容量区间定义（CKB）
// 根据实际数据范围调整为合理的对数刻度
const CAPACITY_RANGES = [
  { min: 0, max: 100, label: "10^0k" },           // 0-100
  { min: 100, max: 1_000, label: "10^1k" },       // 100-1K
  { min: 1_000, max: 10_000, label: "10^2k" },    // 1K-10K
  { min: 10_000, max: 100_000, label: "10^3k" },  // 10K-100K
  { min: 100_000, max: 1_000_000, label: "10^4k" }, // 100K-1M
  { min: 1_000_000, max: 10_000_000, label: "10^5k" }, // 1M-10M
  { min: 10_000_000, max: 100_000_000, label: "10^6k" }, // 10M-100M
  { min: 100_000_000, max: 1_000_000_000, label: "10^7k" }, // 100M-1B
];

// 格式化容量范围显示
const formatCapacityRange = (min: number, max: number) => {
  const formatNumber = (num: number) => {
    if (num >= 1_000_000_000) return `${num / 1_000_000_000}B`;
    if (num >= 1_000_000) return `${num / 1_000_000}M`;
    if (num >= 1000) return `${num / 1000}K`;
    return num.toString();
  };
  return `${formatNumber(min)}-${formatNumber(max)} CKB`;
};


// 所有可用的通道状态
const ALL_CHANNEL_STATES: ChannelState[] = [
  "open",
  "closed_waiting_onchain_settlement",
  "closed_uncooperative",
  "closed_cooperative",
];

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

export const Channels = () => {
  const router = useRouter();
  
  // 资产固定为 CKB（USDI 已下线；历史多资产切换逻辑已移除，后续新增代币时可恢复）
  const overviewAsset = 'ckb';
  
  const [currentPage, setCurrentPage] = useState(1); // 1-based for display
  const [selectedStates, setSelectedStates] = useState<ChannelState[]>([]); // 默认为空，表示 all statuses
  const [selectedStatus, setSelectedStatus] = useState<string>(''); // 状态下拉选择框，''表示 all statuses
  const [sortKey, setSortKey] = useState<string>("");
  const [sortState, setSortState] = useState<SortState>("none");
  const PAGE_SIZE = 10; // 每页显示10条
  const { apiClient, currentNetwork } = useNetwork();
  
  // 计算后端页码（从1开始转换为从0开始）
  const backendPage = currentPage - 1;

  // 将前端的 sortKey 映射到后端的 sort_by 字段
  const getBackendSortBy = (frontendKey: string): string => {
    // assetLiquidity 固定按 capacity 排序（仅支持 CKB）
    const liquiditySortBy = 'capacity';
    const mapping: Record<string, string> = {
      'createdOn': 'create_time',
      'lastCommitted': 'last_commit_time',
      'capacity': 'capacity',
      'assetLiquidity': liquiditySortBy,
    };
    return mapping[frontendKey] || 'last_commit_time';
  };

  // 将前端的 sortState 映射到后端的 order
  const getBackendOrder = (state: SortState): 'asc' | 'desc' => {
    return state === 'ascending' ? 'asc' : 'desc';
  };

  // 计算实际的排序参数
  const backendSortBy = sortKey ? getBackendSortBy(sortKey) : 'last_commit_time';
  const backendOrder = sortState !== 'none' ? getBackendOrder(sortState) : 'desc';

  // 使用新的后端聚合接口获取容量分布数据
  // 返回格式：{ "asset": {...}, "capacity": {"ckb": {"Capacity 10^0k": 10, ...}, "usdi": {...}} }
  const { data: capacityDistribution } = useQuery({
    queryKey: ["channel-capacity-distribution", currentNetwork],
    queryFn: () => apiClient.getChannelCapacityDistribution(),
    refetchInterval: 300000, // 5分钟刷新
  });

  // 使用新的后端聚合接口获取各状态通道数量
  // 返回格式：{"ckb": {"open": 100, "closed_cooperative": 50, ...}, "usdi": {...}}
  const { data: channelCountByState } = useQuery({
    queryKey: ["channel-count-by-state", currentNetwork],
    queryFn: () => apiClient.getChannelCountByState(),
    refetchInterval: 300000, // 5分钟刷新
  });

  // 使用服务端分页接口获取指定状态的通道数据
  // 如果没有选中任何状态，则请求所有状态
  const statesToFetch = selectedStates.length === 0 ? ALL_CHANNEL_STATES : selectedStates;
  const { data: channelsData, isLoading } = useChannelsByState(
    statesToFetch,
    backendPage,
    backendSortBy,
    backendOrder,
    '', // 不使用搜索功能
    overviewAsset // 使用 Channel Overview 的资产筛选
  );
  
  // 从返回数据中提取 total_count
  const totalCount = channelsData?.total_count ?? 0;
  const totalPages = Math.ceil(totalCount / PAGE_SIZE);

  // 计算 CKB 各状态通道数量（用于 Channel Overview 图表和状态选择器）
  const getStateCount = useCallback((state: ChannelState) => {
    if (!channelCountByState) return 0;
    
    // 服务端返回格式：{"ckb": {"open": 100, ...}}
    type StateData = Record<string, Record<string, number>>;
    const stateData = channelCountByState as unknown as StateData;
    const ckbData = stateData["ckb"] || {};
    return ckbData[state] || 0;
  }, [channelCountByState]);

  // 组装 PieChart 数据 - Channel Status Distribution
  const pieChartData = useMemo(() => {
    return [
      { name: "Open", value: getStateCount("open"), status: "Open" },
      { name: "Closing", value: getStateCount("closed_waiting_onchain_settlement"), status: "Closing" },
      { name: "Cooperative closed", value: getStateCount("closed_cooperative"), status: "Cooperative closed" },
      { name: "Uncooperative closed", value: getStateCount("closed_uncooperative"), status: "Uncooperative closed" },
    ];
  }, [getStateCount]);

  // 计算容量分布数据 - 使用后端返回的分桶结果
  // 根据 overviewAsset 过滤和聚合数据
  const capacityDistributionData = useMemo(() => {
    if (!capacityDistribution) {
      return CAPACITY_RANGES.map(range => ({ 
        label: range.label, 
        value: 0, 
        min: range.min, 
        max: range.max 
      }));
    }

    // 服务端返回格式: { "capacity": {"ckb": {"Capacity 10^0k": 10, ...}} }
    type DistributionData = { 
      capacity: Record<string, Record<string, number>>;
    };
    const distributionData = capacityDistribution as unknown as DistributionData;
    const capacityData = distributionData.capacity || {};
    const ckbCapacity = capacityData["ckb"] || {};
    
    return CAPACITY_RANGES.map(range => {
      const key = `Capacity ${range.label}`;
      return {
        label: range.label,
        value: ckbCapacity[key] || 0,
        min: range.min,
        max: range.max,
      };
    });
  }, [capacityDistribution]);

  // 计算容量分布的总数，用于百分比计算
  const totalChannelsForCapacity = useMemo(() => {
    return capacityDistributionData.reduce((sum, item) => sum + item.value, 0);
  }, [capacityDistributionData]);

  // Convert API data to table format - 直接使用当前页的数据
  const tableData: ChannelData[] = channelsData?.list?.map((channel: BasicChannelInfo) => {
    // 将容量从十六进制 Shannon 转换为 CKB
    const capacityInShannon = hexToDecimal(channel.capacity);
    const capacityInCKB = Number(capacityInShannon) / 100_000_000;
    
    // 格式化时间
    const formatDate = (isoString: string) => {
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
    const assetName = channel.name || 'ckb'; // 默认为 ckb
    const assetColor = getAssetColorUtil(assetName);
    
    // 计算资产流动性
    let assetLiquidity = '';
    let assetLiquidityUnit = 'CKB';
    
    if (assetName.toLowerCase() === 'ckb') {
      // CKB 资产：Asset liquidity 与 Capacity 相同
      assetLiquidity = capacityInCKB.toLocaleString('en-US', { maximumFractionDigits: 2 });
      assetLiquidityUnit = 'CKB';
    } else {
      // 其他资产：使用 udt_value
      if (channel.udt_value) {
        const udtValue = hexToDecimal(channel.udt_value);
        assetLiquidity = Number(udtValue).toLocaleString('en-US', { maximumFractionDigits: 2 });
      } else {
        assetLiquidity = '0';
      }
      assetLiquidityUnit = assetName.toUpperCase();
    }
    
    return {
      channelId: channel.channel_outpoint,
      asset: assetName.toUpperCase(), // 转换为大写
      assetColor,
      transactions: channel.tx_count,
      capacity: capacityInCKB.toLocaleString('en-US', { maximumFractionDigits: 2 }),
      assetLiquidity,
      assetLiquidityUnit,
      createdOn: formatDate(channel.create_time),
      lastCommitted: formatDate(channel.last_commit_time),
      state: channel.state || 'open', // 通道状态，默认为 open
    };
  }) || [];

  // Reset to first page when state changes
  useEffect(() => {
    setCurrentPage(1);
  }, [selectedStates]);

  // Reset to first page when sorting changes
  useEffect(() => {
    setCurrentPage(1);
  }, [sortKey, sortState]);

  // 列定义
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
      sortable: true,
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
        <div className="flex items-center">
          <StatusBadge status={value as string} />
        </div>
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

  const handleSort = (key: string, state: SortState) => {
    setSortKey(key);
    setSortState(state);
  };

  const handlePageChange = (page: number) => {
    setCurrentPage(page);
  };

  // 处理状态选择变化
  const handleStatusChange = (statusValue: string) => {
    setSelectedStatus(statusValue);
    if (statusValue === '') {
      // All statuses: 获取所有状态
      setSelectedStates([]);
    } else {
      // 单个状态
      setSelectedStates([statusValue as ChannelState]);
    }
  };
  
  // 状态颜色映射
  const STATUS_COLORS: Record<string, string> = {
    'open': '#156F5C',
    'closed_waiting_onchain_settlement': '#B45309',
    'closed_cooperative': '#2563EB',
    'closed_uncooperative': '#B34846',
  };
  
  // 生成状态选择器选项（根据 overviewAsset 过滤，与图表保持一致）
  const statusOptions = useMemo(() => {
    if (!channelCountByState) return [];
    
    const options: Array<{ value: string; label: string; color?: string }> = [
      {
        value: 'open',
        label: `Open (${getStateCount("open")})`,
        color: STATUS_COLORS['open'],
      },
      {
        value: 'closed_waiting_onchain_settlement',
        label: `Closing (${getStateCount("closed_waiting_onchain_settlement")})`,
        color: STATUS_COLORS['closed_waiting_onchain_settlement'],
      },
      {
        value: 'closed_cooperative',
        label: `Cooperative closed (${getStateCount("closed_cooperative")})`,
        color: STATUS_COLORS['closed_cooperative'],
      },
      {
        value: 'closed_uncooperative',
        label: `Uncooperative closed (${getStateCount("closed_uncooperative")})`,
        color: STATUS_COLORS['closed_uncooperative'],
      },
    ];
    
    // 计算所有状态的总数
    const allCount = options.reduce((sum, opt) => {
      const match = opt.label.match(/\((\d+)\)/);
      return sum + (match ? parseInt(match[1]) : 0);
    }, 0);
    
    // 添加 "All statuses" 选项在最后（不需要 color）
    options.push({
      value: '',
      label: `All statuses (${allCount})`,
    });
    
    return options;
  }, [channelCountByState, getStateCount]);

  return (
    <div className="flex flex-col gap-5">
      {/* Channel Overview 标题 */}
      <div className="flex items-center">
        <h2 className="type-h2 font-semibold text-primary">
          Channel Overview
        </h2>
      </div>

      {/* 第一行：Channel Status Distribution 和 CKB Channel Liquidity Distribution */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <GlassCardContainer>
          <PieChart
            data={pieChartData}
            title="Channel Status Distribution"
            height="400px"
            colors={["#BDEB88", "#FBE38E", "#59ABE6", "#E06B6B"]}
            tooltipFormatter={(params) => {
              const dataItem = pieChartData[params.dataIndex];
              const totalChannels = pieChartData.reduce((sum, item) => sum + item.value, 0);
              const percentage = totalChannels > 0
                ? ((params.value / totalChannels) * 100).toFixed(1)
                : "0.0";
              
              return [
                { label: "Status", value: dataItem.status || params.name, showColorDot: true },
                { label: "# of Channels", value: params.value.toString() },
                { label: "% of Total", value: `${percentage}%` },
              ];
            }}
          />
        </GlassCardContainer>

        <GlassCardContainer>
          <BarChart
            data={capacityDistributionData}
            title="CKB Channel Liquidity Distribution"
            height="400px"
            tooltipFormatter={item => {
              const dataItem = capacityDistributionData.find(d => d.label === item.label);
              const percentage = totalChannelsForCapacity > 0 
                ? ((item.value / totalChannelsForCapacity) * 100).toFixed(1)
                : "0.0";
              const range = dataItem 
                ? formatCapacityRange(dataItem.min, dataItem.max)
                : item.label;
              
              return [
                { label: "Liquidity Range", value: range },
                { label: "Total Channels", value: item.value.toString() },
                { label: "% of Total", value: `${percentage}%` },
              ];
            }}
          />
        </GlassCardContainer>
      </div>

      <div className="flex items-center gap-4 flex-wrap">
        {/* Status Select - 使用下拉选择框 */}
        <StatusSelect
          options={statusOptions}
          value={selectedStatus}
          onChange={handleStatusChange}
          placeholder="All statuses"
          className="w-[260px]"
        />
      </div>

      <GlassCardContainer className="relative min-h-[528px]">
        <Table<ChannelData>
          columns={columns}
          data={tableData}
          onSort={handleSort}
          defaultSortKey={sortKey}
          defaultSortState={sortState}
          loading={isLoading}
          loadingText="Loading channels..."
          className="min-h-[528px]"
          onRowClick={(row) => router.push(`/channel/${row.channelId}`)}
        />

        {!isLoading && tableData.length > 0 && (
          <Pagination
            currentPage={currentPage}
            totalPages={totalPages}
            onPageChange={handlePageChange}
            className="mt-4"
          />
        )}

        {!isLoading && tableData.length === 0 && (
          <div className="absolute top-0 left-0 right-0 bottom-0 flex items-center justify-center">
            <div className="flex flex-col items-center gap-3 text-center">
              <div className="text-primary text-button1">
                No matching channels
              </div>
              <div className="text-tertiary text-button1">
                Try clearing filters or changing your search term to see more channels.
              </div>
            </div>
          </div>
        )}
      </GlassCardContainer>
    </div>
  );
};
