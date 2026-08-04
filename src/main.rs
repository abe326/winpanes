#[cfg(windows)]
mod appmsg;
mod config;
mod dock;
mod layout;
#[cfg(windows)]
mod win_util;

#[cfg(windows)]
fn main() {
    // 後続タスクでメッセージループを実装する
    println!("window-panel-tool: not yet implemented");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This tool runs on Windows only. (ロジックのテストは cargo test で実行可)");
}
