# 外部レビュー依頼テンプレート — smooth (After Effects plugin)

外部レビュアーが本プラグインの前提・設計・要件をゼロから把握した上で
レビューを始められるよう、初回依頼時に提示すべき情報をまとめたもの。
個別のレビュー依頼ごとに「対象範囲」「期待するフィードバック」節を
書き換えて使う。テンプレ部(プロジェクト概要〜参照ドキュメント)は
原則変更しない。

---

## 1. プロジェクト概要(初回レビュアー向け)

**`smooth`** は After Effects (Adobe AE) 上で動作するエフェクトプラグイン。
入力レイヤーの色境界(エッジ)を解析し、近接する同色領域を結合して
**ジャギーを smoothing する** ことが主目的。1 px 単位の細部を保ったまま
階段状の段差だけを抑える特性が特徴で、アニメ・モーショングラフィックスの
中間生成物に多用される。

- 配布: 内製・小規模(社内利用 + 限定配布)。Adobe Add-ons では未公開
- 対応 AE: AE 2025(SDK 25.6_61)、Mac arm64/x86_64 + Win x64
- 言語構成:
  - C++(AE SDK の薄皮、effect entry / Pipl.r / dispatch)
  - Rust(コア演算 + 並列化 + 32bpc f32 path、`rust/smooth_core/` の staticlib)
  - C++/Rust FFI(`rust/smooth_core/include/smooth_core_ffi.h`)
- 履歴: Phase 1 で C++ コアを Rust 移植 + rayon 並列化 → v1.5.0 出荷。
  Phase 2-B で MFR(Multi-Frame Rendering)対応 → v1.5.1 出荷。
  現在は **Phase 2-A** で SmartRender + 32bpc 対応を進めて **v1.6.0** 出荷
  (CPU only)を目指している段階。

## 2. アーキテクチャ概観

```
After Effects (host)
  │
  ▼
Effect.cpp (AE SDK selector / dispatch)
  │  ├── About / GlobalSetup / ParamsSetup
  │  ├── Render (legacy)        ─┐
  │  └── SmartPreRender / SmartRender ┴─→ smooth_core::process<PixelType>
  │                                          │
  │                                          ▼
smooth_core.h (C++ template wrapper)
  │
  ▼ FFI
rust/smooth_core/ (staticlib)
  ├── lib.rs            FFI 入口(extern "C")
  ├── preprocess.rs     白抜き + bbox 検出
  ├── process.rs        スキャン + ブレンド
  ├── types.rs          SmoothPixel / SmoothScalar trait
  └── tests             cargo test 15 cases
```

- `PixelType` は `PF_Pixel8` (u8 ARGB) / `PF_Pixel16` (u16 ARGB、AE は 0..0x8000) /
  `PF_PixelFloat` (f32 ARGB、AE は 0..1 + overbright 許容)の 3 種。
- 並列化は Rust 内部の rayon に閉じている(`SMOOTH_PARALLEL` マクロで
  serial に切替可能、回帰テストで両モード PASS が要件)。
- SmartRender(AE の 2 段階 PreRender → Render 経路)対応は Phase 2-A.1 で完了、
  legacy `Render` は後方互換のため残置。

## 3. テスト・回帰戦略

- **goldens-based regression**: `tests/goldens/<suite>/` 配下の SMDP raw dump
  (header 64 bytes + raw pixels)を入力として `smooth_core::process` を
  再実行 → 期待出力と byte/f32 比較。
- **suite 一覧**:
  - `v1.4.0-ae2025/`: 8/16bpc 14 frames、AE bench 経由で 2026-04-21 取得
  - `v1.6.0-32bpc/`: 32bpc 14 frames、`tests/synthesize_32bpc_goldens.sh` で
    v1.4.0 入力を f32 promote した synthetic suite(2026-05-03 生成)
- **manifest schema** (`tests/goldens/<suite>/manifest.toml`): `mac_reference_policy`
  (Mac CPU bit-identical)と `cross_platform_policy`(Mac↔Win 許容)を分離し、
  per-frame `policy_overrides` で例外を表現(frame 135 の Phase 1 strip-parallel
  境界残差が代表例)。Phase 2-A.2 Step 3 で確定。
- **fixture の所在**: `tests/goldens/<suite>/*.raw` は `.gitignore` で commit 除外
  (1 suite あたり 502 MB 〜 1 GB 級)、`manifest.toml` のみ tracked。
  fresh clone から `tests/fetch_goldens.sh` または `tests/synthesize_32bpc_goldens.sh`
  で再取得・再生成可能。
- **cargo test** は 32bpc f32 の overbright / NaN / subnormal 防御を unit test で網羅。

## 4. 必読ドキュメント(順番厳守)

レビュアーは以下を **この順** で読むことを強く推奨:

1. [`README.md`](../README.md) — プラグインの利用者向け概要(短い)
2. [`docs/CAPTURE_32BPC_RUNBOOK.md`](CAPTURE_32BPC_RUNBOOK.md) — 32bpc goldens の
   取得 / 再生成手順(Phase 2-A.2 関連レビュー時のみ必要)
