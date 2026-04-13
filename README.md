# えぃにめ一閃流奥義「一閃 改」

アニメのスクショ・短尺動画を撮って Discord に投げる営みを保証する Windows 用 GUI ツール。
本家 [えぃにめ一閃流奥義「一閃」](https://github.com/Nu-Pan/aynime_issen_style) を Rust でフルリライトし、**1画面で完結**するように改良。

## ユーザー向け TL;DR
- DL は [Releases](https://github.com/ywrk-univ/aynime_issen_style/releases) から `aynime-issen.exe` をダウンロード
- ダウンロードして実行するだけ（インストール不要、シングルバイナリ 12MB）
- 初回起動時に FFmpeg を自動ダウンロード（要インターネット接続）

## 主な機能
- **スクリーンキャプチャ** — DXGI Desktop Duplication による高速キャプチャ
- **オーバーレイUI** — UIがキャプチャに映らない（WDA_EXCLUDEFROMCAPTURE）
- **Snipping Tool 風の範囲選択** — ドラッグで範囲指定、選択範囲の青枠表示
- **即一閃モード** — 範囲選択と同時にキャプチャ → クリップボードへ
- **静止画** — PNG, WebP, JPEG, BMP
- **動画** — GIF, MP4, WebM（FFmpeg バックグラウンドエンコード）
- **ファイルサイズ制限** — 超過時に自動品質調整
- **設定GUI** — フォーマット・FPS・最大サイズを UI から変更可能
- **config.json** でカスタマイズ（`%LOCALAPPDATA%\aynime-issen\config.json`）

## 動作要件
- Windows 10 2004 以降
- インターネット接続（初回 FFmpeg ダウンロードのみ）

## 開発者向け

### 技術スタック
- **Rust** — メイン言語
- **egui/eframe** — GUI フレームワーク
- **windows-rs** — Win32 API (DXGI, GDI, クリップボード等)
- **FFmpeg** — 動画エンコード（サブプロセス呼び出し）

### プロジェクト構成
```
rust-native/
├── Cargo.toml
└── src/
    ├── main.rs              # アプリケーション本体 + egui UI
    ├── config.rs            # 設定ファイル管理
    ├── capture/
    │   └── screen.rs        # DXGI Desktop Duplication
    ├── overlay/
    │   ├── window.rs        # WDA_EXCLUDEFROMCAPTURE
    │   ├── selection.rs     # Win32 + GDI 範囲選択オーバーレイ
    │   └── border.rs        # 選択範囲の青枠表示
    └── processing/
        ├── clipboard.rs     # クリップボード操作
        ├── ensure_tools.rs  # FFmpeg 自動ダウンロード
        ├── export.rs        # 静止画・動画エンコード
        └── region.rs        # キャプチャ範囲管理
```

### 開発環境セットアップ
1. [Rust ツールチェーン](https://rustup.rs/) をインストール
2. `cd rust-native && cargo run` で開発ビルド実行

### リリースビルド
```
cd rust-native
cargo build --release
```
出力: `rust-native/target/release/aynime-issen.exe`

## ライセンス
MIT License — 詳細は [LICENSE](LICENSE) を参照
