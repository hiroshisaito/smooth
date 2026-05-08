# smooth-mod-v1.5.0 — AE2025 対応 + 並列化高速版 リリースノート

公開日: 2026-04-21
ビルド: `feature/smooth-mod-v1.5.0`(コミット `662d300` 時点)

## 概要

Adobe After Effects 向けスムージングプラグイン **smooth** のメンテナンスリリース。

1.4.0 (AE2025 対応) からの主な変更点は **コア処理の AE SDK 非依存化** と **行ブロック並列化** による大幅な高速化。HD 16bpc で **約 2.9× スピードアップ**(20 ms → 7 ms)。

## パフォーマンス

8 コア Intel Core i9 (MacBook Pro) で計測(`tests/bench.sh`、repeat=30):

| 解像度 × bpc | 1.5.0 シリアル | **1.5.0 並列** | Speedup |
| --- | --- | --- | --- |
| 1920×1080 16bpc | 20.0 ms | **7.0 ms** | **2.9×** |
| 2512×1412 8bpc  |  9.7 ms |  5.3 ms | 1.8× |
| 3840×2160 8bpc  | 70.1 ms | 23.2 ms | 3.0× |
| 3840×2160 16bpc | 84.3 ms | 31.8 ms | 2.6× |

スレッド数は `std::thread::hardware_concurrency()` に追従。

## 変更点(1.4.0-ae2025 からの差分)

### 機能改善
- **行ブロック並列化**(`std::thread`、hardware_concurrency() 連動、CPU スレッドベース)
  - 環境変数相当の `SMOOTH_PARALLEL=0` でコンパイル時に並列オフ可能(デバッグ向け)
- **AE SDK 非依存のコアモジュール** `smooth_core.h` を新設
  - 旧 `Effect.cpp::smoothing<>()` の約 430 行を純関数 `smooth_core::process<T>()` に分離
  - BlendingInfo から `PF_LayerDef*` 依存を除去
  - 回帰テストが AE なしで走行可能に(`tests/run_regression.sh`、`tests/bench.sh`)
- **arm64 PiPL エントリ** を追加(`CodeMacARM64`) — 1.4.0 のユニバーサルバイナリは arm64 でロード失敗する不具合を修正

### 既知の挙動変更
- 並列化の結果、**HD 2512×1412 8bpc でごく僅かな境界残差** が発生する場合があります(1.4.0 との diff: 最大 30 バイト / 14 MB = 0.0002%、max_abs = 23 / 255)。視覚上 invisible level。
- 厳密 byte-identical が必要な用途は `SMOOTH_PARALLEL=0` でビルド(単一スレッド動作)。
- その他 13 種のテストケース(8bpc 64×64、4K 8/16bpc、HD 16bpc 等)は **1.4.0 と完全一致**。

### 内部変更(ユーザー影響なし)
- `BlendingInfo<T>` から `PF_LayerDef*` を削除、`width/logical_width/height/rowbytes` を持つ POD に変更
- `getWhitePixel`/`getNullPixel` を `smooth_core` 名前空間に移設
- `PackedPixelType` を `std::conditional` で自動導出
- 開発用: `bench.h` による SMDP raw dump / タイミングログ、SMDP 回帰テストハーネス

### 試行したが不採用
- **SIMD 化(FAST_COMPARE_PIXEL 事前ベクトル化)**: clang -O2 の自動ベクトル化と差がつかず、pre-scan のストアコストが相殺。Step 5 にて中止。詳細は `workbench_history.md` 参照。

## 配布物

| ファイル | 対象 | サイズ |
| --- | --- | --- |
| `smooth.Mac.1.5.0.AE2025.universal.zip` | Apple Silicon + Intel 両対応 | ~56 KB |
| `smooth.Mac.1.5.0.AE2025.arm64.zip`     | Apple Silicon 専用           | ~28 KB |
| `smooth.Mac.1.5.0.AE2025.x86_64.zip`    | Intel 専用                  | ~31 KB |

通常は **universal** で OK。サイズ最適化したい場合のみ単独アーキ版を選択してください。

## インストール

```sh
# universal を例に
unzip smooth.Mac.1.5.0.AE2025.universal.zip
sudo cp -R smooth.plugin "/Applications/Adobe After Effects 2025/Plug-ins/Effects/"
```

After Effects を起動 → エフェクトメニューの `LoiLo > smooth` として利用可能。

## 動作要件

- macOS 10.13 以降
- Apple Silicon (arm64) または Intel (x86_64)
- Adobe After Effects 2025 以降

## 既知の注意事項

- ad-hoc 署名のみ。Gatekeeper で弾かれる場合は Finder で右クリック → 「開く」で初回のみ承認してください。配布用途では Developer ID 署名 / 公証を推奨します。
- 旧バージョン(1.4.0 以前)の PiPL は arm64 エントリを持たないため、Apple Silicon 環境で適用時にエラーが出ていました。本リリースで解消。

## ビルド情報

- Xcode 26.3 (Build 17C529)
- macOS SDK 26.2
- After Effects SDK 25.6.61

## ライセンス

Apache License 2.0