3. **[`workbench_history.md`](workbench_history.md)** — 作業ログ。
   各 Phase / Step の実施記録 + 失敗例 + 根拠付きの設計判断。
   レビュー対象 Step の節を **時系列で前後 1〜2 Step 分** 読むと文脈が掴める
   (「なぜこの選択をしたか」がここに残っている例が多い)
4. レビュー対象のコード差分(個別レビューの「対象範囲」節で指定)

依頼書を渡された段階で 1〜3 を読み終え、設計判断の why が理解できる
ようになっていることを期待。読了所要は初回 1〜2 時間程度。

## 5. レビュー観点の優先度

| 優先度 | 観点 | 補足 |
|---|---|---|
| 高 | AE SDK との契約遵守 | PF_OutFlag / Pipl.r 同期、PreRender 5 条件、PF_Err 戻し方、sequence_data 扱い、MFR thread-safety |
| 高 | FFI surface の安定性 | C++↔Rust 境界 struct layout、`smooth_core_version()` 枝番運用、ABI 互換 |
| 高 | 並列化の正当性 | rayon strip-parallel の境界残差、`SMOOTH_PARALLEL=0` で deterministic、frame 135 NEAR-ID の根拠 |
| 高 | 32bpc f32 ハンドリング | overbright (>1.0) / NaN / Inf / subnormal の各経路、AE の max_value=1.0 仮定 |
| 中 | regression coverage | manifest schema 整合、fixture 再生成性、tolerance 設定の妥当性 |
| 低 | コード品質 / コメント | 名前付け、コメントの過不足、テスト用 tooling の robust 度 |

## 6. レビュー対象範囲(個別依頼ごとに記入)

> **記入例**:
>
> - 対象 commit 範囲: `43c0e11..ccd6439`(Phase 2-A.2 Step 4a + 4b、3 commits)
> - 主な変更ファイル:
>   - `bench.h` — SMDP v2 header schema
>   - `tests/regression_test.cpp` — 32bpc dispatch + bpc-aware NEAR-ID
>   - `tests/synth_32bpc.cpp` + `tests/synthesize_32bpc_goldens.sh` — synthetic capture path
>   - `tests/goldens/v1.6.0-32bpc/manifest.toml` — committed fixture metadata
>   - `tests/capture_32bpc.py` — EXR alternative path(committed but未使用)
> - 関連節: RFC §3.2、§3.2.6、workbench_history "Phase 2-A.2 Step 4a / 4b"
> - 内部 review 結果: `/review` で M1〜M3、L1〜L5 検出済(別添)。`/security-review` は HIGH/MEDIUM 0 件
> - 関連実機テスト結果: 該当 commit 時点で Mac AE 2025 で 8/16/32bpc Comp 動作確認 PASS、regression 28/28 PASS

## 7. 期待するフィードバック(個別依頼ごとに記入)

> **記入例**:
>
> 内部 /review が拾えなかった以下の観点を中心に確認:
>
> 1. SMDP v2 header の reserved[0] → params_range_f32 への配置で、
>    将来の SMDP v3 拡張余地が縮退していないか
> 2. `synth_32bpc.cpp` の f32 promotion math(u8/255、u16/32768)が
>    AE の 32bpc working space の semantics と整合しているか
>    (premultiplied / linear / sRGB の前提)
> 3. synthetic capture を primary path に採用する判断(RFC §3.2.6 を
>    根拠とする)が、将来 HDR 素材を扱う際にレッドフラグにならないか
> 4. frame 135 の `policy_overrides` を v1.6.0-32bpc/manifest.toml で
>    現状省略しているが、Step 5 manifest-driven harness 移行時に
>    どのレベルで穴埋めすべきか
> 5. それ以外、AE SDK 仕様や 32bpc f32 の落とし穴で気付いたもの

## 8. レビュー対象外(外部リソースを使わせない範囲)

- 8/16bpc コア演算アルゴリズム(Phase 1 で確定済み、本範囲では touch しない)
- AE SDK そのもののバグ報告(Adobe 側 issue tracker で扱う)
- パフォーマンス最適化(機能 freeze 後に別途)
- shipping パッケージング / コードサイニング / インストーラ

## 9. 連絡・成果物

- レビュー結果は **markdown 1 ファイル**(`tmp/external-review-YYYYMMDD-<reviewer>.md` 等)に
  まとめて返送。Severity(High/Medium/Low)+ ファイル/行番号 + 修正提案の 3 点で。
- 未確定 / 議論したい点は別段 "Open questions" として明示。
- 機密情報や Adobe SDK 添付ファイルの抜粋は外部送信不可
  (`references/AfterEffectsSDK_25.6_61_*` 配下は git に commit していない)。
- 依頼者: Hiroshi Saito <hiroshi@pinapics.com>。
