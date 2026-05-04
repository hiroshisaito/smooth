# Phase 2-A Status(進行状況)

常時参照用。各 Step 完了ごとに更新。詳細は [`PHASE_2A_GPU_RFC.md`](PHASE_2A_GPU_RFC.md) + [`workbench_history.md`](../workbench_history.md)。

**現在地**: Phase 2-A.3 Sub-stage **C-2.5b.2-prep2a** 完了 — Mac 実機 5 点 PASS(build `084b470` clean、ffi=0x00020007)。GPU 経路で **mode_flg=15 ピクセルの corner blend が動作**(初の "GPU で smooth が走る" 状態)+ 警告/`FrameTask 517` 解消(intermediate buffer 廃止 + 単一 kernel `smooth_combined` 化)。次は **Phase 2-A.3 Sub-stage C-2.5b.2-prep2b** 以降(link8_01/02/04、up_mode/down_mode corner、line-level blends)。

Phase 2-A.2(32bpc + manifest 化)は Step 1〜4 完了、Step 5(Mac↔Win cross-platform)は Win セッション待ちで前倒し可能。詳細は §「Win 着手前 de-risking チェックポイント」。

**Last update**: 2026-05-04(C-2.5b.2-prep2a 実機 5 点 PASS、commit `084b470` で intermediate buffer 廃止 + combined kernel 化により AE 警告 / FrameTask 517 解消)。

---

## Phase 2-A 全体構成

