// Service Worker：静态壳「网络优先、离线回退」——保证服务器更新后前端立刻生效；
// 旧版本号缓存会在 activate 时清空。数据（/api、/s、缩略图/原图）永不缓存。
const SHELL = "hs-shell-v2";

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(SHELL));
  self.skipWaiting();
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== SHELL).map((k) => caches.delete(k)))
    )
  );
  self.clients.claim();
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  // 只处理静态壳；/api、/s、缩略图/原图一律直连网络（不拦截）
  if (e.request.method !== "GET" || url.pathname.startsWith("/api") || url.pathname.startsWith("/s/")) {
    return;
  }
  if (url.pathname === "/" || url.pathname === "/app.js" || url.pathname === "/manifest.webmanifest") {
    e.respondWith(
      fetch(e.request)
        .then((res) => {
          const copy = res.clone();
          caches.open(SHELL).then((c) => c.put(e.request, copy));
          return res;
        })
        .catch(() => caches.match(e.request).then((hit) => hit || Response.error()))
    );
  }
});
