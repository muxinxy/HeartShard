// 探索（网页端 viewExplore 同款）：类别（最近添加/照片/视频/动态照片/未入相册）
// + 地点（GPS 0.1° 网格聚类，卡片内嵌小地图）；本地模式附地图入口。
import { useEffect, useMemo, useState } from "react";
import { MapContainer, CircleMarker, TileLayer } from "react-leaflet";
import "leaflet/dist/leaflet.css";
import { api, BlobEntry, platform } from "../../lib/api";
import { currentMapSource, placeLabel } from "../../lib/mapTiles";
import { PhotoGrid } from "../PhotoGrid";
import { usePhotoActions } from "../PhotoActions";
import { Ic } from "../icons";

interface Facet {
  key: string;
  label: string;
  icon: string;
  blobs: BlobEntry[];
  cover?: string;
}

export function ExploreView() {
  const actions = usePhotoActions();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [orphans, setOrphans] = useState<BlobEntry[] | null>(null);
  const [open, setOpen] = useState<{ title: string; blobs: BlobEntry[] } | null>(null);

  useEffect(() => {
    api
      .listBlobs()
      .then((all) => setBlobs([...all].sort((a, b) => b.taken_at - a.taken_at)))
      .catch(() => {});
    if (platform.isRemote) {
      api
        .orphans()
        .then((o) => setOrphans([...o].sort((a, b) => b.taken_at - a.taken_at)))
        .catch(() => setOrphans([]));
    }
  }, [actions.dataVersion]);

  const kinds = useMemo<Facet[]>(() => {
    const out: Facet[] = [
      { key: "recent", label: "最近添加", icon: "i-refresh", blobs: blobs.slice(0, 12) },
      { key: "photo", label: "照片", icon: "i-image", blobs: blobs.filter((b) => b.mime.startsWith("image/")) },
      { key: "video", label: "视频", icon: "i-video", blobs: blobs.filter((b) => b.mime.startsWith("video/")) },
      { key: "motion", label: "动态照片", icon: "i-motion", blobs: blobs.filter((b) => b.is_motion) },
    ];
    if (platform.isRemote && orphans) out.push({ key: "orphans", label: "未入相册", icon: "i-box", blobs: orphans });
    return out;
  }, [blobs, orphans]);

  const places = useMemo(() => {
    const map = new Map<string, BlobEntry[]>();
    for (const b of blobs) {
      if (b.gps_lat == null || b.gps_lon == null) continue;
      const key = `${b.gps_lat.toFixed(1)}, ${b.gps_lon.toFixed(1)}`;
      const arr = map.get(key) || [];
      arr.push(b);
      map.set(key, arr);
    }
    return [...map.entries()].sort((a, b) => b[1].length - a[1].length);
  }, [blobs]);

  // 地点标签：配了高德 Key 时显示城市/区县名（否则显示坐标）
  const [labels, setLabels] = useState<Record<string, string>>({});
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const out: Record<string, string> = {};
      for (const [key, items] of places.slice(0, 20)) {
        const lat = items.reduce((s, b) => s + (b.gps_lat ?? 0), 0) / items.length;
        const lon = items.reduce((s, b) => s + (b.gps_lon ?? 0), 0) / items.length;
        const name = await placeLabel(lat, lon);
        if (name) out[key] = name;
      }
      if (!cancelled && Object.keys(out).length > 0) setLabels(out);
    })();
    return () => {
      cancelled = true;
    };
  }, [places]);

  const goMap = (key: string) => {
    sessionStorage.setItem("map_place", key);
    window.location.hash = "#/map";
  };
  const label = (key: string) => (labels[key] ? `📍 ${labels[key]}` : `📍 ${key}`);

  if (open) {
    return (
      <div>
        <div className="view-head">
          <a className="back-btn" onClick={() => setOpen(null)}>
            ← 返回探索
          </a>
          <h2>{open.title}</h2>
          <span className="badge">{open.blobs.length} 张</span>
        </div>
        <PhotoGrid
          blobs={open.blobs}
          onOpen={(_b, idx) => actions.openViewer(open.blobs, idx)}
        />
      </div>
    );
  }

  return (
    <div>
      <div className="view-head">
        <h2>探索</h2>
      </div>

      <h3 style={{ fontSize: 15, color: "var(--muted)" }}>类别</h3>
      <div className="explore-grid">
        {kinds.map((f) => (
          <ExploreCard key={f.key} facet={f} onOpen={() => setOpen({ title: f.label, blobs: f.blobs })} />
        ))}
      </div>

      <h3 style={{ fontSize: 15, color: "var(--muted)", marginTop: 24 }}>地点</h3>
      {places.length === 0 ? (
        <p className="hint">带 GPS 信息的照片会按拍摄地点自动聚类（EXIF）。可在「设置」里切换地图源（高德 / 卫星影像等）。</p>
      ) : (
        <div className="explore-grid">
          {places.map(([key, items]) => {
            const lat = items.reduce((s, b) => s + (b.gps_lat ?? 0), 0) / items.length;
            const lon = items.reduce((s, b) => s + (b.gps_lon ?? 0), 0) / items.length;
            const src = currentMapSource();
            return (
              <div key={key} className="explore-card" onClick={() => goMap(key)}>
                <div style={{ height: 120, background: "var(--input)" }}>
                  <MapContainer
                    center={[lat, lon]}
                    zoom={12}
                    scrollWheelZoom={false}
                    dragging={false}
                    zoomControl={false}
                    attributionControl={false}
                    style={{ height: "100%" }}
                  >
                    <TileLayer
                      url={src.url}
                      attribution={src.attribution}
                      maxZoom={src.maxZoom}
                      {...(src.subdomains ? { subdomains: src.subdomains } : {})}
                    />
                    <CircleMarker center={[lat, lon]} radius={9} pathOptions={{ color: "#5b8def", fillOpacity: 0.5 }} />
                  </MapContainer>
                </div>
                <div className="nm">
                  <span>{label(key)}</span>
                  <span className="cnt">{items.length}</span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ExploreCard({ facet, onOpen }: { facet: Facet; onOpen: () => void }) {
  return (
    <div className="explore-card" onClick={onOpen}>
      <div className="cov">
        <Ic name={facet.icon} />
      </div>
      <div className="nm">
        <span>{facet.label}</span>
        <span className="cnt">{facet.blobs.length}</span>
      </div>
    </div>
  );
}
