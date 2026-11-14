const fs = require("fs");
const files = [
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/lib.rs",
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/library.rs",
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/storage_s3.rs",
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/storage_openlist.rs",
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/storage_webdav.rs",
  "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/storage_ftp.rs",
];
for (const f of files) {
  const s = fs.readFileSync(f, "utf8");
  const i = s.indexOf("trait BlobStore");
  if (i >= 0) {
    console.log("== " + f);
    console.log(s.slice(i - 200, i + 900));
  }
}
