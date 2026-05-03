# Phase 2-A 設計 RFC: GPU 対応(SmartRender + 32bpc + Metal/CUDA)

## 0. Status / 改訂履歴

- **Status**: Draft
- **前提 doc**: [`docs/PHASE_2A_GPU_RESEARCH.md`](PHASE_2A_GPU_RESEARCH.md) @ `66a139f`(review rounds 1-5 経由で確定)
- **対象リリース**: v1.6.0(2-A.1 + 2-A.2 + 2-A.3 を合算して 1 本のリリースとして出荷、フォールバックで v1.5.2 / v1.6.0-32bpc-only もあり得る)
- **前段リリース**: v1.5.1(MFR + build-id UI、CPU-only、`981a795`)

| Rev | Date | 内容 |
|---|---|---|
| 0.1 | 2026-04-23 | 初版 Draft(研究 doc から設計決定を移送、spike 項目を独立章に集約、Step 粒度のタスク分解) |
| 0.2 | 2026-04-23 | external review rounds 1-2 反映済み(文言整合 fix 17 件)。Status: Draft → Under Review |
| 0.3 | 2026-04-24 | Sub-stage A 部分観測を §4.1 / §4.3 / §4.4 / §4.5 に追記(PoC scenario A / D / E、Mac Intel / AE 25.6.5x3)。**§4.4 採用分岐確定 = (i) device→host→device + `PF_Err_NONE`**(AE は PF_Err retry 後 job abort、OOM は user-visible dialog でブロックするため (ii) は両方とも採用不可)。§4.1 (A) Serialize 成立、§4.5 scenario A のみ観測済(B / C は残件、Sub-stage B 以降で実施可) |

## 1. Summary

### 1.1 目的

smooth プラグインを **Mac Metal + Win CUDA の full-GPU plugin** に拡張する。Phase 2-B v1.5.1 で確立した CPU-MFR 契約を壊さずに、GPU 経路を積み増す。

### 1.2 スコープ

| 含む | 含まない(defer) |
|---|---|
| Mac Metal backend(native) | Windows DX12 backend(Phase 2-A.4 以降 or 無期限保留) |
| Windows CUDA backend(NVIDIA 専用、静的リンク) | AMD discrete Windows / Intel Arc 対応 |
| SmartRender 三本化(`SMART_RENDER` / `SMART_RENDER_GPU` / 既存 `RENDER` 後方互換) | wgpu / Vulkan / OpenCL(議論終了、復活なし) |
| 32bpc(f32)対応 + 新規 goldens 取得 | 32bpc を GPU 化せずに出荷するパターン(= 下準備のみ)は fallback 案としてのみ保持 |
| GPU Acceleration checkbox 1 個(default ON、GPU 非対応時は `PF_ParamFlag_DISABLED` で静的グレイアウト) | GPU 強制モード(失敗時エラー表示)、Auto/CPU/GPU 3 値 popup |
| once-fallen-always-fall per SETUP/RESETUP 区間の fallback policy | effect instance 全寿命 sticky、プロジェクト保存時の永続化 |

### 1.3 出荷形態

- **成功パス**: 2-A.1 + 2-A.2 + 2-A.3 を合算して **v1.6.0 GPU-accelerated** として 1 本リリース
- **Fallback パス**(GPU 実装が行き詰まった場合): 版数は **v1.5.2 または v1.6.0 32bpc-only のいずれか、§5.1 の出荷ゲートで確定**(RFC 採択時点では未決)
  - 原則: 2-A.1 + 2-A.2 の成果を組み込んだ `v1.6.0 32bpc-only` を default
  - `v1.5.2`(SmartRender のみ)は例外: 2-A.2 が不成立な場合のみ §5.1 で例外判断(2-A.1 / 2-A.2 個別リリースを原則禁止している方針からの意図的な例外)
  - **片 platform GPU only**(Mac Metal or Win CUDA の片方のみ成功)は例外選択肢: もう一方が明確に詰んだ場合のみ user confirm で採用、default ではない
- 2-A.1 / 2-A.2 単独リリースは **原則なし**(上記 fallback の例外判断を除く)

### 1.4 非目標

- ピーキーな GPU モデル専用最適化(特定 Apple Silicon 世代 / 特定 NVIDIA compute capability 専用 shader 等)
- GPU 経路でのアルゴリズム変更(numerical result は CPU と視覚上無差別、byte-identical は非要求)
- 新 UI の足し込み(checkbox 1 個に留める)

## 2. 確定事項(研究 doc から固定、本 RFC では再議論しない)

**運用ルール**: 以下 6 項目は [`PHASE_2A_GPU_RESEARCH.md`](PHASE_2A_GPU_RESEARCH.md) の review rounds 1-5 で決着済み。本 RFC のレビューでは再議論の対象外とする。再議論は **SDK 契約上の制約・実装不能・UAT 観測不整合のいずれかが発生した場合のみ** 許容し(SDK が想定していた host behavior と実機挙動が乖離するケースを含む)、その際は研究 doc 側に戻して別 PR で扱う。好みや代替案の再提案は受け付けない。RFC レビューは §3 以降に集中する。

### 2.1 ステージ分割と出荷方針

| ステージ | 範囲 | 単独リリース |
|---|---|---|
| Phase 2-A.1 | SmartRender 経路追加(legacy `PF_Cmd_RENDER` 残しつつ `PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` を実装) | なし |
| Phase 2-A.2 | 32bpc 対応(f32 domain 拡張 + 32bpc goldens 新規取得) | なし |
| Phase 2-A.3 | GPU render 実装(Mac Metal + Win CUDA) | あり(v1.6.0) |

根拠: 研究 doc §「実装ステージ分割」、§「GPU 実装失敗時の fallback リリース計画」。

### 2.2 Framework 選定

- **Mac**: Metal native のみ(`metal-rs` / `objc2-metal`)
- **Windows**: CUDA のみ(NVCC build-time static link + Rust extern "C"、SDK サンプル準拠。`cudarc` crate は kernel launch には使わず、device query 等の補助用途に任意で使用可、詳細は §3.3.1 / §3.3.6)
- **DX12 defer の根拠**: memory-bandwidth bound アルゴリズムで iGPU は CPU-MFR と同等 or 劣る(4K 16bpc で iGPU 実効 10-20 ms vs CPU-MFR 33 ms、divergence ペナルティで逆転のリスク)。AMD discrete Win は pro video では少数派。Adobe 自身も主力は CUDA + Metal の 2 本柱
- **wgpu / Vulkan 不採用**: AE が native handle を渡す設計に対して wgpu/Vulkan は自前 device 管理前提、CPU↔GPU memcpy が毎フレーム発生し GPU 化の恩恵が目減り

根拠: 研究 doc §「GPU framework 確定」、§「不採用(確定)」、§3.1-§3.7。

### 2.3 Fallback policy

**Once-fallen-always-fall、scope は per SETUP/RESETUP 区間**(effect instance 全寿命 sticky ではない)。

- バッチ書き出し 1 回の中で boundary residual artifact を避ける、が本来の保証
- `SEQUENCE_RESETUP`(save/load / duplicate / in_data 変更)で UUID を再生成するため、RESETUP 契機で fallen 状態は自然にリセットされる
- 単一 Render Queue 書き出し中に user が params を触ることは通常ない前提(§4.5 で spike 検証)

根拠: 研究 doc §4.6。

### 2.4 2 層分離データ構造

```rust
// 1. sequence_data(render 時 read-only、per-instance unique)
#[repr(C)]
struct SmoothSequenceData {
    version: u32,
    instance_uuid_hi: u64,  // u128 を 2 × u64 に分割(FFI 互換)
    instance_uuid_lo: u64,
}

// 2. plugin-global(プロセス生存期間のみ、thread-safe)
static GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>> = Lazy::new(DashMap::new);
```

契約レベルで押さえるべき 4 点(API 詳細は §6.1 に寄せる):

- sequence_data への書き込みは lifecycle selector(SETUP / RESETUP / SETDOWN / CHANGED)のみ、render 時は read-only
- `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` は不採用(MFR 並列度低下を避ける + 「span 境界で discard」仕様が sticky 要件と合わない)
- `SEQUENCE_RESETUP` は flattened UUID を参照せず毎回新規生成(duplicate 時の UUID 衝突回避)
- RESETUP / SETDOWN の thread-affinity は前提しない(AE_Effect.h L1123 / L1140)、全て thread-safe な構造で扱う

根拠: 研究 doc §4.6、§6.5、round 2-5 での詰め。

### 2.5 UI

- **パラメータ**: `GPU Acceleration` checkbox 1 個を追加、default **ON**
- **意味**: ☑ = Auto(GPU 試す、失敗時 CPU) / ☐ = CPU 固定
- **GPU 非対応システム**: `PF_ParamFlag_DISABLED` を **param 登録時に静的に立てる**(動的 UI 更新は使わない)
- **検出**: `PF_Cmd_GLOBAL_SETUP` で 1 度だけ、plugin-global static にキャッシュ。検出ソース(`GetDeviceCount` vs OS API 直接)は §4.3 の spike で確定

根拠: 研究 doc §5.2-§5.3.1、§5.4。

### 2.6 Reference 実装

AE SDK 同梱の `Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp`(1210 行、full GPU plugin の canonical reference)を **雛形として 80% そのまま流用**。

smooth 側で追加する差分(研究 doc §6.3):
- CPU side: `iterate` callback ではなく自前 loop(pixel-independent でないため)
- 2-pass(検出 → blending)で中間 buffer 必須、shader が長い
- sequence_data を UUID 格納に使う(サンプルは未使用)
- GPU error 時の返却方式(`PF_Err_NONE` + CPU fallback / `PF_Err` + 次 frame 以降 CPU 固定)は **§4.4 Spike で確定**(サンプルは `PF_Err` をそのまま AE に返す実装、smooth の最終方式は spike 結論で決定)

根拠: 研究 doc §6.2-§6.7。

---

## 3. ステージ別 計画

各ステージを `[スコープ / 成果物 / 成功条件(ハード要件) / 検証手順 / 次ステージ進行判断基準]` で揃える。単独リリースしないステージ(2-A.1 / 2-A.2)は **次ステージに進んでよいか** のゲートのみ評価、ユーザー公開は行わない。

### 3.1 Phase 2-A.1 SmartRender 経路追加

#### 3.1.1 スコープ

Phase 2-B まで smooth は legacy `PF_Cmd_RENDER` のみ実装。GPU 経路(`PF_Cmd_SMART_RENDER_GPU`)の前段階として **SmartRender 三本化の CPU 側 2 本を先に実装する**。GPU 側は 2-A.3 まで着手しない。

**含む**:
- `PF_OutFlag2_SUPPORTS_SMART_RENDER` を `GlobalSetup` で追加
- `PF_Cmd_SMART_PRE_RENDER` ハンドラ新規実装(入力 layer の checkout、`result_rect` / `max_result_rect` 設定、`pre_render_data` に params スナップショット)
- `PF_Cmd_SMART_RENDER` ハンドラ新規実装(`checkout_layer_pixels` → 既存 `process()` call、8bpc + 16bpc)
- 既存 `PF_Cmd_RENDER` ハンドラ残置(SmartRender 非対応 AE 向けの後方互換、コード変更なし)

**含まない**(スコープ外、後続ステージで扱う):
- 32bpc(Pixel32 / f32 domain、Phase 2-A.2)
- GPU 関連一式(`SMART_RENDER_GPU`、`GPU_DEVICE_SETUP`、Metal / CUDA backend、Phase 2-A.3)
- sequence_data 機構(UUID / `SEQUENCE_SETUP` / `RESETUP` / `FLATTEN` / `SETDOWN`、Phase 2-A.3 で GPU fallback 機構と同時導入)
- GPU Acceleration checkbox と `PF_ParamFlag_DISABLED`(Phase 2-A.3)

#### 3.1.2 成果物

| カテゴリ | ファイル | 変更内容 |
|---|---|---|
| Effect main | [Effect.cpp](../Effect.cpp) | `EffectMain` switch に `PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` case 追加、`SmartPreRender()` / `SmartRender()` 関数新設 |
| Flags(GlobalSetup) | [Effect.cpp](../Effect.cpp) `GlobalSetup` | `out_flags2` に `PF_OutFlag2_SUPPORTS_SMART_RENDER` を OR |
| Flags(PiPL) | [Pipl.r](../Pipl.r) `AE_Effect_Global_OutFlags_2` | 同 bit を v1.5.1 時点の現値(`0x08800010` = I_AM_THREADSAFE + SUPPORTS_GET_FLATTENED_SEQUENCE_DATA + SUPPORTS_THREADED_RENDERING)に OR して更新。後続 stage(2-A.2 / 2-A.3)でさらに bit が加算されるため、最終値は各 stage 実装時に再計算。GlobalSetup と PiPL の flag 値は一致必須、MFR 対応時にも同期ルール確立済み、既存 comment `must match Effect.cpp GlobalSetup out_flags2` を更新 |
| 既存 flag | [Effect.cpp](../Effect.cpp) / [Pipl.r](../Pipl.r) | `I_WRITE_INPUT_BUFFER` / `DEEP_COLOR_AWARE` / `I_AM_THREADSAFE` / `SUPPORTS_THREADED_RENDERING` / `SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` は維持(既存 flag は MFR 維持のため残すが、UUID を持つ sequence_data lifecycle 実装は 2-A.3 まで入れない。両者を混同しないこと) |
| テスト | 既存 `tests/` | 変更なし(既存 regression suite で通過を確認、新規テスト追加なし) |
| Rust crate | `rust/smooth_core/` | **変更なし**(`process_row_range` の CPU 本体は触らない、SmartRender から呼ぶ入口が増えるだけ) |

#### 3.1.3 成功条件(ハード要件、全て PASS)

1. **ビルド成功**: Mac Xcode + Windows MSVC の両 toolchain で warning 無し、既存 `smooth.plugin` / `smooth.aex` と同等のバイナリが生成される(サイズ ±10% 以内、偽成功検証 3 段クリア)
2. **AE が SmartRender 経路を選ぶ**: AE 2025 上で smooth を適用した時、`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` が呼ばれる(debug-only temporary instrumentation で一度確認すれば足りる、PR merge 前に instrumentation は削除)
3. **画質保持**: v1.5.1 goldens に対する regression で 8bpc + 16bpc 全 14 フレーム IDENTICAL or NEAR-ID(既存の NEAR-ID 1 件 = frame 135、2512×1412、30/14187776 bytes、max_abs=23、Phase 1 baseline 一致の continuity として許容。新規 NEAR-ID が出たら不合格)
4. **MFR 保持**: `SUPPORTS_THREADED_RENDERING` 継続有効を一次証跡で確認(Mac: Multithreaded render report ログに出現、Windows: aerender.exe stdout で Thread-safe 宣言 / Render threads used 行を確認。黄色 ⚠️ アイコンは 32bpc 非対応マーカーであって MFR 警告ではないため、2-A.1 の判定材料に使わない)
5. **後方互換**: 既存 `PF_Cmd_RENDER` ハンドラがコード上残置、legacy 経路が削除されていない
6. **MFR regression なし**: `tests/run_regression.sh` を `SMOOTH_PARALLEL=1` / `SMOOTH_PARALLEL=0` 両方で PASS、`cargo test --release` で 3/3 PASS

#### 3.1.4 検証手順

**Step 1: Rust 層 + 既存テスト**

```bash
cd rust/smooth_core && cargo test --release                 # 3/3 PASS
SMOOTH_PARALLEL=1 tests/run_regression.sh                    # 14/14 + synthetic 6/6
SMOOTH_PARALLEL=0 tests/run_regression.sh                    # 14/14 + synthetic 6/6
```

**Step 2: Mac AE 2025 実機テスト**

1. `xcodebuild -project Mac/smooth.xcodeproj -configuration Release -arch x86_64 -arch arm64 ONLY_ACTIVE_ARCH=NO clean build`
2. `Mac/build/Release/smooth.plugin` を AE プラグインフォルダへ配置
3. v1.5.1 UAT プロジェクト(既存テスト素材、8bpc + 16bpc comp)を開く
4. Effect Controls で smooth を適用、UI 操作で応答・クラッシュなし
5. Render Queue で 8bpc + 16bpc それぞれ書き出し、v1.5.1 output と視覚上無差別であることを確認
6. Render log に Multithreaded render report が出ていること(MFR が壊れていない一次証跡)
7. `Effect Controls > smooth > Build` に `0.1.0+<HEAD SHA>` 表示、クリックで About ダイアログが開く(既存 UI の regression がないこと)

**Step 3: Windows AE 2025 実機テスト**

1. MSVC v143 + Rust stable 1.95.0 + `+crt-static` で build(既存 build 手順踏襲)
2. `smooth.aex` を AE プラグインフォルダへ配置
3. 同上の UAT プロジェクトで 8bpc + 16bpc 書き出し(UI 目視で応答・クラッシュなし)
4. **一次証跡**: `aerender.exe` で CLI 書き出しを実行し stdout を収集、`Thread-safe` 宣言行と `Render threads used` 行(または同等の並列動作表示)を確認。本運用は Phase 2-B Windows MFR 検証で確立済み([`workbench_history.md`](../workbench_history.md) L1135 付近「Windows 固有の発見 — Multithreaded render report が GUI Render Queue ログに出ない」参照)
5. **補助資料のみ**: GUI progress バーの並列更新目視は一次証跡ではなく、stdout が取れない環境での診断補助として扱う

