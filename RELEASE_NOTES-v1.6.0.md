# smooth-mod-v1.6.0 — 32bpc 対応 リリースノート

公開日: 2026-05-05
リポジトリ HEAD: `c407725`(Mac binary embedded build-id `0.1.0+c407725`)
Windows binary embedded build-id: `0.1.0+d172dec`(Windows clean build 時の HEAD `d172dec`)

## 概要

Adobe After Effects 向けスムージングプラグイン **smooth** のマイナーリリース。

v1.5.1(`b874f87` Mac / `df07a80` Windows、Multi-Frame Rendering + build-id UI)に対し、**32bpc(PF_PixelFloat / float color)対応** を導入。8/16bpc プロジェクトの挙動は不変、32bpc Comp / HDR / float color workflow でも smooth が黄色 ⚠️ なしで適用できるようになる。CPU only。

### ハイライト

- **32bpc(float color)対応**: AE の 32bpc Comp で smooth が `FLOAT_COLOR_AWARE` 経路で動作。8/16bpc は従来通り、32bpc は f32 (PF_PixelFloat) で処理(`smooth_core::process<PF_PixelFloat>`)
- **SmartRender 経路化**: legacy `PF_Cmd_RENDER` は後方互換のため残置しつつ、`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` 経路を主として AE と通信。bbox 計算と pixel checkout が AE 側で最適化される
- **manifest-driven 回帰テスト**: `tests/goldens/<suite>/manifest.toml` で 8/16bpc(v1.4.0-ae2025)+ 32bpc(v1.6.0-32bpc)の両 suite を schema 化、`mac_reference_policy` / `cross_platform_policy` の 2 段で Mac CPU bit-identical / Mac↔Win f32 tolerance を分離

### 非対応 / 範囲外

- **GPU 対応は本リリースには含まれない**(Mac Metal / Win CUDA とも非対応)。Phase 2-A.3 として GPU 化を試行したが、smooth アルゴリズムが GPU 苦手領域(scatter pattern + 後勝ちセマンティクス + 可変長 loop + cross-pixel decision)に位置することが確認され、AE/Metal の実用 envelope を超えるため中止。詳細は `workbench_history.md` の「Phase 2-A close 判定」節
- 8/16bpc プロジェクトの動作は v1.5.1 と完全 bit-identical(回帰 28/28 PASS)

## 変更点(v1.5.1 → v1.6.0)

### 機能追加

- **32bpc(PF_PixelFloat / float color)対応**:
  - `PF_OutFlag2_FLOAT_COLOR_AWARE` (bit 12 = `0x1000`) を GlobalSetup + Pipl.r で立てる
  - `Pipl.r::AE_Effect_Global_OutFlags_2 = 0x08801410`(I_AM_THREADSAFE | SUPPORTS_SMART_RENDER | FLOAT_COLOR_AWARE | SUPPORTS_GET_FLATTENED_SEQUENCE_DATA | SUPPORTS_THREADED_RENDERING)
  - Rust 側に `Pixel32`(f32 ARGB)、`smooth_core_preprocess_f32`、`smooth_core_process_row_range_f32` を追加。`SmoothScalar` trait + `SmoothPixel::Scalar` 関連型で 8/16/32bpc を統一抽象化
  - C++ 側の `smoothing<>()` を `if constexpr (sizeof==16)` で `range_f32` ブランチ化、`detect_pixel_format()` で AE の `PF_GetPixelFormat` 取得 → 3 段 bpc dispatch
- **SmartRender 経路化**(`e04e836`、`b64f1ee`):
  - `PF_OutFlag2_SUPPORTS_SMART_RENDER` (bit 10 = `0x400`) を立て、`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` のハンドラを実装
  - `PF_OutFlag_I_WRITE_INPUT_BUFFER` 撤去(SmartRender と排他、AE 2025 の verifier failure 回避)。代わりに `smoothing<>()` 内部で scratch buffer を確保
  - SmartPreRender → SmartRender の 2 段化に伴い、非 layer params を pre_render_data に snapshot
- **manifest-driven 回帰テスト**:
  - `tests/goldens/v1.4.0-ae2025/manifest.toml`(8/16bpc 14 frames)+ `tests/goldens/v1.6.0-32bpc/manifest.toml`(32bpc 14 frames、`tests/synthesize_32bpc_goldens.sh` で v1.4.0 入力を f32 promote した synthetic suite)
  - `tests/run_regression.sh` を manifest-driven 化(glob 廃止、SHA256 verify 統合)
  - 28/28 PASS(SMOOTH_PARALLEL=1/0 両方)

### ドキュメント