| Stage | 範囲 | Steps | 進行 |
|---|---|---|---|
| Step 0 | 設計 RFC 起草 | 1 | ✅ 完了(`74284c6`、`docs/PHASE_2A_GPU_RFC.md` Rev 0.2 → 0.3) |
| Phase 2-A.1 | SmartRender 経路追加 | 2 Steps | ✅ Mac side 完了(Win 検証は Phase 2-A.2 Step 5 と合流) |
| Phase 2-A.2 | 32bpc + manifest 化 | 5 Steps | 🟡 Step 1〜4 ✅、Step 5(Mac↔Win)⬜ |
| Phase 2-A.3 | GPU render + v1.6.0 出荷 | 6 Steps | 🟡 Step 2(Sub-stage B)✅、Step 1(spike)4/7 完、Step 3(Sub-stage C)C-2.5b.1 まで完了 / C-2.5b.2 進行中、Step 4〜6 ⬜ |

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
  - ✅ **C-2**: Effect.cpp 8 selector + Pipl.r flag + PreRender 5 条件 + GPU_FALLEN/UUID FFI bridge + 基本 checkbox stub
    - **C-2a**: Rust GPU plumbing FFI(`smooth_core_gpu_uuid_new` / `mark_fallen` / `is_fallen` / `forget` / `set_backend_usable` / `is_backend_usable` / `should_force_error`)、`uuid` crate 追加、`smooth_core_version()` を 0x0002_0004 に bump、cargo test 19/19 PASS
    - **C-2b**: Effect.cpp に 8 selector(SEQUENCE_{SETUP/RESETUP/FLATTEN/SETDOWN}/GET_FLATTENED_SEQUENCE_DATA/GPU_DEVICE_{SETUP/SETDOWN}/SMART_RENDER_GPU)+ Pipl.r flags2 = 0x0A801410(`SUPPORTS_GPU_RENDER_F32` 追加、Pipl.r/GlobalSetup/GPU_DEVICE_SETUP 3 箇所同期)+ `PARAM_GPU_ACCELERATION` checkbox(default ON、SUPERVISE)+ `SequenceData{uuid_lo,uuid_hi}` PF_Handle で AE 管理 + `SmartPreRender` の 5-condition AND(input bpc=32 / checkbox ON / not fallen / backend usable / DEVICE_SETUP 成功 — C-2 では (d)/(e) を merge、Sub-stage D で分離)+ `SmartRenderGpu` stub、Mac plugin Release BUILD SUCCEEDED、regression 28/28 SMOOTH_PARALLEL=1/0 両方 PASS
    - **C-2 dispatch gate(2026-05-03 実機テスト fail 受け修正、commit `1a07c28`)**: Mac AE 2025 実機で 32bpc Comp に effect 適用すると `internal verification failure: gpu effect world is not supported yet` で plugin crash。原因は `SmartRenderGpu` stub が GPU world(device memory)を `SmartRender` の CPU 経路に fallthrough させ、`PF_GET_PIXEL_DATA16` が GPU world に対して呼ばれたこと。修正: `SMOOTH_GPU_DISPATCH_READY` macro(default 0)を導入、`SmartPreRender` の `GPU_RENDER_POSSIBLE` flag 書き込みを gate、`SmartRenderGpu` 本体も gate で守る(reach 不可だが念のため `PF_Err_INTERNAL_STRUCT_DAMAGED` で即抜け、CPU SmartRender に fallthrough しない)。C-2.5 で実 Metal dispatch が入る時に gate を 1 に flip。GPU plumbing は完全装着、C-2.5 が来るまで dormant という運用
    - **C-2 実機 retest PASS(2026-05-03、build `1a07c28` clean)**: (1) About `rust_core 0.1.0+1a07c28` + ffi=0x00020004 = **PASS**、(2) Effect Controls に `GPU Acceleration (32bpc only)` checkbox 表示 + 操作可能 = **PASS**、(3) **8/16/32bpc Comp 全て crash 無し** + effect 正常適用 + log で `KOJI_SMOOTH thread-safe` + `Render threads used: 2`(MFR 動作)= **PASS**、(4) GPU checkbox toggle の cache invalidation = **観測不能**(C-2 stub では GPU/CPU 両経路が同じ CPU 出力を返すため AE cache が "no change" 判定で re-render skip するのが正常、C-2.5 で GPU shader が違う出力を出すようになれば可視化される)。crash 解消 + plumbing 完全装着 + dormant 状態を確認
  - 🟡 **C-2.5**: GPU 経路 round-trip 完成 + 2-pass smooth shader(分割実行中)
    - ✅ **C-2.5a**: GPU device suite 配線完了。`smooth_core_metal_{create,destroy,dispatch_passthrough}` FFI(Mac only)、`GpuDeviceSetup` で `kPFGPUDeviceSuite` 経由 MTLDevice/MTLCommandQueue 取得 → `MetalBackend` 生成 → handle を `PF_GPUDeviceSetupOutput->gpu_data` に格納、`SmartRenderGpu` で AE round-trip 経由 handle 取得 → `GetGPUWorldData` で MTLBuffer 取得 → identity passthrough kernel dispatch、`SMOOTH_GPU_DISPATCH_READY = 1` に flip。`smooth_core_version` 0x0002_0004 → 0x0002_0005、cargo test 22/22(metal_ffi 単体 3 件追加)、Mac plugin Release BUILD SUCCEEDED、CPU regression 28/28 SMOOTH_PARALLEL=1/0 両方。**32bpc + checkbox ON では shader が identity なので smooth が見かけ上適用されない**(C-2.5b で本物の 2-pass shader が入るまでの一時状態)
    - **C-2.5a 実機 PASS(2026-05-03、build `9cf9c24` clean)**: (1) About `rust_core 0.1.0+9cf9c24` + ffi=0x00020005 = **PASS**、(2) 8/16bpc Comp で smooth 通常適用 = **PASS**(CPU 非劣化)、(3) 32bpc + checkbox ON で **smooth も white_option も両方 bypass** = **PASS**(identity passthrough = preProcess 無し + process 無し = 入力をそのまま出力に書き戻す。両方が無効化されることが GPU 経路稼働の確証 — CPU SmartRender が呼ばれていれば preProcess は走る)、(4) 32bpc + checkbox OFF で CPU SmartRender 経由で smooth + white_option 通常適用 = **PASS**。crash 無し。GPU 経路 round-trip 動作確認、C-2.5b で MSL に preProcess + 2-pass smooth を実装すれば 3 と 4 が視覚同等になる
    - 🟡 **C-2.5b**: smooth.metal を **2-pass(detect + blend)** smooth に書き換え(分割実行中)
      - ✅ **C-2.5b.1**: preProcess kernel を MSL に port。`smooth_preprocess(src, dst, white_opt)` + `MetalBackend::dispatch_preprocess` + `smooth_core_metal_dispatch_preprocess` FFI 追加。Effect.cpp `SmartRenderGpu` を `dispatch_passthrough` から `dispatch_preprocess` に切替、`info->white_option` を kernel に渡す。`smooth_core_version` 0x0002_0005 → 0x0002_0006、cargo test 22/22、regression 28/28、Mac plugin Release BUILD SUCCEEDED。**32bpc + GPU checkbox ON で white_option(transparent)が GPU 経路でも動作**するようになった(C-2.5a の "両方 bypass" 状態から半歩進む)。smooth blend は依然 identity(C-2.5b.2 で実装)
      - **C-2.5b.1 + prep1 + blending_pixel_f groundwork 実機 5 点 PASS(2026-05-03、build `fea2c8c` clean)**: (1) About `rust_core 0.1.0+fea2c8c` + ffi=0x00020006 = **PASS**、(2) 8/16bpc CPU 非劣化 = **PASS**、(3) **32bpc + GPU ON + transparent ON で white が透明化 + smooth blend は見かけ上不適用** = **PASS**(preprocess kernel が AE の GPU BGRA128 world に対して white-key strip を正しく適用、prep1 の detect kernel と blending_pixel_f は production path 未統合で影響無し)、(4) 32bpc + GPU ON + transparent OFF で出力=入力(identity copy)= **PASS**、(5) **32bpc + GPU OFF で smooth + white_option 両方適用**(CPU SmartRender 経由)= **PASS** — 5-condition AND の (b) checkbox=OFF で GPU 経路 gate 動作確認。crash 無し
      - 🟡 **C-2.5b.2**: 実 smooth blend を MSL に port(分割中)
        - ✅ **C-2.5b.2-prep1**: compare 関数群を MSL device function に port(`pixel_delta_sum` / `compare_pixel` / `compare_pixel_equal` / `fast_compare_pixel`、`compare.rs` と等価)+ `smooth_detect` kernel(per-pixel mode_flg byte を中間 buffer に書く、`process_row_range` 冒頭の `fast_compare → 4-cardinal compare → mode_flg` ロジックを mirror)+ `MetalBackend::pipeline_detect` + `dispatch_detect`(返却値が modes Buffer)+ unit test 2 件(tight range で `(1,1)` が `0x8F`、loose range で `0x80` のみ)。**Effect.cpp は未変更**(prep2 で blend kernel と一緒に `dispatch_smooth_chain` で連鎖統合する設計)、cargo test 24/24、regression 28/28 不変
        - ✅ **C-2.5b.2-prep2a**: `smooth_blend` MSL kernel(mode_flg=15 = `link8_square` の中心 pixel 4-corner 加重平均のみ実装、他 mode は identity copy で fall-through)+ `MetalBackend::pipeline_blend` + Effect.cpp `SmartRenderGpu` を `dispatch_preprocess` から smooth chain 経路に切替。`smooth_core_version` 0x0002_0006 → 0x0002_0007、cargo 24/24、regression 28/28、Mac plugin Release BUILD SUCCEEDED
        - **C-2.5b.2-prep2a 実機テストで判明した問題と修正**(2026-05-04、commit `8001aca` → `084b470`):
          - **症状**: 32bpc + GPU ON で AE 警告 "smooth did not render anything. Transparent pixels will be rendered." + log で `FrameTask threw 517` × 複数。当初 chain 設計(preprocess → inter / detect → modes / blend → dst の 3-pass で per-call StorageModePrivate buffer 確保)が原因
          - **根本原因**: per-call で width×height×16 byte の intermediate buffer 確保、MFR で 5 thread 並行 × 4K で 0.6 GB の per-call memory pressure。AE 側 gpu_suite tracker の管轄外なので、AE が GPU world synchronisation で `dst` を読みに行ったタイミングで「未書込」と判定されて警告発火
          - **修正(commit `084b470`)**: preprocess + detect + blend を **1 つの MSL kernel `smooth_combined` に inline** + `load_strip` device function で各 read 時に white-key strip を即時適用。intermediate buffer **完全廃止**、`cb.wait_until_completed()` も不要化(削除済)、各 thread は src から最大 9 read + dst に 1 write のみ
          - **実機 5 点 PASS(build `084b470` clean)**: (1) About `rust_core 0.1.0+084b470` ffi=0x00020007 = **PASS**、(2) 8/16bpc CPU 非劣化 = **PASS**、(3) **32bpc + GPU ON + transparent ON: 警告なし + FrameTask 517 なし + white 透明化 + corner blend 動作** = **PASS**(prep2a の本来の意図が達成、memory pressure 問題解消)、(4) GPU ON + transparent OFF で identity copy = **PASS**、(5) GPU OFF で CPU 通常動作 = **PASS**
          - **学び**: AE GPU 経路では metal-rs `device.new_buffer()` ベースの intermediate は AE の synchronisation 視野外。後続 line-level blend(thread 間 write 競合あり)が必要になった時点で multi-pass が必須となるため、その時は `gpu_suite->AllocateDeviceMemory` 経由に切替える方針(ハンドオーバ note: `docs/SUB_STAGE_E_HANDOVER.md` 候補項目)
        - ⬜ **C-2.5b.2-prep2b**: `link8_square_blend_outside` の line-level blend を data-parallel 化(各 thread は自分の pixel しか書かない設計)。`count_length_two_lines` を MSL device function に、`link8_square` 完全実装
        - ⬜ **C-2.5b.2 残り**: link8_01/02/04(mode_flg 7/11/13)→ up_mode_corner(mode_flg 3)→ down_mode_corner(mode_flg 5)→ lack mode → 突起 mode3。各 ~50〜100 LOC の MSL に落ちる予定だが、up/down mode は spatial extent scan(行内可変長)があるので serial scan を避ける形に再設計が要る
    - ⬜ **C-2.5c**: regression manifest に `gpu_metal_policy` field 追加、`v1.6.0-32bpc` の goldens に対する Mac Metal output が `gpu_metal_policy` 許容内 PASS
  - ⬜ **C-3**: Mac AE 2025 実機 + `SMOOTH_FORCE_GPU_ERROR` injection で fallback テスト + MFR + GPU stress
    - **GPU メモリ不足時の回避策・安全策**(2026-05-04 Hiroshi さん指示で要検討バックログ): C-3 / D / F のいずれかで対処予定。`SMOOTH_FORCE_GPU_ERROR=oom` 注入による once-fallen-always-fall fallback の実機検証(C-3)、`MTLDevice::recommendedMaxWorkingSetSize` ベースの pre-emptive GPU 経路 decline(D)、4 GB GPU で 4K MFR フル稼働の UAT 検証(F)。算出根拠は workbench_history.md「GPU メモリ要件算出」節参照
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