**Step 4: SmartRender 経路到達の確認(debug-only temporary instrumentation)**

実装中のみ `SmartPreRender()` / `SmartRender()` / 既存 `Render()` の先頭に一時的な instrumentation を仕込み(手段は問わない、`fprintf(stderr, ...)` / `DebugOutputString` 等)、AE 実機で Render Queue 実行時に SmartRender 側のみが呼ばれることを一度だけ事実確認する。運用機能ではないため assert 化や環境変数化は不要、PR merge 前に instrumentation は削除する。

#### 3.1.5 次ステージ(2-A.2)進行判断基準

以下 **すべて YES** で 2-A.2 に進む:

- [ ] §3.1.3 の 6 項目すべて PASS
- [ ] SmartRender 経路が実機で呼ばれることを一度確認済み(debug-only instrumentation、PR merge 前に削除済み)
- [ ] `GlobalSetup` と `Pipl.r` の `out_flags2` が同期済み(SUPPORTS_SMART_RENDER 含む、Effect.cpp comment `must match ...` も更新)
- [ ] Mac + Win の両プラットフォームで画質・MFR ともに v1.5.1 と同等
- [ ] `workbench_history.md` に 2-A.1 の Step エントリ完備

NO が 1 つでもあれば、以下のいずれかを実行:

- **実装バグ起因**: 修正して同 Step 内で再検証
- **SDK 仕様誤解起因**: [`PHASE_2A_GPU_RESEARCH.md`](PHASE_2A_GPU_RESEARCH.md) §4.8 を再読、内部理解を修正して同 Step 内で再検証(§2 運用ルールの trigger には該当しない、研究 doc 側差し戻しは不要)
- **SDK 契約上の制約 / 実装不能 / UAT 観測不整合**: §4 Spike の既存項目に類似があるか確認、なければ spike 項目を追加(§2 運用ルール trigger、研究 doc 側に戻す判断が必要)

#### 3.1.6 スコープ外の補足(実装者向けメモ、2-A.2 以降で扱う)

- `SmartPreRender` の `result_rect`: smooth の出力 bbox は preprocess 後にしか確定しないため、**conservatively 入力 rect と同じサイズを返す**(研究 doc §4.8、AE は余分な領域を render することはない)
- `pre_render_data` に格納: `range` / `line_weight` / `white_option` の 3 値(heap-allocated struct、`delete_pre_render_data_func` で解放)。GPU Acceleration checkbox の値は 2-A.3 で追加
- `PF_OutFlag_I_WRITE_INPUT_BUFFER` は legacy flag だが SmartRender でも意味は保つ(in-place 更新可という plugin 側の宣言)。`SmartRender` 実装時に input buffer を書き換えていないことを確認

### 3.2 Phase 2-A.2 32bpc 対応(f32 domain 拡張 + goldens 新規取得)

#### 3.2.1 スコープ

smooth_core のピクセル型を `u8` / `u16` に加えて `f32`(`PF_PixelFloat`、AE の 32bpc 世界、alpha + RGB 各 f32、0.0〜1.0 domain で overbright 許容)に拡張する。GPU 化の前提として必要: `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` (2-A.3) は 32bpc 対応を要求する。

本ステージは diff が大きいので **1 stage を複数 Step に分ける前提**で進める(§7 で Step 分解)。想定内訳: (a) Rust core f32 拡張、(b) Effect.cpp + Pipl.r flag 同期、(c) test harness manifest migration、(d) 32bpc goldens capture、(e) Mac/Win cross-platform validation。各 Step ごとに `workbench_history.md` エントリを切る。

**含む**:
- smooth_core Rust 側の f32 domain 拡張(`delta_sum` → `f32`、`max_value` → `1.0f32`、blending を f32 乗除、`range` 内部換算の bpc 別分岐、研究 doc §2.3 採用案)
- `Effect.cpp` SmartRender ハンドラの `PF_PixelFloat` 分岐追加(既存 `PF_Pixel8` / `PF_Pixel16` と並置)
- `PF_OutFlag2_FLOAT_COLOR_AWARE` を `GlobalSetup` で追加(SDK サンプル準拠、研究 doc §6.2)
- **Test harness manifest 化**: `tests/goldens/*/manifest.json` を導入し、frame 一覧・bpc・per-frame tolerance policy・synthetic test 一覧を明示的に記述。既存 `tests/goldens/v1.4.0-ae2025/` にも backfill manifest を置き、8/16bpc の frame 135 NEAR-ID 例外を manifest 上で表現(コード内ハードコード `diff < 0.01% && max_abs <= 32` を manifest 駆動へ置換)
- 32bpc goldens `tests/goldens/v1.6.0-32bpc/` 新規取得: 既存 14 frames と**同じシーンを 32bpc native で再収録**、Mac AE 2025 で capture、Win は cross-platform segmented 判定で検証のみ(IDENTICAL primary / near-ID fallback、§3.2.3 条件 5)
- `tests/compare_raw.py` / `tests/regression_test.cpp` を Pixel32 認識に拡張、32bpc 比較は manifest の `{metric: "f32_abs", max_abs: <num>}` policy で判定(SMDP header は既存通り `bpc` フィールドで判別、schema 詳細は §3.2.6)

**含まない**(2-A.3 で扱う):
- GPU 経路一式(`SMART_RENDER_GPU` / `GPU_DEVICE_SETUP` / Metal / CUDA backend / `SUPPORTS_GPU_RENDER_F32` flag)
- sequence_data 機構(UUID / `SEQUENCE_SETUP` / `RESETUP` / `FLATTEN` / `SETDOWN`)
- GPU Acceleration checkbox と `PF_ParamFlag_DISABLED`
- 32bpc overbright(>1.0)/ HDR シーンの新規テスト素材追加(sample 収集コストを理由にスコープ外、2-A.3 以降で 32bpc 価値を示す必要が出た時に検討)

#### 3.2.2 成果物

| カテゴリ | ファイル | 変更内容 |
|---|---|---|
| Rust core | [rust/smooth_core/src/types.rs](../rust/smooth_core/src/types.rs) 他 | `SmoothPixel` trait を f32 対応に拡張、`PixelType` / `delta_sum` / `max_value` / blending の f32 化、`range` の内部換算を `u32` → associated type で bpc 別分岐(u8: u32、u16: u32、f32: f32) |
| Rust core FFI | [rust/smooth_core/src/lib.rs](../rust/smooth_core/src/lib.rs) | `smooth_core_preprocess_f32` / `smooth_core_process_row_range_f32` 新設、既存 `u8` / `u16` 版と並置 |
| Effect main | [Effect.cpp](../Effect.cpp) | `SmartRender` の bpc switch に `PF_PixelFloat` ケース追加、`GlobalSetup` の `out_flags2` に `PF_OutFlag2_FLOAT_COLOR_AWARE` を OR |
| Flags(PiPL) | [Pipl.r](../Pipl.r) `AE_Effect_Global_OutFlags_2` | 同 bit を OR して更新(GlobalSetup と PiPL の flag 値は一致必須、§3.1.2 と同じ同期ルール) |
| Test tooling | [tests/compare_raw.py](../tests/compare_raw.py) | manifest 読み込み + Pixel32 (4×f32 LE) 比較サポート、tolerance policy を per-frame で解決 |
| Test tooling | [tests/regression_test.cpp](../tests/regression_test.cpp) | Pixel32 比較パス追加、NEAR-ID tolerance を manifest 駆動に置換(既存の `diff < 0.01% && max_abs <= 32` は manifest の `v1.4.0-ae2025` side へ移動) |
| Test tooling | [tests/run_regression.sh](../tests/run_regression.sh) | glob 駆動 → manifest 駆動へ置換、32bpc golden directory を新たに enumerate |
| **Goldens 配置方針**(新規決定、LFS 不使用) | [.gitignore](../.gitignore) 更新 + 外部 artifact | 現時点で `tests/goldens/v1.4.0-ae2025/` は既に **502 MB**(16bpc frame 135 単体で 14 MB)、32bpc 版は f32 化で概算 1 GB。通常 git には**不適**と確定。**採用方針**: 生 `.raw` は repo 外 artifact(GitHub Release assets に `goldens-v1.4.0-ae2025.tar.zst` / `goldens-v1.6.0-32bpc.tar.zst` として添付、tag = smooth release tag)、repo には **manifest.json + 期待 SHA256 + fetch/verify スクリプト**のみ commit。`.gitignore` パターンは「親ディレクトリを unignore → 中身は ignore → manifest だけ許可」の順で記述:<br>`!/tests/goldens/`<br>`/tests/goldens/**`<br>`!/tests/goldens/*/`<br>`!/tests/goldens/*/manifest.json`<br>(`!tests/goldens/**/manifest.json` 単独では親が ignore されていると Git が下位を探索しない) |
| Goldens 取得スクリプト | `tests/fetch_goldens.sh` (新規) | manifest.json を読み、GitHub Release(または manifest 内で指定した URL)から該当 artifact を DL、tar 展開、展開後に manifest 記載の per-file SHA256 を検証。hash 不一致は exit non-zero。CI / new dev onboarding で 1 コマンド実行 |
| Goldens manifest(既存 backfill) | `tests/goldens/v1.4.0-ae2025/manifest.json` (新規、commit) | 既存 14 frames の bpc / size / SHA256 を backfill、frame 135 の NEAR-ID 継続例外を `mac_reference_policy` override で明示(§3.2.3 参照) |
| Goldens manifest(新規) | `tests/goldens/v1.6.0-32bpc/manifest.json` (新規、commit) | 14 frames の 32bpc 再収録 metadata + SHA256、artifact URL、policy: `mac_reference_policy: {kind:"identical"}` / `cross_platform_policy: {kind:"near-id", metric:"f32_abs", max_abs:1e-5}`(§3.2.3 参照) |
| Goldens 素材 | `tests/goldens/v1.6.0-32bpc/frame_NNNN_{in,out}.raw` (新規 × 14 × 2) | Mac AE 2025 で 32bpc capture、GitHub Release に tar 添付、**repo には commit しない**(manifest 内 SHA256 で integrity 保証) |
| capture スクリプト | `tests/capture_32bpc.py` (新規) | AE で書き出した EXR を SMDP header 付き `.raw` に変換するユーティリティ、commit 対象。**README or ヘッダコメントに明記すべき事項**: (1) 依存ライブラリ(`OpenEXR` / `numpy` 等の pin バージョン)、(2) EXR の channel 順序(R/G/B/A → SMDP の A/R/G/B への並べ替えがあるか)、(3) 0-1 domain 仮定で overbright clip するかしないか |

#### 3.2.3 成功条件(ハード要件、全て PASS)

1. **ビルド成功**: Mac Xcode + Windows MSVC で warning 無し、バイナリサイズは v1.5.1 から ±15% 以内(f32 code path 追加で多少増えることは許容)、偽成功検証 3 段クリア
2. **Rust 層 32bpc 単体テスト**: `cargo test --release` で 32bpc 用の新規 unit test 含め全 PASS。overbright(>1.0)/ edge 値(NaN 入力 / Inf 入力 / ±0.0 / subnormal)での NaN/Inf 出力防御テストは **synthetic unit test にのみ含める**(goldens 対象外。AE 32bpc/HDR 素材の新規追加は 2-A.2 のスコープ外なので goldens には反映しない)
3. **既存 8/16bpc regression 不変**: `tests/goldens/v1.4.0-ae2025/` に対する 14 frames IDENTICAL or NEAR-ID(§3.1.3 条件 3 と同じく frame 135 の既存 NEAR-ID のみ許容、新規 NEAR-ID が出たら不合格)+ synthetic white_option 6/6 PASS
4. **Mac CPU reference regression**(新規 32bpc goldens): `tests/goldens/v1.6.0-32bpc/` に対する 14 frames **IDENTICAL**(Mac ローカルで smooth_core の CPU 実装が自分の capture 済 goldens に bit-identical)
5. **Mac ↔ Win cross-platform 32bpc 整合**: Win で書き出した 32bpc output を Mac goldens と比較する際、**primary: IDENTICAL(byte-exact)**。取れない場合の fallback: manifest の `cross_platform_policy = {kind:"near-id", metric:"f32_abs", max_abs:1e-5}` を許容(schema 詳細は §3.2.6)。`max_abs` を超える差分が出た場合は hard gate、§4 Spike 項目追加(platform 間 f32 非決定性の原因特定)。これにより platform 差があっても 2-A.2 を止めない設計
6. **32bpc 対応の一次証跡**(3 点セット): (a) GlobalSetup + Pipl.r 双方で `PF_OutFlag2_FLOAT_COLOR_AWARE` が立っている、(b) AE 2025 の 32bpc project で smooth が実際に render 完走する、(c) 条件 4(Mac CPU reference regression)PASS。AE Effect Controls 上の黄色 ⚠️ 消失は **UI 上の補助確認** に格下げ(確認すると親切だが gate にはしない)
7. **MFR 保持**: `SUPPORTS_THREADED_RENDERING` 継続有効、Mac Multithreaded render report + Win aerender.exe stdout の Thread-safe / Render threads used 行確認(§3.1.3 条件 4 と同じ一次証跡、32bpc でも同様に出ること)
8. **後方互換**: legacy `PF_Cmd_RENDER` ハンドラ残置、SmartRender 三本化構造(`SMART_PRE_RENDER` / `SMART_RENDER` / legacy `RENDER`)が崩れていない

#### 3.2.4 検証手順

**Step 1: Rust 層 + manifest 駆動 regression**

```bash
cd rust/smooth_core && cargo test --release                # f32 unit test 含め PASS
tests/fetch_goldens.sh                                      # 初回 or artifact 不足時、manifest 駆動で GitHub Release から DL + per-file SHA256 検証
SMOOTH_PARALLEL=1 tests/run_regression.sh                   # manifest 駆動、v1.4.0-ae2025 14/14 + v1.6.0-32bpc 14/14 + synthetic 6/6
SMOOTH_PARALLEL=0 tests/run_regression.sh                   # 同上
```

`run_regression.sh` は冒頭で manifest が参照する raw ファイルの存在を確認し、不足していれば `fetch_goldens.sh` を自動呼び出し(または明示エラーで呼ぶよう促す)する実装とする。これにより fresh clone 再現が 1 コマンドで閉じる。

**Step 2: Mac AE 2025 で 32bpc goldens 取得(初回のみ、および §3.2.6 の再 capture 条件該当時)**

1. 既存 v1.4.0-ae2025 goldens の 14 シーンと**同じ input 素材を 32bpc project** に投入(AE の Project Settings で Depth = 32 bits per channel)
2. smooth を適用、range / line_weight / white_option を既存 goldens と同値設定
3. Render Queue で Output Module を EXR(各チャンネル f32 preserved)に設定、14 frames 書き出し
4. `tests/capture_32bpc.py` で input layer と output を SMDP header 付き `.raw` に変換、ローカル `tests/goldens/v1.6.0-32bpc/frame_NNNN_{in,out}.raw` として保存(これらは `.gitignore` で ignore される)
5. `tar -cf - tests/goldens/v1.6.0-32bpc/*.raw | zstd -19 > goldens-v1.6.0-32bpc.tar.zst` で tar artifact を作成、tar 全体の SHA256 と tar 内各 .raw の SHA256 を計算
6. GitHub Release(smooth release tag、初回は `v1.6.0-rc1` 等の pre-release タグでよい)に `goldens-v1.6.0-32bpc.tar.zst` を asset 添付
7. `tests/goldens/v1.6.0-32bpc/manifest.json` に以下を記述して commit:
    - test-suite metadata: schema version、artifact URL、artifact tar SHA256、capture source(Mac AE version)、capture date、smooth version、SDK version
    - suite-level policies: `mac_reference_policy: {kind: "identical"}`、`cross_platform_policy: {kind: "near-id", metric: "f32_abs", max_abs: 1e-5}`
    - per-frame metadata: frame番号 / width / height / bpc=32 / range / line_weight / white / in/out の SHA256
8. **`.raw` 本体は commit しない**、`manifest.json` と `tests/fetch_goldens.sh` のみ commit。fresh clone から `fetch_goldens.sh` → `run_regression.sh` で再現性確認

**Step 3: Mac AE 2025 実機 32bpc 動作確認(毎ビルド)**

1. `xcodebuild ...` で build、`smooth.plugin` を配置
2. 32bpc project を開き smooth を適用、UI 応答・クラッシュなし、32bpc render が完走することを確認(§3.2.3 条件 6 (b) の一次証跡)
3. Render Queue で同 14 frames を EXR 書き出し、Step 2 と同じ変換で `.raw` 化、goldens と byte-exact 一致確認(IDENTICAL のみ許容、§3.2.3 条件 4)
4. MFR 一次証跡: Multithreaded render report が render log に出現
5. 既存 8/16bpc project でも smooth 適用、8/16bpc regression 時と output 同等(視覚確認 + 既存 goldens との byte-exact)
6. **補助確認**(gate ではない): Effect Controls 上の smooth から黄色 ⚠️ が消えていることを目視

**Step 4: Win AE 2025 実機 32bpc cross-platform 検証(毎ビルド)**

