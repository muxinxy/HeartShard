const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/server/src/webapp/app.js";
let s = fs.readFileSync(p, "utf8");
let n = 0;
const rep = (from, to, tag) => {
  if (!s.includes(from)) { console.log("MISS:", tag); return; }
  s = s.split(from).join(to);
  n++;
  console.log("ok:", tag);
};
rep('<button class="danger" id="asel-delete">🗑 删除相册</button>',
    '<button class="danger" id="asel-delete"><svg class="ic"><use href="#i-trash"/></svg>删除相册</button>', "asel-delete");
rep('<div class="row"><h2 style="margin:6px 0">📁 ${a.name}</h2>',
    '<div class="row"><h2 style="margin:6px 0"><svg class="ic"><use href="#i-albums"/></svg> ${a.name}</h2>', "album detail title");
rep('<tr><td>${kind === "user" ? "👤" : "🏷"} ${label}</td>',
    '<tr><td><svg class="ic"><use href=${kind === "user" ? "#i-users" : "#i-groups"}/></svg> ${label}</td>', "acl row");
rep('<button class="primary" id="acl-tab-user">👤 用户</button>',
    '<button class="primary" id="acl-tab-user"><svg class="ic"><use href="#i-users"/></svg> 用户</button>', "acl tab user");
rep('<button id="acl-tab-group">🏷 用户组</button>',
    '<button id="acl-tab-group"><svg class="ic"><use href="#i-groups"/></svg> 用户组</button>', "acl tab group");
rep('<tr><td>📁 ${a.name}</td>',
    '<tr><td><svg class="ic"><use href="#i-albums"/></svg> ${a.name}</td>', "acl album row");
fs.writeFileSync(p, s);
console.log("total:", n);
