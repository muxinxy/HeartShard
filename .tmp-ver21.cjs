const fs = require("fs");
const root = "D:/Documents/Code/HeartShard/";
let n = 0;
const rep = (f, from, to) => {
  let s = fs.readFileSync(root + f, "utf8");
  if (!s.includes(from)) { console.log("MISS:", f); return; }
  s = s.split(from).join(to);
  fs.writeFileSync(root + f, s);
  n++;
  console.log("ok:", f);
};
rep("apps/tauri/src-tauri/tauri.conf.json", '"version": "0.2.0"', '"version": "0.2.1"');
rep("apps/tauri/src-tauri/Cargo.toml", 'version = "0.2.0"', 'version = "0.2.1"');
rep("apps/tauri/package.json", '"version": "0.2.0"', '"version": "0.2.1"');
rep("server/Cargo.toml", 'version = "0.2.0"', 'version = "0.2.1"');
rep("core-rs/Cargo.toml", 'version = "0.2.0"', 'version = "0.2.1"');
rep("tools/syncer/Cargo.toml", 'version = "0.2.0"', 'version = "0.2.1"');
rep(".github/workflows/build.yml", "HeartShard-0.2.0-x64-portable.exe", "HeartShard-0.2.1-x64-portable.exe");
rep("scripts/build-android-apk.ps1", "HeartShard-0.2.0-arm64.apk", "HeartShard-0.2.1-arm64.apk");
console.log("total:", n);
