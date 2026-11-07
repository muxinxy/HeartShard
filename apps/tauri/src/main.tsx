import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// 相册应用：禁用 webview 默认右键菜单（返回/刷新/另存为/打印），全程左键操作。
document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