- `README.md` を v1.6.0 へ更新(対応表に 32bpc 行追加、配布 zip 名を 1.6.0 に統一)
- `docs/CAPTURE_32BPC_RUNBOOK.md`(synthetic primary path、EXR alternative)
- `docs/EXTERNAL_REVIEW_REQUEST.md`(レビュアー向け概要、CPU only 前提に整理)
- `tests/README.md` の 32bpc goldens capture 手順
- `workbench_history.md` に Phase 2-A.1(SmartRender)/ 2-A.2(32bpc)/ 2-A.3(GPU 試行 → 中止)/ 2-A close の各節を時系列で記録

### 内部変更(ユーザー影響なし)

- Rust crate の dependency を `rayon` のみに整理(GPU 撤退に伴い `metal` / `objc` / `foreign-types` / `block` / `dashmap` / `once_cell` / `thiserror` / `uuid` を削除)。`Cargo.lock` 自動再生成
- `smooth_core_version()` = `0x0002_0003`(CPU only の安定 ABI)
- LICENSE / THIRD_PARTY_LICENSES.md の Rust dep 表を縮小、Apple Metal / NVIDIA CUDA 商標表記を撤去

### 既知の挙動変更

- 32bpc Comp で AE 黄色 ⚠️ マークが消える(従来は 32bpc 非対応マーク)。
- v1.5.1 までで「`Non-thread-safe effects used:`」に落ちていた MFR 分類は v1.5.1 から既に修正済(本リリースでも維持)

## 配布物 & ゴールド SHA256

### Mac

| ファイル | 対象 | サイズ | SHA256 |
| --- | --- | --- | --- |
| `smooth.Mac.1.6.0.AE2025.universal.zip` | Apple Silicon + Intel 両対応 | 535,856 B | `b0209373b472767849ea8c3ecfabb80fecdd01d5ceaabf7b3c5e49066eea73d5` |
| `smooth.Mac.1.6.0.AE2025.arm64.zip`     | Apple Silicon 専用           | 251,585 B | `cdb7d1e93ee770b0d3e64ed0fe202e6c45c07a11b106fbd2bc5449ac6301ed28` |
| `smooth.Mac.1.6.0.AE2025.x86_64.zip`    | Intel 専用                  | 293,567 B | `fff27097241b9bc2bb96e9e6c6c305a77ba3287ff9e7fb41c60dbccd37551630` |

内部バイナリ(`smooth.plugin/Contents/MacOS/smooth`)SHA256:
- universal: `e1c651cdff25f61bac66f500ceb181c7824a23ee39cbe2b677766eaa9b458682`(1,267,152 B、x86_64 + arm64 fat Mach-O)
- arm64: `65b3518fc24713c04470d9486130203c53517e37ba4ff1146a7b40cba73706e6`(608,800 B)
- x86_64: `2ba198920568935166fbe0fbda6150f0978ff4f9ed90e6600f13df7d444cde21`(663,152 B)

### Windows

| ファイル | 対象 | サイズ | SHA256 |
| --- | --- | --- | --- |
| `smooth.Win.1.6.0.AE2025.x64.zip` | Windows x64 | 231,822 B | `7c338a756ce8630cafa3388078c4d27df719c102095b58070fc6fa6d2e84c0e6` |

内部 `smooth.aex`: 445,440 B / SHA256 `ac88d80a0c03a6fe52f4b8f76d36ab51cebc8e98145fedae5a47067b58192b72`(Windows clean build HEAD `d172dec`、UAT 8/8 PASS の固定参照値)

## 動作確認(3 段偽成功検証)

AE 2025 を起動 → 任意のレイヤーに `LoiLo > smooth` を適用、Effect Controls を開いて:

1. **Build** キャプションに `0.1.0+<sha>` が表示される(古いビルドが残っていないことの確認)
2. **Build** キャプションをクリックして About ダイアログで `smooth, v1.6.0` + `rust_core 0.1.0+<sha> ffi=0x00020003` 表示
3. AE 起動時・プロジェクト読込時に verification-failure ダイアログが出ない(MFR / SmartRender / FLOAT_COLOR_AWARE flag が正しく同期されていることを確認)
4. **32bpc 動作確認**: 32bpc Comp(`File > Project Settings > Color > Depth: 32 bits per channel`)で smooth を適用 → エフェクト名横に黄色 ⚠️ なし + クラッシュなし + 出力が 8/16bpc と視覚同等

## 互換性

- AE 2025(SDK 25.6.61)対象。AE 2024 以前は未検証
- 8/16bpc プロジェクトの出力は v1.5.1 と byte-identical(回帰 14/14 PASS)
- 32bpc プロジェクトは v1.5.1 では非対応のため対比なし(従来 32bpc では smooth が skip / downgrade されていた)

## ライセンス

Apache 2.0([upstream](https://github.com/loilo-inc/smooth) から継承)。再配布時は `LICENSE` + `THIRD_PARTY_LICENSES.md` 同梱必須。

## クレジット

- **upstream**: LoiLo 株式会社 — smooth プラグイン原作者(Apache 2.0 で公開)
- **本 fork**: Hiroshi Saito — メンテナンス、Claude (Anthropic) とのペアプログラミング
