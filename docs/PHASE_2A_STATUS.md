# Phase 2-A Status(進行状況)

常時参照用。各 Step 完了ごとに更新。詳細は [`PHASE_2A_GPU_RFC.md`](PHASE_2A_GPU_RFC.md) + [`workbench_history.md`](../workbench_history.md)。

**現在地**: Phase 2-A.2 Step 4 完了(synthetic 経路) — `tests/synth_32bpc.cpp`(SMDP v1 → SMDP v2 32bpc 変換 + `smooth_core::process<PF_PixelFloat>` 適用)+ `tests/synthesize_32bpc_goldens.sh`(driver、SMOOTH_PARALLEL=0 で deterministic capture)、`tests/goldens/v1.6.0-32bpc/manifest.toml`(synthesized、14 frames)、`regression_test.cpp` の bpc=32 経路を `f32_abs <= 0.125` の f32_diff 判定に拡張。AE プロジェクト・EXR 取得・GitHub Release upload は不要。次は **Phase 2-A.2 Step 5**(Mac/Win cross-platform 検証)。

Phase 2-A.3 Sub-stage A / B / C-1(Rust 側)は先行完了済、Phase 2-A.3 の Effect.cpp 統合(Sub-stage C-2)は 2-A.2 完了後。

**Last update**: 2026-05-03(2-A.2 Step 3: manifest schema v1 + fetch_goldens.sh + manifest-driven runner、harness は behavior 不変、artifact_url は Step 4 で埋める placeholder のまま)。

---

## Phase 2-A 全体構成

| Stage | 範囲 | Steps | 進行 |
|---|---|---|---|
| Step 0 | 設計 RFC 起草 | 1 | ✅ 完了(`74284c6`、`docs/PHASE_2A_GPU_RFC.md` Rev 0.2 → 0.3) |
| Phase 2-A.1 | SmartRender 経路追加 | 2 Steps | ⬜ 未着手 |
| Phase 2-A.2 | 32bpc + manifest 化 | 5 Steps | ⬜ 未着手 |
| Phase 2-A.3 | GPU render + v1.6.0 出荷 | 6 Steps | 🟡 Step 1 部分完了 |

---

## Phase 2-A.1 SmartRender 経路追加(2 Steps)

- ✅ **Step 1**: Effect.cpp + Pipl.r に SmartRender handlers + `SUPPORTS_SMART_RENDER` flag(GlobalSetup + Pipl.r flags2 = 0x08800410)、`smoothing<>()` を SmartRenderInfo ベースに refactor、Mac universal build SUCCEEDED、cargo test 10/10、regression `SMOOTH_PARALLEL=1/0` 両方で 14/14 + synthetic 6/6 PASS
- 🟡 **Step 2**: Mac AE 2025 実機 PASS(§3.1.4 Step 2-4 完了: SmartRender 経路稼働、Render Queue 724 frames 完走、MFR 16 threads engaged、KOJI_SMOOTH thread-safe)、I_WRITE_INPUT_BUFFER 撤去 + scratch 化の 2 番目の修正で verifier failure 解消。**Win 実機検証は別 build 環境で実施予定**(本 commit は Mac side のみ close)
  - **Follow-up メモ**: preview/cache pass で `FrameTask threw 517` × 3 観測(time 69600 / 594400 / 595200)。pre_render_data null の edge case が原因と推定、Render Queue 本体には影響なし。Phase 2-A.2 進行中 or 別 issue で対処

## Phase 2-A.2 32bpc + manifest 化(5 Steps)

