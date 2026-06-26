/**
 * Asset color mapping utility
 * Provides consistent color mapping for assets across the application
 */

export const ASSET_COLORS: Record<string, string> = {
  'ckb': '#674BDC',      // purple
  // 'usdi': '#6ED4CA',  // teal（USDI 已下线，颜色先注释保留）
  'btc': '#EFA171',
  'other': '#64748b',
};

// 其他资产可用的颜色池
export const AVAILABLE_ASSET_COLORS = [
  '#FBE38E',
  '#59ABE6',
  '#7983D1',
  '#BDEB88',
  '#E659AB',
  '#E06B6B',
];

/**
 * Get color for a specific asset
 * @param assetName - The asset name (case-insensitive)
 * @param assetList - Optional list of all assets for consistent color assignment
 * @returns Hex color string
 */
export function getAssetColor(assetName: string, assetList?: string[]): string {
  const assetKey = assetName.toLowerCase();
  
  // Check if it's a predefined asset
  if (ASSET_COLORS[assetKey]) {
    return ASSET_COLORS[assetKey];
  }
  
  // If no asset list provided, return first available color
  if (!assetList) {
    return AVAILABLE_ASSET_COLORS[0];
  }
  
  // Find the index of the asset in the list (excluding predefined ones)
  const nonPredefinedAssets = assetList
    .filter(asset => !ASSET_COLORS[asset.toLowerCase()])
    .map(asset => asset.toLowerCase());
  
  const index = nonPredefinedAssets.indexOf(assetKey);
  
  if (index === -1) {
    return AVAILABLE_ASSET_COLORS[0];
  }
  
  return AVAILABLE_ASSET_COLORS[index % AVAILABLE_ASSET_COLORS.length];
}

/**
 * Create a color map for a list of assets
 * @param assets - Array of asset names
 * @returns Map of asset name (lowercase) to color
 */
export function createAssetColorMap(assets: string[]): Map<string, string> {
  const colorMap = new Map<string, string>();
  let colorIndex = 0;
  
  assets.forEach((asset) => {
    const assetKey = asset.toLowerCase();
    
    // If it's a predefined asset, use the specified color
    if (ASSET_COLORS[assetKey]) {
      colorMap.set(assetKey, ASSET_COLORS[assetKey]);
    } else {
      // Otherwise, assign from the color pool
      colorMap.set(assetKey, AVAILABLE_ASSET_COLORS[colorIndex % AVAILABLE_ASSET_COLORS.length]);
      colorIndex++;
    }
  });
  
  return colorMap;
}
