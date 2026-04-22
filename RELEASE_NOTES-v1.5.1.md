# smooth-mod-v1.5.1 — Multi-Frame Rendering 対応 リリースノート

公開日: 2026-04-22
リポジトリ HEAD: `b874f87`(Mac binary embedded build-id)
Windows binary embedded build-id: `df07a80`(同等内容、docs-only 差分)

## 概要

Adobe After Effects 向けスムージングプラグイン **smooth** のマイナーリリース。

v1.5.0(`8f0ce84`、Rust コア + Windows ビルド統合時点)に対し、**Adobe 公式推奨の高速化機構である Multi-Frame Rendering (MFR) 対応** と **build-id UI によるフォルスサクセス検知機構** を導入。どちらも非破壊追加のため、既存プロジェクトの挙動は不変。

### ハイライト

- **Multi-Frame Rendering 対応**: AE が複数フレームを同時に並列レンダーできるようになり、書き出し時の CPU 使用率が実質全コアまで跳ねる。Phase 2-C で導入済みの行ブロック並列(rayon)と直交する階層で効く
- **build-id UI**: Effect Controls に `Build: 0.1.0+<sha>` を常時表示、About ダイアログで詳細バージョン情報。「ビルドが古い / 入れ替え忘れ」で発生するフォルスサクセスを視認で検知可能に
- **Mac + Windows 両プラットフォーム同時リリース**: 両バイナリに対し SHA256 ゴールドを固定、CI 化時の基準点として `workbench_history.md` に記録

### パフォーマンス(MFR による変化)

Phase 2-C 時点(v1.5.0)で単フレーム処理は既に rayon で並列化済み(HD 16bpc 20ms → 7ms)。v1.5.1 はその上に MFR(フレーム間並列)を積むため、**バッチ書き出し等で AE が複数フレームを同時投入する場面**で追加の速度向上が得られる。

実測(macOS, AE 2025 バッチ書き出し):
- レンダー報告 `Multithreaded render report` で `Render threads used: 11 / 13` / `Max allowed concurrency: 16` を確認
- `Thread-safe effects used: KOJI_SMOOTH`(`Non-thread-safe effects used:` 側ではない)に分類されるようになった

単フレームプレビュー等の AE 側が MFR を意図的に絞るコンテキストでは引き続き 2 スレッドまでに留まる(これは AE 仕様、plugin 側要因ではない)。

## 変更点(v1.5.0 → v1.5.1)

### 機能追加

- **Multi-Frame Rendering (MFR)** 対応(`42688f8`):
  - `PF_OutFlag2_SUPPORTS_THREADED_RENDERING` (bit 27 = `0x08000000`) を GlobalSetup 時に立てる
  - `PF_OutFlag2_SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` (bit 23) も同時に必要(AE 2025 の `FLTp_EnforceFlagCombinations` が SDK doc よりも厳しく legacy render + MFR 全般に要求するため)
  - `Pipl.r` の `AE_Effect_Global_OutFlags_2` = `0x08800010`、Effect.cpp の out_flags2 と同期
- **build-id UI**(`a47d468`):
  - `smooth_core_build_id()` FFI で Rust 側から build 時の git short SHA を取得、Effect Controls の Build カスタムコントロールに表示
  - About ダイアログに `smooth, v1.5.1 / rust_core 0.1.0+<sha> ffi=0x00020003` を表示
  - build.rs が HEAD の SHA + dirty flag を `SMOOTH_CORE_GIT_SHA` 環境変数としてクレートに焼き込む仕組み
- **Review findings 反映**(`0c5b06d`): 独立レビューで指摘された 4 件の軽微な改善(cbindgen 周りの型定義、コメント精度等)を PR 前に反映

### ドキュメント

- LTO 環境における FFI シンボル検証手順の訂正(`a3bed27`): Release + WholeProgramOptimization では FFI シンボルが .aex にインライン化されるため、`smooth_core.lib` 側で検証する方針に変更
- workbench_history.md に Phase 2-B (MFR) の 1 ~ 4 ステップを完全記録(監査、実装、Mac 実機確認、Windows 追従)
- MSVC linker 非決定性の記録(MSVC PE header timestamp が rebuild 毎に変わる問題)と、Windows AE 25.x の GUI Render Queue ログに `Multithreaded render report` が出ない仕様差分

### 内部変更(ユーザー影響なし)

- Effect.cpp に PF_Cmd_GET_FLATTENED_SEQUENCE_DATA 受け入れ可能な状態(本 plugin は sequence_data 未使用のため AE が NULL を受けて満足、ハンドラ実装不要)

### 既知の挙動変更

- 本リリースから Mac / Windows 両バイナリが `Thread-safe effects used:` に分類される。従来 `Non-thread-safe effects used:` に落ちていたのが正しい位置に移動
- AE 2025 の 32bpc プロジェクトではエフェクトヘッダに黄色 ⚠️ マークが表示されるが、これは 32bpc 非対応マーク(smooth は 8/16bpc のみ対応)であり MFR とは無関係

## 配布物 & ゴールド SHA256

### Mac

