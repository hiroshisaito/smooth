# Phase 2-A Status(進行状況)

常時参照用。各 Step 完了ごとに更新。詳細は [`PHASE_2A_GPU_RFC.md`](PHASE_2A_GPU_RFC.md) + [`workbench_history.md`](../workbench_history.md)。

**現在地**: Phase 2-A.3 Step 2(Sub-stage B)完了。次は Step 3(Sub-stage C、Mac Metal 本実装)。
**Last update**: 2026-04-24(Sub-stage B: Rust gpu/ scaffold、cargo test 9/9 + regression 14/14 + synthetic 6/6 PASS)。

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

- [ ] **Step 1**: Effect.cpp + Pipl.r に SmartRender handlers + flag、local regression PASS
- [ ] **Step 2**: Mac + Win AE 実機検証、§3.1.5 gate 全 YES

## Phase 2-A.2 32bpc + manifest 化(5 Steps)

- [ ] **Step 1**: Rust `smooth_core` f32 domain 拡張、cargo test PASS
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
- ⬜ **Step 3 (Sub-stage C)**: Mac Metal backend 本実装 + Effect.cpp GPU path + 基本 UI
- ⬜ **Step 4 (Sub-stage D)**: UI DISABLED wiring + GPU 検出機構 + About
- ⬜ **Step 5 (Sub-stage E)**: Win CUDA backend 本実装 + Effect.cpp CUDA path
- ⬜ **Step 6 (Sub-stage F)**: Full UAT + 性能測定 + v1.6.0 配布

---

## 次のアクション

Sub-stage C(Mac Metal backend 本実装 + Effect.cpp GPU path + 基本 UI stub)に進む。`gpu/metal.rs` を実装、`gpu/mod.rs` の `dispatch_*` メソッドを最終確定、Effect.cpp に 8 selector 追加、GPU_FALLEN の FFI bridge 追加。

RFC §3.3.4 Sub-stage C の 8 項目が gate。

## 現時点の PoC(disposable)

- Repo 外: `/Users/hiroshi/Documents/GitHub/smooth-spike-poc/`
- Symlink: `smooth/spike-poc/` → 上記(workspace からクリック可、`.gitignore` 済)
- CHEATSHEET: [spike-poc/observations/CHEATSHEET.md](../spike-poc/observations/CHEATSHEET.md)
- 破棄タイミング: Sub-stage A 完全クローズ時。現時点では残す
