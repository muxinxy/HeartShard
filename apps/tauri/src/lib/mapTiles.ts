// 地图瓦片源：设置页可选，localStorage hs_map_source 持久化。
// 百度地图使用私有投影（BD-09 Mercator），Leaflet 无法直接挂瓦片，暂未内置。
export interface MapSource {
  id: string;
  label: string;
  url: string;
  attribution: string;
  subdomains?: string[];
  maxZoom: number;
}

export const MAP_SOURCES: MapSource[] = [
  {
    id: "osm",
    label: "OpenStreetMap（默认）",
    url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
    attribution: "© OpenStreetMap contributors",
    maxZoom: 19,
  },
  {
    id: "amap",
    label: "高德地图（矢量）",
    url: "https://webrd0{s}.is.autonavi.com/appmaptile?lang=zh_cn&size=1&scale=1&style=8&x={x}&y={y}&z={z}",
    subdomains: ["1", "2", "3", "4"],
    attribution: "© 高德地图",
    maxZoom: 18,
  },
  {
    id: "amap-sat",
    label: "高德卫星影像",
    url: "https://webst0{s}.is.autonavi.com/appmaptile?style=6&x={x}&y={y}&z={z}",
    subdomains: ["1", "2", "3", "4"],
    attribution: "© 高德地图",
    maxZoom: 18,
  },
  {
    id: "esri",
    label: "Esri 世界影像（卫星）",
    url: "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}",
    attribution: "© Esri, Maxar, Earthstar Geographics",
    maxZoom: 19,
  },
  {
    id: "topo",
    label: "OpenTopoMap（地形）",
    url: "https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png",
    subdomains: ["a", "b", "c"],
    attribution: "© OpenTopoMap (CC-BY-SA)",
    maxZoom: 17,
  },
];

const KEY = "hs_map_source";

export function currentMapSource(): MapSource {
  const id = localStorage.getItem(KEY) || "osm";
  return MAP_SOURCES.find((s) => s.id === id) ?? MAP_SOURCES[0];
}

export function setMapSource(id: string) {
  localStorage.setItem(KEY, id);
}

// ---------------- 逆地理编码（高德，可选；Key 在设置页填写） ----------------

const geoCache = new Map<string, string>();

/** 经纬度 → 城市/区县名；未配 Key 或失败时返回 null（调用方回退坐标显示）。 */
export async function placeLabel(lat: number, lon: number): Promise<string | null> {
  const key = localStorage.getItem("hs_amap_key");
  if (!key) return null;
  const k = `${lat.toFixed(2)},${lon.toFixed(2)}`;
  if (geoCache.has(k)) return geoCache.get(k) ?? null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const name = await invoke<string>("geocode_regeo", { key, lng: lon, lat });
    if (name) geoCache.set(k, name);
    return name || null;
  } catch {
    return null;
  }
}
