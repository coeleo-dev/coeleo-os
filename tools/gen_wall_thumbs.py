#!/usr/bin/env python3
"""
Generate 120x68 JPEG thumbnails for Coeleo OS wallpapers and output
kernel/src/ui/deskset_thumbs.rs.
"""

import os
import glob
import io
from PIL import Image

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
DOCS_IMG = os.path.join(REPO_ROOT, "docs", "image")
WALLPAPERS_DIR = os.path.join(DOCS_IMG, "wallpapers")
OUT_RUST = os.path.join(REPO_ROOT, "kernel", "src", "ui", "deskset_thumbs.rs")

THUMB_W = 120
THUMB_H = 68

def clean_title(filename):
    if filename == "wallpaper.jpg":
        return "Default"
    name = filename
    if name.startswith("wallpaper_"):
        name = name[len("wallpaper_"):]
    if name.endswith(".png") or name.endswith(".jpg"):
        name = os.path.splitext(name)[0]
    
    # Custom titles for the known coeleo wallpapers
    title_map = {
        "cartoon_2d": "Cartoon 2D",
        "cartoon_2d000": "Cartoon 2D Alt",
        "cartoon_forte": "Cartoon Forte",
        "cartoon_pastel": "Cartoon Pastel",
        "coeleo_equilibrado": "Equilibrado",
        "coeleo_math_logo": "Math & Logo",
        "coeleo_math_logo copy": "Math & Logo (Var 1)",
        "coeleo_math_logo copy 2": "Math & Logo (Var 2)",
        "coeleo_math_logo copy 3": "Math & Logo (Var 3)",
        "coeleo_math_logo copy 4": "Math & Logo (Var 4)",
        "coeleo_math_logo copy 5": "Math & Logo (Var 5)",
        "pastel_cartoon": "Pastel Cartoon",
    }
    if name in title_map:
        return title_map[name]
    
    return name.replace("_", " ").title()

def main():
    items = []

    # 1. Default wallpaper (wallpaper.jpg)
    def_path = os.path.join(DOCS_IMG, "wallpaper.jpg")
    if os.path.exists(def_path):
        im = Image.open(def_path).convert("RGB")
        im_thumb = im.resize((THUMB_W, THUMB_H), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        im_thumb.save(buf, format="JPEG", quality=85)
        items.append({
            "key": "default",
            "filename": "wallpaper.jpg",
            "title": clean_title("wallpaper.jpg"),
            "jpeg_bytes": buf.getvalue(),
        })

    # 2. Wallpapers in docs/image/wallpapers
    png_files = sorted(glob.glob(os.path.join(WALLPAPERS_DIR, "*.png")))
    for p in png_files:
        fn = os.path.basename(p)
        im = Image.open(p).convert("RGB")
        im_thumb = im.resize((THUMB_W, THUMB_H), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        im_thumb.save(buf, format="JPEG", quality=85)
        items.append({
            "key": fn,
            "filename": fn,
            "title": clean_title(fn),
            "jpeg_bytes": buf.getvalue(),
        })

    print(f"Generated {len(items)} thumbnails ({THUMB_W}x{THUMB_H})")
    total_bytes = sum(len(it["jpeg_bytes"]) for it in items)
    print(f"Total thumbnail size: {total_bytes} bytes ({total_bytes / 1024:.1f} KiB)")

    # Generate Rust source file
    rust_lines = [
        "//! Pre-computed wallpaper thumbnails (120x68 JPEG) and metadata.",
        "//!",
        "//! Generated automatically by tools/gen_wall_thumbs.py. Do not edit directly.",
        "",
        f"pub const THUMB_W: u32 = {THUMB_W};",
        f"pub const THUMB_H: u32 = {THUMB_H};",
        "",
        "pub struct ThumbEntry {",
        "    pub filename: &'static str,",
        "    pub title: &'static str,",
        "    pub jpeg: &'static [u8],",
        "}",
        "",
    ]

    for idx, it in enumerate(items):
        byte_list = ", ".join(str(b) for b in it["jpeg_bytes"])
        rust_lines.append(f"const THUMB_{idx}: [u8; {len(it['jpeg_bytes'])}] = [")
        rust_lines.append(f"    {byte_list}")
        rust_lines.append("];")
        rust_lines.append("")

    rust_lines.append("pub const THUMBS: &[ThumbEntry] = &[")
    for idx, it in enumerate(items):
        rust_lines.append("    ThumbEntry {")
        rust_lines.append(f"        filename: \"{it['filename']}\",")
        rust_lines.append(f"        title: \"{it['title']}\",")
        rust_lines.append(f"        jpeg: &THUMB_{idx},")
        rust_lines.append("    },")
    rust_lines.append("];")
    rust_lines.append("")

    rust_lines.append("pub fn find_thumb(filename_or_key: &str) -> Option<&'static ThumbEntry> {")
    rust_lines.append("    let key = if filename_or_key == \"default\" || filename_or_key == \"Default\" {")
    rust_lines.append("        \"wallpaper.jpg\"")
    rust_lines.append("    } else {")
    rust_lines.append("        filename_or_key.rsplit('/').next().unwrap_or(filename_or_key)")
    rust_lines.append("    };")
    rust_lines.append("    THUMBS.iter().find(|t| t.filename == key)")
    rust_lines.append("}")
    rust_lines.append("")

    with open(OUT_RUST, "w", encoding="utf-8") as f:
        f.write("\n".join(rust_lines))

    print(f"Written: {OUT_RUST}")

if __name__ == "__main__":
    main()
