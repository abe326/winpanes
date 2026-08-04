use embed_manifest::{embed_manifest, new_manifest};
use embed_manifest::manifest::DpiAwareness;

fn main() {
    // ターゲットが Windows のときだけマニフェストを埋め込む(WSL からのクロスビルド対応)
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Winpanes").dpi_awareness(DpiAwareness::PerMonitorV2))
            .expect("manifest embedding failed");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