- ✅ **Step 1**: Rust `smooth_core` f32 domain 拡張(`SmoothScalar` trait 導入、`SmoothPixel::Scalar` 関連型、`Pixel32` 追加、`smooth_core_preprocess_f32` + `smooth_core_process_row_range_f32` FFI、cargo test 15/15 PASS、既存 8/16bpc regression 非劣化 14/14)
- ✅ **Step 2**: Effect.cpp + Pipl.r `FLOAT_COLOR_AWARE` flag(GlobalSetup + Pipl.r flags2 = 0x08801410)、`detect_pixel_format()` ヘルパで `PF_GetPixelFormat` 取得 → 3 段 bpc dispatch(8/16/32)、`smoothing<>()` を `if constexpr (sizeof==16)` で `range_f32` ブランチ化、`KP_PIXEL128` placeholder 追加、Mac Universal build SUCCEEDED、cargo 15/15 + regression 14/14×{parallel,serial} 非劣化、**Mac AE 2025 実機 3 点確認 PASS(8/16/32bpc Comp 全て ⚠️ 無し + クラッシュ無し、2026-05-03)**。pixel-perfect 32bpc 検証(goldens 比較)は Step 4 へ
- ✅ **Step 3**: Test harness manifest migration — schema v1 を `docs/PHASE_2A_GPU_RFC.md §3.2.6` に従い TOML で確定、`tests/goldens/v1.4.0-ae2025/manifest.toml` backfill(14 frames + suite-level mac_reference / cross_platform policy + frame 135 policy_overrides)、`tests/fetch_goldens.sh` で per-file SHA256 検証(artifact 未 upload 時は integrity check のみ実施)、`tests/run_regression.sh` を manifest-driven 化(glob 廃止)、`.gitignore` を 3 段パターンに更新(親 unignore → 中身 ignore → manifest だけ許可)。regression 14/14 SMOOTH_PARALLEL=1/0 両方 PASS。`artifact_url` は Step 4 で埋める placeholder のまま、harness の tolerance 判定は regression_test.cpp 内のハードコード `diff < 0.01% && max_abs <= 32` を継続(Step 4 で manifest 駆動に置換予定)
- ✅ **Step 4**: 32bpc goldens(synthetic 経路)
  - ✅ **Step 4a (code only)**: SMDP v2 schema(`bench.h::DumpHeader.params_range_f32`)、`tests/regression_test.cpp` を 32bpc 対応(v2 header 読み取り + `smooth_core::process<PF_PixelFloat>` dispatch)、`tests/capture_32bpc.py`(EXR → SMDP v2 converter、HDR 用 alternative path として保持、self-test PASS)、`tests/requirements-capture.txt`(numpy + OpenEXR pin)、tests/README.md に capture 手順追記
  - ✅ **Step 4b (synthetic capture)**: AE / EXR / GitHub Release を経由しない自己完結 path に切り替え。`tests/synth_32bpc.cpp`(v1.4.0 SMDP v1 → SMDP v2 32bpc 変換 + `smooth_core::process<PF_PixelFloat>` 適用、SMOOTH_PARALLEL=0 で deterministic baseline)、`tests/synthesize_32bpc_goldens.sh`(driver、v1.4.0 manifest を walk、14 frames 一括生成 + manifest 自動再生成 + SHA256 自己検証)、`tests/goldens/v1.6.0-32bpc/manifest.toml`(committed、`mac_reference_policy = identical` / `cross_platform_policy = near-id, f32_abs, max_abs=1e-5`)。`regression_test.cpp` の NEAR-ID 判定を bpc-aware 化(8/16bpc は従来 byte_abs<=32、32bpc は新規 f32_abs<=0.125、両者とも diff_pct<0.01 を要求)。背景: 切替理由は (a) v1.4.0 capture .aep が repo 未 commit + frame 135 source 不明、(b) AE project は color depth が global で 8/16bpc 混在を 1 session で再現不可。RFC §3.2.6 の "CPU 32bpc 実装 = reference" 規定に依拠
  - **Step 4 gate 結果**: regression PASS = **28/28** (v1.4.0-ae2025 14/14 + v1.6.0-32bpc 14/14)、SMOOTH_PARALLEL=1 と =0 両方 PASS。frame 135 32bpc PARALLEL=1 は `floats=30/14187776 (0.0002%) max_f32_abs=9.19e-02` で NEAR-ID 判定(8bpc 30/14187776 max_abs=23 と同根の boundary residual の f32 表現)。cargo 15/15 非劣化、Mac plugin Release rebuild SUCCEEDED
  - **Mac 実機 3 点確認(2026-05-03、build = `e6f0a7f` clean)**: (1) About ダイアログで `rust_core 0.1.0+e6f0a7f`(`+dirty` 無し)= **PASS**、(2) 8/16/32bpc Comp 全て ⚠️ 無し + クラッシュ無し + 効果適用正常 = **PASS**、(3) `synthesize_32bpc_goldens.sh` 再実行後 `git status -s` 出力空 = manifest 不変 = synthesize 決定論性確認 = **PASS**
  - 操作詳細: [`docs/CAPTURE_32BPC_RUNBOOK.md`](CAPTURE_32BPC_RUNBOOK.md)(synthetic primary、EXR alternative)。AE 経路用の `tests/capture_32bpc.py` / template / requirements は HDR test material 取得用に temple へ残置