**Phase 2-A.3 Sub-stage C-2.5b.2-prep2**(Mac、最大の山): link8_square blend(mode_flg=15)を MSL kernel 化 + `dispatch_smooth_chain`(preprocess → detect → blend)を Rust + FFI に追加 + Effect.cpp `SmartRenderGpu` を `dispatch_preprocess` から `dispatch_smooth_chain` に切替。`count_length_two_lines` の per-thread serial scan を MSL に port するのが要点。GPU 出力が CPU 経路と byte-equivalent でなくとも near-id(後段 C-2.5c で `gpu_metal_policy` 合意)で吸収する設計。

その後の流れ: C-2.5b.2 残り(link8_01/02/04 → up_mode/down_mode corner → lack mode → 突起 mode3、各 ~50〜100 LOC ずつ)→ **C-2.5c**(`gpu_metal_policy` を manifest に追加 + Mac Metal output が CPU goldens と near-id 一致確認)→ **C-3**(実機 + `SMOOTH_FORCE_GPU_ERROR` 注入で fallback test + MFR + GPU stress)→ **Sub-stage D**(UI DISABLED + GPU 検出 + About)→ **Sub-stage E pre-flight design-freeze review**(RFC §3.3.7、Mac 単独 1 commit)→ **Sub-stage E**(Win CUDA、別 Win 環境)→ **Sub-stage F**(Full UAT + 性能測定 + v1.6.0 配布)。

**前倒し可能タスク(Phase 2-A.2 Step 5)**: Win 環境が取れた時点で `cargo build --release` + `tests/synthesize_32bpc_goldens.sh` 実行 → Mac の committed manifest との f32 比較。AE 操作不要、Sub-stage E より前のいつでも消化可能。詳細は §「Win 着手前 de-risking チェックポイント」。

## 現時点の PoC(disposable)

- Repo 外: `/Users/hiroshi/Documents/GitHub/smooth-spike-poc/`
- Symlink: `smooth/spike-poc/` → 上記(workspace からクリック可、`.gitignore` 済)
- CHEATSHEET: [spike-poc/observations/CHEATSHEET.md](../spike-poc/observations/CHEATSHEET.md)
- 破棄タイミング: Sub-stage A 完全クローズ時。現時点では残す