1. MSVC build で `smooth.aex` 配置
2. 32bpc project で smooth 適用、UI 応答・クラッシュなし
3. `aerender.exe` で同 14 frames 書き出し → SMDP 変換 → **Mac で取った v1.6.0-32bpc goldens と比較**。primary は IDENTICAL、fallback は manifest の `cross_platform_policy`({metric: "f32_abs", max_abs: 1e-5})許容(§3.2.3 条件 5 の段階方針)。`cross_platform_policy` の許容を超えた場合は hard fail で §4 Spike 追加(bit-identical が取れなくても許容内なら PASS)
4. `aerender.exe` stdout に `Thread-safe` 宣言と `Render threads used` 行(§3.1.4 Step 3 と同じ運用、[`workbench_history.md`](../workbench_history.md) L1135 付近参照)

**Step 5: Tooling 回帰確認**

新 manifest 駆動の `run_regression.sh` が v1.4.0-ae2025 の既存 14 frames を**これまでと同じ結果**(13 IDENTICAL + 1 NEAR-ID frame 135)で扱うことを確認。manifest への移行で既存 NEAR-ID 許容が壊れていないことを検証。

#### 3.2.5 次ステージ(2-A.3)進行判断基準

以下 **すべて YES** で 2-A.3 に進む:

- [ ] §3.2.3 の 8 項目すべて PASS
- [ ] v1.6.0-32bpc goldens が Mac で capture 完了、GitHub Release に tar artifact として添付済み、`manifest.json`(per-file SHA256 含む)が repo に commit 済み、`tests/fetch_goldens.sh` で fresh clone から 1 コマンドで再現可能
- [ ] v1.4.0-ae2025 の `manifest.json` も backfill + commit、frame 135 の既存 NEAR-ID 例外が `mac_reference_policy` override で表現されている
- [ ] `GlobalSetup` と `Pipl.r` の `out_flags2` が同期済み(FLOAT_COLOR_AWARE 含む、Effect.cpp comment `must match ...` も更新)
- [ ] Win で Mac goldens に対して条件 5 の segmented 判定をクリア(IDENTICAL または manifest の `cross_platform_policy` 許容の near-ID)
- [ ] 既存 8/16bpc regression が劣化していない(frame 135 以外の新規 NEAR-ID なし)
- [ ] `workbench_history.md` に 2-A.2 の Step 群エントリ完備(§3.2.1 の 5 Step 分、または同等の粒度で)

NO が 1 つでもあれば、§3.1.5 と同じ 3 分岐(実装バグ起因 / SDK 仕様誤解起因 / SDK 契約上の制約・実装不能)で対応。特に Mac/Win で **`cross_platform_policy` の許容を超えた場合**、SDK の f32 domain の platform 差が Phase 2-A.3 の GPU f32 実装にも影響するため §4 Spike に項目追加を検討(`max_abs_f32 <= 1e-5` 内の差分は PASS 扱いで Spike 不要)。

#### 3.2.6 スコープ外の補足(実装者向けメモ、2-A.3 以降で扱う)

- **goldens の "正しさ" 判断**: 32bpc goldens は CPU 32bpc 実装の output をそのまま reference とする(Phase 1 / v1.5.1 までの integer domain 実装に対する independent numerical reference は存在しない)。capture 前に: (1) NaN/Inf が含まれない、(2) overbright が spurious に発生しない、(3) 視覚上 16bpc 結果と連続的(大きなトーン不整合がない)を Mac AE で目視確認すること
- **再 capture 条件**(通常ビルドでは固定 reference を使う、以下いずれかの場合のみ再取得): (i) AE major update(25.x → 26.x 等)で 32bpc 内部実装が変わった疑い、(ii) AE SDK major update で `PF_PixelFloat` の semantics 変更、(iii) `tests/capture_32bpc.py` / manifest schema の incompatible 変更、(iv) smooth アルゴリズム仕様の意図的変更(Phase 2-B 以降の機能追加で output が意図的に変わるケース)。再 capture 時の退避ルール: 生 .raw は repo 外 artifact のため、**GitHub Release asset 側で旧 tar.zst を保持したまま新 tar.zst を `goldens-v1.6.0-32bpc-r2.tar.zst` 等の revision 付き名称で追加添付**、manifest 側は旧 manifest を `manifest.v1.json.deprecated` に rename して temporarily 残す + 新 `manifest.json` を commit、差分レポートを commit message に残す
- **32bpc NEAR-ID tolerance の将来的な扱い**: GPU f32 実装(2-A.3)では丸め順序差で CPU と bit-identical に一致しない可能性がある。その場合は 32bpc goldens の manifest に `gpu_metal_policy` / `gpu_cuda_policy = {kind:"near-id", metric:"f32_abs", max_abs: 1e-4}` 等を追加する拡張余地を残す(manifest schema 設計時にフィールドを確保するだけ、実際の許容は 2-A.3 の GPU regression で必要になったら有効化)
- **`PF_OutFlag2_FLOAT_COLOR_AWARE` と `SUPPORTS_GPU_RENDER_F32` の違い**: 前者は 2-A.2 で必須(CPU 32bpc 対応の宣言、GlobalSetup + Pipl.r 両方)、後者は 2-A.3 で GPU 対応時に追加(GPU 側の f32 対応宣言、同じく両方)。2-A.2 時点では前者のみ、後者は立てない
- **manifest schema**(tolerance policy 分離 + metric 明示): JSON でも TOML でもよい。最低限含めるフィールド:
  - **test-suite metadata**: schema version、artifact URL、artifact tar SHA256、capture source platform、capture date、smooth version、AE version、SDK version
  - **suite-level policies**(2 種類を分離): `mac_reference_policy` / `cross_platform_policy`。policy は `{kind: "identical"}` または `{kind: "near-id", metric: "byte_abs"|"f32_abs", max_abs: <num>, max_diff_pct?: <num>}` の形
    - `metric: "byte_abs"` は 8/16bpc 用(integer domain、u8 の絶対差)
    - `metric: "f32_abs"` は 32bpc 用(float domain、f32 の絶対差)
    - harness は bpc に応じて正しい metric が指定されているか検証し、型混在時はエラー
  - **per-frame metadata**: frame 番号、width、height、bpc、range、line_weight、white、SHA256(in/out 別々)、任意で `policy_overrides: { mac_reference_policy?, cross_platform_policy? }`
  - **重要**: harness 側は Mac CPU reference 検証と Win cross-platform 検証で**別フィールドを参照**する実装とする(単一 `tolerance` で両方を扱わない、near-ID が Mac reference まで緩くならないよう)
  - v1.4.0-ae2025 の frame 135 は `policy_overrides.mac_reference_policy = {kind: "near-id", metric: "byte_abs", max_abs: 32, max_diff_pct: 0.01}` で既存 NEAR-ID 継続を表現(既存の実測 `max_abs=23`、`30/14187776 bytes` に対し `max_abs=32` / `max_diff_pct=0.01%` の余裕を持たせる運用そのまま)
  - schema バージョニングを入れておくと将来の bpc 追加や tolerance 表現の拡張が楽

### 3.3 Phase 2-A.3 GPU render(Mac Metal + Win CUDA)

#### 3.3.1 スコープ

Phase 2-A の出荷対象本体。§2 で確定した full-GPU plugin 構造(SDK_Invert_ProcAmp.cpp 80% 流用、per-platform native、DX12 defer)に従い、Metal (Mac) + CUDA (Win) の 2 backend を実装する。**Metal を先行、CUDA を後追い** で進め、両 backend を満たした時点で v1.6.0 として一括出荷。

**対象 bitdepth の明確化**(以下の「含む / 含まない」全体にかかる前提): GPU path は **32bpc(`PF_PixelFloat`)専用**。8bpc / 16bpc は GPU 経路を使わず、常に CPU `SMART_RENDER` を通す。根拠: `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` が唯一の GPU render flag(8/16bpc 用の GPU flag は SDK に存在しない)、smooth の GPU 化動機は 4K 32bpc の性能改善であり 8/16bpc は現行 CPU で十分高速。このルールは PreRender 条件(後述)で enforce する。

**含む**:

- **Rust 側**(§2.6 / 研究 doc §3.4 採用):
  - `rust/smooth_core/src/gpu/{mod.rs, metal.rs, cuda.rs, shaders/}` 新設、共通 `GpuBackend` trait + platform-gated 実装
  - Mac: `metal-rs` / `objc2-metal` で Metal device / buffer / compute pipeline、MSL shader(smooth.metal)を build-time に `.metallib` コンパイル + `include_bytes!` 埋め込み(§2.5 既採用、研究 doc §3.5 採用案)
  - Win: **NVCC static link + Rust extern "C"**(SDK サンプル準拠、研究 doc §6.4)で方針を pin。`.cu` を build-time に NVCC でコンパイル → `.obj` を plugin バイナリに静的リンク、Rust からは extern "C" 宣言で kernel launch 関数を呼ぶ。`cudarc` は kernel launch に使わず、必要な場合のみ device query / driver API の範囲で補助利用。(§3.3.6 にも再掲)
  - 2-pass アルゴリズム(研究 doc §2.4 案 2: 検出 → blending、v1.0 GPU 実装の本命)。行並列(案 3)は PoC 段階のみ、v1.6.0 出荷は案 2 で
- **Effect.cpp 側**:
  - `PF_Cmd_GPU_DEVICE_SETUP` / `PF_Cmd_GPU_DEVICE_SETDOWN` ハンドラ新設(per-device 1 回、MSL compile / CUDA pipeline load をここで)。`GPU_DEVICE_SETUP` は **`out_data->out_flags2 |= PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を返す**(framework ごとに plugin が「この device で GPU 対応あり」を宣言、SDK サンプル準拠、AE_Effect.h L1007 参照)。GlobalSetup 側の同 flag と併せて **3 箇所同期**(GlobalSetup / Pipl.r / GPU_DEVICE_SETUP)
  - `PF_Cmd_SMART_RENDER_GPU` ハンドラ新設(`SMART_RENDER` とは **distinct selector**、§2.6 研究 doc §6.2)
  - `SmartPreRender` で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を**条件付きで**立てる:**全条件を AND** したときのみ:
    - (a) `extraP->input->bitdepth == 32`(comp が 32bpc、8/16bpc は常に CPU)
    - (b) GPU Acceleration checkbox が ON である
    - (c) `GPU_FALLEN.get(&uuid)` で fallen でない
    - (d) GPU 検出機構(研究 doc §5.3.1 / RFC §4.3)で使用可能な backend(Metal / CUDA)が存在する
    - (e) `GPU_DEVICE_SETUP` が成功済み(SETUP で失敗した backend は usable とみなさない)
    - いずれか 1 つでも false なら GPU_RENDER_POSSIBLE は立てず、AE は `SMART_RENDER`(CPU)を呼ぶ
  - `SMART_RENDER_GPU` 入口で **GPU world のピクセル形式を検証**: SDK サンプル準拠で `PF_PixelFormat_GPU_BGRA128`(float4 BGRA)以外は reject(`PF_Err_UNRECOGNIZED_PARAM_TYPE` 等を返す、[SDK_Invert_ProcAmp.cpp](../references/AfterEffectsSDK_25.6_61_mac/ae25.6_61.64bit.AfterEffectsSDK/Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp) L846 同等)。PreRender 側の bitdepth 判定と区別し、両方を実装する(PreRender = comp-level の振り分け / SMART_RENDER_GPU 内 = device-level のピクセル形式防御)
  - **sequence_data 機構の全面導入**(§2.4 2 層分離設計):
    - `SmoothSequenceData { version, instance_uuid_hi, instance_uuid_lo }` struct
    - `PF_Cmd_SEQUENCE_SETUP` / `PF_Cmd_SEQUENCE_RESETUP`(毎回 UUID 再生成、duplicate 衝突回避) / `PF_Cmd_SEQUENCE_FLATTEN` / `PF_Cmd_GET_FLATTENED_SEQUENCE_DATA` / `PF_Cmd_SEQUENCE_SETDOWN` ハンドラ実装
    - render 時は `PF_EffectSequenceDataSuite1::PF_GetConstSequenceData` 経由で read-only、lifecycle selector のみ書き込み
  - **plugin-global fallen flag**(§2.4):
    - `static GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>>`
    - GPU エラー検出時の動作は **§4.4 Spike の結論で確定**(下記 "SMART_RENDER_GPU 内 fallback の実現方式" 参照、RFC の現段階では未確定の設計判断)
    - `SEQUENCE_SETDOWN` で `GPU_FALLEN.remove(&uuid)` 掃除
  - **SMART_RENDER_GPU 内 fallback の実現方式**(§4.4 Spike で決定、成功条件 6 の前提):
    - GPU 失敗時に **その同じ SMART_RENDER_GPU 呼び出し内で CPU 結果を GPU output world に書き込んで `PF_Err_NONE` を返す** 方式は non-trivial(device→host→device の転送実装が必要、Mac Apple Silicon は unified memory で安い / Intel Mac + Win は discrete VRAM で blit or cudaMemcpy が要る)
    - 代替案: 当該 frame は `PF_Err` を返して fail(次 frame の PreRender は GPU_FALLEN=true により GPU_RENDER_POSSIBLE を立てず、AE は CPU 経路を呼ぶ)。但し「1 frame の失敗で render queue job が abort するか」が AE 側挙動依存
    - **Spike で両方式を実測**: (i) device→host→device 転送の実装可能性と overhead、(ii) AE の single-frame PF_Err 時の挙動、結果を見て最終方式を確定
- **UI**(§2.5):
  - `GPU Acceleration` checkbox param 追加、default ON、意味 = ☑ Auto(GPU 試行、失敗時 CPU) / ☐ CPU 固定
  - `PF_ParamFlag_DISABLED` を **param 登録時に静的に立てる**(研究 doc §5.3.1 / RFC §4.3 の検出機構を `PF_Cmd_GLOBAL_SETUP` で 1 回実行、結果を plugin-global static にキャッシュ)
  - About ダイアログに GPU 状態の**静的テキスト**埋め込み(「GPU: Metal (Apple M1 Max)」等、ARBITRARY_DATA 動的更新は使わない、研究 doc §5.6)
- **テスト**:
  - Rust 側: GPU backend unit test(Metal + CUDA、`#[cfg(target_os)]` gate)、基本 shader dispatch の correctness
  - 統合: 既存 manifest 駆動 regression に **GPU path(32bpc のみ)** を追加。GPU output を CPU 32bpc goldens と比較する際の policy field を manifest schema に拡張(`gpu_metal_policy` / `gpu_cuda_policy`、fallback は `cross_platform_policy` と同じ形)。8/16bpc 側は GPU 経路を持たないので、GPU 版の v1.4.0-ae2025 regression は存在しない
  - Fallback 動作テスト: **dev/test build 限定の `SMOOTH_FORCE_GPU_ERROR=setup|render|oom` 環境変数フック**で GPU 失敗を意図的に発火、once-fallen-always-fall per SETUP/RESETUP 区間が動くことを確認(release build では無効化、または `#[cfg(feature = "test-fault-injection")]` で完全除外)。§4 Spike 結論を反映
  - MFR + GPU stress: concurrent render threads で crash / freeze しないこと
- **配布**(v1.6.0):
  - `RELEASE_NOTES-v1.6.0.md`(v1.5.1 からの delta、GPU 対応機能を主要変更点として記述)
  - Mac universal + Win x64 build、既存の偽成功検証 3 段適用、SHA 記録
  - `workbench_history.md` に 2-A.1 / 2-A.2 / 2-A.3 の全 Step エントリ完備

**含まない**:
- DX12 backend / AMD discrete Win / Intel Arc(Phase 2-A.4 以降、§1.2 スコープ表)
- GPU 強制モード(Auto/CPU/GPU popup、§2.5、将来 v1.7.x 候補)
- 32bpc overbright / HDR 素材の新規テスト(2-A.2 と同じ理由で sample 収集コスト外、§3.2.1 継続)
- About ダイアログ GPU 状態の動的更新(ARBITRARY_DATA 使用、§5.10、実装コスト増のため見送り)

#### 3.3.2 成果物

