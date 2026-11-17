const fs = require("fs");
const root = "D:/Documents/Code/HeartShard/";
let n = 0;
const rep = (f, from, to) => {
  const p = root + f;
  let s = fs.readFileSync(p, "utf8");
  if (!s.includes(from)) { console.log("MISS:", f); return; }
  s = s.split(from).join(to);
  fs.writeFileSync(p, s);
  n++;
  console.log("ok:", f);
};
rep("apps/tauri/src-tauri/tauri.conf.json", '"version": "0.1.0"', '"version": "0.2.0"');
rep("apps/tauri/src-tauri/Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"');
rep("apps/tauri/package.json", '"version": "0.1.0"', '"version": "0.2.0"');
rep("server/Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"');
rep("core-rs/Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"');
rep("tools/syncer/Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"');
rep(".github/workflows/build.yml", "HeartShard-0.1.0-x64-portable.exe", "HeartShard-0.2.0-x64-portable.exe");
console.log("total:", n);
