const fs = require("fs");
const base = "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/";
for (const f of ["storage_openlist.rs", "storage_webdav.rs", "storage_ftp.rs"]) {
  const s = fs.readFileSync(base + f, "utf8");
  const i = s.indexOf("pub struct");
  console.log("== " + f);
  console.log(s.slice(i, i + 500));
}
