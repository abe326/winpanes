#!/usr/bin/env python3
"""winpanes.ico の生成スクリプト。

モチーフ: ツールの本質である「1枚のフレームに束ねた4分割パネル」。
ダークタイル + ライトなパネル + アクセント1枚(Windows 既定の青系)で、
ライト/ダーク双方のタスクバー・トレイで視認できる配色にしている。

実行: python3 assets/gen_icon.py
要件: Pillow 9.3+ (append_images によるサイズ別画像の格納に必要。
      それ未満だと 256px からの縮小になり小サイズ向けの描き分けが失われる)
出力: assets/winpanes.ico (16/24/32/48/64/128/256px)
"""

from pathlib import Path

from PIL import Image, ImageDraw

SIZES = [256, 128, 64, 48, 32, 24, 16]
SS = 4  # スーパーサンプリング倍率(縮小時のエッジを滑らかにする)

BG = (36, 41, 51, 255)  # ダークスレート(タイル地)
PANE = (232, 237, 245, 255)  # ライトグレー(パネル)
ACCENT = (51, 150, 255, 255)  # アクセント青(取り込んだウィンドウ)


def render(size: int) -> Image.Image:
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def px(frac: float) -> float:
        return frac * s

    # タイル(角丸スクエア)。小サイズでは余白を詰めて面積を確保する
    margin = px(0.03 if size <= 24 else 0.045)
    radius = px(0.16 if size <= 24 else 0.20)
    d.rounded_rectangle(
        (margin, margin, s - 1 - margin, s - 1 - margin), radius=radius, fill=BG
    )

    # 2x2 パネル。小サイズほどギャップを太らせて分割線を残す
    outer = px(0.17)
    gap = px(0.10 if size <= 24 else 0.07)
    pane_r = px(0.02 if size <= 24 else 0.045)
    half_w = (s - outer * 2 - gap) / 2
    for row in range(2):
        for col in range(2):
            x0 = outer + col * (half_w + gap)
            y0 = outer + row * (half_w + gap)
            fill = ACCENT if (row, col) == (0, 0) else PANE
            d.rounded_rectangle(
                (x0, y0, x0 + half_w, y0 + half_w), radius=pane_r, fill=fill
            )

    return img.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    out = Path(__file__).parent / "winpanes.ico"
    imgs = [render(s) for s in SIZES]
    imgs[0].save(
        out,
        format="ICO",
        sizes=[(s, s) for s in SIZES],
        append_images=imgs[1:],
    )
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