| カテゴリ | ファイル | 変更内容 |
|---|---|---|
| Rust crate(骨格) | `rust/smooth_core/src/gpu/mod.rs` (新規) | `GpuBackend` trait + platform dispatch glue、CPU backend を同 trait で包んで fallback / unit test / bench を trait 単位で書けるようにする |
| Rust crate(Mac) | `rust/smooth_core/src/gpu/metal.rs` (新規、`#[cfg(target_os = "macos")]`) | Metal device / command queue / buffer / compute pipeline の Rust 側 wrapper、MSL shader の library ロード、dispatch 実装 |
| Rust crate(Win) | `rust/smooth_core/src/gpu/cuda.rs` (新規、`#[cfg(target_os = "windows")]`) | NVCC build-time static link した kernel launch 関数(C ABI)を `extern "C"` 宣言で呼ぶ wrapper、dispatch 実装。`cudarc` は kernel launch には使わない(必要なら device query / driver version など補助のみ、§3.3.6 pin) |
| Shaders | `rust/smooth_core/src/gpu/shaders/smooth.metal` (新規) | MSL source、2-pass(detect + blend)アルゴリズム |
| Shaders | `rust/smooth_core/src/gpu/shaders/smooth.cu` (新規) | CUDA source、同 2-pass アルゴリズム |
| Build | `rust/smooth_core/build.rs` | Mac: `xcrun metal -c ... && xcrun metallib ...` → `.metallib` を `include_bytes!` 対象に / Win: NVCC invocation で `.cu` → static obj(研究 doc §6.4、cudart は static 形式を採用) |
| Cargo | `rust/smooth_core/Cargo.toml` | `metal-rs` / `objc2-metal`(Mac)を platform feature / target 条件で追加。Win は NVCC static link + Rust `extern "C"` が主経路、`cudarc` は任意の device query 補助のみ、kernel launch には使わない(方針は §3.3.6 に pin)。bind バージョンは実装時に pin(研究 doc §3.8) |
| Effect main(selector) | [Effect.cpp](../Effect.cpp) | `PF_Cmd_GPU_DEVICE_SETUP` / `GPU_DEVICE_SETDOWN` / `SMART_RENDER_GPU` / `SEQUENCE_SETUP` / `SEQUENCE_RESETUP` / `SEQUENCE_FLATTEN` / `GET_FLATTENED_SEQUENCE_DATA` / `SEQUENCE_SETDOWN` の 8 selector 追加。`GPU_DEVICE_SETUP` 内で `out_data->out_flags2 |= PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を返す実装(framework-level で GPU 対応宣言) |
| Effect main(Smart) | [Effect.cpp](../Effect.cpp) `SmartPreRender` | `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を **§3.3.1 の 5 条件 AND** を満たすときのみ立てる(無条件に立てない)、`pre_render_data` に GPU checkbox 値を追加格納 |
| Flags(3 箇所同期) | [Effect.cpp](../Effect.cpp) `GlobalSetup` + [Pipl.r](../Pipl.r) `AE_Effect_Global_OutFlags_2` + [Effect.cpp](../Effect.cpp) `GPU_DEVICE_SETUP` の `out_data->out_flags2` | `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を **3 箇所に OR**(GlobalSetup / Pipl.r / GPU_DEVICE_SETUP)。前者 2 つは plugin-level、GPU_DEVICE_SETUP は per-framework-device level の宣言。Effect.cpp comment 更新 |
| UI params | [Effect.cpp](../Effect.cpp) `ParamsSetup` | `GPU Acceleration` checkbox を追加、GPU 非対応検出結果に応じて `PF_ParamFlag_DISABLED` を param 登録時に静的設定 |
| Fallback 機構 | Rust / Effect.cpp 両側 | `DashMap<u128, AtomicBool> GPU_FALLEN` を Rust 側に static で配置、C++ から FFI で insert / query / remove。UUID は `uuid::Uuid::new_v4()` を Rust 側で生成、C++ へは u64 × 2 で渡す。**SMART_RENDER_GPU 内の fallback 実装方式は §4.4 Spike で確定**(§3.3.1 参照) |
| Fault injection(dev 専用) | Rust / Effect.cpp 両側、`Cargo.toml` に `test-fault-injection` feature | 環境変数 `SMOOTH_FORCE_GPU_ERROR=setup\|render\|oom` を読み、該当タイミングで擬似エラーを発火(GPU_FALLEN の設定 + fallback 経路の実行検証に使う)。`#[cfg(feature = "test-fault-injection")]` でガード、release build では完全除外 |
| Tests(Rust) | `rust/smooth_core/src/gpu/tests.rs` (新規) | Metal / CUDA それぞれの unit test、shader dispatch basics、fallen flag の DashMap 動作確認 |
| Tests(manifest) | `tests/goldens/v1.6.0-32bpc/manifest.json` 拡張 | `gpu_metal_policy` / `gpu_cuda_policy` フィールド追加、§3.2.6 schema 拡張で確保済みの field を有効化 |
| Tests(fallback) | `tests/gpu_fallback_test.cpp` (新規) | `SMOOTH_FORCE_GPU_ERROR` フック経由で once-fallen-always-fall per SETUP/RESETUP 動作確認(§4 Spike 結果を反映) |
| 配布 | `RELEASE_NOTES-v1.6.0.md` (新規) | v1.5.1 → v1.6.0 delta、GPU 対応を主要変更点として記述。**最低限含める GPU 注意事項**: (1) Metal は対応 macOS / AE バージョン、(2) Windows は NVIDIA CUDA 対応 driver 必須、(3) AMD / Intel GPU Windows は CPU fallback、(4) GPU と CPU の output は byte-identical を保証しない(視覚無差別は保証)、(5) once-fallen-always-fall: バッチ書き出し中 GPU 失敗時は以降 CPU 固定 / save-load 後に retry、(6) GPU Acceleration checkbox OFF で常に CPU |
| 配布 | Mac universal + Win x64 build、SHA 記録、配布 zip 3 種(Phase 2-B v1.5.1 リリースと同じ形式、`workbench_history.md` L1198 付近参照) | v1.6.0 gold |

#### 3.3.3 成功条件(ハード要件、全て PASS)

1. **ビルド成功**: Mac Xcode + Windows MSVC + Mac metal toolchain(`xcrun metal`)+ Win NVCC の全 toolchain で warning 無し、バイナリサイズ v1.5.1 の ±40% 以内(GPU code + shader embed で増加許容、過大な場合はサイズ調査)、偽成功検証 3 段クリア
2. **`PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` flag 3 箇所同期**: (a) GlobalSetup の `out_flags2`、(b) Pipl.r の `AE_Effect_Global_OutFlags_2`、(c) GPU_DEVICE_SETUP の `out_data->out_flags2` の**全てに**立っていること(§3.3.2 Flags 行参照、SDK サンプル準拠、AE_Effect.h L1007)
3. **Mac Metal 実機動作**: AE 2025 の 32bpc project で GPU checkbox ON → `SMART_RENDER_GPU` が呼ばれ Metal kernel が実行される(debug-only instrumentation で一度確認、PR merge 前に削除)
4. **Win CUDA 実機動作**: 同上、CUDA 経路
5. **GPU path の 32bpc regression**: Mac Metal output を CPU 32bpc goldens と比較、`gpu_metal_policy` 許容内 PASS / Win CUDA 出力を同 goldens と比較、`gpu_cuda_policy` 許容内 PASS(primary は IDENTICAL、現実的には丸め順序差で近い near-ID になる可能性を manifest で吸収)。**8/16bpc は GPU 経路なし**(§3.3.1 対象 bitdepth)、比較対象は 32bpc goldens のみ
6. **Fallback 動作**(§4.4 Spike 結論を反映): once-fallen-always-fall per SETUP/RESETUP が実機 + `SMOOTH_FORCE_GPU_ERROR` 注入 test で動く:
    - GPU error 検出 → Spike 結論に従って: (i) device→host→device で CPU fallback + `PF_Err_NONE` 完走、または (ii) `PF_Err` で当該 frame fail → 次 frame 以降 CPU 固定。**(ii) は spike で「PF_Err を返しても Render Queue job が abort しない」実測が取れた場合のみ採用可**、job abort する結果なら (i) を必須採用(Sub-stage F の Render Queue 完走要求と矛盾させない)
    - 同 SETUP/RESETUP 区間の以降 frame は、PreRender で `GPU_FALLEN.get(&uuid)=true` により `GPU_RENDER_POSSIBLE` が立たず、AE は CPU `SMART_RENDER` を呼ぶ(GPU 再試行が走らない)
    - `SEQUENCE_RESETUP`(save/load、duplicate、params 変更)で UUID 再生成 → `DashMap` miss で fresh retry が発生
    - `SEQUENCE_SETDOWN` で `DashMap` エントリ掃除
7. **MFR + GPU 両立**: concurrent render threads で crash / freeze / numerical divergence なし、Mac Multithreaded render report + Win aerender.exe stdout の Thread-safe / Render threads used 行に出現
8. **UI 動作**:
    - GPU 対応 system: checkbox 有効、ON → GPU path(32bpc のみ、8/16bpc は CPU)、OFF → 全 bitdepth で CPU path(§3.1 / §3.2 の CPU regression と同一結果)
    - GPU 非対応 system(Win + NVIDIA なし等): checkbox が **param 登録時から DISABLED**(AE UI 上でグレイアウト、操作不能)
    - About ダイアログに GPU 状態の静的テキスト表示
    - **PreRender 条件**: §3.3.1 の 5 条件(32bpc / checkbox ON / not fallen / backend usable / DEVICE_SETUP 成功)AND が `GPU_RENDER_POSSIBLE` の必要十分条件になっている
9. **既存 regression の非劣化**: v1.4.0-ae2025(8/16bpc)+ v1.6.0-32bpc(CPU 経路)の全 28 frames が §3.1 / §3.2 と同等結果、synthetic 6/6 PASS、`cargo test --release` で GPU 込みの全 unit test PASS
10. **性能**: 代表 scene(HD + 4K の 32bpc)で **同 build の CPU 経路**(checkbox OFF = 2-A.2 で実装した CPU 32bpc SMART_RENDER)を baseline として GPU 経路と比較し、**明確な速度向上**を測定、`workbench_history.md` に baseline vs GPU の計測表を記録(必須)。32bpc CPU baseline は v1.5.1 には存在しないので v1.5.1 と比較しない。8/16bpc の**非劣化**確認(§3.3.3 条件 9)のみ v1.5.1 baseline 対象。**「4K 32bpc で 3× 以上」は release claim の条件**として別途判定するが、hard gate には**しない**(AE synchronization / VRAM / driver overhead が未知のため 3× 未達でも v1.6.0 を止めない、数字は RELEASE_NOTES の表現で調整)
11. **§4 Spike 全項目が結論済み**: 7 項目すべて PASS(採用設計)または不合格 → 代替設計で回避済み、結論が §4 に反映されている
12. **配布成果物整備**: `RELEASE_NOTES-v1.6.0.md` 完成(§3.3.2 の 6 項目 GPU 注意事項含む)/ Mac universal + Win x64 build + SHA 記録 / `workbench_history.md` に 2-A.1 / 2-A.2 / 2-A.3 の全 Step エントリ

#### 3.3.4 検証手順

本ステージは Sub-stage(A)〜(F)の順序で進め、各 Sub-stage 末で `workbench_history.md` に Step エントリを追加する。

**Sub-stage A: §4 Spike 完了**

§4 の 7 項目を実測で決着、結論を §4 本文に反映。優先度高の 4.1 / 4.4 / 4.5 が不合格なら代替設計を §4 に具体化し、本 RFC §3.3 の該当箇所(Fallback 機構 / MFR 設計 / sticky span)を合わせて更新してから Sub-stage B に進む。

**Sub-stage A の実装粒度**: 4.1 / 4.4 / 4.6 は最小 GPU 実装がないと観測できないため、**disposable な PoC / SDK_Invert_ProcAmp.cpp への直接パッチでの実測**を許容する。この PoC は Sub-stage A 限定の使い捨てで、本番実装(Sub-stage B 以降の `gpu/mod.rs` + trait + 本番 shader)は spike 結論を踏まえてから書く。PoC コードは `workbench_history.md` に結論を記録した時点で破棄して良い(repo には残さない)。循環依存を避けつつ、未決部分の設計を本実装に持ち込まない分離。

**Sub-stage B: Rust crate GPU scaffold**

1. `gpu/mod.rs` の `GpuBackend` trait、platform dispatch、CPU backend の同 trait 包装
2. `cargo test --release` で既存 CPU regression が trait 経由でも壊れないことを確認
3. shader 空ファイル(smooth.metal / smooth.cu)を置いて `build.rs` の compile が通ることまで

**Sub-stage C: Mac Metal backend 実装 + Effect.cpp Mac GPU path + 基本 UI**

1. `gpu/metal.rs` + MSL shader 本体(2-pass)、Rust 単体で kernel dispatch が動くことを `cargo test` で確認
2. Effect.cpp に GPU_DEVICE_SETUP / SMART_RENDER_GPU / sequence_data 8 selector + GPU_FALLEN 機構追加、GPU_DEVICE_SETUP で `out_data->out_flags2 |= SUPPORTS_GPU_RENDER_F32` を返す
3. `SmartPreRender` の `GPU_RENDER_POSSIBLE` 条件 5 項目を実装(§3.3.1 参照、入力 bitdepth / checkbox / fallen / backend / DEVICE_SETUP 成功)
4. **基本 UI**: GPU Acceleration checkbox を `ParamsSetup` に追加(default ON、この段階では常に enabled スタブ、`PF_ParamFlag_DISABLED` の動的設定は Sub-stage D で §4.3 spike 結論を受けて入れる)
5. `tests/fetch_goldens.sh` → `run_regression.sh` で GPU path 含む全 regression、Mac Metal `gpu_metal_policy` 許容内 PASS
6. Mac AE 2025 実機: 32bpc project で checkbox ON → GPU path 稼働、Render Queue 完走、goldens regression PASS / 8bpc + 16bpc project は GPU 経路を持たず CPU `SMART_RENDER` で走る(Effect Controls で checkbox ON でも 32bpc 以外は CPU)
7. Fallback injection(`SMOOTH_FORCE_GPU_ERROR=setup|render|oom`): §4.4 spike 結論に従った fallback 実装が動くこと、`GPU_FALLEN` が set され以降 CPU 固定、Render Queue 完走
8. MFR + GPU stress: 大きな 32bpc comp で concurrent frames、crash / freeze なし、Multithreaded render report 出現

**Sub-stage D: UI の DISABLED wiring + GPU 検出機構**

1. §4.3 spike(`GetDeviceCount` の Software Only 反映挙動)結論を反映: 検出ソースを `GetDeviceCount` / OS API(`MTLCreateSystemDefaultDevice` / `cuInit`)/ その組合せのいずれかに確定
2. `PF_Cmd_GLOBAL_SETUP` で 1 度だけ検出、結果を plugin-global static にキャッシュ
3. `ParamsSetup` で検出結果に応じて `PF_ParamFlag_DISABLED` を static に設定(Sub-stage C のスタブを置換)
4. About ダイアログに GPU 状態静的テキスト表示
5. Mac(metal 非対応環境を再現できない場合は artificial disable で代替)で checkbox DISABLED を確認

**Sub-stage E: Win CUDA backend 実装 + Effect.cpp Win GPU path**

1. `gpu/cuda.rs` + CUDA shader + `build.rs` の NVCC invocation、Rust 単体で kernel launch が動くことを `cargo test` で確認(Win のみ、Mac は `#[cfg(target_os = "windows")]` で skip)
2. Effect.cpp の CUDA 分岐(既存 Metal 分岐と並置、switch `extraP->input->what_gpu`)
3. Win AE 2025 実機: 32bpc project で checkbox ON → CUDA path 稼働、aerender.exe stdout で Thread-safe + Render threads used、goldens regression `gpu_cuda_policy` 許容内 PASS
4. `cuCtxPushCurrent` / `cuCtxPopCurrent` は §4.2 spike 結論に従う(省略 or 追加)
5. Fallback injection + MFR + GPU stress を Win でも実施
6. Win NVIDIA なし系(または artificial disable)で checkbox DISABLED を確認

**Sub-stage F: Full UAT + v1.6.0 配布**

1. Mac + Win 両 platform、有効な 4 組み合わせ全 regression を UAT プロジェクトで実行:
    - 8bpc × CPU(§3.1 CPU SmartRender)
    - 16bpc × CPU(§3.1 CPU SmartRender)
    - 32bpc × CPU(§3.2 / checkbox OFF または GPU 非対応系)
    - 32bpc × GPU(§3.3 / checkbox ON + GPU 対応系)
    - **8bpc × GPU / 16bpc × GPU は存在しない**(§3.3.1 対象 bitdepth、PreRender 条件 (a) で除外)
2. 性能測定: 代表 2 シーン(HD 32bpc / 4K 32bpc)× CPU / GPU で wall-clock 計測、`workbench_history.md` に baseline vs GPU の表を記録(必須、条件 10)。release claim 用の 4K 32bpc 3× 判定はここで別途確認
3. GPU 非対応 system で checkbox DISABLED を確認(Win + NVIDIA なし / Metal 非対応環境、確保困難なら artificial disable で代替検証、§4.3 spike 結論に従う)
4. `RELEASE_NOTES-v1.6.0.md` 作成(`RELEASE_NOTES-v1.5.1.md` 文体踏襲、v1.5.1 → v1.6.0 delta を整理、§3.3.2 の 6 項目 GPU 注意事項含む)
5. Mac universal + Win x64 build、偽成功検証 3 段、SHA 記録、配布 zip 3 種作成
6. `v1.6.0` annotated tag 作成、GitHub Release に配布 zip と goldens artifact を添付

#### 3.3.5 v1.6.0 出荷判断基準

**これは 2-A の最終ゲート**。§3.1.5 / §3.2.5 のような「次ステージに進むか」ではなく「v1.6.0 を出すか、fallback path に切替えるか」の判断。

以下 **すべて YES** で v1.6.0 を出荷:

- [ ] §3.3.3 の 12 項目すべて PASS
- [ ] §4 Spike 7 項目すべて結論済み(PASS または 代替設計で回避済み)、結論が §4 本文に反映
- [ ] `workbench_history.md` に 2-A.1 / 2-A.2 / 2-A.3 の全 Step エントリ完備
- [ ] `RELEASE_NOTES-v1.6.0.md` 完成、配布 SHA 記録、tag 作成準備完了