| ファイル | 対象 | サイズ | SHA256 |
| --- | --- | --- | --- |
| `smooth.Mac.1.5.1.AE2025.universal.zip` | Apple Silicon + Intel 両対応 | 492,177 B | `2eb4fe222409468d4ced198a2bd9feaf0277145920dc0eb4ebcb686d40784824` |
| `smooth.Mac.1.5.1.AE2025.arm64.zip`     | Apple Silicon 専用           | 229,741 B | `1cb28bf137faf19752dbf7dc8dade862a4fd13b058ab472d40eb839401e7fc49` |
| `smooth.Mac.1.5.1.AE2025.x86_64.zip`    | Intel 専用                  | 261,941 B | `2f22bc43a57ddf8b77921f18a6bf2723fe61d1d89a2b2ac1491fae1a052a6a64` |

内部バイナリ(`smooth.plugin/Contents/MacOS/smooth`)SHA256:
- universal: `64092413675c48058764bc31ae7a1f1f4ce155d538de57208f2d50869f9f775f`(1,177,200 B、x86_64 + arm64 fat Mach-O)
- arm64: `334fc78f760ed5f7e698200e268abdf99124d2c05166624e53ddbfd3e18b98a7`(568,208 B)
- x86_64: `e11a82e589caefd11b899ac4ce68bb299c875f6c90134e03200b14c8f370a33a`(606,240 B)

### Windows

| ファイル | 対象 | サイズ | SHA256 |
| --- | --- | --- | --- |
| `smooth.Win.1.5.0.AE2025.x64.zip` | Windows x64 | 200,072 B | `4D36B3415532AAD543375517CDF39FC30EDFD2BB387D705E2DFB18E3C8868CB7` |

内部 `smooth.aex`: `825DA078FF3E18C2C305204706ED65AEF93738A397BCE6FED233593F1532C836`(393,216 B)

Windows アーカイブ名が `1.5.0` のままなのは、Windows チームのゴールドビルドが `df07a80` 時点で固定済みのため。中身は v1.5.1 と同等(`b874f87` と `df07a80` の差分は docs のみで機能コード差分はゼロ)。Build caption で両者が区別可能:
- Mac 版: `Build: 0.1.0+b874f87`
- Windows 版: `Build: 0.1.0+df07a80`

## インストール

### Mac

```sh
# universal 推奨
unzip smooth.Mac.1.5.1.AE2025.universal.zip
sudo cp -R smooth.plugin "/Applications/Adobe After Effects 2025/Plug-ins/Effects/"
```

### Windows

zip を展開して `smooth.aex` を以下にコピー:

```
C:\Program Files\Adobe\Adobe After Effects 2025\Support Files\Plug-ins\Effects\
```

### 共通: インストール確認(3 段偽成功検証)

AE 起動後、任意のレイヤーに `LoiLo > smooth` を適用して Effect Controls を開き、

1. **Build キャプション**に `0.1.0+b874f87`(Mac)または `0.1.0+df07a80`(Windows)が表示されている
2. Build キャプションをクリックすると **About ダイアログ**が開き、`rust_core 0.1.0+<sha> ffi=0x00020003` が表示される
3. エフェクト適用で verification-failure ダイアログが出ない(MFR 対応が AE に正しく認識されている)

## 動作要件

### Mac

- macOS 10.13 以降
- Apple Silicon (arm64) または Intel (x86_64)
- Adobe After Effects 2025 以降

### Windows

- Windows 10 19041 以降
- x64 CPU
- Adobe After Effects 2025 以降

## 既知の注意事項

- **ad-hoc 署名 / Windows 未署名**: Gatekeeper / SmartScreen で弾かれる場合は初回のみ手動で許可。配布用途では Developer ID 署名 / EV 署名 + 公証を推奨
- **MFR と 32bpc の黄色 ⚠️**: 32bpc プロジェクトでは smooth が対応していないため黄色マークが付く。これは MFR 警告ではなく bpc 非対応マーク、MFR 自体は正常に動作している
- **Windows MFR 実効確認手段**: AE 25.x の GUI Render Queue ログには `Multithreaded render report` ブロックが出ない。Windows で並列稼働を確認する時は GUI プログレスバー目視または `aerender.exe` 経由(stdout に出る)を使用
- **MSVC linker / codesign timestamp 非決定性**: Windows は PE header の timestamp、Mac は codesign timestamp のため、同一ソース + 同一環境の clean rebuild でも SHA256 が変わる。上記ゴールド SHA と一致しない等価バイナリの確認方法は `workbench_history.md` の「等価性検証手順」セクション参照

## ビルド情報

### Mac

- Xcode 26.3 (Build 17C529)
- macOS SDK 26.2
- After Effects SDK 25.6.61
- Rust stable 1.95.0 (target x86_64-apple-darwin + aarch64-apple-darwin lipo 合成)

### Windows

- Windows 10 Pro 19045.6456
- Visual Studio 2022 v143 (MSVC 19.44.35225)
- Windows SDK 10.0.26100.0
- After Effects SDK 25.6.61
- Rust stable 1.95.0 (target x86_64-pc-windows-msvc, `+crt-static`)

## ライセンス

Apache License 2.0
