const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/server/src/share.rs";
let s = fs.readFileSync(p, "utf8");
// meta 端点：已过期仍可读清单（仅元信息）；keys/pkg 端点保留 check_expired
const marker = `let cache_dir = state.share_cache_dir(&share.id);
    match std::fs::read(cache_dir.join("meta.json")) {`;
const before = `let cache_dir = state.share_cache_dir(&share.id);
    match std::fs::read(cache_dir.join("meta.json")) {`;
// 找到 meta 处理函数中 check_expired 的行（meta.json 读取前的唯一 check_expired）
const metaFnStart = s.indexOf("pub async fn share_meta");
const keysFnStart = s.indexOf("pub async fn share_keys");
if (metaFnStart < 0 || keysFnStart < 0) { console.log("anchors missing"); process.exit(1); }
const metaBody = s.slice(metaFnStart, keysFnStart);
const patched = metaBody.replace(
  /    if let Err\(e\) = check_expired\(&share\) \{\n        return e;\n    \}\n/,
  "    // 已过期仍可读清单（仅元信息）；密钥/密文端点继续拦截\n"
);
if (patched === metaBody) { console.log("no check_expired found in meta body"); process.exit(1); }
s = s.slice(0, metaFnStart) + patched + s.slice(keysFnStart);
fs.writeFileSync(p, s);
console.log("share_meta expiry relaxed");