- [ ] **Step 5**: Mac + Win cross-platform 32bpc 検証、§3.2.5 gate 全 YES

## Phase 2-A.3 GPU render + v1.6.0 出荷(6 Steps)

- 🟡 **Step 1 (Sub-stage A)**: §4 Spike 7 項目実測(disposable PoC)
  - ✅ §4.1 MFR serialize 確認(scenario A、16 threads / overlap 0)
  - ✅ §4.4 Part 2 PF_Err(scenario D、AE retry → abort)
  - ✅ §4.4 Part 3 OOM(scenario E、AE GPU Effects dialog でブロック)
  - ✅ §4.4 採用確定 **(i) device→host→device + PF_Err_NONE**
  - 🟡 §4.5 Scenario A のみ(RESETUP 0 件)、B / C 残件
  - 🟡 §4.3 通常設定のみ(device_count=2 Metal)、Software Only 比較(scenario F)残件
  - ⬜ §4.2 CUDA context push/pop(Win PoC 必要、Sub-stage E 後)
  - ⬜ §4.4 Part 1 DPU overhead 実測(Patch C で PoC 拡張 or 本実装中)
  - ⬜ §4.6 Metal storage mode 3 variant 計測(Sub-stage C 本実装中)
  - ⬜ §4.7 checkbox invalidation(Sub-stage D)
- ✅ **Step 2 (Sub-stage B)**: Rust `gpu/` scaffold + GpuBackend trait(`gpu/{mod,cpu,metal,cuda,fallback,detection,tests}.rs` + shader stubs、cargo test 9/9 PASS、既存 regression 非劣化)
- 🟡 **Step 3 (Sub-stage C)**: Mac Metal backend 本実装 + Effect.cpp GPU path + 基本 UI(分割実行中)
  - ✅ **C-1**: Rust MetalBackend + MSL identity passthrough + cargo test で実機 Metal device 上で MSL compile 動作確認
  - ⬜ **C-2**: Effect.cpp 8 selector + Pipl.r flag + PreRender 5 条件 + GPU_FALLEN/UUID FFI bridge + 基本 checkbox stub
  - ⬜ **C-2.5**: shader を 2-pass(detect + blend)smooth に書き換え、`gpu_metal_policy` 許容内で 32bpc goldens regression PASS
  - ⬜ **C-3**: Mac AE 2025 実機 + `SMOOTH_FORCE_GPU_ERROR` injection で fallback テスト + MFR + GPU stress
- ⬜ **Step 4 (Sub-stage D)**: UI DISABLED wiring + GPU 検出機構 + About
- ⬜ **Step 5 (Sub-stage E)**: Win CUDA backend 本実装 + Effect.cpp CUDA path
- ⬜ **Step 6 (Sub-stage F)**: Full UAT + 性能測定 + v1.6.0 配布

---

## Win 着手前 de-risking チェックポイント

「macOS RC まで Win に着手しない」運用ポリシー(Hiroshi さん 2026-05-03 確認)を保ったまま、Win セッション当たりの不確実性を減らすためのチェックポイント。Mac 単独で完了できるものを前倒しで潰し、Win セッションは「設計を変えない実装作業」だけに切り詰める。詳細設計は [`docs/PHASE_2A_GPU_RFC.md §3.3.7`](PHASE_2A_GPU_RFC.md) を参照。