NO の場合は **§5.1.3 Gate 4(Sub-stage F 完了時)として扱い** §5.1 Fallback 出荷パスの版数選択ツリー(§5.1.2)に移行(**default は `v1.6.0 32bpc-only`**、例外として片 platform GPU only)。`v1.5.2` は 2-A.2 不成立時のみ該当するためここには該当しない(Sub-stage F 到達時点で §7.2 Step 5 は YES = 2-A.2 完了済み)。どちらの版数に倒すかは §5.1.3 のゲートで確定、RFC レビュー時点では未決のまま。

#### 3.3.6 スコープ外の補足(実装者向けメモ)

- **Metal commandBuffer**: 研究 doc §6.2 の通り commit のみ、`waitUntilCompleted` は呼ばない(AE 側が synchronization、呼ぶと deadlock のリスク)
- **CUDA 実装方針の pin**: **NVCC build-time static link + Rust extern "C"** を主経路とする(SDK サンプル準拠、研究 doc §6.4)。`.cu` を NVCC で `.obj` にコンパイル → plugin バイナリに静的リンク → Rust 側は `extern "C" fn kernel_launch_xxx(...)` で呼ぶ。`cudarc` crate は **kernel launch には使わない**(PTX/cubin を driver API で動的 load する model ではない)。`cudarc` を入れる場合は device enumeration や driver version query など補助的な用途に限定
- **CUDA context**: SDK サンプル準拠で `cuCtxPushCurrent` / `cuCtxPopCurrent` を初期実装で省略、§4.2 spike が省略を否定する結果なら zero-cost safety margin として追加。spike PASS/FAIL に関わらず overhead は < 1 µs/call の想定
- **UUID 生成戦略**: `uuid::Uuid::new_v4()` を SEQUENCE_SETUP / SEQUENCE_RESETUP の両方で**毎回新規生成**、flattened UUID は復元時に参照しない。研究 doc §4.6 / §6.5 の通り、duplicate 時の UUID 衝突を避ける唯一の正解(flattened 参照は dc1889a / 7356b51 の round 2-3 で却下確定)
- **Fallback 機構の sticky 範囲**: 「effect instance 全寿命で sticky」ではなく「SETUP/RESETUP 区間で sticky」の理由は §2.3 確定事項に記述済み。UAT で「save/load 後に GPU retry が起きる」を**バグと誤認しない**よう PR body か release notes に明記推奨
- **性能計測の表現**: RELEASE_NOTES で "N× faster" を謳う場合は (1) 計測 scene、(2) CPU baseline の core 数、(3) GPU 機種 を併記。ピーキーな最適化で特定機種でのみ出た数字を前に出さないこと(§1.4 非目標)
- **GPU goldens の扱い**: `gpu_metal_policy` / `gpu_cuda_policy` はまず CPU reference に対する許容差で運用。もし Metal / CUDA 側の出力を **独立した goldens artifact** として固定する必要が出たら(例: GPU 実装の regression を CPU の regression から切り離したい場合)、`tests/goldens/v1.6.0-32bpc-gpu-metal/` 等を後付けで新設する拡張余地を manifest schema に残す。**v1.6.0 時点では独立 goldens は作らない**
- **Phase 2-A.4 以降の拡張余地**: DX12 / AMD / Intel 対応は GpuBackend trait の backend 追加で対応、CPU/GPU 切替 UI(checkbox)も checkbox → popup 差し替えで互換性破壊なし(§2.5)。shader 抽象化は入れない方針(§2.2 確定事項、既存 2 backend への影響最小化優先)

#### 3.3.7 Sub-stage E (Win CUDA) pre-flight design-freeze checkpoint

**運用ポリシー**: 「macOS 側が RC 品質に到達するまで Win CUDA に着手しない」(Hiroshi さん 2026-05-03 確認)。Mac で発見される設計修正を Win に持ち込むコストは大きい(両 backend の rework + 2 platform 並行検証)ため、Sub-stage E を始める前に **Mac 単独で 1 commit "design-freeze review" を挟む** 運用を本 RFC で明記する。

**チェックポイントの位置**: Sub-stage C-3 完了 + Sub-stage D 完了の直後、Sub-stage E 着手の直前。`Sub-stage C / D の Mac AE 2025 実機 PASS が前提条件`。

**レビュー対象(全 4 項目、各項目 "変更なし" or "以下を修正" を commit body に明記)**:

1. **Rust `GpuBackend` trait surface**:
   - CUDA push/pop / async stream / driver-side OOM error variant が、既に Mac で確定した Metal command buffer / completion handler / `MTLCommandBufferStatusError` と **同形に収まるか** を机上検証
   - 不整合がある場合は trait をここで修正(Sub-stage B で freeze した surface を解凍する唯一のタイミング)
   - 検証手順: `gpu/cuda.rs` の stub を実装視点で 30 分眺め、CUDA 実装で必要となるが trait に無い hook を列挙(該当時は trait に追加 + Mac 側 `gpu/metal.rs` を no-op 追従)
2. **Rust GPU FFI surface(C 側公開)**:
   - C++ Effect.cpp が呼ぶ `smooth_core_gpu_*` の struct layout、enum 値(`GpuBackendKind`、`GpuFallbackReason` 等)を **Win build でもバイナリ互換** に保てるか確認
   - `smooth_core_version()` の枝番(low 16 bit)が GPU FFI 追加で bump 済みか確認、Win 側 plugin が古い枝番を見て abort できる準備があるか確認
3. **`sequence_data` UUID layout + once-fallen-always-fall fallback policy**:
   - UUID layout(§2.4 で確定)+ DashMap `<u128, AtomicBool>` 2 層構造が **CUDA 例外経路でも同じセマンティクスで動く** ことを確認
   - 特に CUDA `cuCtxSynchronize` / `cuStreamSynchronize` が返す `cudaError_t` を `GpuFallbackReason` にどう map するかをここで決定し、Mac 側の `MTLCommandBufferStatus` map と整合させる
   - flattened sequence_data の `MERGE_FLATTENED_FUNCTIONS` 経路は Win でも同じ Rust deserialize 経路を通るので、Mac で動いていれば Win でも動く想定。それでも一度確認
4. **Error model: `PF_Err` 戻し方 + DPU host-process-upload 採用方針(§4.4 採用 (i)) + `SMOOTH_FORCE_GPU_ERROR` hook 点**:
   - Mac 側で実装した「GPU 失敗時に device→host download → CPU 処理 → host→device upload + `PF_Err_NONE` で完走」の制御フローが、CUDA 側でも同じ関数境界で発火するか確認
   - error injection 用の `SMOOTH_FORCE_GPU_ERROR` env 変数(C-3 で実装予定)が CUDA path にも通るよう、Effect.cpp 側で platform 中立の hook 点に置かれているか確認

**checkpoint の運用形式**:

- Mac 単独、commit subject は `chore(phase-2a): Sub-stage E pre-flight design-freeze review` 程度
- commit body にレビュー結果(4 項目それぞれ"変更なし"または具体修正)+ 修正の場合の差分概要
- Sub-stage E 担当者(Win セッション側)は、この commit より前の沿革は `git log` 程度に流し読みで OK、**この commit 以降の差分しか触らない** 規約とする
- review で trait/FFI/error model の修正が発生した場合は、その修正自体を Mac で 1 つ以上の前置 commit として落としてから design-freeze commit を打つ(design-freeze commit 自体は変更ゼロ + 結論のドキュメンテーションに専念)

**前倒し de-risk(Sub-stage E 着手とは独立に Win 環境で消化可能)**:

- **Phase 2-A.2 Step 5(Mac↔Win cross-platform 32bpc)** は GPU 不要、AE 不要、`cargo build --release` + `tests/synthesize_32bpc_goldens.sh` だけで完結する。Win セッションが取れる任意のタイミング(2-A.3 進行中でもよい)に消化することで、`cross_platform_policy.f32_abs <= 1e-5` が実測で成立するかを Sub-stage E 着手より前に把握できる。閾値超過時は §4 Spike 項目追加(平台間 f32 非決定性の原因特定)、ここで GPU 着手前に解決を図る
- **`docs/SUB_STAGE_E_HANDOVER.md`(Sub-stage C-2 完了時に新規作成、随時更新)**: Mac 進行中に発見される SDK 仕様の落とし穴(`PF_Err` 戻し方、PreRender 5 条件、DPU ハンドラ呼び出し順序、checkbox invalidation 等)を蓄積。Win セッション開始時はこのファイルを Sub-stage E の playbook として使う

## 4. Spike 項目(実測で決着させる確認事項)

研究 doc の実装時確認リスト(§4.10 / §5.3.1 / §5.10 / §6.8)を独立章に集約。各項目を `[背景 / 方法 / 合格条件 / 不合格時の代替設計 / 実施タイミング]` の 5 フィールドで揃える。

**運用ルール**:
- 各 spike 項目の結論は **該当 Sub-stage** の `workbench_history.md` Step エントリに記録、本 RFC 該当項目に追記する形で確定(例: 4.1/4.4/4.5 は Sub-stage A、4.2 は Sub-stage E、4.3 は Sub-stage A 後半 or D 前半、4.6 は Sub-stage C、4.7 は Sub-stage D)
- 優先度高(4.1 / 4.4 / 4.5)は不合格時に設計根幹に波及、代替設計を具体化してから Sub-stage B 以降へ
- 優先度中・低(4.2 / 4.3 / 4.6 / 4.7)は本番実装で吸収可能、Sub-stage B-E の最中に解決して構わない
- Sub-stage A の PoC は disposable(§3.3.4 Sub-stage A 参照)、SDK_Invert_ProcAmp.cpp への直接パッチでの実測を許容

**優先度区分**:
- **高**(並列性・fallback 可視性・UAT 安定性の前提): 4.1 / 4.4 / 4.5
- **中**(本番実装の方針に影響): 4.2 / 4.3
- **低**(実装時の細部最適化): 4.6 / 4.7

---

### 4.1 AE MFR が同一 plugin に同時 `SMART_RENDER_GPU` を呼ぶか(優先度: 高)

**背景**: 研究 doc §4.10 / §6.1。SDK コメントは「full GPU plugin では exclusive access is always held」と宣言するが、SDK サンプルは thread-safety の追加 guard なしで書かれている。smooth は Phase 2-B で `PF_OutFlag2_SUPPORTS_THREADED_RENDERING`(MFR)を有効化済み。MFR + GPU 両立の設計は「per-call で buffer allocation、per-device で read-only pipeline(SETUP 時作成)」で naturally thread-safe としているが、**実際に AE が同一 plugin instance + 同一 device に対して並行 SMART_RENDER_GPU を呼ぶのか、逆に per-device で serialize されるのか**が不明。これが決まらないと pipeline state の共有可否が確定しない。

**方法**:
1. Sub-stage A の PoC(SDK_Invert_ProcAmp.cpp ベース)で `SMART_RENDER_GPU` 入口に `(thread_id, sequence_ptr, timestamp_ns)` のログを仕込む
2. 32bpc の重い comp(4K 100 frames 以上)を MFR 有効で Render Queue 書き出し
3. 取得したログから、同一 `sequence_ptr` に対して `SMART_RENDER_GPU` が**時間的に overlap するか**を判定(開始-終了区間の交差で判定、単純な thread_id 差分ではなく)

**合格条件**:
- **(A) Serialize される**: 同一 sequence_ptr への SMART_RENDER_GPU は時間 overlap しない → SDK サンプル準拠の naturally-thread-safe 実装で OK、本番実装で追加 guard 不要
- **(B) 並行するが per-call buffer + read-only pipeline で安全**: overlap するが、各 call の一時 buffer は独立、共有されるのは読み取りのみの pipeline state(MSL library / CUDA function handle)→ 本番実装で追加 guard 不要。ただし **(B) 採用時の本番実装制約**(shader / backend に課す):
  - per-device の mutable shared state に書き込まない(共有される pipeline / library handle は生成後 read-only)
  - global counter / shared scratch buffer / cached command buffer を持たない(per-call で都度生成)
  - 以上の制約を GpuBackend trait の不変条件として `gpu/mod.rs` の doc comment に明記、shader 実装にも同様のコメントを入れる

**不合格時の代替設計**:
- **(C) 並行かつ shared state の mutation を要求される**: per-device で mutex を入れて MFR throughput を犠牲にする、または per-thread pipeline pool を持つ(初期化コスト増)。後者の実装コストが高ければ MFR 並列度を plugin 側で絞る(thread 数ヒントを AE に伝える API は無いため、実質 serialize)
- いずれも §3.3.1 の「per-call buffer + per-device read-only pipeline」構造を撤回して設計し直すため、Sub-stage B 以降の Rust GpuBackend trait を並行 safety 前提で再設計する必要がある

**実施タイミング**: Sub-stage A、Sub-stage C の Mac Metal 本実装より前に結論必須。

**実測結果**(2026-04-24、Sub-stage A scenario A、PoC: `smooth-spike-poc/SmoothSpike.plugin` Mac Intel / AE 25.6.5x3):
- 観測 comp: 4K 32bpc、100 frames、Fractal Noise(Evolution time*360)+ SmoothSpike 適用、Render Queue 書き出し
- **16 thread IDs が SRG_ENTER/SRG_EXIT を発行、99 frames 分の SRG 区間で thread 間時間 overlap 0 件**
- 結論: **合格条件 (A) Serialize**。AE は同一 plugin instance への `SMART_RENDER_GPU` を per-device で直列化。本番実装で per-device mutex / per-thread pipeline pool 不要、SDK サンプル準拠の naturally-thread-safe 構造で OK
- 代替設計 (C) 発動条件には該当しないため、§3.3.1 の「per-call buffer + per-device read-only pipeline」構造そのままで進行可

---

### 4.4 GPU 失敗時 fallback 実装方式 + VRAM OOM + `PF_Err` 時の Render Queue 挙動(優先度: 高)

**背景**: §3.3.1 / 研究 doc §6.8。`SMART_RENDER_GPU` の input/output は GPU world(device memory)で受け渡されるため、GPU 失敗を catch して **その同じ call 内で CPU 経路を実行して output を埋める** には **device→host の download、CPU 処理、host→device の upload** が必要になる。SDK サンプルはこの download-process-upload を書いておらず、失敗時は単に `PF_Err` を返す。smooth は once-fallen-always-fall policy を採るため、**当該 frame を fail させずに CPU に切り替えて完走させたい** が、「`PF_Err` を返した frame で AE の Render Queue job がどう振る舞うか」も未知。ここを決めないと §3.3.3 条件 6 の (i) / (ii) を選べない。

**方法**: 3 つの部分 spike を並行で実施。
1. **device-host 転送実装**: Metal / CUDA それぞれで `SMART_RENDER_GPU` 内で input GPU world を CPU buffer に download、既存 Rust CPU 32bpc 実装で処理、output GPU world に upload する経路を PoC で実装
   - Metal: unified memory(Apple Silicon)と discrete(Intel Mac)の両方で計測
   - CUDA: `cudaMemcpy` D2H + H2D の往復 overhead を計測
   - 4K 32bpc 1 frame の総 overhead を GPU compute 時間の何倍か比で記録(**閾値は実測後に確定、RFC に追記する運用**、下記合格条件参照)
2. **`PF_Err` 返却時の AE 挙動**: PoC で特定 frame(例: frame index % 10 == 3)で意図的に `PF_Err_INTERNAL_STRUCT_DAMAGED` 等を返し、Render Queue 書き出しで:
   - (a) job が abort するか、(b) 当該 frame だけ skip / 空白で完走するか、(c) AE が自動 retry するか、を観測
   - Mac + Win 両 platform で確認
3. **OOM 時の AE 挙動**:
   - **primary**: `SMOOTH_FORCE_GPU_ERROR=oom` フック(§3.3.2 で定義)を経由して `AllocateDeviceMemory` 相当の失敗戻り値を擬似注入、AE への返却で Render Queue の振る舞いを観測。再現性と実施コストの点でこちらを主経路とする
   - **補助**: 可能なら実 VRAM 枯渇も試す(別 process で GPU 占有、AE と同じ allocator を使う保証はないため参考値扱い)。手段は Sub-stage A で詳細化

**合格条件**(3 部分の結果で 2 つの設計分岐を決める):
- **(i) 採用**: 部分 1 で download-process-upload が実装可能、かつ overhead が **実用許容範囲内**(閾値は実測後に RFC 追記で確定、初期の見当として ~2× GPU compute 時間、超えた場合に連続判定に進む)→ once-fallen 発動 frame は `PF_Err_NONE` で完走、以降は PreRender で CPU に振り分け
- **(ii) 採用**: 部分 1 が実装不能 or 過大 overhead(下記連続判定で NG)、かつ部分 2 で「AE が `PF_Err` を返した frame を単体失敗として扱い、job を abort しない」が取れている場合 → 当該 frame は `PF_Err` で fail(warning 表示等あれば受容)、以降は CPU 固定
- **連続判定**(閾値固定せず、PoC 実測結果で決定):
  - overhead ≤ 2× 相当: (i) を主採用
  - overhead が 2× を超え 5× 以下: UX 影響(once-fallen 発動は基本 1 frame のみ、その後は CPU 経路に切替わるので「1 frame だけ遅い」事態)と失敗頻度で再判断。1 frame 限定なら許容する余地あり
  - overhead が 5× を超えた場合: 実用外と扱い、(ii) が成立するなら (ii) を採用
  - 上記いずれも不成立なら下記「不合格時の代替設計」に進む
