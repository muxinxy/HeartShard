// 地图探索（参考主流相册应用）：按当前视野把带 GPS 的照片做像素级聚类，
// 缩略图最多堆叠 3 张 + 数量角标；单击单张 → 灯箱，点簇 → 放大下钻；
// 已放大到最大层级仍有多张时打开该簇的照片时间线。
// 地点标签可选接入高德逆地理编码（设置页填 Web 服务 Key）。
import { useCallback, useEffect, useRef, useState } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { api, BlobEntry } from "../lib/api";
import { currentMapSource, placeLabel } from "../lib/mapTiles";
import { PhotoGrid } from "./PhotoGrid";
import { usePhotoActions } from "./PhotoActions";

const CLUSTER_PX = 72;

export function MapExplorePage({ placeKey }: { placeKey?: string }) {
  const actions = usePhotoActions();
  const mapEl = useRef<HTMLDivElement>(null);
  const mapObj = useRef<L.Map | null>(null);
  const layerRef = useRef<L.LayerGroup | null>(null);
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [title, setTitle] = useState("地图");
  const [timeline, setTimeline] = useState<BlobEntry[] | null>(null);
  const [done, setDone] = useState(false);

  // 初始集 + 缩略图预取
  useEffect(() => {
    (async () => {
      try {
        const all = await api.listBlobs();
        const gps = all.filter((b) => b.gps_lat != null && b.gps_lon != null);
        const list = placeKey
          ? gps.filter((b) => `${b.gps_lat!.toFixed(1)}, ${b.gps_lon!.toFixed(1)}` === placeKey)
          : gps;
        const urls: Record<string, string> = {};
        await Promise.all(
          list.map(async (b) => {
            urls[b.blob_hash] = await api.thumbnailUrl(b.blob_hash).catch(() => "");
          })
        );
        setThumbs(urls);
        setBlobs(list);
      } catch {
        setBlobs([]);
      } finally {
        setDone(true);
      }
    })();
  }, [placeKey]);

  // 地点标题：逆地理编码（可选）
  useEffect(() => {
    if (!done) return;
    if (blobs.length === 0) {
      setTitle(placeKey ? placeKey : "地图");
      return;
    }
    const lat = blobs.reduce((s, b) => s + (b.gps_lat ?? 0), 0) / blobs.length;
    const lon = blobs.reduce((s, b) => s + (b.gps_lon ?? 0), 0) / blobs.length;
    const base = placeKey ?? "全部地点";
    setTitle(base);
    placeLabel(lat, lon).then((name) => {
      if (name) setTitle(`${base} · ${name}`);
    });
  }, [done, blobs, placeKey]);

  const rebuild = useCallback(() => {
    const map = mapObj.current;
    const layer = layerRef.current;
    if (!map || !layer) return;
    layer.clearLayers();
    if (blobs.length === 0) return;

    // 像素级聚类（当前缩放）
    const pts = blobs.map((b) => ({
      b,
      p: map.project([b.gps_lat!, b.gps_lon!], map.getZoom()),
    }));
    const groups: { items: BlobEntry[]; lat: number; lon: number; px: { x: number; y: number } }[] = [];
    for (const pt of pts) {
      const g = groups.find(
        (grp) => Math.hypot(grp.px.x - pt.p.x, grp.px.y - pt.p.y) <= CLUSTER_PX
      );
      if (g) {
        g.items.push(pt.b);
      } else {
        groups.push({ items: [pt.b], lat: pt.b.gps_lat!, lon: pt.b.gps_lon!, px: { x: pt.p.x, y: pt.p.y } });
      }
    }

    const maxZoom = map.getMaxZoom() || currentMapSource().maxZoom;
    for (const g of groups) {
      const thumbsN = g.items.slice(0, 3);
      const stack = thumbsN
        .map(
          (b, i) =>
            `<div style="position:absolute;left:${i * 8}px;top:${i * -4}px;width:52px;height:52px;border-radius:8px;overflow:hidden;border:2px solid rgba(255,255,255,.85);box-shadow:0 2px 8px rgba(0,0,0,.45)"><img src="${thumbs[b.blob_hash] ?? ""}" style="width:100%;height:100%;object-fit:cover"></div>`
        )
        .join("");
      const badge =
        g.items.length > 1
          ? `<div style="position:absolute;right:-6px;top:-6px;background:#5b8def;color:#fff;border-radius:999px;font-size:11px;padding:1px 6px;z-index:2">${g.items.length}</div>`
          : "";
      const icon = L.divIcon({
        html: `<div style="position:relative;width:84px;height:64px">${badge}${stack}</div>`,
        className: "thumb-cluster",
        iconSize: [84, 64],
        iconAnchor: [26, 26],
      });
      const m = L.marker([g.lat, g.lon], { icon }).addTo(layer);
      m.on("click", () => {
        if (g.items.length === 1) {
          actions.openViewer(blobs, blobs.indexOf(g.items[0]));
        } else if (map.getZoom() < maxZoom) {
          map.fitBounds(
            L.latLngBounds(g.items.map((b) => L.latLng(b.gps_lat!, b.gps_lon!))).pad(0.15),
            { maxZoom }
          );
        } else {
          setTimeline([...g.items].sort((a, b) => b.taken_at - a.taken_at));
        }
      });
    }
  }, [blobs, thumbs, actions]);

  // 初始化地图
  useEffect(() => {
    if (!done || !mapEl.current || mapObj.current) return;
    const src = currentMapSource();
    const map = L.map(mapEl.current, { attributionControl: true });
    L.tileLayer(src.url, {
      attribution: src.attribution,
      maxZoom: src.maxZoom,
      ...(src.subdomains ? { subdomains: src.subdomains } : {}),
    }).addTo(map);
    layerRef.current = L.layerGroup().addTo(map);
    mapObj.current = map;

    if (blobs.length > 0) {
      map.fitBounds(L.latLngBounds(blobs.map((b) => L.latLng(b.gps_lat!, b.gps_lon!))).pad(0.2));
    } else {
      map.setView([35.0, 105.0], 4);
    }
    map.on("moveend zoomend", rebuild);
    rebuild();

    return () => {
      map.off("moveend zoomend", rebuild);
      map.remove();
      mapObj.current = null;
      layerRef.current = null;
    };
  }, [done, blobs, rebuild]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      <div className="view-head">
        <a className="back-btn" onClick={() => (window.location.hash = "#/explore")}>
          ← 返回探索
        </a>
        <h2>{title}</h2>
        <span className="badge">{blobs.length} 张</span>
        {blobs.length > 0 && (
          <button onClick={() => setTimeline([...blobs].sort((a, b) => b.taken_at - a.taken_at))}>
            照片列表
          </button>
        )}
      </div>
      <div style={{ position: "relative", flex: 1, minHeight: 420 }}>
        <div ref={mapEl} style={{ position: "absolute", inset: 0, borderRadius: 12 }} />
        {!done && (
          <p className="hint" style={{ position: "absolute", top: 8, left: 8 }}>
            加载中…
          </p>
        )}
      </div>

      {/* 簇内照片时间线 */}
      {timeline && (
        <div className="modal-backdrop" onClick={() => setTimeline(null)}>
          <div className="modal" style={{ width: 720 }} onClick={(e) => e.stopPropagation()}>
            <h3>此处照片（{timeline.length} 张）</h3>
            <div style={{ maxHeight: "60vh", overflowY: "auto" }}>
              <PhotoGrid blobs={timeline} onOpen={(_b, idx) => actions.openViewer(timeline, idx)} />
            </div>
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 12 }}>
              <button className="primary" onClick={() => setTimeline(null)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
