const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/apps/tauri/src/components/SettingsPage.tsx";
let s = fs.readFileSync(p, "utf8");
s = s.split('<div className="settings-card">').join('<div className="settings-card vform">');
s = s.split('<label className="setting-row">').join('<label className="vfield">');
fs.writeFileSync(p, s);
console.log("vform:", (s.match(/settings-card vform/g) || []).length, "vfield:", (s.match(/vfield/g) || []).length);