- **実測結果を Sub-stage A 完了時に本 §4.4 に追記**(overhead 数値、AE 挙動、採用分岐、根拠)。hard cut を RFC draft 時点で固定しないこと
- **部分 3(OOM)**: (i) 採用時は OOM を trigger に (i) を実行、(ii) 採用時は OOM で `PF_Err` 返却

**不合格時の代替設計**: (i) も (ii) も採用できない(転送高すぎ、かつ AE が job abort する)場合:
- 代替 1: **事前予防型**。PreRender で VRAM 使用量を保守的に見積もり、閾値超過なら `GPU_RENDER_POSSIBLE` を立てず、AE に CPU 経路を選ばせる。GPU 失敗を render 時点で起こさせない
- 代替 2: **per-device concurrent dispatch の制限**。AllocateDeviceMemory の失敗頻度を下げるため MFR 並列度を plugin 側で絞る(ただし AE 側に並列度ヒント API がないため難しい)
- 代替 3: **Phase 2-A.3 を Fallback 出荷パス(§5.1)に切替**(GPU 機能を v1.6.0 から外す判断、**default = `v1.6.0 32bpc-only`** で出荷。§4.4 検証時点で 2-A.2 完了済みなので `v1.5.2` は該当しない、§5.1.2 table 参照)

**実施タイミング**: Sub-stage A、Sub-stage C より前。**優先度は 4.1 より高い**(4.1 不合格でも per-thread pipeline で吸収可能だが、4.4 不合格は fallback policy そのものが崩れて §5.1 発動の可能性を招く)。

**実測結果**(2026-04-24、Sub-stage A scenario D / E、Mac Intel / AE 25.6.5x3):

- **Part 2 `PF_Err_INTERNAL_STRUCT_DAMAGED` 返却時の AE 挙動**(`SPIKE_FORCE_ERROR=render`、frame%10==3 で注入):
  - frame 3 で注入 → AE が別 thread で **retry** → 再度失敗 → **job abort + エラーダイアログ "Error Code 512"**
  - 残り frames は rendering 途中で停止
  - → **(ii) `PF_Err` + 次 frame CPU 固定 方式は採用不可**(AE が retry 後 abort するため Sub-stage F Render Queue 完走要求と両立不能、§3.3.3 条件 6 の前提違反)

- **Part 3 `PF_Err_OUT_OF_MEMORY` 返却時の AE 挙動**(`SPIKE_FORCE_ERROR=oom`):
  - AE は OOM を **GPU 専用エラーとして特別扱い**、GPU Effects Error dialog を表示(code 19969 系)
  - Dialog は「Ignore / Render Effects Using Software Only」の 2 択、**user 介入必須**
  - Ignore 選択 → 同 frame retry → 再度 error dialog → batch render 進行不能
  - → **OOM でも (ii) 系は採用不可**(user-visible dialog がブロック、batch / aerender.exe 無人運転と両立不能)

- **Part 1 device→host→device の overhead**: 本 PoC で未実装、DPU patch(C)を追加して Sub-stage A 後半 or 本実装中に計測

**採用分岐 確定**: **(i) device→host→device + `PF_Err_NONE` が唯一の有効 fallback 実装方式**。本番実装で MUST 実装。overhead は実測後に本節に追記。

---

### 4.5 Render Queue 書き出しが SETUP/RESETUP 区間で完結する前提検証(優先度: 高)

**背景**: 研究 doc §4.6 / §2.3 確定事項。once-fallen-always-fall policy の sticky 範囲を「per SETUP/RESETUP 区間」と定めたのは、**単一の Render Queue 書き出し中に `SEQUENCE_RESETUP` が発火しない** という前提に依存する。もし書き出し中に RESETUP が fire すると UUID が再生成され、`GPU_FALLEN` lookup が miss に戻り、**batch 中盤で GPU 再試行が再開** して boundary residual artifact のリスクが出る。「user が mid-batch で params を触らない」だけなら安全だが、AE が自動 snapshot や Render Queue 内部事情で RESETUP を fire させる可能性が否定できていない。

**方法**:
1. PoC で `SEQUENCE_SETUP` / `SEQUENCE_RESETUP` / `SEQUENCE_SETDOWN` の各エントリに観測用 ID セットをログする:
   - `timestamp_ns`、`effect_ref` ポインタ値、`sequence handle` pointer、**旧 UUID**(RESETUP 前のもの、存在すれば)、**新 UUID**(RESETUP で再生成したもの)、`render frame index`(取得可能な場合)、`trigger_hint`(in_data から推測可能な情報: params が変わったか、save/load 直後か 等)
   - これらは分析用の観測 ID であって key 採用ではない(fallen flag の key は依然 UUID 単独)。UUID 再生成後に「同一 instance での mid-batch RESETUP」を追跡するために effect_ref / sequence handle の相関を併記
2. 3 種類の負荷シナリオで Render Queue 書き出しを走らせ、RESETUP の発火タイミングを観測:
   - **シナリオ A(素)**: 100 frames の 32bpc comp を純粋に書き出し、user 操作なし
   - **シナリオ B(自動 save 有効)**: AE の Preferences で auto-save を 1 分間隔に設定、書き出し中に auto-save が走るよう 3 分以上の render
   - **シナリオ C(並行 preview 切替)**: 書き出し中に別 comp を開く / preview タイムラインを動かす等、user が軽い操作をする
3. ログから `SEQUENCE_RESETUP` が SETUP→SETDOWN の区間内(= 書き出し中)に fire しているか確認、併せて effect_ref / sequence handle を相関させて同一 instance か別 instance かを判別

**合格条件**:
- **(A) 完全合格**: シナリオ A / B / C いずれでも Render Queue 実行中に同一 instance(effect_ref + sequence handle で相関)への `SEQUENCE_RESETUP` は fire しない → 現行 policy そのまま採用可
- **(B) 条件付き許容**: シナリオ A + B で fire せず、C のみで fire する → **合格ではなく条件付き許容**として扱う:
  - 許容する場合の必須条件: RELEASE_NOTES に「Render Queue 書き出し中に AE で他 comp を開く / preview を操作すると、該当 smooth instance の GPU sticky 状態がリセットされ、batch 途中で GPU 再試行が起きる可能性がある。boundary residual artifact の懸念がある場合は書き出し中は AE 操作を控える」を明記、UAT チェックリストにも入れる
  - 頻度 / 影響が大きい(= 通常の user workflow で頻発する、or 視覚 artifact が目立つ)と Sub-stage F UAT で判明した場合は、(B) 許容を撤回して「不合格」扱いに切替え、代替設計に進む

**不合格時の代替設計**: シナリオ A だけで RESETUP が fire する(= AE の内部事情で勝手に発火)場合:
- **代替 1**: `GPU_FALLEN` の key を `instance_uuid`(RESETUP で再生成) から **`effect_ref` 相当のプロセス生存期間 ID** に切り替える。ただし SDK に公式な「stable per-instance ID」が無いため、`in_data->effect_ref` のポインタ値等を hashable key にする危険な方法になる(AE の内部管理に依存)
- **代替 2**: `AEGP_ComputeCacheSuite` による unified cache で fallen state を persist(研究 doc §4.6 で不採用とした AEGP 経路を復活)。実装コスト大
- **代替 3**: sticky を「effect instance 全寿命」に強化し、mid-batch 再試行を諦める(研究 doc round 5 で不採用とした元設計への revert)。save/load 後の retry は失われるが batch 中の安全は確保

**実施タイミング**: Sub-stage A。実装方針への影響は大きくない(4.1 / 4.4 ほどの根幹影響はない)が、不合格時に §2.3 確定事項の再議論(`SDK 契約上の制約` 枠での §2 運用ルール発動)が必要になるため、Sub-stage A で決着させる。

**実測結果**(2026-04-24、Sub-stage A scenario A、Mac Intel / AE 25.6.5x3):
- Render span 約 40 秒間で `SEQ_RESETUP` 発火回数 = **0** / `SEQ_SETUP` = 1(バッチ開始前のみ)
- **シナリオ A(素)**: 現行 policy と整合、RESETUP は batch 内で fire しない
- シナリオ B(auto-save)/ C(並行操作)の追加観測は Sub-stage A 残件として workbench_history に記録。Sub-stage B 以降でも実施可
- 暫定結論: **(A) 完全合格の可能性が高い**が、B / C 観測後に最終確定

---

### 4.2 CUDA context push/pop の要否(優先度: 中)

**背景**: 研究 doc §4.10 / §6.2 / §6.8。SDK サンプル(SDK_Invert_ProcAmp.cpp)は `cuCtxPushCurrent` / `cuCtxPopCurrent` を呼ばずに `cuLaunchKernel` 相当を実行している。これは「AE が entry 前に current context をセット済み」という**暗黙の前提**に依存しているが、SDK header にはこの保証は明記されていない。MFR と組み合わせると、別 thread で別 context が current になっている状況で `SMART_RENDER_GPU` に入る可能性が否定できない。SDK サンプル準拠で省略してまず書き、必要なら push/pop を補う判断が必要。

**方法**:
1. Sub-stage E PoC で 2 variant を実装:
   - **variant X**: SDK サンプル準拠、push/pop なし
   - **variant Y**: `cuCtxPushCurrent(ae_ctx)` / `cuCtxPopCurrent` で push/pop の対象を **kernel launch だけでなく Rust `extern "C"` 経由の kernel 呼び出し全体** に取る(NVCC static link した entry 関数内部で memcpy や launch が連鎖するため、`cuLaunchKernel` 単体を囲むのでは不足)。`ae_ctx` は `GPU_DEVICE_SETUP` で渡された `contextPV`(CUDA の場合)を `SEQUENCE_SETUP` 時点で保持して使う
2. Win AE 2025 で MFR 有効の concurrent 書き出し(4K 32bpc heavy comp)を variant X / Y 両方で実施
3. 観測項目:
   - X で `cuGetErrorString` 相当の context-lost / invalid-context エラーが出るか
   - X での kernel launch が **誤った context 上**で実行された兆候(numeric divergence、hang、silent wrong output)があるか
   - Y の overhead 計測(期待値 < 1 µs/call、`cuCtxPushCurrent` の ABI から保守的に見積もり)

**合格条件**:
- **(A) X で問題なし**: SDK 準拠の省略で十分 → 本実装は push/pop を入れない(コード簡素化、overhead ゼロ)
- **(B) X で問題あり、Y で clean、overhead が許容範囲**: Y を採用、push/pop で囲む

**不合格時の代替設計**: 両 variant とも問題が残る(極めて考えにくいが):
- **代替 1**: **AE 提供 context を必ず使う前提を維持**しつつ、MFR 並列を実質 serialize する。具体的には plugin 側に per-device mutex を入れ、`SMART_RENDER_GPU` 全体を排他化する。AE 提供 device pointer / buffer は AE context 上のものなので、別 `cuCtxCreate` した独立 context では **それらにアクセス不能**な可能性が高く(AE 提供 pointer の context 所属が保証されていない)、plugin 専用 context の新設は不採用
- **代替 2**: CUDA backend を断念、Win は CPU 固定(§5.1 Fallback 出荷パス発動条件に該当)

**実施タイミング**: Sub-stage E(Win CUDA 本実装と同時)。Mac Metal には該当しない spike。

---

### 4.3 `GetDeviceCount` の Software Only 設定反映挙動(優先度: 中)

**背景**: 研究 doc §5.3.1。`PF_GPUDeviceSuite1::GetDeviceCount` は「host がサポートする device 数を返す」とだけ定義される。§3.3.1 の UI PF_ParamFlag_DISABLED 検出機構の **1 次候補** として使いたいが、以下 3 仮説の真偽が未確認:
- **H1**: AE の Project Settings > Video Rendering and Effects > Use: Software Only 時、`GetDeviceCount` が 0 を返す or 全 device の `compatibleB` が false になる
- **H2**: driver 不良 / GPU 非認識時も `GetDeviceCount` に反映される
- **H3**: 複数 GPU 環境で「AE が実際に使える」ものだけが列挙される(AE 側の pruning が effective)

これが全部確認できれば単一の検出源で済む。確認できない部分は OS API 直接呼び出し(`MTLCreateSystemDefaultDevice` / `cuInit` + `cuDeviceGetCount`)との組み合わせが必要。

**方法**:
1. Mac AE で 3 パターンのテスト:
   - (a) Project Settings = GPU → `GetDeviceCount` > 0 を期待
   - (b) Project Settings = Software Only → `GetDeviceCount` の戻り値を観測
   - (c) 外部 eGPU 接続環境での列挙挙動(入手困難なら skip)
2. Win AE で 2 パターン:
   - (a) NVIDIA driver 正常 + Project Settings = GPU → `GetDeviceCount` > 0
   - (b) NVIDIA driver 無効化 / アンインストール → `GetDeviceCount` の戻り値
3. PoC で `GLOBAL_SETUP` に `GetDeviceCount` + 各 device の `compatibleB` を log、さらに OS API 直接呼び出し(`MTLCreateSystemDefaultDevice != nil` / `cuDeviceGetCount > 0`)の結果を併記

**合格条件**:
- **(A) H1 + H2 + H3 すべて確認**: `GetDeviceCount` を単一検出源として採用、`PF_ParamFlag_DISABLED` を `GetDeviceCount == 0` で静的設定
- **(B) 限定的部分確認**: **H1 は確認済みで H2 / H3 だけ不確実**な場合に限り、`GetDeviceCount` を primary に保ちつつ OS API 直接呼び出しを secondary cross-check に追加。「両方が GPU あり」と答えた時のみ checkbox enabled、どちらかが 0 なら disabled。H1 が false(Software Only で GetDeviceCount が非 0 を返す)場合は OS API も GPU ありを返すので AND では Software Only を検出できない → この組合せで (B) を採用してはならない

**不合格時の代替設計**: `GetDeviceCount` の挙動が仕様と大きく乖離する(特に H1 が false で Software Only でも非 0 を返し、compatibleB も true のまま)場合:
- **代替 1**: OS API 直接呼び出しのみを検出源とする(`GetDeviceCount` は使わない)。Mac は `MTLCreateSystemDefaultDevice`、Win は `cuDeviceGetCount`。Project Settings = Software Only の事前 UI 反映は諦める(Software Only 時でも checkbox は enabled のまま、render 時に AE が GPU_DEVICE_SETUP を呼ばず自動的に CPU 経路になる動作に依存)
- **代替 2**: 検出源としては代替 1 を使いつつ、GPU_DEVICE_SETUP 失敗時は **backend-level の usable state** を plugin-global static に false で記録(`static GPU_BACKEND_USABLE: AtomicBool`)。PreRender は既存の条件 (e)「DEVICE_SETUP 成功」をこの state から読むだけで、追加実装は state 1 個と SETUP/SETDOWN フックのみ。**GPU_FALLEN(per-instance DashMap)には触らない**(全 instance 事前 set は instance enumeration API がなく実装不能、混同しないこと)

**実施タイミング**: Sub-stage A の後半 or Sub-stage D の前半。Sub-stage C の Mac Metal 実装で PoC 用に GetDeviceCount を log しておくと追加コストゼロで並行観測可能。

**実測結果**(2026-04-24、Sub-stage A scenario A、Mac Intel / AE 25.6.5x3):
- 通常設定(Project Settings = GPU)で `GetDeviceCount = 2`、両 device `framework=2`(Metal)、`compatibleB=1`
- H1(Software Only 反映)/ H2(driver 不良反映)/ H3(multi-GPU pruning)の比較観測は未実施(optional scenario F に相当)
- 暫定: **(A) を前提に本番実装を進める**(GetDeviceCount > 0 を単一検出源、Software Only 時 render が CPU に自動回る AE の挙動に依存)
- 確実性を上げるには Sub-stage D で Project Settings = Software Only シナリオ F を 1 回実施するのが望ましい。§3.3.1 代替 2 の `GPU_BACKEND_USABLE` を使えば H1 false でも実装上は吸収可能

---

### 4.6 Metal storage mode Managed vs Private の選択(優先度: 低)

**背景**: 研究 doc §6.8。Metal の buffer には storage mode があり、smooth の 2-pass 実装で使う **中間 buffer**(Pass 1 detect 結果 → Pass 2 blend 入力、GPU 内完結)は理論上 Private(GPU 専用、CPU 不可視、discrete で最速)で十分。ただし:
- Apple Silicon は unified memory で Shared が最適(Private でも動く)
- Intel Mac は discrete GPU で Private が最適、Managed(CPU-GPU 同期 auto)は overhead あり
- Private を選んだ時に Apple Silicon で regression しないか、両環境で確認が必要

**方法**:
1. Sub-stage C の Mac Metal PoC で、中間 buffer の storage mode を **Private / Managed / Shared の 3 variant** で切り替え可能にする(feature flag or env var)。Shared を候補として扱うなら実測対象に入れる
2. 4K 32bpc 1 frame の dispatch 時間を 3 variant で計測、Apple Silicon(M1 Max 等)と Intel Mac(入手可能な範囲で)両方
3. VRAM pressure 挙動(意図的に heavy scene で buffer 大量 allocate)の 3 mode 差異

**合格条件**:
- Private が両 Mac タイプで動作、Apple Silicon で Shared / Managed に対して劣化がない、Intel Mac で期待通り速い → **Private 統一採用**
- Apple Silicon で Shared が Private より有意に速い場合: Apple Silicon は Shared、Intel Mac は Private に **platform 別分岐** も許容
- Apple Silicon で Private が動作不良: Shared を default、Intel Mac だけ Private に分岐 も許容

