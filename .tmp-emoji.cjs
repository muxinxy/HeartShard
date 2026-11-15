const fs = require("fs");
const files = [
  "D:/Documents/Code/HeartShard/server/src/webapp/app.js",
  "D:/Documents/Code/HeartShard/server/src/webapp/index.html",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/Shell.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/PhotoActions.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/PhotoGrid.tsx",
  "D:\Documents\Code\HeartShard/apps/tauri/src/components/Lightbox.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/MapExplorePage.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/Thumb.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/StoragePage.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/AlbumTree.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/TimelineView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/AlbumsView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/ExploreView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/TrashView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/SharesView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/FavoritesView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/admin/AdminUsersView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/admin/AdminGroupsView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/views/admin/AdminStatusView.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/BlobDetailModal.tsx",
  "D:/Documents/Code/HeartShard/apps/tauri/src/components/ImportWizard.tsx",
];
const emojiRe = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}\u{FE0F}\u{2190}-\u{21FF}\u{2700}-\u{27BF}]/gu;
for (const f of files) {
  try {
    const s = fs.readFileSync(f.replace(/\\\\/g, "\\"), "utf8");
    const lines = s.split("\n");
    const hits = [];
    lines.forEach((line, i) => {
      emojiRe.lastIndex = 0;
      if (emojiRe.test(line)) hits.push(`${i + 1}: ${line.trim().slice(0, 100)}`);
    });
    if (hits.length) {
      console.log("== " + f.split("/").pop());
      hits.slice(0, 30).forEach((h) => console.log("  " + h));
    }
  } catch (e) {
    console.log("ERR " + f + " " + e.message);
  }
}
