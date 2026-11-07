// 构建期校验：webapp 前端脚本必须通过 node 语法检查（M5a 曾因模板字符串少闭合
// 导致整个 PWA 脚本失效、按钮全无反应——编译期拦截此类事故）。
use std::process::Command;

fn main() {
    // node 语法检查（node 不存在时跳过——测试环境允许）
    if which("node") {
        for f in ["src/webapp/app.js", "src/webapp/sw.js"] {
            let status = Command::new("node").arg("--check").arg(f).status();
            match status {
                Ok(s) if s.success() => {}
                Ok(s) => panic!("前端脚本语法检查失败: {f}（exit {}）", s.code().unwrap_or(-1)),
                Err(e) => eprintln!("cargo:warning=无法运行 node --check {f}: {e}"),
            }
        }
    }
    println!("cargo:rerun-if-changed=src/webapp/app.js");
    println!("cargo:rerun-if-changed=src/webapp/sw.js");
    println!("cargo:rerun-if-changed=src/webapp/index.html");
    println!("cargo:rerun-if-changed=src/webapp/manifest.webmanifest");
    println!("cargo:rerun-if-changed=src/share.html");
}

fn which(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