**不合格時の代替設計**: Private に特定環境で問題(alignment / サイズ上限 / compatibility)が出る:
- Managed で統一、Intel Mac 側の overhead を受容。性能ロスは §3.3.3 条件 10 の release claim に影響しうる
- 中間 buffer サイズを縮小する shader 側の最適化(2-pass を tile 分割して中間 buffer を小さくする)を検討、実装コスト大なので v1.6.0 では見送り、Phase 2-A.4+ で

**実施タイミング**: Sub-stage C(Mac Metal 本実装と同時)。優先度低なので本実装中の計測で決着させる、Sub-stage A に前倒ししない。

---

### 4.7 GPU checkbox 状態変更時の AE 再 render invalidation 挙動(優先度: 低)

**背景**: 研究 doc §5.10。user が Effect Controls の GPU Acceleration checkbox を ON ↔ OFF に toggle した時、AE が自動的に cached preview frames を invalidate して再 render するか不明。もし invalidate されないと、checkbox OFF にした後も古い GPU 結果が画面に残り、ユーザーは「OFF にしたのに反映されない」と混乱する。

**方法**:
1. Sub-stage D で checkbox を実装した後、PoC で以下を実施:
   - 32bpc comp で smooth 適用、GPU 経路で preview を cache(タイムライン上で再生、frame キャッシュ貯める)
   - Effect Controls で GPU Acceleration を ☑ → ☐ に toggle
   - キャッシュが invalidate されるか、再生時に CPU 経路で新たに render されるかを観測
2. 同様に ☐ → ☑ の逆方向も確認
3. PoC で `PF_Cmd_USER_CHANGED_PARAM` ハンドラに log を仕込み、toggle 時に fire するか確認

**合格条件**:
- **(A)**: AE が自動 invalidate(cached frames が消え、次 render で新経路)→ 追加コード不要
- **(B)**: USER_CHANGED_PARAM は fire するが自動 invalidate しない → plugin 側で `out_data->out_flags` に `PF_OutFlag_FORCE_RERENDER` 等を立てる追加実装で対処

**不合格時の代替設計**: AE が何もしない(invalidate せず USER_CHANGED_PARAM も fire しない):
- **代替 1**: SDK の別 API(`AEGP_InvalidateAllCaches` 相当)を呼ぶ。AEGP 経由は full GPU plugin では通常使わないので追加 binding が要る
- **代替 2**: 未解決のまま RELEASE_NOTES に「GPU Acceleration toggle 後は preview を手動 purge(Edit > Purge > All Memory & Disk Cache)してください」を記載。UX 劣化だが実装回避

**実施タイミング**: Sub-stage D(UI wiring 完了後すぐ、Mac だけで十分、Win の CUDA backend 実装前に結論必要なし)。Sub-stage F UAT で最終確認。

## 5. Risks / Fallback 出荷パス

### 5.1 GPU 実装失敗時の Fallback 出荷パス

§3.3 の 2-A.3 が出荷条件を満たせない場合、何もしない(Phase 2-A 全体を retract)ではなく、2-A.1 + 2-A.2 の成果(SmartRender + 32bpc CPU)を活かして **CPU-only の中間リリース** として出荷する。ここでは trigger 条件と版数選択ツリーを明文化する。§1.3 は RFC 採択時点では未決のままでよい、その判断基準をここで定義する。

#### 5.1.1 Fallback 発動の trigger 条件(いずれか 1 つ以上が成立した時点で検討)

1. **§4 Spike 高優先度の救済不能**: 4.1 / 4.4 / 4.5 のいずれかが「不合格、かつ §4 の代替設計でも解決しない」と Sub-stage A で判明
2. **2-A.3 本実装のブロック**: Sub-stage C(Mac Metal)が PoC 結論から本実装に移行できない状態が続く(判定は §5.1.3 の evidence-based 再評価条件 = 次の仮説なし / 2 回以上の代替設計失敗 に従う、暦ベースの期限は設けない)
3. **platform 片方だけ成功**: Mac Metal は動くが Win CUDA が `§4.2 不合格時 代替 2`(CUDA 断念)に倒れた、またはその逆
4. **性能目標の完全未達**: §3.3.3 条件 10 の「明確な速度向上」が計測で取れない(CPU 経路に対して差がない or 劣る)

#### 5.1.2 版数選択ツリー(完了ステージ × 発動原因で決定)

| 2-A.1 | 2-A.2 | 2-A.3 Metal | 2-A.3 CUDA | 出荷形態 | 扱い |
|---|---|---|---|---|---|
| ✅ | ✅ | ✅ | ✅ | **v1.6.0 フル**(通常成功パス、§3.3.5) | default |
| ✅ | ✅ | ❌ | ❌ | **v1.6.0 32bpc-only**(GPU 全滅、32bpc CPU は user-facing value ありとして出荷) | **default fallback** |
| ✅ | ✅ | ✅ | ❌ | **v1.6.0 Mac GPU only**(Win は CPU 固定、checkbox は Win で DISABLED、RELEASE_NOTES で platform 差を明示) | 例外、user confirm 必須 |
| ✅ | ✅ | ❌ | ✅ | **v1.6.0 Win GPU only**(同上、逆 platform) | 例外、user confirm 必須 |
| ✅ | ❌ | 不問 | 不問 | **v1.5.2**(SmartRender のみ、marginal feature add 扱い、以降の GPU / 32bpc は Phase 2-A.4+ へ) | 例外、§1.3 の「2-A.1/2-A.2 単独リリースなし」原則からの意図的例外 |
| ❌ | - | - | - | 2-A 全体を retract(通常ここまで落ちない、§3.1.5 ゲートで検知される) | 最終手段 |

**推奨原則**:
- **原則**: 成功パスは `v1.6.0 フル`、GPU 全滅時の fallback default は `v1.6.0 32bpc-only`
- **片 platform GPU only** は例外選択肢: もう一方が明確に詰んだ(§4 Spike 救済不能 + 2 回以上の代替設計失敗)場合のみ user confirm で採用。ユーザーに「Mac users と Win users で feature が違う」認知コストを課すため、default に置かない
- **`v1.5.2`** は例外: 2-A.2 が不成立な場合のみ。§1.3 の「2-A.1 / 2-A.2 個別リリースなし」原則から意図的に逸脱する判断で、SmartRender + MFR refinement のみの minor release として扱う

#### 5.1.3 発動判断の gate タイミング

暦ベースの N 週間ルールは設けない。代わりに **evidence-based の再評価条件**を gate で適用する。再評価は以下のいずれかが成立した時点で実施:
- 次に検証可能な仮説がない(Spike / 本実装で試せる変数を使い切った)
- 2 回以上の代替設計が失敗(§4 各項目の代替案や本実装リトライを積み重ねて成果なし)
- platform 片方の backend が §4.2 代替 2(CUDA 断念)等で明示的に詰んだ

上記が見えたタイミングで以下の gate を発動:

- **Gate 1**: Sub-stage A 完了時。§4 Spike 7 項目の結論を見て、高優先度の救済不能が確定した場合、**版数選択ツリーの default fallback = `v1.6.0 32bpc-only`** に倒すことをユーザーと合意(Sub-stage A 到達時点で §7.2 Step 5 YES = 2-A.2 完了済みなので `v1.5.2` は該当しない、§5.1.2 table 参照)
- **Gate 2**: Sub-stage C 完了時。Mac Metal が動かないと確定した場合、Win CUDA 単独で進めるか(例外「Win GPU only」)、両 GPU を諦めるか(default fallback「v1.6.0 32bpc-only」)を合意
- **Gate 3**: Sub-stage E 完了時。Win CUDA が動かないと確定した場合、Mac Metal のみで v1.6.0 を出すか(例外「Mac GPU only」)、両 GPU 撤退(default fallback「v1.6.0 32bpc-only」)かを合意
- **Gate 4**: Sub-stage F(Full UAT)完了時。**出荷前の最終判断**。想定外の regression / 性能未達で発動なら、**v1.6.0 GPU claim を撤回し fallback 版数に切替**(tag / release は未発行の段階、Gate 4 は出荷手前の判断であり "既にリリース済みを戻す" 操作ではないことに注意)、または出荷せず beta 止めかをユーザーと合意

各 gate で「RFC §5.1.1 の trigger に該当 ⇒ §5.1.2 のツリーでどの行に倒すか」を `workbench_history.md` に記録し、decision commit でユーザーと共有。

### 5.2 AMD / Intel GPU Windows ユーザー扱い

§1.2 で「含まない」として明示済み。ここでは除外理由と user 影響を補完的に記述(重複ではなく補完)。

#### 5.2.1 除外理由(研究 doc §「DX12 除外判断の根拠」の要約)

1. **memory-bandwidth bound で iGPU は CPU-MFR と同等 or 劣る**: 4K 16bpc で iGPU 実効 10-20 ms vs 現行 CPU-MFR 33 ms、divergence ペナルティで逆転のリスクあり。ユーザーが「GPU を有効にしたら遅くなった」体験を招く
2. **AMD discrete Win は pro video では少数派**、かつ CPU-MFR (v1.5.1) で十分速い fallback がある
3. **Adobe の GPU サポート pattern と整合**: Lumetri / Magic Bullet / Red Giant 等、主力は CUDA + Metal の 2 本柱
4. **実装コスト削減**: shader 言語 3 → 2、Rust backend 3 → 2、build 環境要件(DXC)不要、テスト環境(AMD discrete Win)不要

#### 5.2.2 User 影響

- **AMD / Intel GPU Windows users** は §4.3 Spike 結論を反映した検出機構により GPU Acceleration checkbox が **DISABLED**(Metal / NVIDIA が見つからない環境)。画面上は checkbox がグレイアウトし操作不能、実際の動作は CPU SmartRender
- **体感性能**:
  - 8bpc / 16bpc は **v1.5.1 相当**(CPU-MFR で動作、v1.5.1 からの regression なし)
  - 32bpc は **v1.6.0 で新規対応の benefit**(v1.5.1 には 32bpc CPU baseline が存在しない、AMD/Intel Windows users も v1.5.1 では 32bpc 黄色 ⚠️ 表示だったのが v1.6.0 で正式対応に)
  - GPU accelerated users と比べて 32bpc の絶対性能は劣るが、「v1.5.1 で動かなかった 32bpc が動くようになる」点で v1.6.0 インストール価値あり
- **視覚的な差**: 8/16bpc CPU 経路は **`v1.4.0-ae2025` golden に対して IDENTICAL or 既存 NEAR-ID**(§3.1 継続)、32bpc CPU 経路は **`v1.6.0-32bpc` reference に対して IDENTICAL**(§3.2.3 条件 4 継続)、視覚上区別なし
- **RELEASE_NOTES での扱い**: 「Windows は NVIDIA CUDA 対応 driver 必須、AMD / Intel GPU は CPU fallback で動作」を §3.3.2 通り明記、AMD / Intel Windows users にも v1.6.0 をインストールする価値(32bpc 対応 + 8/16bpc の MFR)を伝える

#### 5.2.3 将来の DX12 復活 trigger(記録)

研究 doc §「DX12 復活の条件」にある通り:
- AMD discrete Win ユーザーから明確な需要申請
- Adobe が CUDA を deprecate する方向に動く
- smooth の user base が広がり vendor coverage が実運用課題になる

このいずれかが成立したら Phase 2-A.4 以降で DX12 backend 追加を検討。現時点では trigger なし。

### 5.3 将来 DX12 追加時のために守る設計原則

§2.2 確定事項 + §3.3.6 Phase 2-A.4 拡張余地と重複するため、ここは**原則の箇条書き**で要約。詳細は該当節を参照。

1. **`GpuBackend` trait は増加型変更のみ**: 新 backend 追加で既存 Mac Metal / Win CUDA の signature を変えない(method 追加 OK、既存 method 削除・変更 NG)
2. **shader 抽象化は入れない**: SPIR-V / Slang 等の共通 IR を挟まない、既存 2 backend への影響最小化。MSL + CUDA C++ + HLSL の 3 言語併存が限界、統一検討は Phase 3 以降
3. **once-fallen-always-fall の 2 層分離流用**: sequence_data UUID + plugin-global DashMap 構造は新 backend 追加時も共用、key/value 拡張不要
4. **CPU / GPU 切替 UI の popup 互換性**: checkbox(☑ Auto / ☐ CPU)から popup(Auto / CPU / GPU)に差し替える際、`true ↔ Auto`、`false ↔ CPU` のマッピングを保つ(§2.5)
5. **DX12 復活時の追加整備**: `#[cfg(target_os = "windows")]` の新 `dx12.rs` module + HLSL shader + DXC toolchain の Win CI 整備、build 環境要件の増加判断が必要(研究 doc §3.5 の compile 戦略を踏襲)

## 6. コード変更の概形

### 6.1 Rust crate 構造(研究 doc §3.4 採用案、§4.1 制約反映)

```
rust/smooth_core/
├── Cargo.toml              # 既存 + platform target 条件、test-fault-injection feature
├── build.rs                # 既存 + Mac: xcrun metal → .metallib embed / Win: NVCC → static obj
├── src/
│   ├── lib.rs              # 既存 FFI + `smooth_core_gpu_*` 系 FFI 新設(命名は §6.2 の lib.rs 2-A.3 行で統一)
│   ├── types.rs            # 既存 + SmoothPixel trait の f32 対応拡張
│   ├── {compare,blend,process,preprocess,down_mode,up_mode,lack,link8}.rs
│   │                       # 既存 CPU コア、f32 domain 対応で bpc 別分岐追加
│   ├── gpu/
│   │   ├── mod.rs          # GpuBackend trait + CPU backend 同 trait 包装 + dispatch glue
│   │   ├── metal.rs        # #[cfg(target_os = "macos")]、metal-rs / objc2-metal 使用
│   │   ├── cuda.rs         # #[cfg(target_os = "windows")]、NVCC static link した extern "C" を呼ぶ
│   │   ├── fallback.rs     # GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>> 管理、§4.4 spike 結論の fallback 実装
│   │   ├── detection.rs    # GPU 検出機構(§4.3 spike 結論、GLOBAL_SETUP で 1 回、plugin-global static にキャッシュ)
│   │   ├── tests.rs        # backend unit test、shader dispatch basics、fallen flag 動作
│   │   └── shaders/
│   │       ├── smooth.metal   # MSL 2-pass(detect + blend)
│   │       └── smooth.cu      # CUDA C++ 2-pass、NVCC static link 対象
```

**`GpuBackend` trait**(§4.1 合格条件 (B) 制約を反映):

```rust
pub trait GpuBackend {
    type Device;
    type Buffer;
    type FrameContext;   // per-call、command buffer / stream / encoder 等を抱える stack-lived 状態

    // AE が渡す raw handle から wrap(per-device、SETUP 時に 1 回)
    unsafe fn from_ae_device(device_ptr: *mut c_void, context_ptr: *mut c_void) -> Result<Self, GpuError>;

    // per-call の FrameContext 生成(render 毎、Metal なら commandBuffer 取得、CUDA なら stream 取得相当)
    fn begin_frame(&self) -> Result<Self::FrameContext, GpuError>;

    // per-call の buffer 確保(render 毎)
    fn allocate_buffer(&self, ctx: &mut Self::FrameContext, size: usize) -> Result<Self::Buffer, GpuError>;

    // dispatch、shader は per-device の read-only pipeline を参照、状態は ctx に積む
    fn dispatch_preprocess(&self, ctx: &mut Self::FrameContext, ...) -> Result<(), GpuError>;
    fn dispatch_smoothing(&self, ctx: &mut Self::FrameContext, ...) -> Result<(), GpuError>;

    // frame 完結(Metal: commandBuffer.commit のみ、CUDA: stream sync)、ctx を consume
    fn finish_frame(&self, ctx: Self::FrameContext) -> Result<(), GpuError>;
}
```

**設計意図**:
- FrameContext は per-call の stack-lived 状態(command buffer / encoder / 一時 buffer lifetime 束ね)、`&self` の field ではない → §4.1 (B) 制約の「cached command buffer なし」と整合
- Metal は `finish_frame` で commandBuffer.commit のみ(`waitUntilCompleted` は呼ばず、AE の synchronization を信頼、§3.3.6)、CUDA は stream sync
- `begin_frame` / `finish_frame` の非対称(`&mut ctx` → `ctx` consume)で同 ctx を 2 回 finish する誤用を型で防ぐ

**不変条件**(§4.1 (B) 制約、doc comment で明記):
- 実装の `&self` field には per-device の mutable shared state を持たない(pipeline / library handle 等は SETUP 時生成の read-only 参照のみ)
- global counter / shared scratch buffer / cached command buffer を持たない
- dispatch_* の一時 buffer は全て FrameContext に紐付け、`finish_frame` の ctx consume と共に解放

### 6.2 既存ファイル変更リスト

