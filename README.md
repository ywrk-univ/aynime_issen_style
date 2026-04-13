# えぃにめ一閃流奥義「一閃 改」

アニメのスクショ・短尺動画を撮って Discord に投げる営みを保証する Windows 用 GUI ツール。
本家 [えぃにめ一閃流奥義「一閃」](https://github.com/Nu-Pan/aynime_issen_style) を Rust でフルリライトし、**1画面で完結**するように改良。

## ユーザー向け TL;DR
- DL は [最新リリース](https://github.com/ywrk-univ/aynime_issen_style/releases/latest) から `aynime-issen.exe` をダウンロード
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

## 使い方

### 基本操作（UIボタン）
1. **構え** — Snipping Tool 風のオーバーレイでキャプチャ範囲をドラッグ選択（Esc / 右クリックでキャンセル）
2. **構え解除** — 範囲選択をリセットし全画面に戻す
3. **即一閃**（チェックボックス） — 範囲選択と同時にスクショ → クリップボードへコピー
4. **形式** を選択 — 静止画: PNG / WebP / JPEG / BMP、動画: GIF / MP4 / WebM
5. 静止画の場合:
   - **「一閃」** — スクショを撮ってクリップボードにコピー
   - **収納** — ファイルとして保存
6. 動画の場合:
   - **キンキン…ボタン** — 録画開始
   - **停止・収納** — 録画停止 → エンコード → クリップボードにコピー

### グローバルショートカット
アプリがバックグラウンドでも使えるシステムワイドなショートカット:

| ショートカット | 動作 |
|---|---|
| `Ctrl+Shift+Z` | 静止画キャプチャ（「一閃」と同じ） |
| `Ctrl+Shift+X` | 録画 開始/停止トグル（動画フォーマット選択時のみ） |

### ミニモード
タブバー右端の **「ミニ」** ボタンでコンパクトな常駐バーに切り替え。

- 黒背景の小さいバーで、枠の色でステータスを表示
  - 🟢 緑枠 **「待機中」** — クリックでキャプチャ / 録画開始
  - 🔴 赤枠 **「キンキン...」** — 録画中、クリックで停止
  - 🟡 黄枠 **「処理中...」** — エンコード中
- **「拡大表示」** ボタンで通常モードに復帰
- ウィンドウは半透明（Win32 レイヤードウィンドウ）、ドラッグで移動可能
- グローバルショートカットはそのまま使用可能

### ワークフロー例
- **Discord にスクショを貼る**: 構え → 範囲選択 → `Ctrl+Shift+Z` → Discord で `Ctrl+V`
- **短尺動画を撮る**: 形式を GIF/MP4 に → `Ctrl+Shift+X` で録画開始 → もう一度 `Ctrl+Shift+X` で停止 → Discord で `Ctrl+V`
- **即一閃で最速**: 即一閃をON → 構え → ドラッグするだけでクリップボードにコピー完了

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
