// §10 地图视图：GPS-tagged blob 显示在可选瓦片源上（设置页可切换 OSM/高德/Esri/OpenTopoMap）。
import { useEffect, useRef } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { BlobEntry } from "../lib/api";
import { currentMapSource } from "../lib/mapTiles";

export function MapView({ blobs, onDone }: { blobs: BlobEntry[]; onDone: () => void }) {
  const mapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!mapRef.current) return;
    const src = currentMapSource();
    const map = L.map(mapRef.current);
    L.tileLayer(src.url, {
      attribution: src.attribution,
      maxZoom: src.maxZoom,
      ...(src.subdomains ? { subdomains: src.subdomains } : {}),
    }).addTo(map);

    const gpsBlobs = blobs.filter(
      (b) => b.gps_lat != null && b.gps_lon != null
    );
    if (gpsBlobs.length === 0) {
      map.setView([35.0, 105.0], 4); // 中国中心（无 GPS 时默认视野）
    } else {
      const bounds = L.latLngBounds(
        gpsBlobs.map((b) => L.latLng(b.gps_lat!, b.gps_lon!))
      );
      map.fitBounds(bounds.pad(0.1));
      for (const b of gpsBlobs) {
        L.marker([b.gps_lat!, b.gps_lon!])
          .addTo(map)
          .bindPopup(`<b>${b.file_name}</b><br>${b.gps_lat!.toFixed(4)}, ${b.gps_lon!.toFixed(4)}`);
      }
    }

    return () => {
      map.remove();
    };
  }, [blobs]);

  const gpsCount = blobs.filter((b) => b.gps_lat != null).length;

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">地图视图</span>
        <span className="storages">{gpsCount} 个 GPS 标记</span>
        <span className="spacer" />
        <button onClick={onDone}>返回</button>
      </header>
      {gpsCount === 0 && (
        <p className="hint">暂无 GPS 标记的照片。EXIF GPS 在导入时自动解析。</p>
      )}
      <div ref={mapRef} style={{ height: "calc(100vh - 120px)", borderRadius: 12 }} />
    </div>
  );
}
