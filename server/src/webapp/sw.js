// Service Worker：静态壳 cache-first（登录与数据 API 永不缓存——密文/明文数据都走网络直连）。
const SHELL = "hs-shell-v1";

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches.open(SHELL).then((c) => c.addAll(["/", "/app.js", "/manifest.webmanifest"]))
  );
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
  // 只缓存静态壳；/api、/s、缩略图/原图一律直连网络
  if (e.request.method !== "GET" || url.pathname.startsWith("/api") || url.pathname.startsWith("/s/")) {
    return; // 不拦截，浏览器默认行为
  }
  if (url.pathname === "/" || url.pathname === "/app.js" || url.pathname === "/manifest.webmanifest") {
    e.respondWith(
      caches.match(e.request).then(
        (hit) =>
          hit ||
          fetch(e.request).then((res) => {
            const copy = res.clone();
            caches.open(SHELL).then((c) => c.put(e.request, copy));
            return res;
          })
      )
    );
  }
});