| ファイル | stage | 変更概要 |
|---|---|---|
| [Effect.cpp](../Effect.cpp) | 2-A.1 | `SMART_PRE_RENDER` / `SMART_RENDER` selector + `SUPPORTS_SMART_RENDER` flag、legacy `PF_Cmd_RENDER` 維持 |
| [Effect.cpp](../Effect.cpp) | 2-A.2 | `SmartRender` の bpc switch に `PF_PixelFloat`、`FLOAT_COLOR_AWARE` flag |
| [Effect.cpp](../Effect.cpp) | 2-A.3 | GPU 8 selector(`GPU_DEVICE_SETUP/SETDOWN` / `SMART_RENDER_GPU` / `SEQUENCE_*`)、`SUPPORTS_GPU_RENDER_F32` flag(Effect.cpp 内 2 箇所 = GlobalSetup + GPU_DEVICE_SETUP、Pipl.r 含め**全 3 箇所同期**)、`GPU Acceleration` checkbox param、About ダイアログ GPU 状態表示、`SmartPreRender` の GPU_RENDER_POSSIBLE 5 条件判定 |
| [Pipl.r](../Pipl.r) | 2-A.1 / 2-A.2 / 2-A.3 | `AE_Effect_Global_OutFlags_2` に `SUPPORTS_SMART_RENDER`(2-A.1)、`FLOAT_COLOR_AWARE`(2-A.2)、`SUPPORTS_GPU_RENDER_F32`(2-A.3)を OR。Effect.cpp comment `must match ...` との同期ルール継続 |
| [rust/smooth_core/src/lib.rs](../rust/smooth_core/src/lib.rs) | 2-A.2 | `smooth_core_preprocess_f32` / `smooth_core_process_row_range_f32` FFI 新設 |
| [rust/smooth_core/src/lib.rs](../rust/smooth_core/src/lib.rs) | 2-A.3 | GPU dispatch FFI 新設(`smooth_core_gpu_*`)、UUID 生成 / GPU_FALLEN query / GPU backend detection の C++ 側 interface |
| [rust/smooth_core/src/types.rs](../rust/smooth_core/src/types.rs) | 2-A.2 | `SmoothPixel` trait を f32 対応に拡張、`delta_sum` / `max_value` の型を associated type に(u8/u16 は u32、f32 は f32) |
| [rust/smooth_core/src/{compare,blend,preprocess,process,down_mode,up_mode,lack,link8}.rs](../rust/smooth_core/src/) | 2-A.2 | 既存 integer domain 実装を trait 経由に統一、f32 分岐追加。`range` 内部換算の bpc 別分岐 |
| [rust/smooth_core/Cargo.toml](../rust/smooth_core/Cargo.toml) | 2-A.2 / 2-A.3 | Mac: `metal-rs` / `objc2-metal`(target 条件)、共通: `dashmap` / `uuid` / `once_cell`、`test-fault-injection` feature 追加 |
| [rust/smooth_core/build.rs](../rust/smooth_core/build.rs) | 2-A.3 | Mac: `xcrun metal -c` → `xcrun metallib` → `.metallib` embed、Win: NVCC で `.cu` → static obj、linker に渡す |
| [tests/regression_test.cpp](../tests/regression_test.cpp) | 2-A.2 | Pixel32 (4×f32) 比較サポート、NEAR-ID tolerance を manifest 駆動化 |
| [tests/compare_raw.py](../tests/compare_raw.py) | 2-A.2 | manifest 読み込み、per-frame policy 解決、Pixel32 比較 |
| [tests/run_regression.sh](../tests/run_regression.sh) | 2-A.2 | manifest 駆動化、不足時 `fetch_goldens.sh` 呼び出し |
| [.gitignore](../.gitignore) | 2-A.2 | §3.2.2 の `.gitignore` パターン(親 unignore → 中身 ignore → manifest 許可) |

### 6.3 新規 crate / module / 成果物

| path | stage | 内容 |
|---|---|---|
| `rust/smooth_core/src/gpu/mod.rs` | 2-A.3 | GpuBackend trait、dispatch、CPU 包装 |
| `rust/smooth_core/src/gpu/metal.rs` | 2-A.3 | Mac Metal 実装(`#[cfg(target_os = "macos")]`) |
| `rust/smooth_core/src/gpu/cuda.rs` | 2-A.3 | Win CUDA 実装(`#[cfg(target_os = "windows")]`) |
| `rust/smooth_core/src/gpu/fallback.rs` | 2-A.3 | `GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>>` 管理 |
| `rust/smooth_core/src/gpu/detection.rs` | 2-A.3 | GPU 検出機構、`GPU_BACKEND_USABLE: AtomicBool`(§4.3 代替 2 対応も可能な実装) |
| `rust/smooth_core/src/gpu/tests.rs` | 2-A.3 | GPU backend unit test |
| `rust/smooth_core/src/gpu/shaders/smooth.metal` | 2-A.3 | MSL 2-pass shader |
| `rust/smooth_core/src/gpu/shaders/smooth.cu` | 2-A.3 | CUDA C++ 2-pass kernel |
| `tests/capture_32bpc.py` | 2-A.2 | AE EXR → SMDP `.raw` 変換、依存 pin + channel 順序 + overbright clip policy を header comment に |
| `tests/fetch_goldens.sh` | 2-A.2 | GitHub Release から tar.zst DL + per-file SHA256 検証 |
| `tests/gpu_fallback_test.cpp` | 2-A.3 | `SMOOTH_FORCE_GPU_ERROR` 経由の once-fallen 動作確認 |
| `tests/goldens/v1.4.0-ae2025/manifest.json` | 2-A.2 | 既存 14 frames の backfill manifest(frame 135 の NEAR-ID policy_overrides 含む)、§6.2 の `.gitignore` パターンで commit 対象 |
| `tests/goldens/v1.6.0-32bpc/manifest.json` | 2-A.2 / 2-A.3 | 14 frames 32bpc manifest、2-A.3 で `gpu_metal_policy` / `gpu_cuda_policy` 追加、§6.2 の `.gitignore` パターンで commit 対象 |
| `RELEASE_NOTES-v1.6.0.md` | 2-A.3 (成功パス) | §3.3.2 の 6 項目 GPU 注意事項含む |
| fallback RELEASE_NOTES | fallback パス、**どちらか 1 本**(§5.1.3 ゲートで確定) | 発動時のみ作成、§5.1.2 の行に応じて `RELEASE_NOTES-v1.6.0.md` の variant(32bpc-only / Mac GPU only / Win GPU only)または `RELEASE_NOTES-v1.5.2.md` のいずれか |

## 7. Task 分解(Step 粒度、`workbench_history.md` と 1:1)

絶対日付なし、各 Step に **成果物 / go-no-go 判断条件** を記す。各 Step 完了時に `workbench_history.md` に `### YYYY-MM-DD HH:MM JST — Phase 2-A.X Step N: <summary>` 形式でエントリ追加してから commit(memory 記録ルール)。

### 7.1 Phase 2-A.1 Step リスト(SmartRender 経路追加、2 Steps)

- **Step 1**: Effect.cpp に `SMART_PRE_RENDER` / `SMART_RENDER` ハンドラ追加、GlobalSetup + Pipl.r に `SUPPORTS_SMART_RENDER` を OR、`SmartPreRender()` / `SmartRender()` 関数実装、legacy `PF_Cmd_RENDER` ハンドラ維持。`tests/run_regression.sh` と `cargo test` で §3.1.3 条件 1/5/6 PASS。Go-no-go: local regression 全 PASS かつ build warning なし
- **Step 2**: Mac + Win AE 2025 実機検証(§3.1.4 Step 2-4)、debug-only instrumentation で SMART_RENDER 経路到達確認後 instrumentation 削除。§3.1.3 条件 2/3/4 PASS、§3.1.5 ゲート全 YES。Go-no-go: §3.1.5 チェック全 YES → 2-A.2 Step 1 へ / NO → 原因切り分け(§3.1.5 の 3 分岐)

### 7.2 Phase 2-A.2 Step リスト(32bpc + manifest 化、5 Steps)

- **Step 1**: Rust `smooth_core` の f32 domain 拡張(`SmoothPixel` trait / `types.rs` / 既存 CPU 本体の bpc 別分岐 / `range` 内部換算)。`cargo test --release` で f32 unit test 含め全 PASS、overbright / NaN / Inf 防御 test も synthetic unit で PASS。Go-no-go: Rust 単体 test 全 PASS
- **Step 2**: Effect.cpp `SmartRender` の `PF_PixelFloat` 分岐追加、GlobalSetup + Pipl.r に `FLOAT_COLOR_AWARE` を OR、Rust `smooth_core_*_f32` FFI 接続。Go-no-go: 8/16bpc 既存 regression 非劣化 + 32bpc 単体 cargo test PASS
- **Step 3**: Test harness manifest migration(`compare_raw.py` / `regression_test.cpp` / `run_regression.sh` manifest 駆動化、`v1.4.0-ae2025/manifest.json` backfill、`.gitignore` パターン更新、frame 135 の既存 NEAR-ID policy_overrides 表現)。Go-no-go: 既存 14 frames が manifest 駆動でこれまでと同じ結果(13 IDENTICAL + 1 NEAR-ID frame 135)
- **Step 4**: 32bpc goldens capture(Mac AE で §3.2.4 Step 2 の手順実行、`capture_32bpc.py` 作成、tar.zst artifact 作成、GitHub Release pre-release tag に添付、`v1.6.0-32bpc/manifest.json` 作成、`fetch_goldens.sh` 作成)。Go-no-go: fresh clone から `fetch_goldens.sh` → `run_regression.sh` で 32bpc 14/14 IDENTICAL
- **Step 5**: Mac + Win cross-platform 32bpc 検証(§3.2.4 Step 3-5)。§3.2.3 条件 1-8 PASS、§3.2.5 ゲート全 YES。Go-no-go: §3.2.5 チェック全 YES → 2-A.3 Step 1 へ / NO → 原因切り分け + §5.1 Gate 1 検討

### 7.3 Phase 2-A.3 Step リスト(GPU + v1.6.0 出荷、6 Steps)

- **Step 1 (Sub-stage A)**: §4 Spike 7 項目を実測。SDK_Invert_ProcAmp.cpp への直接パッチ / disposable PoC での観測を許容、本番実装より前に結論を §4 本文に追記。Go-no-go: 優先度高(4.1 / 4.4 / 4.5)全項目が PASS または代替設計で救済可能 → Step 2 へ / どれかが救済不能 → §5.1 Gate 1 発動、**原則として `v1.6.0 32bpc-only` に倒す**(この時点で §7.2 Step 5 は YES = 2-A.2 完了済みなので `v1.5.2` は該当しない、§5.1.2 table 参照)
- **Step 2 (Sub-stage B)**: Rust `gpu/` scaffold(`mod.rs` の GpuBackend trait、CPU backend 同 trait 包装、shader 空ファイル、`build.rs` compile 通し)。Go-no-go: `cargo test --release` で既存 CPU regression が trait 経由でも壊れないこと
- **Step 3 (Sub-stage C)**: Mac Metal backend 実装(metal.rs + MSL shader + Effect.cpp GPU 8 selector + sequence_data + GPU_FALLEN + PreRender 5 条件 + 基本 checkbox stub)+ `SMOOTH_FORCE_GPU_ERROR` injection test + MFR + GPU stress。Go-no-go: §3.3.4 Sub-stage C の 1-8 全 PASS
- **Step 4 (Sub-stage D)**: UI DISABLED wiring(§4.3 spike 結論反映)+ About ダイアログ GPU 状態表示 + checkbox invalidation(§4.7 spike 結論反映)。Go-no-go: Mac で checkbox ON/OFF の invalidate 動作確認、Mac(artificial disable でも可)で DISABLED 動作確認
- **Step 5 (Sub-stage E)**: Win CUDA backend 実装(cuda.rs + CUDA shader + NVCC build.rs + §4.2 context 判断反映)+ Win 実機 regression + fallback injection + MFR + GPU stress。Go-no-go: §3.3.4 Sub-stage E の 1-6 全 PASS / CUDA 断念なら §5.1 Gate 3 で判断(**default = `v1.6.0 32bpc-only`、例外 = `Mac GPU only`(user confirm 必須、§5.1.2 扱い列参照)**)
- **Step 6 (Sub-stage F)**: Full UAT(§3.3.4 Sub-stage F の 4 組み合わせ regression + 性能測定 + 非対応 system DISABLED 確認)+ `RELEASE_NOTES-v1.6.0.md` 作成 + Mac universal + Win x64 build + 偽成功検証 3 段 + 配布 zip + `v1.6.0` tag + GitHub Release。Go-no-go: §3.3.5 v1.6.0 出荷判断基準 全 YES / NO なら §5.1.3 Gate 4 発動

## 8. Open Questions / Deferred Work

### 8.1 Open Questions(RFC 採択後に判断が必要になり得るもの)

本 RFC で方針を固定したが、実装 Phase 中の観測次第で再判断が必要になる項目。発生したら別 PR で議論。

- **GPU goldens 独立 artifact 化**: §3.3.6 の通り v1.6.0 時点では CPU reference に対する policy で運用。GPU 側の丸め順序差が安定に観測され、「GPU regression を CPU regression から切り離したい」需要が出たら manifest schema 拡張余地(§3.2.6)を有効化して `v1.6.0-32bpc-gpu-metal/` / `v1.6.0-32bpc-gpu-cuda/` を後付け導入する判断
- **About ダイアログ GPU 状態の動的更新**: v1.6.0 は静的テキスト(§2.5 / §5.6 研究 doc)で確定。ARBITRARY_DATA 動的更新は UX 改善 trigger(user が mid-session で GPU device を差し替える等)が明確になったら検討
- **GPU 強制モード(Auto / CPU / GPU popup)**: v1.6.0 は checkbox のみ(§2.5)。「GPU 失敗時に黙って CPU に落ちると困る power user」からの明示需要が出たら v1.7.x 以降で popup に差し替え。checkbox ↔ popup の mapping 互換(§5.3 原則 4)は維持

### 8.2 Deferred / Future Work(Phase 2-A.4+ への記録、本 RFC では判断しない)

方針が既に「やらない(当面)」で固まっている項目の記録。trigger 条件が揃わない限り re-open しない。

- **32bpc overbright / HDR シーン test 素材の追加**: §3.2.1 で 2-A.2 スコープ外。trigger = 32bpc 価値を user に示す必要が出た時(sample 収集コストが trigger の裏返し)
- **Mac + Win f32 cross-platform 絶対 bit-identical**: §3.2.3 条件 5 で near-ID(`max_abs_f32 <= 1e-5`)許容を設けた。byte-exact 追求は smooth user にとっての実用差が小さいため先送り
- **DX12 / AMD / Intel backend 追加**: §5.2.3 trigger(AMD ユーザー需要申請 / Adobe CUDA deprecate / user base 拡大)が揃ったら Phase 2-A.4+ で検討。設計原則は §5.3 に記録済み
- **ビルド決定論化**: `workbench_history.md` L1195 付近に Phase 3 以降の CI パイプライン整備事項として記録済み(Windows `/Brepro`、Mac `--timestamp=none`、Rust `--remap-path-prefix`)
- **Phase 2-B 以降の機能追加**: 隣接ピクセル重み調整など v1.5.1 で「Phase 2-A or 機能拡張」と並記されていた選択肢。v1.6.0 出荷後の user feedback を見てから判断

## 9. 参照

- [`docs/PHASE_2A_GPU_RESEARCH.md`](PHASE_2A_GPU_RESEARCH.md)(前提 doc、本 RFC の根拠全部)
  - §2.3 32bpc 拡張影響 / §2.4 GPU 実装戦略 3 案 / §3.4 crate 構造提案 / §3.5 shader compile 戦略 / §4.6 fallback policy / §5.3.1 UI disabled 検出 / §6.2 SDK_Invert_ProcAmp 構造 / §6.5 2 層分離
- AE SDK `Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp`(full GPU plugin canonical reference、80% 流用)
- AE SDK `Examples/Headers/AE_EffectGPUSuites.h` / `AE_Effect.h`(行番号は AE SDK 25.6_61 での参照、macro/selector 名を主、行番号を補助に)
  - `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` の `GPU_DEVICE_SETUP` 宣言要件(AE_Effect.h L1007 付近)
  - `PF_Cmd_SEQUENCE_RESETUP` のトリガ 3 経路(save/load / duplicate / in_data 変更)と thread-affinity(AE_Effect.h L1094-1099 / L1112-1113 付近)
  - `PF_Cmd_SEQUENCE_SETUP` の `GET_FLATTENED_SEQUENCE_DATA` 有効時 UI thread 限定(AE_Effect.h L1123 付近)
  - `PF_Cmd_SEQUENCE_SETDOWN` の thread-affinity 未保証(AE_Effect.h L1140 付近、記述無しで読む)
  - `PF_EffectSequenceDataSuite1::PF_GetConstSequenceData` の read-only 契約(AE_Effect.h L926-930 付近 + AE_EffectSuites.h)
  - `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` の "span of frames 境界で discard" 仕様(AE_Effect.h L1010 付近)
- `RELEASE_NOTES-v1.5.1.md`(Phase 2-B 完了時点の contract)
- [`workbench_history.md`](../workbench_history.md)(実装 Step ログ、Phase 2-A 着手以降の進捗 + Phase 2-B 成果物)
  - L1135 付近: Windows AE の GUI render log には Multithreaded render report が含まれない件、aerender.exe stdout が一次証跡という運用確立(§3.1.4 Step 3 / §3.3.3 条件 7 の根拠)
  - L1156 付近: v1.5.1 配布ゴールド SHA 参照値(偽成功検証 3 段の CI 基準点)
  - L1195 付近: ビルド決定論化の将来課題(§8 Open Questions 参照)
