/**
 * 支持的资产配置
 * 用于通道分析和数据展示
 */

export interface AssetConfig {
  value: string; // 资产值（小写）
  label: string; // 显示标签（大写）
  color: string; // 资产颜色
  unit: string;  // 显示单位
}

/**
 * 支持的资产列表
 * 注意：匹配时会将 API 返回的 name 转为小写进行比较
 */
export const SUPPORTED_ASSETS: AssetConfig[] = [
  {
    value: "ckb",
    label: "CKB",
    color: "#674BDC",
    unit: "CKB",
  },
  // USDI 已下线，配置先注释保留，便于后续新增代币时恢复
  // {
  //   value: "usdi",
  //   label: "USDI",
  //   color: "#6ED4CA",
  //   unit: "USDI",
  // },
];

/**
 * 根据资产值获取配置
 * @param assetValue - 资产值（不区分大小写）
 */
export function getAssetConfig(assetValue: string): AssetConfig | undefined {
  const normalizedValue = assetValue.toLowerCase();
  return SUPPORTED_ASSETS.find(asset => asset.value === normalizedValue);
}

/**
 * 检查资产是否被支持
 * @param assetValue - 资产值（不区分大小写）
 */
export function isSupportedAsset(assetValue: string): boolean {
  return getAssetConfig(assetValue) !== undefined;
}