### 前倒し可能タスク(GPU 不要、Win 機さえあればいつでも)

| タスク | 内容 | 所要 | 効果 |
|---|---|---|---|
| **2-A.2 Step 5** | Win で `cargo build --release` + `tests/synthesize_32bpc_goldens.sh` 実行 → Mac committed manifest との f32 比較。AE 操作不要、CUDA 不要 | 1〜2 時間 | cross-platform 32bpc CPU の保証取得、Rust toolchain on Win 動作確認、`cross_platform_policy.f32_abs <= 1e-5` 内に収まるかの一次測定 |

このタスクが PASS した時点で 2-A.2 Phase は正式クローズ、以降の Mac GPU 進行(Sub-stage C-2 / C-2.5 / C-3 / D)は Win-side のリスクから完全に独立する。

### Sub-stage E 着手直前の "design-freeze review" commit(Mac 単独)

Sub-stage C-3 完了 + Sub-stage D 完了後、E 着手の **直前に 1 commit はさむ運用**。レビュー対象は RFC §3.3.7 で固定された下記 4 項目:

1. Rust `GpuBackend` trait surface(CUDA push/pop / async stream / OOM error variant が Metal command buffer / completion handler と同形に収まるか)
2. Rust GPU FFI surface(C 側に露出する struct layout、`smooth_core_version()` で枝番判定可能か)
3. `sequence_data` UUID layout + once-fallen-always-fall fallback policy(platform 中立)
4. error model: `PF_Err` 戻し方、DPU host-process-upload 採用方針(§4.4 採用 (i))の実装位置と、シミュレートする `SMOOTH_FORCE_GPU_ERROR` の hook 点

review 結果は同 commit の本文に「変更なし」または「以下を修正」で残し、Win セッション側は **その commit から先しか触らない** 規約とする。

### Mac 進行中に集積する SDK 仕様ノート

Sub-stage C-2 / C-3 / D で見つかる「PF_Err の戻し方」「PreRender 5 条件」「DPU ハンドラ呼び出し順序」「checkbox invalidation」等を、随時 [`docs/SUB_STAGE_E_HANDOVER.md`](SUB_STAGE_E_HANDOVER.md)(将来作成、初回は Sub-stage C-2 完了時)に追記する。Win セッション開始時はこのファイルが Sub-stage E の playbook として機能する。

---

## 次のアクション

**Phase 2-A.2 Step 5**(Mac/Win cross-platform + 実機 32bpc 検証): Win 環境で smooth_core を build → `tests/synthesize_32bpc_goldens.sh` 実行 → Mac の committed manifest と SHA256 比較。Mac↔Win f32 LSB 差分は manifest の `cross_platform_policy.f32_abs <= 1e-5` で許容範囲内に収まることを確認。実機 32bpc Comp の visual 確認は Step 2 で済(2026-05-03 PASS)。**この task は GPU 着手前の de-risking として早期前倒し推奨**(上記「Win 着手前 de-risking チェックポイント」節参照)。

その後の流れ: Sub-stage C-2(Effect.cpp の GPU 統合)→ C-2.5(2-pass shader)→ C-3(実機 + fallback test + MFR + GPU stress)→ Sub-stage D → **Sub-stage E pre-flight design-freeze review**(RFC §3.3.7、Mac 単独 1 commit)→ Sub-stage E(Win CUDA)→ Sub-stage F。

Win build は外部の Win 環境で 2-A.1 + 2-A.2 まとめて実施(Phase 2-A.2 close 後 / もしくは Phase 2-A.3 着手前のチェックポイントで)。

## 現時点の PoC(disposable)

- Repo 外: `/Users/hiroshi/Documents/GitHub/smooth-spike-poc/`
- Symlink: `smooth/spike-poc/` → 上記(workspace からクリック可、`.gitignore` 済)
- CHEATSHEET: [spike-poc/observations/CHEATSHEET.md](../spike-poc/observations/CHEATSHEET.md)
- 破棄タイミング: Sub-stage A 完全クローズ時。現時点では残す
