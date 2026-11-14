const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/core-rs/crates/heart-core/src/library.rs";
const s = fs.readFileSync(p, "utf8");
const re = /(?:pub )?(?:trait|enum|struct) [A-Za-z]+/g;
let m;
while ((m = re.exec(s))) console.log(m.index, m[0]);
