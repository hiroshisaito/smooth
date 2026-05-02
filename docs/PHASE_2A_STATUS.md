# Phase 2-A Status(進行状況)

常時参照用。各 Step 完了ごとに更新。詳細は [`PHASE_2A_GPU_RFC.md`](PHASE_2A_GPU_RFC.md) + [`workbench_history.md`](../workbench_history.md)。

**現在地**: Phase 2-A.2 Step 1 完了 — Rust `smooth_core` の f32 domain 拡張(`SmoothScalar` trait 抽象 + `Pixel32` + 既存 `<P: SmoothPixel>` ジェネリックそのまま 32bpc 対応)。次は **Phase 2-A.2 Step 2**(Effect.cpp に PF_PixelFloat 分岐追加、`FLOAT_COLOR_AWARE` flag 同期)。

Phase 2-A.3 Sub-stage A / B / C-1(Rust 側)は先行完了済、Phase 2-A.3 の Effect.cpp 統合(Sub-stage C-2)は 2-A.2 完了後。

**Last update**: 2026-05-03(2-A.2 Step 1: SmoothScalar trait 導入 + Pixel32 追加、cargo test 15/15 + 既存 regression 14/14 PASS、overbright/NaN/subnormal 防御 unit tests 追加)。

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
- [ ] **Step 2**: Effect.cpp + Pipl.r に FLOAT_COLOR_AWARE、32bpc regression PASS
- [ ] **Step 3**: Test harness manifest migration、v1.4.0-ae2025 backfill manifest
- [ ] **Step 4**: 32bpc goldens capture、GitHub Release artifact、fetch_goldens.sh
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

## 次のアクション

**Phase 2-A.2 Step 2**(Effect.cpp + Pipl.r 32bpc 統合): `SmartRender()` の bpc switch に `PF_PixelFloat` ケース追加、`smoothing<Pixel32, ...>` インスタンス化 → Rust 側 f32 FFI を呼ぶ。GlobalSetup + Pipl.r `out_flags2` に `PF_OutFlag2_FLOAT_COLOR_AWARE` (bit 12) を OR。

その後の流れ: 2-A.2 Step 3(test harness manifest migration)→ Step 4(32bpc goldens capture)→ Step 5(Mac/Win cross-platform 検証)→ Sub-stage C-2(Effect.cpp の GPU 統合)→ C-2.5(2-pass shader)→ C-3(実機 + fallback test + MFR + GPU stress)→ Sub-stage D / E / F。

Win build は外部の Win 環境で 2-A.1 + 2-A.2 まとめて実施(Phase 2-A.2 close 後 / もしくは Phase 2-A.3 着手前のチェックポイントで)。

## 現時点の PoC(disposable)

- Repo 外: `/Users/hiroshi/Documents/GitHub/smooth-spike-poc/`
- Symlink: `smooth/spike-poc/` → 上記(workspace からクリック可、`.gitignore` 済)
- CHEATSHEET: [spike-poc/observations/CHEATSHEET.md](../spike-poc/observations/CHEATSHEET.md)
- 破棄タイミング: Sub-stage A 完全クローズ時。現時点では残す
