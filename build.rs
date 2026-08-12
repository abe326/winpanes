use embed_manifest::{embed_manifest, new_manifest};
use embed_manifest::manifest::DpiAwareness;

fn main() {
    // ターゲットが Windows のときだけマニフェストを埋め込む(WSL からのクロスビルド対応)
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Winpanes").dpi_awareness(DpiAwareness::PerMonitorV2))
            .expect("manifest embedding failed");
        // アプリアイコン(リソースID=1)。MSVC は rc.exe、GNU クロスは windres を使う
        winresource::WindowsResource::new()
            .set_icon("assets/winpanes.ico")
            .compile()
            .expect("icon resource embedding failed");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/winpanes.ico");
}
