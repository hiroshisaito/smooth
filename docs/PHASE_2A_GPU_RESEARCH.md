# Phase 2-A: GPU 対応 調査ノート

開始日: 2026-04-23
最終改訂: 2026-04-23(review round 4 反映)
対象: smooth プラグインの GPU レンダリング対応(Phase 2-B MFR 済み、タグ `v1.5.1` リリース済の上に載せる形)

**現行リポジトリのバージョン文字列基準**:
- **Release tag / GitHub Release / README / RELEASE_NOTES-v1.5.1.md**: `v1.5.1`
- **`version.h` / About ダイアログ埋め込み文字列**: `v1.5.0`(Phase 2-B では `version.h` を bump しなかったため現行バイナリの About は `smooth, v1.5.0`、build-id 行で SHA で区別する方針)
- Phase 2-A 着手時に `version.h` を `v1.6.0` 目標として bump 予定(本文中「現行 About 表示」と「現行リリース名」はこの前提で読むこと)

## 改訂履歴

- **2026-04-23 round 1 review 反映**: GPU selector を `PF_Cmd_SMART_RENDER_GPU` 前提に統一、CPU fallback 経路を legacy `PF_Cmd_RENDER` ではなく SmartRender 化後の `PF_Cmd_SMART_RENDER`(CPU)に修正、CUDA context push/pop 方針を「サンプル準拠で省略、spike で検証」に後退、once-fallen-always-fall を sequence_data 単位に一本化、Metal `waitUntilCompleted` を commit-only に統一、FLOAT_COLOR_AWARE flag を smooth の ToDo に明示追加、DX12 ステージ表矛盾修正、range UI 値域を実コードに合わせて訂正、進捗表を更新、参考実装を `SDK_Invert_ProcAmp` に確定(GP/EMP は blit hook で別物)、`.claude/` 非公開メモ依存を本文に繰り込み、GetDeviceCount の project-level 設定反映の断定を仮説化、newBufferWithLength NG の表現を「フレーム本体のみ」に限定。
- **2026-04-23 round 2 review 反映**: once-fallen-always-fall の保存設計を **sequence_data 直接書き込みから 2 層分離に変更**(sequence_data には UUID のみ、fallen flag は plugin-global `DashMap<UUID, AtomicBool>`)、理由は SDK の render 時 sequence_data read-only 契約(AE_Effect.h L926)と `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` の span-boundary-discard 仕様が本用途と不整合のため。永続化自己矛盾(save/load 後の再試行可否)も 2 層分離で解消。CUDA context push/pop を Item 6 側でも「spike 検証、default は SDK 準拠で省略」に統一。GetDeviceCount 由来の判定を結論部でも「第一候補 + spike 実測確認」に揃える。Item 6 §6.3 差分表の sequence_data セル、§5.8 sequence_data 節を新設計に書き直し。冒頭プラットフォーム順序節を現結論(Metal + CUDA、DX12 defer)に合わせて書き直し、当初案だった wgpu/Vulkan 抽象化も議論終了済みである旨を明記。
- **2026-04-23 round 4 review 反映**: SEQUENCE_RESETUP / SEQUENCE_SETDOWN の thread-affinity 記述を修正。round 3 で §4.6 に「main-thread 系 selector のみで書き込む」「SETDOWN の selector は main thread」という表現が残っていたが、SDK AE_Effect.h L1123 は RESETUP が either thread で発生する旨を明記、L1140 の SETDOWN 記述に thread 保証は無い。main-thread 前提で lock 無し書き込みを設計すると AE が render thread で RESETUP/SETDOWN を発行した際に race を起こすため、表現を「render 以外の lifecycle selector で書く、ただし thread-affinity は前提しない。副作用(GPU_FALLEN insert/remove、pipeline HashMap 更新等)は thread-safe 構造で扱う」に修正(§4.6 の書き込み範囲説明、SEQUENCE_SETDOWN 動作フロー bullet、§4.6 要件表の read-only 契約行の 3 箇所)。SETUP のみ GET_FLATTENED_SEQUENCE_DATA 有効時に UI thread 保証(L1123)であることを併記。
- **2026-04-23 round 3 review 反映**: (1) **SEQUENCE_RESETUP で UUID を必ず再生成する方針に修正**。SDK AE_Effect.h L1094-L1113 の通り RESETUP は save/load・duplicate(複製元と複製先の両方)・in_data 変更で呼ばれ、plugin から duplicate を判別できない。以前の「flattened から UUID 復元」案では複製元と複製先が同一 UUID を共有し `GPU_FALLEN` 干渉と片側 SEQUENCE_SETDOWN による他方の sticky 状態消去が起きるため、RESETUP で常に新 UUID を振り直す形に統一。save/load 越しは `GPU_FALLEN` がプロセス境界で消えるので新 UUID でも fresh retry という意図通りの挙動になる。(2) **`PF_EffectSequenceDataSuite1` の骨子コードを実 API に訂正**。SDK ヘッダ AE_GeneralPlug.h L5713-L5718 の通り suite は `PF_GetConstSequenceData(PF_ProgPtr, PF_ConstHandle*)` 一本で、checkout/checkin ペアは存在しない。round 2 で書いていた `CheckoutConstSequenceData` / `CheckinSequenceData` 名の骨子をそのまま書くとコンパイル通らないので、`PF_ConstHandle` 受け取り → 1 段 dereference → struct cast に書き直し。(3) **§4.8 SmartRender パイプライン図の "sequence_data の gpu_fallen セット" 表記を修正**(round 2 で 2 層分離に移行したのに 1 箇所旧設計のまま残っていた)、"plugin-global DashMap<UUID, AtomicBool>(GPU_FALLEN)に fallen セット" に訂正。(4) **`SmoothSequenceData` の ABI を 2 × u64(`instance_uuid_hi`/`instance_uuid_lo`)に本 doc 全域で統一**、§4.6 冒頭で `instance_uuid: u128` になっていた箇所を §6.5 と揃えた(C 側 align と C⇄Rust FFI 互換のため 2 × u64 を採用)。

## スコープ確定

### プラットフォーム順序(最終確定、Item 3 結果反映済み)
**Mac (Metal) 先行 → Win (CUDA) 後追い、DX12 は defer**

理由: 一気に両プラットフォーム対応すると変数が多くなり debug 困難になるため、Mac Metal で "動く・安定・CPU fallback あり" を実証してから Windows (CUDA) に展開する。

当初案では「クロスプラットフォーム抽象化(wgpu / Vulkan)も調査次第で残す」としていたが、Item 3 の調査結果で **wgpu / Vulkan は AE の native handle 受入設計と相性が悪く** 議論終了とし、**native per-platform(Metal + CUDA)で進める**ことに確定(§「不採用」節、Item 3 §3.1-§3.7 参照)。

### 成功条件(優先順位つき)

1. **安定性**: GPU 経路で AE がクラッシュしない、render session 中に一貫して動く
2. **画質**: CPU 経路の出力と視覚上無差別(byte-identical は要求しないが、near-identical でユーザーが区別不可)
3. **保守性**: 将来 AE SDK / GPU driver / Rust toolchain 更新で継続メンテできる構造
4. **性能**: GPU fallback なしのベストケースで明確な速度向上(具体目標は Item 2 のプロファイル結果から逆算)

**明示的に回避するもの**:
- ピーキーなアーキ依存チューニング(特定 GPU モデル専用の shader トリック等)
- "動くが保守不能" な外部 dependency(メンテされていない Rust GPU crate 等)
- CPU fallback の品質劣化(GPU 化によって CPU 経路がバグったら本末転倒)

**Phase 2-B MFR 対応(v1.5.1)で実装済みの thread-safety 契約は必ず保持する**。GPU 対応は MFR の上に積み、MFR-compatible な設計(per-thread GPU resource 等)にする。

## 調査項目一覧

| # | 項目 | 状態 |
|---|---|---|
| 1 | AE SDK GPU API 把握 | **完了**(§1) |
| 2 | smooth アルゴリズムのプロファイリング | **完了**(§2) |
| 3 | Rust GPU バインディング選択肢の比較 | **完了**(§3) |
| 4 | MFR + GPU の両立要件 | **完了**(§4) |
| 5 | CPU/GPU 切替 UI 設計 | **完了**(§5) |
| 6 | 競合 / 参考実装調査 | **完了**(§6) |

## 実装順序とリリース方針(2026-04-23 確定)

### 実装ステージ分割

| ステージ | 範囲 | 単独リリース? |
|---|---|---|
| Phase 2-A.1 | SmartRender 経路追加(legacy `PF_Cmd_RENDER` 残しつつ `PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` を追加実装) | **しない**(GPU 下準備として) |
| Phase 2-A.2 | 32bpc 対応(アルゴリズムを f32 domain に拡張、**32bpc goldens を新規取得**) | **しない**(GPU 下準備として) |
| Phase 2-A.3 | GPU render 実装(Mac Metal + Win CUDA、DX12 は defer) | **する**(これが Phase 2-A の出荷物) |

SmartRender / 32bpc はプラグイン単体での user 価値が薄いため、単独リリースしない。3 ステージ分割は実装・検証のしやすさのためで、GPU が成功した時点で**合算して 1 つのリリース**(例: v1.6.0 GPU-accelerated)として出荷する。

### GPU 実装失敗時の fallback リリース計画

Phase 2-A.3 GPU 実装が技術的または時間的に行き詰まった場合:
- 代替案として Phase 2-A.1 + 2-A.2 の成果(SmartRender + 32bpc)のみを組み込んだ CPU-only リリースを出荷する可能性を保持
- この場合のバージョンは v1.5.2(マイナー機能追加)または v1.6.0(32bpc は新機能)を想定
- 32bpc 単独の user 価値は薄いが、「投資回収しない」よりは「下準備分は出荷」の方が Good

### 32bpc goldens 方針

- 既存 `tests/goldens/v1.4.0-ae2025/` は 8/16bpc のみ
- 32bpc 用は新規取得(別ディレクトリ or v1.6.0-32bpc などでバージョン管理)
- 取得元は現行 CPU 実装の 32bpc 版が完成した時点(Phase 2-A.2 で同時確立)

### GPU framework 確定(2026-04-23、DX12 defer 判断含む)

**Mac**: Metal native のみ
**Windows**: **CUDA のみ**(NVIDIA 専用)

**DX12 は Phase 2-A.3 スコープから除外**、defer(Phase 2-A.4 以降 or 無期限保留)。

#### DX12 除外判断の根拠(2026-04-23)

1. **memory bandwidth bound アルゴリズムで iGPU は CPU-MFR と同等 or 劣る**:
   - Intel HD 520〜UHD 630 (DDR4 shared, ~30 GB/s) → 4K 16bpc 理論下限 4.3 ms、実効 10-20 ms = 現行 CPU-MFR (16 core i9 で 33 ms) と拮抗
   - AMD iGPU (Ryzen 5000G/6000G) も同様
   - divergence ペナルティが乗ると GPU 経路の方が遅くなるケース多発の可能性
   - ユーザーが「GPU を有効にしたら遅くなった」という体験を招くリスク
2. **AMD discrete GPU Windows ユーザーは pro video では少数派**、かつ CPU-MFR (v1.5.1) で十分速い fallback がある
3. **Adobe 自身の GPU サポート パターンと整合**: Lumetri / Magic Bullet / Red Giant 等、主力は CUDA + Metal の 2 本柱、DX12 は後追い一部
4. **実装コスト削減**: shader 言語 3 → 2、Rust backend 3 → 2、build 環境要件(DXC)不要、テスト環境(AMD discrete Win)不要

#### DX12 復活の条件(将来判断)

- AMD discrete Win ユーザーから明確な需要申請
- Adobe が CUDA を deprecate する方向に動く
- smooth の user base が広がり vendor coverage が実運用課題になる

その際は Phase 2-A.3 の成果物(GpuBackend trait 抽象、CPU-GPU 切替 UI 等)を再利用して DX12 backend を追加する形。現時点では shader 抽象化は設計に入れない(Metal + CUDA の 2 本で十分かつ将来 3 本目追加時に既存 2 本への影響が小さくなるよう注意)。

### 不採用(確定)

- **wgpu / Vulkan**: AE の GPU 統合経路と相性が悪い(AE が native handle を渡す設計なのに wgpu/Vulkan は自前 device 管理が前提、結果 CPU ↔ GPU memcpy が毎フレーム発生し GPU 化の恩恵が目減り) → **議論終了、Item 3 以降で触れない**
- **OpenCL**: AE は framework として受け入れるが、Apple が Mac で deprecated 指定、Windows でもベンダー依存で長期保守性低い

### 2 プラットフォームの shader 言語(DX12 defer 後)

| Framework | shader 言語 | Rust binding 候補(Item 3 で詳細) |
|---|---|---|
| Mac Metal | MSL (Metal Shading Language) | `metal-rs` / `objc2-metal` |
| Win CUDA | CUDA C++ (PTX embed) | `cust` / `cudarc` |

---

## Item 1: AE SDK GPU API 把握

**参照**: `references/AfterEffectsSDK_25.6_61_mac/ae25.6_61.64bit.AfterEffectsSDK/` の以下:
- `Examples/Headers/AE_EffectGPUSuites.h`(GPU suite 定義)
- `Examples/Headers/AE_Effect.h`(selector / flag 定義)
- `Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp`(full GPU plugin の canonical reference、Item 6 で精読)
- `Examples/AE GPU SDK Build Instructions.pdf`(未読、実装 spike で参照予定)

### 1.1 サポートされる GPU フレームワーク(`PF_GPU_Framework` enum)

```c
PF_GPU_Framework_NONE    = 0
PF_GPU_Framework_OPENCL
PF_GPU_Framework_METAL
PF_GPU_Framework_CUDA
PF_GPU_Framework_DIRECTX  // DX12
```

プラットフォーム別の実際の選択肢:

| OS | 典型的な framework |
|---|---|
| macOS | **Metal**(Apple Silicon / Intel AMD / Intel NVIDIA)、OpenCL(deprecated) |
| Windows | **CUDA**(NVIDIA 専用)、**DirectX 12**(ベンダー中立)、OpenCL |

AE 側が各デバイスを 1 つの framework に紐づけて列挙し、プラグインは 1 つ以上の framework に対応する実装を提供する形。

### 1.2 デバイス情報の受け渡し(`PF_GPUDeviceInfo`)

AE は `void*` 経由で native handle を直接渡してくる:

```c
typedef struct {
    PF_GPU_Framework device_framework;
    PF_Boolean compatibleB;
    void* platformPV;               // cl_platform_id
    void* devicePV;                 // CUdevice / cl_device_id / MTLDevice / ID3D12Device
    void* contextPV;                // CUcontext / cl_context
    void* command_queuePV;          // CUstream / cl_command_queue / MTLCommandQueue / ID3D12CommandQueue
    void* offscreen_opengl_contextPV;
    void* offscreen_opengl_devicePV;
} PF_GPUDeviceInfo;
```

**含意**: AE は「抽象化された GPU API」を提供しない。プラグイン側で framework 別に分岐した native コードを書く必要がある(Metal なら `id<MTLDevice>` をキャストして使う)。

### 1.3 必須 OutFlags

```c
PF_OutFlag2_SUPPORTS_GPU_RENDER_F32   = 1L << 25   // bit 25 = 0x02000000
PF_OutFlag2_SUPPORTS_DIRECTX_RENDERING = 1L << 29   // bit 29 = 0x20000000 (opt-in、上記とセット)
```

**重要**: GPU render は `f32` のみ。8bpc / 16bpc 用の GPU 経路は存在しない。プラグイン側で 8/16bpc → f32 変換 or 32bpc 対応追加が必要。

### 1.4 新規に実装する必要がある Command selectors

```c
PF_Cmd_SMART_PRE_RENDER    // SmartRender の入力要求 + GPU 可否フラグ設定
PF_Cmd_SMART_RENDER        // CPU SmartRender 本体(GPU 不可時にここへ)
PF_Cmd_SMART_RENDER_GPU    // ★ GPU 専用の別 selector(SMART_RENDER と distinct、Item 6 §6.1 で SDK 確認済み)
PF_Cmd_GPU_DEVICE_SETUP    // per-device 初期化(デバイス毎に shader compile 等)
PF_Cmd_GPU_DEVICE_SETDOWN  // per-device 後始末
```

通常 render 経路は legacy `PF_Cmd_RENDER` から SmartRender 二本立て(CPU: `PF_Cmd_SMART_RENDER` / GPU: `PF_Cmd_SMART_RENDER_GPU`)に移行する必要あり。PreRender で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を立てれば AE が GPU device が使える状況で `SMART_RENDER_GPU` を呼び、それ以外(GPU 不可 / user opt-out / once-fallen 後)では `SMART_RENDER` を呼ぶ。**legacy `PF_Cmd_RENDER` は SmartRender 非対応の古い AE 呼出しパスとして残すのみ**、GPU 不能時の通常 fallback ではない。

### 1.5 メモリ管理

**フレーム本体 / 中間 world の GPU メモリ**は必ず `AllocateDeviceMemory` / `CreateGPUWorld` 経由で確保する(AE の VRAM 圧迫監視下で動作させるため)。直接 `cuMemAlloc` / `[MTLDevice newBufferWithLength:]` を呼ぶのはこの用途では NG。

**例外**: 小さな kernel parameter buffer(数十〜数百 byte の定数 struct)は SDK サンプル [SDK_Invert_ProcAmp.cpp:1047] で `[device newBufferWithBytes:...length:sizeof(Params) options:...]` を直接呼んで作っている。VRAM 圧迫に影響しないサイズなら suite を介さず直接作って OK(autoreleasepool で管理)。

`CreateGPUWorld` / `DisposeGPUWorld` で AE の `PF_EffectWorld` 型の GPU 版を作れる。`GetGPUWorldData` で raw device pointer(`MTLBuffer*` 等)を取り出せる。

### 1.6 排他制御(MFR 対応の観点)

```c
AcquireExclusiveDeviceAccess / ReleaseExclusiveDeviceAccess
```

SDK コメントに重要な記述:
> For full GPU plugins (those that use a separate entry point for GPU rendering) **exclusive access is always held**. These calls do not need to be made in that case.

つまり **full GPU plugin パターン**(専用 GPU entry point を持つ形)を採れば、デバイス排他は AE が自動管理してくれる。partial GPU plugin(render 中に一部だけ GPU 呼ぶ)は自前排他が必要。

**smooth の選択**: Full GPU plugin パターンで進める(コード分岐がクリーンになる、MFR と組み合わせた時の排他ロジックが AE 任せで済む)。

### 1.7 Full GPU plugin の参考実装(canonical reference)

**canonical reference = `Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp`**(Item 6 §6.2 で 1210 行全体を精読、smooth 向けに 80% 流用可)。CUDA / OpenCL / Metal / DirectX 12 の 4 framework 対応で、full GPU plugin のすべての要素(GPU_DEVICE_SETUP / SETDOWN、SMART_PRE_RENDER、SMART_RENDER、SMART_RENDER_GPU、PF_GPUDeviceSuite1 利用)を網羅。

**補足**: 当初 `Examples/GP/EMP/` を候補として挙げていたが、精読の結果これは **blit hook plugin(AE の画面更新フック、計 74 行)**であって full GPU compute plugin ではなかった。Item 6 で canonical を SDK_Invert_ProcAmp に確定。

### 1.8 smooth への含意(Item 1 の結論)

- **やるべきこと**(実装に必須):
  - SmartRender 二本化: legacy `PF_Cmd_RENDER` は古い AE 後方互換用に残しつつ、`PF_Cmd_SMART_PRE_RENDER` + `PF_Cmd_SMART_RENDER`(CPU)+ `PF_Cmd_SMART_RENDER_GPU`(GPU、distinct selector)を新規追加
  - `out_flags2` に以下 3 つを追加:
    - `PF_OutFlag2_SUPPORTS_SMART_RENDER`(SmartRender 経路を AE に宣言)
    - **`PF_OutFlag2_FLOAT_COLOR_AWARE`**(32bpc 対応、SDK 上 GPU render f32 とは別フラグで必要。SDK_Invert_ProcAmp.cpp L128-130 と AE_Effect.h L994 で確認)
    - `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32`(GPU render 可能を宣言)
  - MFR 関連は Phase 2-B で導入済みの 3 flag を継続(`I_AM_THREADSAFE`、`SUPPORTS_GET_FLATTENED_SEQUENCE_DATA`、`SUPPORTS_THREADED_RENDERING`)
  - `PF_Cmd_GPU_DEVICE_SETUP` / `SETDOWN` に shader compile + per-device リソースキャッシュの初期化
  - Full GPU plugin パターン(ユーザーの GPU/CPU 切替 checkbox の有無に関わらず、GPU 経路が走る時は必ず full で)

- **検討が必要**:
  - **32bpc 対応**: GPU render は f32 必須。現 smooth は 8/16bpc 対応のみ、32bpc 用のテストデータ・golden が無い
    - オプション A: GPU path 専用で 8/16bpc → f32 → GPU → 8/16bpc 変換(追加 I/O コスト発生、省略可能性あり要検証)
    - オプション B: 本来の 32bpc 対応をエフェクト全体に追加(smooth のアルゴリズムが 32bpc range でも意味を保つか要検証。スムージング閾値等は輝度スケール固定値ではないか? — Item 2 §2.3 で実コードと合わせて詳細検討)
    - オプション C: GPU path は f32 のみで、8/16bpc のプロジェクトは CPU fallback(ユーザーが「GPU 欲しい時は 32bpc プロジェクトで」する)
  - **shader 言語**(Phase 2-A.3 最終採用は Item 3 §3.7):
    - Metal: MSL (Metal Shading Language)
    - CUDA: CUDA C++ ベース、NVCC で PTX or static lib 化
    - (DX12 / OpenCL は scope 外)

- **CPU fallback 戦略**: Pre-render で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を立てるか否かで AE が CPU/GPU 経路を選ぶ。
  - 立てる: AE は `PF_Cmd_SMART_RENDER_GPU` を呼ぶ
  - 立てない: AE は `PF_Cmd_SMART_RENDER`(CPU SmartRender)を呼ぶ
  - legacy `PF_Cmd_RENDER` は SmartRender 非対応 AE 向けの別ルート、通常 CPU fallback ではない
  - GPU 初期化失敗 / user opt-out / once-fallen 発動 は PreRender でフラグを立てないロジックで制御

### 1.9 追加調査が必要な項目(PDF 未読 + 実装時に確認)

- `AE GPU SDK Build Instructions.pdf` の内容(特に Mac / Win ビルド手順、shader の embed 方法)
- MFR と GPU path の同時利用で AE が per-thread context をどう管理しているか(Item 4 で深掘り)

---

---

## Item 2: smooth アルゴリズム プロファイリング

**参照コード**: `rust/smooth_core/src/{lib,preprocess,types,compare,blend,process,up_mode,down_mode,lack,link8}.rs`

### 2.1 現行 CPU ベースライン(16-core Intel i9 MacBook Pro、Phase 2-C rayon 有効)

`tests/bench.sh` repeat=30 実測(2026-04-23):

| 解像度 × bpc | avg | min |
|---|---|---|
| 2512×1412 8bpc  |  8.1 ms |  7.9 ms |
| 3840×2160 8bpc  | 36.0 ms | 32.6 ms |
| 3840×2160 16bpc | 33.6 ms | 31.5 ms |
| 1920×1080 16bpc | 10.5 ms |  9.9 ms |

理論下限(GPU 帯域律速)参考:
- 4K 8bpc frame = 8.3 M pixels × 4 bytes = 33 MB。read + write = 66 MB。GPU 帯域 300-400 GB/s なら 0.2 ms/frame が下限
- 実装オーバーヘッドを考慮しても **1-2 ms / 4K frame** が現実的目標、CPU 比 18-36× 高速化余地あり

### 2.2 アルゴリズム構造(2 フェーズ)

#### Phase A: `pre_process` — フレーム全体を 1 pass scan

[rust/smooth_core/src/preprocess.rs:35-99](rust/smooth_core/src/preprocess.rs#L35-L99)

```rust
for j in 0..height {
    for i in 0..width {
        let p = pixels[t];
        if is_white_trans && p.rgb_eq(&key) {
            pixels[t] = null;                          // 白 → 透明に置換
        } else if !p.alpha_is_zero() {
            update_bbox(&mut top/left/right/bottom);   // 非透明領域の bbox 集計
        }
        t += 1;
    }
}
```

- 入力: pixel buffer(in-place で白→透明置換)
- 出力: `SmoothBbox { top, left, right, bottom }` + 白置換済み buffer
- アクセスパターン: 完全 row-wise sequential、各 pixel 独立
- **GPU-friendly 評価**: ◎
  - 白置換: 完全 data-parallel(独立 1 pixel/thread)
  - bbox: classic parallel reduction(log N)
  - 単 pass bandwidth-bound、GPU で理論下限近くまで最適化可能

#### Phase B: `process_row_range` — per-pixel corner detection + mode dispatch

[rust/smooth_core/src/process.rs:49-248](rust/smooth_core/src/process.rs#L49-L248)

```rust
for j in j_start..j_end {                              // 行ループ(rayon で並列化可)
    let mut lack_flg = false;                          // ★ 行内キャリー状態
    for i in i_start..i_end {                          // 列ループ(serial within row)
        if lack_flg { lack_mode_0304_execute(...); }

        if fast_compare_pixel(center, center+1) {      // 隣接ピクセル packed 比較
            let mut mode_flg = 0u8;
            if compare_pixel(center, right)  { mode_flg |= 1<<0; }
            if compare_pixel(center, up)     { mode_flg |= 1<<1; }
            if compare_pixel(center, down)   { mode_flg |= 1<<2; }
            if compare_pixel(center, left)   { mode_flg |= 1<<3; }

            match mode_flg {
                3  => up_corner_detect_and_blend(),    // 複数の *_count_length() + blending
                5  => down_corner_detect_and_blend(),
                7  => link8_mode_01_execute(),
                11 => link8_mode_02_execute(),
                13 => link8_mode_04_execute(),
                15 => link8_square_execute(),
                _  => {}
            }
            check_projection_mode3();                  // 突起 mode3 の追加チェック
        }
    }
}
```

メイン処理の**構造的特徴**:

| 特徴 | GPU 適性 |
|---|---|
| ピクセル単位で 4 近傍比較(stencil) | ◎ 完全 data-parallel |
| `mode_flg` による 6 分岐(3/5/7/11/13/15) | ✗ 強い wavefront divergence |
| `*_count_length()`: コーナーから一直線にスキャンし差異ピクセルまでの距離を測定 | ✗ 可変長 loop、divergence 大 |
| `lack_flg` が同一行内で i → i+1 に情報を持ち越す | ✗ row-wise serial、 i 並列化阻害 |
| コーナー検出時、`*_blending()` が**上下左右複数 pixel に書き込み**(blend line length 分) | ✗ 書き込み領域が overlap する可能性(adjacent corners) |
| 書き込み先は主に out_ptr、読み取りは in_ptr のみ(双方向 memcpy 済) | ◎ read-only input / write-only output 分離、同期不要 |

### 2.3 32bpc 拡張の影響(Phase 2-A.2 で実施予定)

現行の `SmoothPixel` trait は:
- `delta_sum` で `u32`(u8 は max 4×255=1020、u16 は max 4×0x8000=0x20000)
- `max_value` が `0xFF`(u8)/ `0x8000`(u16)
- blending は `(a*alpha + b*r_alpha) / max_value` の**整数除算**

32bpc(`PF_PixelFloat`、alpha + RGB 各 f32、AE の 0.0〜1.0 domain)に拡張する場合:
- `delta_sum` → `f32` 和
- `max_value` → 1.0f32
- blending → f32 乗算+除算、オーバーフロー懸念なし
- `fast_compare_pixel` の packed u64 比較 → f32×4 の packed 比較(128-bit SIMD or `[f32;4]` tuple 比較)

**現行 `range` パラメータの実装**(Effect.cpp L454-464 実測):
```
PF_ADD_FLOAT_SLIDER("range",
                    0.0f,   // VALID_MIN
                    100.0f, // VALID_MAX
                    0.0f,   // SLIDER_MIN
                    10.0f,  // SLIDER_MAX
                    1.00f,  // CURVE_TOLERANCE
                    1.0f,   // DFLT
                    ...);
```

- UI: **float slider**、表示 0.0〜10.0、入力許容 0.0〜100.0、default 1.0
- 内部変換(Effect.cpp L531): `core_params.range = (unsigned int)(slider_value * (getMaxValue<PixelType>() * 4)) / 100`
  - Pixel8(max=0xFF): slider 1.0 → 内部 range ≈ 10(単位は 4-channel abs-diff sum、つまり一色あたり 2〜3 階調差が閾値)
  - Pixel16(max=0x8000): slider 1.0 → 内部 range ≈ 1310(16bpc スケールの同等値)
  - この設計で bpc 別に UI 値域を変えずに意味を保っている(slider value は「max の 4 倍 × 1%」)

**32bpc 拡張時の設計**(Phase 2-A.2):
- slider UI は現状維持(0.0〜100.0、default 1.0、表示 0.0〜10.0)で **user 互換性を保つ**
- 内部変換を bpc 別の分岐に拡張: Pixel32(max=1.0): slider 1.0 → 内部 range = 1.0 × 4 / 100 = 0.04(f32 domain)
- `line_weight` は元から f32(0.5 等の normalized 値)、bpc 非依存で影響なし
- `count_length` 系は閾値比較の結果(true/false)しか使わないので、`delta_sum` と `range` の型整合さえ取れば残りは自動追随

### 2.4 GPU 実装戦略 3 案(Phase 2-A.3 候補)

#### 案 1: ピクセル並列・単 pass(各スレッド 1 pixel の完全処理)
- 1 thread = 1 output pixel
- 各スレッドが detect + count_length + blend まで全部実行
- **問題**: 
  - Adjacent corners の blend 書き込み overlap → atomic ops or 入力側から「自分が書かれる予定の pixel か」を逆引き再計算
  - `count_length` と `mode_flg` 分岐の divergence で warp 効率大幅低下
- 実装は単純だが性能は出にくい

#### 案 2: 2 pass(検出 → blending 適用)
- Pass 1: 1 thread = 1 input pixel、mode_flg + count_length 結果を intermediate buffer に書く
- Pass 2: 1 thread = 1 output pixel、intermediate から自分に影響する corner を逆引き、blend 計算して書く
- **利点**: overlap 問題が消える(Pass 2 は output pixel ごとに集約)
- **欠点**: intermediate buffer が必要、Pass 1/2 の間で同期、total ops 増
- 正確性が担保しやすく、**v1.0 GPU 実装としては本命**

#### 案 3: 行並列(1 thread = 1 row)
- row 内はそのまま serial(lack_flg もそのまま動く)
- 並列度 = H(1080 / 2160 行数)
- **利点**: アルゴリズム完全保存、既存 Rust コードから shader への 1:1 移植
- **欠点**: GPU core 数を十分使い切れない(ハイエンド GPU は 数千 ALU、1080 row だと不足)、memory access pattern が row-linear で cache 効きにくい
- **PoC / 初期動作確認用**として有用。正確性と性能のベースラインに

**推奨順序**: 案 3 → 案 2 → (必要なら)案 1。案 3 で CPU 完全再現を確立、案 2 で性能引き上げ、案 1 は overkill で保守性低下リスクあり避ける。

### 2.5 Item 2 の結論

- 現行 CPU は HD 16bpc 10 ms / 4K 16bpc 33 ms(rayon 並列化済み)。GPU 化で 5-20× 高速化の余地
- **pre_process は GPU 化で著効が見込める**(完全 data-parallel、bandwidth bound)
- **process_row_range は分岐 + 書き込み overlap + 行内 serial 依存で GPU hostile**。案 2(2 pass)で GPU に馴染む形にリファクタリング推奨
- **32bpc 対応は Phase 2-A.2 で先行実施**、アルゴリズム本体は `delta_sum` / `range` の型 f32 化と blending の f32 乗除置換で対応可能。`range` の UI スケールは bpc 別の内部換算式拡張で **UI は現状維持可能**(§2.3 参照)

### 2.6 追加で深掘りが必要な項目(実装 phase で確認)

- 32bpc 拡張時の `delta_sum` 実装詳細(Pixel32 の alpha + RGB を f32 で合計する時の overflow 挙動と、既存 u32 domain の SIMD 最適化の同等置換手順)
- 案 2(2 pass)の intermediate buffer size 見積もり(pixel 当たり数 byte × W×H + corner 情報)
- `lack_flg` の厳密な伝搬範囲: 次 i のみか、複数 i に渡って持続するか(shader 側で依存 chain をどこまで再現する必要があるか)

---

---

## Item 3: Rust GPU バインディング選択肢の比較

**評価軸**(Phase 2-A 全体のスコープに沿う):

| 優先度 | 評価項目 |
|---|---|
| 最優先 | AE が渡す **raw native handle を受け入れる**能力(`MTLDevice` ptr / `ID3D12Device*` / `CUcontext`) |
| 最優先 | **保守性**(crate のメンテナンス状況、コミット頻度、発行組織の信頼性) |
| 高 | **ライセンス互換性**(Apache 2.0 / MIT / その他との OK/NG、cargo lockfile の transitive 深さ) |
| 高 | **static link 耐性**(Windows `+crt-static`、Mac universal + Rust target lipo と干渉しないか) |
| 中 | **API の人間工学**(unsafe 比率、型安全性、学習曲線) |
| 中 | **shader compile ワークフロー**(MSL / HLSL / PTX の build-time embed か runtime compile か) |

### 3.1 Mac Metal

| crate | 発行元 | 特徴 | 評価 |
|---|---|---|---|
| **`metal`** (通称 metal-rs) | gfx-rs org(wgpu と同組織) | 2015〜、広く採用実績あり。`MTLDevice`/`MTLBuffer`/`MTLCommandQueue` を Rust 側で Obj-C ref カウント管理、raw ptr から wrapper 構築可 | **推奨**: 成熟・実績・AE native handle 受入可 |
| `objc2-metal` | objc2 プロジェクト | Apple headers から auto-gen、Metal API の追従速度は最速。より "Rust-native" な feel | 代替案として候補、ただし smooth の用途(基本 compute + buffer IO)では `metal-rs` の機能で十分。新しい分コミュニティ事例が少ない |
| 直接 Objective-C FFI(`objc` crate + 自前 wrapper) | - | 薄い層で Metal の C/Obj-C API を直叩き | 保守性低い、採らない |

**採用案**: `metal` crate(metal-rs)。長期保守実績、AE SDK 公開例(他の AE プラグインで採用例)とも矛盾しない。

### 3.2 Windows DX12(defer、参考記録のみ)

Phase 2-A.3 スコープ外。将来復活時の候補のみ記録:
- `windows` crate(Microsoft 公式)+ `Win32_Graphics_Direct3D12` / `Direct3D_Dxc` / `Dxgi` feature。

### 3.3 Windows CUDA

| crate | 発行元 | 特徴 | 評価 |
|---|---|---|---|
| **`cudarc`** | coreylowman / candle(HF) コミュニティ | Driver API に近い低レベル、kernel PTX を runtime ロード、`CUcontext`/`CUstream` を外部 init でも受入可、頻繁にメンテ | **推奨**: AE から context を受け取る用途に適合、ML コミュニティ(candle、HF)で実戦投入されている |
| `cust` | Rust-GPU / Rust-CUDA project | より高レベル、抽象化強め | メンテ頻度が不定、AE のように外部 context を引き継ぐ用途には余計な層 |
| `cuda-sys` / 自前 | - | バインディング薄層 | 保守性低い、採らない |

**採用案**: `cudarc`。CUDA Driver API を直接叩けるため、「AE から渡された `CUcontext` を必要なら `cuCtxPushCurrent` してから kernel launch」のパターンが素直に書ける(context push/pop を実際に必要とするかは Item 4 §4.4 / Item 6 §6.1 で分析、SDK サンプル [SDK_Invert_ProcAmp.cpp:960] は push/pop していないが、push/pop を入れても副作用のないコストで safety margin になる)。

### 3.4 クレート構造提案

`rust/smooth_core/` 内で、platform-gated に GPU backend を追加:

```
rust/smooth_core/
├── Cargo.toml              # 既存 + platform feature flags
├── src/
│   ├── lib.rs              # 既存 + FFI 追加(smooth_core_process_gpu_*)
│   ├── {compare,blend,process,...}.rs  # 既存 CPU コア
│   ├── gpu/
│   │   ├── mod.rs          # 共通 trait GpuBackend + dispatch glue
│   │   ├── metal.rs        # #[cfg(target_os = "macos")]
│   │   ├── cuda.rs         # #[cfg(target_os = "windows")]
│   │   └── shaders/
│   │       ├── smooth.metal   # MSL ソース(build-time embed)
│   │       └── smooth.cu      # CUDA ソース(PTX embed)
│   └── ...
```

**共通 trait 案**:

```rust
trait GpuBackend {
    type Device;
    type Buffer;
    type CommandQueue;
    // AE が渡す raw handle から wrap
    unsafe fn from_ae_device(ptr: *mut c_void, queue: *mut c_void) -> Self;
    fn allocate_buffer(&self, size: usize) -> Self::Buffer;
    fn dispatch_preprocess(&self, ...);
    fn dispatch_smoothing(&self, ...);
    fn wait(&self);
}
```

CPU backend(既存)を同じ trait で包むと、fallback 経路 / unit test / benchmark が trait 単位で書ける。

### 3.5 Shader compile 戦略

3 候補:

| 方式 | 利点 | 欠点 |
|---|---|---|
| **Build-time compile + embed**(`.metal` → `.air`/`.metallib`、`.hlsl` → `.dxil`、`.cu` → `.ptx` を `build.rs` で事前 compile し `include_bytes!` で焼き込む) | plugin 起動 ~ 1st frame が速い、user 環境で compiler 不要 | build 手順複雑化、cross-compile 時に toolchain 要件 |
| Runtime compile(plugin 起動後に source から compile) | build 簡単、compiler エラーが user 環境で起きる可能性 | 1st render が遅い、user 環境に compiler 依存(DXC / xcrun metal / NVRTC) |
| **ハイブリッド**(PTX/DXIL は embed、Metal は runtime 可のみ) | 用途別最適 | 実装複雑 |

**採用案**(DX12 除外後、scope は Metal + CUDA): 全 backend で **build-time compile + embed**。`rust/smooth_core/build.rs` に compile step を追加:
- Mac: `xcrun metal -c smooth.metal -o smooth.air && xcrun metallib smooth.air -o smooth.metallib`
- Win CUDA: NVCC で PTX or static obj 生成し embed(SDK サンプル `SDK_Invert_ProcAmp` と同じ方式、Item 6 §6.4 参照)

(参考: DX12 を将来復活させる場合は DXC でビルド時 HLSL → DXIL compile、ただし Windows toolchain 要件が増えるので spike 要 — Phase 2-A.4 以降の検討事項)

### 3.6 static link 耐性確認

- **Mac**: `metal-rs` は Obj-C runtime が dylib link 前提。AE 自身が Obj-C runtime を使うのでリンクは問題なし。Rust 側は static lib のまま維持可
- **Windows DX12**: `windows` crate は pure Rust、DLL は `D3D12.dll` / `dxgi.dll` を runtime load。`+crt-static` と衝突しない
- **Windows CUDA**: `cudarc` は CUDA Driver API を `LoadLibrary("nvcuda.dll")` で runtime load。CUDA SDK 自体が plugin バイナリに同梱されない(ユーザーの NVIDIA driver が提供)。static link 可

### 3.7 Item 3 の結論

| プラットフォーム | Rust crate | shader 言語 | compile 方式 |
|---|---|---|---|
| Mac | `metal` (metal-rs) | MSL | build-time → `.metallib` embed |
| Win (NVIDIA) | `cudarc` | CUDA C++ → PTX | build-time → PTX embed(NVRTC or NVCC) |

全て static link、Apache-2.0/MIT 互換、メンテ継続中の crate を採用。既存 `+crt-static` Windows ビルド環境と衝突なし。

### 3.8 Item 3 で積み残した検討事項(実装時に確認)

- Metal shader の `.metallib` vs `.air`(同じ metal toolchain が出す中間形式、どちらを embed するのが AE 2025 互換で最適か)
- `metal-rs` / `cudarc` の **具体的バージョン固定**(`Cargo.toml` で後日レビュー)
- AE の `PF_Cmd_GPU_DEVICE_SETUP` での shader ロードが Metal / CUDA 両方で成功することの確認(これは Item 6 の実装 spike で検証)
- (将来、DX12 復活判断時)DXC を Windows CI に乗せる方法

---

---

## Item 4: MFR + GPU 両立要件(Metal + CUDA)

Phase 2-B (v1.5.1) で確立した MFR 契約に、GPU 経路を**破壊せずに**載せる要件を確定する。対象 framework は Metal (Mac) + CUDA (Win)、DX12 は scope 外。

### 4.1 Full GPU Plugin における AE の責務契約

[Item 1 §1.6](#16-排他制御mfr-対応の観点) で確認した通り、SDK はこう明記している:

> For full GPU plugins (those that use a separate entry point for GPU rendering) **exclusive access is always held**. These calls [AcquireExclusiveDeviceAccess/Release] do not need to be made in that case.

**解釈**: AE は `PF_Cmd_SMART_RENDER_GPU`(GPU 専用 selector)**1 呼び出しの間、その device への排他アクセスを保証する**。プラグインは複数の MFR 並列 call を受け取るが、**個々の call の中では device resource に対する他の call との競合を気にしなくて良い**。

**重要な追加条件**(SDK に明記されてはいないが実装時に確認):
- AE は MFR で異なる frame を並列 render するとき、**同じ plugin の同じ device に対して同時に `SMART_RENDER_GPU` を呼ぶか?**
  - SDK サンプル [SDK_Invert_ProcAmp.cpp:1201] は追加の thread-safety guard なし。per-render-call の一時 buffer + per-device の read-only pipeline で naturally thread-safe な書き方
  - SDK コメント通り AE 側が device 排他管理している前提で書けば OK(Item 6 §6.1 で裏取り済み)
  - → 初期 PoC で VRAM 圧迫時の挙動を実測して最終確認

### 4.2 GPU resource のライフタイム分類

smooth が保持する GPU-side state を、**ライフタイム粒度別に分類**して設計に落とす:

| リソース | ライフタイム | 保持場所 | 共有スコープ |
|---|---|---|---|
| **compiled shader pipeline**(`MTLComputePipelineState` / `CUfunction`) | **per-device**、plugin load 中継続 | `PF_Cmd_GPU_DEVICE_SETUP` で作成、`PF_Cmd_GPU_DEVICE_SETDOWN` で解放 | 同 device 上の全 MFR thread 間で共有(read-only state、競合なし) |
| **shader binary** (`.metallib` blob / PTX blob) | **plugin 全体で static**、build-time embed | Rust `include_bytes!` | 全 device 全 thread |
| **transient input/output buffers**(frame 1 枚分の input image / output image / intermediate) | **per-render-call**、frame 内で allocate/free | `PF_Cmd_SMART_RENDER_GPU` 内で `AllocateDeviceMemory` / `CreateGPUWorld`、return 前に `FreeDeviceMemory` / `DisposeGPUWorld` | 呼び出し thread のみ(AE の MFR は frame 単位で thread を分ける) |
| **command buffer / stream** | **per-render-call** | Metal: `MTLCommandBuffer` を 1 call で 1 個作成、CUDA: AE 提供 `CUstream` をそのまま使う or frame-local stream 作成 | 呼び出し thread のみ |

**設計原則**: per-device は compile 済み pipeline のみ。buffer は per-call で毎回 allocate → free。VRAM 圧迫は AE が `AllocateDeviceMemory` で監視しているので、我々は毎 frame 正直に要求・返却する。

### 4.3 Metal 側の thread-safety

Apple Metal の公式契約:

| オブジェクト | thread-safety |
|---|---|
| `id<MTLDevice>` | **完全 thread-safe**、複数 thread から同時使用 OK |
| `id<MTLCommandQueue>` | **thread-safe**、複数 thread から同時に command buffer を enqueue 可 |
| `id<MTLCommandBuffer>` | ✗ **single owner**、作成した thread からのみ commit |
| `id<MTLBuffer>` | 作成は thread-safe、内容の同時読み書きは仕様外 |
| `id<MTLComputePipelineState>` | thread-safe、読み取り専用 state(複数 encoder から OK) |

**MFR + Metal 実装パターン**(SDK サンプル [SDK_Invert_ProcAmp.cpp:1057] 準拠):
- pipeline state は per-device 作成、全 MFR thread で共有 read-only 参照
- queue は AE が `PF_GPUDeviceInfo::command_queuePV` で提供するものをそのまま使う(plugin 側で `newCommandQueue` しない、queue は thread-safe なので MFR thread 間で共有可)
- 各 MFR render call で `[queue commandBuffer]` → encoder → dispatch → `endEncoding` → **`commit` のみ(`waitUntilCompleted` しない)**、この command buffer だけ per-call 所有
- **`waitUntilCompleted` を呼ばない**: AE が commit 後の synchronization を自動で hold してくれる。plugin は commit して `[commandBuffer error]` ステータスだけチェック(Item 6 §6.2 で SDK サンプル挙動を確認済み、L1083-L1087)

### 4.4 CUDA 側の thread-safety

CUDA Driver API の契約(一般則):

| オブジェクト | thread-safety |
|---|---|
| `CUcontext` | per-thread current context 制、未設定の thread から呼ぶには push が必要 |
| `CUstream` | thread-safe、複数 thread から同時 launch 可(stream 内の work は順序保証される) |
| `CUmodule` / `CUfunction` | read-only、thread-safe |
| `CUdeviceptr`(device memory) | 同じ stream 上で操作する限り安全 |

**MFR + CUDA 実装パターン**(SDK サンプル [SDK_Invert_ProcAmp.cpp:960-992] 準拠):
- AE が `PF_Cmd_GPU_DEVICE_SETUP` で `CUcontext` を渡してくる(`PF_GPUDeviceInfo::contextPV`)
- plugin は module load (PTX → `CUmodule` → `CUfunction`、もしくは SDK サンプル通り NVCC static link)を SETUP 時に一度だけ実行、結果を per-device state に保存
- **CUDA context の push/pop は SDK サンプルでは省略されている**: `Invert_Color_CUDA(...)` を直接 call しており、`cuCtxPushCurrent` / `cuCtxPopCurrent` / `cudaSetDevice` は一切書かれていない(L960-L992)。SDK の前提として **AE が entry 前に current context を thread 側にセットしている**模様
- **ただし本研究ノートでは safety-first として、実装 spike で両方を検証する**:
  - (a) SDK サンプル準拠で省略 → MFR 並列時に別 thread の context を汚染しないか実機確認
  - (b) 明示的に `cuCtxPushCurrent(ae_ctx)` / `cuCtxPopCurrent` で囲む → overhead を実測、無視できる(想定 < 1 µs/call)ならこちらを safety margin として採用
  - 現時点の採用案: **spike 結果に従う、default は (a) SDK 準拠**(実装作業量最小)
- AE 提供の `CUstream` は 1 本だけ(`command_queuePV`)。per-frame に stream を独立させる必要があれば plugin 側で `cuStreamCreate`(ただしフレーム毎にやるとオーバーヘッド、基本は AE の stream でシリアライズで十分)

### 4.5 VRAM 予算と frame-in-flight 上限

AE の MFR は **最大同時 frame 数 = AE の RenderThreadExecutor 設定**(Phase 2-B で観測した 16 threads 環境)。仮に 16 frames in flight が fully GPU 走る最悪ケース:

4K 16bpc frame の GPU 上使用容量(概算):
- input (f32 RGBA 4K) = 4 ch × 4 byte × 8.3 M px = **133 MB**
- output (同上) = 133 MB
- intermediate (案 2 の 2 pass 中間 buffer、mode_flg + count length 等) = pixel 当たり ~16 byte として 133 MB
- **合計 ~400 MB / frame**

16 frames 同時: **6.4 GB**

多くのコンシューマ GPU VRAM は 8 GB〜、pro 向けは 12-24 GB。**16 frames in flight だと低スペック GPU で OOM リスクあり**。

**対策**:
- `AllocateDeviceMemory` が失敗を返したら、そのフレームの GPU 経路を諦めて CPU fallback(§4.7 の policy 参照)
- MFR 並列度を AE 側に伝える API があれば GPU safe max を返す(SDK に該当 API がなければ AE まかせ)
- 長期的には半精度 (fp16) 経路やタイル分割で VRAM 負荷低減(Phase 2-A.3 v1.0 scope 外)

**推奨初期値**(実装時の debug 用 env var で override 可):
- `SMOOTH_GPU_MAX_CONCURRENT_FRAMES=4` 相当のソフト制限(atomic counter、超過時 fallback)
- VRAM 上限のデフォルトは「frame 1 枚分 × 4」= 1.6 GB、超えたら fallback

### 4.6 Fallback policy: once-fallen-always-fall(per-effect-instance / sequence レベル)

**背景**(以前は private memo 参照だったのを本文化): Phase 2-B 設計時から「GPU 失敗時はセッション内 CPU 固定」を原則として決めていた。根拠は:
- バッチ書き出し中に急に GPU が落ちて CPU に切り替わると、fallback 周辺フレームの bit-identical 性が担保できない(boundary residual が切り替わり点で出る)
- user が結果を見た時に「一部のフレームだけ色が違う」を招く
- CPU/GPU 混在 render は output のコンテンツ品質保証が難しい

**原則**: 1 回でも GPU render 失敗(OOM、shader error、driver timeout 等)が発生したら、**そのエフェクトインスタンスの以降のレンダーを CPU 経路に固定**する。

#### ❗ sequence_data への直接書き込み設計は不可(SDK 制約)

AE_Effect.h L926-L930 に明記:
> sequence_data is read-only at render time and must be accessed with PF_EffectSequenceDataSuite.
> in_data->sequence_data will be NULL during render. AEGP_ComputeCacheSuite is suggested if writing to sequence_data at render time is needed for caching.

さらに SEQUENCE_DATA 書き込みを可能にする opt-in flag `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER`(L1010)も、docstring に:
> Note that changes to sequence_data will be discarded regularly, currently after each span of frames is rendered such as single RAM Preview or Render Queue export.

「span of frames の境界で discard」= バッチ書き出し中の途中で GPU が落ちたフラグが、次の書き出し span で消える。**once-fallen-always-fall の本来の狙い(そのセッション全体で CPU 固定)と挙動が合わない**。また、常時 `SLOWER` flag を立てるのは MFR 並列度低下のコストが割に合わない。

#### 採用設計: plugin-global HashMap + sequence_data に UUID のみ格納

**保存構造(2 層分離)**:

1. **sequence_data(read-only during render、per-instance unique)**: エフェクトインスタンスの一意識別子だけを持つ
   ```rust
   #[repr(C)]
   struct SmoothSequenceData {
       version: u32,          // schema change 検知用
       instance_uuid_hi: u64, // u128 を 2 × u64 に分割(C 側 align と FFI 互換のため、本 doc 全域で統一)
       instance_uuid_lo: u64,
   }
   ```
   SEQUENCE_FLATTEN / SEQUENCE_RESETUP / GET_FLATTENED_SEQUENCE_DATA を実装して save/load / duplicate に対応。**書き込みは render 系(`PF_Cmd_SMART_RENDER` / `PF_Cmd_SMART_RENDER_GPU`)以外の lifecycle selector(SETUP / RESETUP / SETDOWN / CHANGED 等)でのみ行う、render 時は read-only でアクセス**(`PF_EffectSequenceDataSuite1::PF_GetConstSequenceData` 経由、`PF_ConstHandle` を受け取って dereference)。**なお thread-affinity の前提は置かない**: SDK AE_Effect.h L1123 は RESETUP が either thread で発生すると明記、SETDOWN も main-thread 保証なし。SETUP のみ `GET_FLATTENED_SEQUENCE_DATA` 有効時に UI thread 限定(L1123)。したがって lifecycle selector 側の副作用(`GPU_FALLEN` insert/remove、pipeline HashMap 更新等)は全て thread-safe な構造(`DashMap` / `Atomic*` / `RwLock`)で扱う必要がある — これらが render 並列と同時に走り得る前提。

2. **plugin-global HashMap(in-memory、プロセス生存期間のみ)**: fallen flag を保持
   ```rust
   static GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>> = Lazy::new(DashMap::new);
   // key   = instance_uuid(sequence_data から read-only で取得)
   // value = AtomicBool、true になったら sticky
   ```

**動作フロー**:

- `SEQUENCE_SETUP`: UUID を新規生成、`seq.instance_uuid_{hi,lo} = split(new_uuid_v4())`。`GPU_FALLEN` にはエントリを作らない(absence = not-fallen)
- `SEQUENCE_RESETUP`(save/load 後、duplicate 後、in_data 変更後のいずれでも呼ばれる): **flattened data の UUID は参照せず、常に新 UUID を再生成して上書き**する。根拠は SDK AE_Effect.h L1094-L1099 / L1112-L1113 —「RESETUP は (1) save/load 後、(2) duplicate 後(複製元と複製先の両方で呼ばれる)、(3) in_data 変更後」の 3 経路で発生し、plugin には duplicate かどうかの判別情報が来ない。もし flattened UUID をそのまま復元すると複製元と複製先が同一 UUID を共有し、`GPU_FALLEN` エントリと `SEQUENCE_SETDOWN` の `remove(uuid)` が意図せず干渉する(片方の setdown が相手の sticky 状態を消す)。再生成方式なら duplicate 時に両者が独立した UUID を持ち、かつ `GPU_FALLEN` miss で **自動的に fresh retry** になる(save/load / duplicate / in_data 変更のいずれでも一貫した挙動、Medium 1 の自己矛盾が解消)。
- `SMART_RENDER_GPU` 入口: sequence_data(read-only、`PF_GetConstSequenceData` 経由)から UUID 取得、`GPU_FALLEN.get(&uuid)` で fallen 判定
  - fallen の場合: CPU 経路実行、`PF_Err_NONE` return
  - そうでない場合: GPU 試行 → 失敗時 `GPU_FALLEN.entry(uuid).or_insert(AtomicBool::new(false)).store(true, Relaxed)` → CPU fallback → `PF_Err_NONE` return
- `SEQUENCE_SETDOWN`: sequence_data(read-only アクセス)から UUID 取得して `GPU_FALLEN.remove(&uuid)` で掃除。SDK は SETDOWN の thread-affinity を保証していない(AE_Effect.h L1140 に記述無し)ため、render thread と並行発生し得る前提で扱う。`DashMap::remove` は内部 shard lock で thread-safe、かつ他 instance の render thread が同 `DashMap` を別 UUID で読む操作と干渉しない(UUID が key になっているため)。RESETUP で UUID が再生成されているので、duplicate 後は複製元と複製先の setdown が別 key を触り衝突しない。

**この設計で得られるもの**:

| 要件 | 達成 |
|---|---|
| sequence_data render 時 read-only 契約の遵守 | ✓(書き込みは render 以外の lifecycle selector のみ、thread-affinity は前提しない) |
| `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` 不要 | ✓(MFR 並列度を失わない) |
| per-effect-instance の独立性(他インスタンスに波及しない) | ✓(RESETUP 時 UUID 再生成により duplicate 後も独立) |
| セッション/span をまたいで sticky(バッチ書き出し全体で CPU 固定) | ✓(HashMap はプロセス生存中保持、SLOWER flag の "span 境界で discard" の影響なし) |
| プロジェクト再オープンでリトライ可能 | ✓(HashMap はプロセス再起動でクリア、UUID 復元だけでは fallen 状態を引き継がない) |
| MFR 並列書き込み安全性 | ✓(`DashMap` 内部 shard + `AtomicBool` Relaxed store、`UUID` は stable key) |

**plugin-global AtomicBool 案(round 1 で検討)は引き続き不採用**: 複数インスタンスに fallen が波及する問題は本設計(per-UUID)で解消、かつ SDK 制約も遵守できる形に進化。

#### 代替案(比較のため記録、不採用)

- `AEGP_ComputeCacheSuite`: AEGP(別 plugin 型)側の仕組みでキャッシュ層を構築。unified cache で multi-thread 間で再計算されない設計。bool flag 1 個にはオーバーキル、本件では採らない。Phase 2-B 以降に複雑な per-instance cache が必要になった場合の候補として記録。
- `PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER`: 上記の通り「span 境界で discard」仕様と sticky 要件が合わず、かつ常時 MFR コストを払う構造のため不採用。

### 4.7 Fallback を発動させるエラー条件

| エラー源 | 検出方法 | fallback 発動 |
|---|---|---|
| `AllocateDeviceMemory` 失敗 | 戻り値 `PF_Err != PF_Err_NONE` | ✓ |
| shader compile / pipeline 作成失敗(SETUP 時) | Metal: `MTLCompileError`、CUDA: `cuModuleLoadData` != 0 | ✓ |
| kernel launch 失敗 | Metal: command buffer `error` / `.status == .error`、CUDA: `cuLaunchKernel` != 0 | ✓ |
| kernel timeout / driver reset | Metal: `.status == .error` with `MTLCommandBufferErrorTimeout`、CUDA: `cuCtxSynchronize` != 0 with CUDA_ERROR_LAUNCH_TIMEOUT | ✓ |
| 結果の numeric 検証失敗(NaN 等) | debug build 時のみ sampling、release では skip | (debug) |
| user opt-out(UI checkbox OFF) | paramter 読み取り | 通常経路(fallback 扱いしない) |

### 4.8 SmartRender 経路の構造(Phase 2-A.1 作業範囲)

Phase 2-B までの smooth は legacy `PF_Cmd_RENDER` のみ実装。Phase 2-A.1 で SmartRender 三本化(CPU / GPU の**別 selector** + legacy):

```
PF_Cmd_SMART_PRE_RENDER   // 入力要求を AE に伝える(下記フラグを立てる)
  |-- PF_RenderOutputFlag_GPU_RENDER_POSSIBLE  // GPU で走れる状況なら true
  |-- max_result_rect / input_rect 計算(preprocess の bbox 予測は困難なので大きめ)

PF_Cmd_SMART_RENDER         // CPU SmartRender(GPU 不能 / user opt-out / once-fallen 時)
  |-- 既存 process() call、ただし bpc switch に Pixel32 (f32) を追加(Phase 2-A.2)

PF_Cmd_SMART_RENDER_GPU     // GPU SmartRender(AE が GPU device 込みで呼んでくる)
  |-- framework 別分岐(Metal / CUDA)
  |-- AllocateDeviceMemory + kernel dispatch + DisposeGPUWorld
  |-- GPU 失敗時は plugin-global DashMap<UUID, AtomicBool>(GPU_FALLEN)に fallen セット + CPU path 実行 + PF_Err_NONE 返却

PF_Cmd_RENDER                // legacy、SmartRender 非対応 AE 向けの後方互換のみ残す
```

legacy `PF_Cmd_RENDER` は残す(古い AE / scriptable rendering 経路で呼ばれる可能性)。
`SUPPORTS_SMART_RENDER` flag は `AE_Effect_Global_OutFlags_2` に追加、MFR flag と共存する。

### 4.9 Item 4 の結論

- Full GPU plugin パターン採用で device 排他は AE 任せ、実装は per-device pipeline + per-call buffer のシンプル構造
- Metal: queue は AE 提供のものを共有、command buffer は per-call で thread-safe 自然に達成、**`waitUntilCompleted` は呼ばず commit のみ**(AE が synchronization)
- CUDA: SDK サンプル準拠で `cuCtxPushCurrent` / `cuCtxPopCurrent` は**初期実装で省略**、spike で検証、stream は AE 提供のものを使う
- VRAM 予算: 4K 16bpc 2 pass で ~400 MB / frame、16 frames in flight で 6.4 GB の OOM リスクあり → 同時実行数ソフト制限 + 超過時 fallback
- Fallback: once-fallen-always-fall、**sequence_data に UUID のみ格納 + plugin-global `DashMap<UUID, AtomicBool>` で fallen flag を保持**する 2 層分離設計(Item 6 §6.5 で実装案)。render 時の sequence_data mutation を避け、`PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` の MFR コストも回避
- SmartRender **三本化**(CPU `SMART_RENDER` / GPU `SMART_RENDER_GPU` / legacy `RENDER`、Phase 2-A.1 で実装)が GPU 対応の前提工事

### 4.10 Item 4 で積み残した検討事項(実装 phase で確認)

- AE が MFR で同一 plugin・同一 device に**同時に `SMART_RENDER_GPU` を呼ぶか**(SDK サンプルは thread-safety guard なしで書かれている、初期 PoC で実測確認)
- CUDA context push/pop の実際の必要性(SDK サンプル準拠の省略で問題ないか、MFR 並列 thread で別 context が走る状況を実機で確認)
- AE の RenderThreadExecutor 並列度を plugin が query する API の有無(SDK grep 済、見つからず → plugin からは見えない前提で進める)
- VRAM 不足時に AE 側でハンドリングしてくれるか(`AllocateDeviceMemory` 失敗時の AE の挙動が "このフレームだけ CPU" になるか "プラグイン全体を止める" になるか、spike 要)

---

---

## Item 5: CPU/GPU 切替 UI 設計

### 5.1 AE の GPU 設定 2 階層(前提把握)

AE には 2 つの GPU ゲートが存在し、両方が満たされた時のみ plugin は GPU 経路に入る:

1. **Project-level**: `Project Settings > Video Rendering and Effects > Use: [GPU / Software Only]`
   - "Software Only" の場合、AE は plugin に GPU device を提供しない → plugin は自動的に CPU 経路のみ
   - この設定は AE 側で処理される、plugin 側の関心事ではない
2. **Per-effect**(今回新設する): smooth の Effect Controls 上のチェックボックス
   - user が「この smooth インスタンスだけ CPU を強制したい」ユースケースに対応
   - 典型的用途: GPU 経路の挙動に疑問がある時のトラブルシュート、レイヤ単位の性能コントロール

### 5.2 追加する UI パラメータ(v1.6.0、Phase 2-A.3 で導入)

既存パラメータに **GPU Acceleration** を 1 つ追加。

**現行(release tag v1.5.1、バイナリ About は `smooth, v1.5.0`)**:
```
Effect Controls > smooth:
├── white option     [☐ transparent]
├── range            [float slider valid 0-100, display 0-10, default 1.0 → 内部で bpc 別 u32 へ変換、Effect.cpp L454-464]
├── line weight      [float slider valid 0-1, display 0-1]
└── Build            [0.1.0+<sha>] (clickable → About)
```

**v1.6.0 提案**:
```
Effect Controls > smooth:
├── white option     [☐ transparent]
├── range            [slider]
├── line weight      [slider]
├── ─── separator ───
├── GPU Acceleration [☑ Enabled]   ← 新規
└── Build            [0.1.0+<sha>] (clickable → About)
```

### 5.3 パラメータ種別の選択

| 選択肢 | Pro | Con |
|---|---|---|
| **Checkbox**(☑ Enabled / ☐ Disabled) | 最小限、user が理解しやすい | 「Auto(GPU 試して失敗なら CPU)」と「GPU 強制」の区別が出せない |
| Popup(`Auto / CPU / GPU`) | 3 値、power user 向け | UI 複雑化、「GPU」選択時の失敗が user 責任になり不親切 |
| Slider はない | - | - |

**採用**: **Checkbox**。意味は **☑ = Auto(GPU 試す、失敗時 CPU)** / **☐ = CPU 固定**。

GPU 強制モード(失敗時にエラー表示)は v1.0 scope 外。必要になったら v1.7.x で popup に差し替え。

**理由**:
- smooth ユーザーの大半は「速ければ GPU、動かないなら黙って CPU」を望む(動画編集 workflow は失敗時に止まると困る)
- Phase 2-B MFR でも「CPU fallback が堅牢」ことを売りにしている方向性と整合
- 将来 popup に拡張する際、checkbox 値 `true` ↔ `Auto`、`false` ↔ `CPU` でマッピング可能(互換破壊なし)

### 5.3.1 GPU 非対応システムでの checkbox 無効化(2026-04-23 確定)

GPU が使えないシステム(Win + NVIDIA 以外、将来の Mac で Metal 非対応等)では checkbox を **グレイアウトして user が触れないようにする**。

**採用方針**: `PF_ParamFlag_DISABLED` を **param 登録時に静的に立てる**(動的 UI 更新は使わない)。

#### 検出ソース: AE の `PF_GPUDeviceSuite1::GetDeviceCount`(仮説、spike で実測確認)

OS API 直接呼び出し(`MTLCreateSystemDefaultDevice` / `cuInit`)ではなく、AE 経由で検出する候補。

SDK ヘッダ [AE_EffectGPUSuites.h L72] の説明は「host がサポートする device 数を返す」までで、**project-level の `Software Only` 設定や driver 不良の反映は SDK spec からは断定できない**。

本 doc では以下を**作業仮説**として扱い、Phase 2-A.3 実装 spike で実測して最終確定する:

- 仮説 1: AE の project-level GPU 設定が `Software Only` なら `GetDeviceCount` は 0 を返す、もしくは全 device の `compatibleB` が false になる
- 仮説 2: driver 不良で AE 自身が device を認識できない場合も `GetDeviceCount` に反映される
- 仮説 3: 複数 GPU 環境で「AE が実際に使える」ものだけ列挙される

spike 結果次第では以下の fallback 設計も検討する:
- OS API での直接検出(`MTLCreateSystemDefaultDevice` non-nil + `cuDeviceGetCount > 0`)を組み合わせる
- `PF_Cmd_GPU_DEVICE_SETUP` で初めて実 device info が来るので、そこまでは「GPU 対応未確定」扱いで checkbox 有効のまま、SETUP 時点で disabled に切り替え(§5.3 の Item 5 方針は「UI 動的切替しない」だったので、現実装仮説どちらでも UX 確定させられるか再評価)

#### 検出タイミング

`PF_Cmd_GLOBAL_SETUP` 内で 1 度だけ実行、結果を plugin-global static に保存。

```cpp
static bool s_gpu_supported = false;

extern "C" PF_Err GlobalSetup(PF_InData* in_data, ...) {
    // ... 既存の out_flags / out_flags2 設定

    PF_GPUDeviceSuite1* gpuSuite = nullptr;
    suites.Pica()->AcquireSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1,
                                 (const void**)&gpuSuite);
    if (gpuSuite) {
        A_u_long count = 0;
        gpuSuite->GetDeviceCount(in_data->effect_ref, &count);
        for (A_u_long i = 0; i < count; ++i) {
            PF_GPUDeviceInfo info;
            gpuSuite->GetDeviceInfo(in_data->effect_ref, i, &info);
            if (!info.compatibleB) continue;
#ifdef AE_OS_MAC
            if (info.device_framework == PF_GPU_Framework_METAL) {
                s_gpu_supported = true; break;
            }
#else
            if (info.device_framework == PF_GPU_Framework_CUDA) {
                s_gpu_supported = true; break;
            }
#endif
        }
    }
    return err;
}
```

#### ParamsSetup での適用

```cpp
PF_ParamDef def; AEFX_CLR_STRUCT(def);
PF_ADD_CHECKBOX("GPU Acceleration", "Enabled",
                TRUE,                                   // デフォルト ON
                s_gpu_supported ? 0 : PF_ParamFlag_DISABLED,
                GPU_ACCEL_DISK_ID);
```

#### 挙動マトリクス

| システム | checkbox 表示 | plugin 内部動作 |
|---|---|---|
| Mac + Metal 対応 GPU(AE 2025 サポート対象機の全て) | 有効 ☑ Enabled | 値通り(ON なら GPU 試行、OFF なら CPU 固定) |
| Win + CUDA 対応 NVIDIA | 有効 ☑ Enabled | 値通り |
| Win + AMD / Intel iGPU のみ(CUDA 非対応) | **グレイアウト** ☑(変更不可) | CPU 固定(s_gpu_supported == false なので param 値は無視) |
| Win + NVIDIA あるが driver 不良 | **グレイアウト**(AE が device を返さないケース) | CPU 固定 |
| (将来)Mac で Metal 非対応 OS/ハードウェア | **グレイアウト** | CPU 固定 |

#### Mac でも検出機構を入れる理由

AE 2025 が Mac でサポートするシステム要件(Apple Silicon または Metal 2 対応 Intel GPU)を満たす限り、Metal 非対応のケースは**実質的に起こらない**。それでも検出を入れる理由:

- **将来の Adobe のシステム要件変更への耐性**: AE 2026 / 2027 で Metal 3 必須などの変更があった場合、未対応ハードウェアでも plugin load はできる。その際 checkbox がグレイアウトされれば UX として正しい動作
- **eGPU / 外付け GPU の挙動**: エッジケースで Metal device が AE から見えないことが起こり得る
- **コード対称性**: Win 側で必要な検出ロジックを Mac 側でも同じ構造で持つことで、将来の OS 対応追加(例: Linux 想定外)が起きた時に差し込みやすい

#### 動的 UI 更新を採らない理由

`PF_UpdateParamUI` で後から disabled 化する方式は:
- plugin load 直後は checkbox が有効に見え、その後グレイアウトに遷移する過渡期が user 目撃する
- `PF_Cmd_UPDATE_PARAMS_UI` の発火タイミング管理が複雑化

検出タイミングを GLOBAL_SETUP に前倒しすれば param 登録時から正しい状態にできるので、動的更新は不要。

#### 運用上の制限(許容)

| 制限 | 影響 | 判断 |
|---|---|---|
| eGPU のホットプラグ非対応 | plugin 起動後の GPU 追加は再起動要 | 実用上許容 |
| AE の project-level GPU 設定変更への非追従 | GlobalSetup 時点で固定、変更後は AE 再起動要 | 実用上許容(project 設定変更はまれ) |
| `s_gpu_supported` は plugin-global static | 同プロセス内の複数 smooth インスタンスで共通、plugin load 時に 1 回書き以降 read-only、thread-safety 問題なし | 設計通り |

### 5.4 デフォルト値

**☑ Enabled**(GPU 試す、デフォルト ON)。

**理由**:
- ユーザーの期待: 「GPU を積んでいるマシンなら使ってほしい」が標準感覚
- CPU fallback があるので default ON でも安全
- CPU/GPU 経路は visually indistinguishable(near-identical)が設計契約 → default 切替でユーザーの画が変わる懸念は最小
- AE の project-level GPU 設定が OFF なら自動的に CPU 経路(per-effect checkbox は効果を持たない)

**既存プロジェクト読み込み時**: AE が missing param をデフォルト値で埋めるので、v1.5.1 で作った project を v1.6.0 で開くと GPU が有効化される。CPU/GPU の render 結果が視覚的に同等であれば問題ないが、byte-identical ではない点は [RELEASE_NOTES-v1.6.0.md](RELEASE_NOTES-v1.6.0.md)(将来作成)で明記予定。

### 5.5 ラベル文言

| 選択肢 | 採否 |
|---|---|
| `GPU Acceleration` | **採用**: Adobe 公式他エフェクトと揃う、意味が明確 |
| `Use GPU` | 短いが動詞形で場違い |
| `Hardware Acceleration` | GPU より広い意味で誤解の余地 |
| `アクセラレーション(日本語)` | AE の param ラベルは英語が慣例、ローカライズ未対応 |

### 5.6 About ダイアログの拡張

現行(release tag v1.5.1、`version.h` は `v1.5.0` のまま、Effect.cpp L363 の `MyVersion` が `PF_SPRINTF("smooth, v%d.%d"...)` で埋め込み):
```
smooth, v1.5.0
rust_core 0.1.0+<sha> ffi=0x00020003
```

v1.6.0 での案(**`version.h` の `MINOR_VERSION` を 6 に bump、`BUILD_VERSION` を 0 に戻す**):
```
smooth, v1.6.0
rust_core 0.1.0+<sha> ffi=0x00030000
GPU: Metal (Apple M1 Pro) ready
```

GPU 情報の行は plugin が AE から受け取った device info を基に動的生成:
- GPU 非対応プロジェクト設定時: `GPU: disabled in project settings`
- GPU 有効だが plugin 側 checkbox OFF: `GPU: disabled by user`
- GPU 使用中、Metal: `GPU: Metal (<device name>) ready`
- GPU 使用中、CUDA: `GPU: CUDA (<device name>, <VRAM> MB) ready`
- GPU 試行したが一度失敗してから fallback 中: `GPU: fallen to CPU (check AE log for cause)`

### 5.7 パラメータレイアウトへの影響

AE SDK では:
- **param 追加は `my_version` / `AE_Effect_Version` bump 必須**(Phase 2-A build_id UI の時と同様)
- 既存パラメータの ID / 順序は保持(互換性のため、新規 ID は末尾に追加する)
- separator は `PF_AddSupervisedParamWithFlags` 系で `PF_ParamFlag_START_COLLAPSED` などを使うか、空ラベルの group header で表現する(smooth は collapsible group 未使用なので、シンプルな separator は `PF_AddCheckBoxParamDef` の直前に空 group を入れるのが一般的)

Phase 2-A.1(SmartRender 経路追加)か 2-A.3(GPU 本体)のどちらで UI パラメータを入れるかは**検討の余地あり**:
- 2-A.1 で入れると: user は "GPU Acceleration" スイッチが見えるのに、実装がまだ来ていない状態になる(無害だが紛らわしい)
- 2-A.3 で入れる: SmartRender 単独 release の可能性(fallback release 計画)時に UI が変わらない

**推奨**: **UI パラメータ追加は Phase 2-A.3 本体と一緒**。Phase 2-A.1/2 の途中 release(fallback release 発動時)では UI 変更しない、Phase 2-A.3 が成功して初めて UI にスイッチが出る。

### 5.8 sequence_data の扱い(関連設計検討)

Phase 2-B 時点では sequence_data は **未使用のまま `SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` だけ立てて NULL 返し**していた(AE 2025 の `FLTp_EnforceFlagCombinations` が立てさせただけ)。

Phase 2-A.3 で**sequence_data を復活させる**:
- **sequence_data が持つもの**: エフェクトインスタンスの **UUID のみ**(`struct SmoothSequenceData { version, instance_uuid_hi, instance_uuid_lo }`)
- **保存しないもの**: GPU-side state(compiled pipeline、fallen flag 等)は plugin-global 側の in-memory 構造に分離

**state の保存先マトリクス**:

| state 種別 | 保存先 | ライフタイム | 備考 |
|---|---|---|---|
| **instance UUID** | **sequence_data**(per-instance、AE lifecycle 管理) | 各 SETUP/RESETUP 区間 | **SEQUENCE_SETUP で生成、SEQUENCE_RESETUP では flattened 値を無視して必ず再生成**(duplicate で複製元と複製先が同一 UUID になるのを回避、save/load 越しは `GPU_FALLEN` がプロセス境界で消えるので新 UUID で問題ない) |
| **compiled GPU pipeline**(MTLComputePipelineState / CUfunction) | **plugin-global `HashMap<device_index, Pipeline>`**(`dashmap` or `RwLock<HashMap>`) | プロセス生存期間 | 同プロセス内の全 instance で共有、メモリ節約 |
| **GPU fallen flag** | **plugin-global `DashMap<UUID, AtomicBool>`** | プロセス生存期間 | §4.6 / §6.5 の 2 層分離設計、sequence_data に格納すると render 時 read-only 制約に抵触 |

**設計意図**:
- sequence_data は **render 時 read-only 契約(AE_Effect.h L926)** を遵守、書き込み用途には使わない
- instance UUID が sequence_data にあるおかげで、plugin-global HashMap を per-instance に分離できる(fallen flag の過剰波及を防止)
- project close / plugin unload で HashMap が消える → プロセス再起動時に fresh retry 可能(Medium 1 で指摘された save/load 矛盾の解消)

### 5.9 Item 5 の結論

- UI に `GPU Acceleration` **checkbox 1 個のみ**追加、デフォルト ON
- 意味: ☑ = Auto(GPU 試す、失敗で CPU)、☐ = CPU 固定
- **GPU 非対応システムでは checkbox をグレイアウト**(`PF_ParamFlag_DISABLED` を param 登録時に静的適用)
  - 検出方式の**第一候補**: AE の `PF_GPUDeviceSuite1::GetDeviceCount`(OS API 直叩きより project-level 設定や driver 状態の反映が期待できる、ただし SDK header L72 は「host supports する device 数」までしか保証していないので、Phase 2-A.3 実装 spike で実測確認してから最終採用)
  - spike で不十分と判明した場合: `MTLCreateSystemDefaultDevice` non-nil(Mac)+ `cuDeviceGetCount > 0`(Win)の OS API 併用 fallback
  - Mac でも機構を入れる(将来の OS 要件変更への保険、コード対称性)
- UI 追加タイミングは Phase 2-A.3 本体と同時(fallback release 時は UI 不変)
- About ダイアログに GPU 状態を追加表示(Metal/CUDA/disabled/fallen-to-CPU)
- state 保存先の分離: sequence_data に UUID のみ / plugin-global `HashMap<device_index, Pipeline>` に compiled GPU pipeline / plugin-global `DashMap<UUID, AtomicBool>` に fallen flag(詳細は §5.8)
- 既存パラメータ配置は保持、`my_version` + `AE_Effect_Version` bump 必須(build_id UI 時と同じ手順)

### 5.10 Item 5 で積み残した検討事項(実装 phase で確認)

- About ダイアログの GPU 状態表示を「テキスト埋め込み」で済ませるか「動的更新」にするか(AE の ARBITRARY_DATA 系 param を使えば可能、ただし実装コスト増)
- separator/group header の具体的な AE SDK 構造(`PF_AddSupervisedParamWithFlags` の使い分け要確認)
- GPU checkbox 状態変更時の AE の再 render 挙動(param change invalidation)が期待通り動くか(実装 spike で確認)

---

---

## Item 6: 競合 / 参考実装調査

**最重要参考実装**: AE SDK 同梱の `Examples/Effect/SDK_Invert_ProcAmp/SDK_Invert_ProcAmp.cpp`(1210 行)。**smooth にとって雛形として使える完全な full-GPU プラグイン**。Invert Color + ProcAmp の 2 パスを CUDA / OpenCL / Metal / DirectX12 全 framework でサポート。

### 6.1 Item 1-4 の open questions に対する SDK 回答

| open question | SDK 実装から読み取れる回答 |
|---|---|
| MFR で同一 plugin・同一 device に同時 render call されるか | **サンプルは thread-safety の追加ガードなし**。per-render-call の一時 buffer + per-device の read-only pipeline で naturally thread-safe。SDK コメント通り AE 側が device 排他管理 |
| SmartRender と SmartRender_GPU の関係 | **distinct な selector**(`PF_Cmd_SMART_RENDER` と `PF_Cmd_SMART_RENDER_GPU`)。フラグではなく別 entry point |
| GPU error 時の挙動(AE 側 fallback あるか?) | **AE 側の自動 fallback なし**。サンプルはエラー時 `PF_Err` を AE に返すのみ。一度 GPU が落ちても AE は次フレームで再度 GPU を呼ぶ可能性あり |
| CUDA context management(push/pop 要?) | **サンプルは push/pop を書いていない**(事実)。ただし SDK header はこの挙動を保証していないため「AE が entry 前に context を current にセット済み」の前提は仮説。Phase 2-A.3 spike で実測検証、default は SDK サンプル準拠で省略、push/pop を入れても zero-cost safety margin として採用可 |
| VRAM OOM 時の AE ハンドリング | **AllocateDeviceMemory 失敗 → `PF_Err` を返す**のが SDK 実装。AE の挙動はサンプルからは不明(実装 spike 要) |
| AE MFR 並列度 query API | AE_Effect.h grep で見つからず。**plugin からは見えない**、AE が内部で制御している模様 |

### 6.2 SDK_Invert_ProcAmp の構造(smooth 実装の雛形として流用可能)

#### GlobalSetup
```cpp
out_data->out_flags  = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE      // ★ 32bpc 対応必須
                     | PF_OutFlag2_SUPPORTS_SMART_RENDER
                     | PF_OutFlag2_SUPPORTS_THREADED_RENDERING
                     | PF_OutFlag2_SUPPORTS_GPU_RENDER_F32
                     | PF_OutFlag2_SUPPORTS_DIRECTX_RENDERING;  // DX12 使用時
```

smooth 側での差分:
- 既存の `PF_OutFlag_I_WRITE_INPUT_BUFFER` を保持(preprocess が in-place なので)
- DX12 は scope 外 → `SUPPORTS_DIRECTX_RENDERING` 立てない
- MFR 関連は既存(`SUPPORTS_THREADED_RENDERING`, `SUPPORTS_GET_FLATTENED_SEQUENCE_DATA`, `I_AM_THREADSAFE`)を継続

#### PreRender
```cpp
extraP->output->flags |= PF_RenderOutputFlag_GPU_RENDER_POSSIBLE;  // ★ GPU 可能を AE に伝達
// params を checkout して pre_render_data にセット、Render 時に参照
extraP->output->pre_render_data = infoP;  // heap-allocated
extraP->output->delete_pre_render_data_func = DisposePreRenderData;
// 入力レイヤを checkout
extraP->cb->checkout_layer(..., &in_result);
UnionLRect(&in_result.result_rect, &extraP->output->result_rect);
```

**smooth 側の設計変更点**:
- `pre_render_data` に smooth の params(range, line_weight, white_option, GPU checkbox の値)+ 予想 bbox をセット
- smooth の bbox は実 preprocess 後に確定するため、PreRender 時は全領域(input と同じ rect)を conservatively 返す(AE が余分な領域を render することはない)

#### GPU_DEVICE_SETUP(per-device、一度きり)
```cpp
// framework 別分岐
switch (extraP->input->what_gpu) {
    case PF_GPU_Framework_METAL:
        // 1. MTLDevice 取得(device_info.devicePV)
        // 2. MSL source を newLibraryWithSource でコンパイル
        // 3. newFunctionWithName で kernel 取得
        // 4. newComputePipelineStateWithFunction で pipeline 作成
        // 5. MetalGPUData に格納して extraP->output->gpu_data へ
    case PF_GPU_Framework_CUDA:
        // SDK コメント: "Nothing to do here. CUDA Kernel statically linked"
        // → build 時 NVCC でコンパイル、.aex に静的リンク、runtime で直接 kernel launch
}
out_dataP->out_flags2 = PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;  // ★ この framework で実装あり、を示す
```

**smooth 側**:
- Metal: 同パターン、MSL は `include_bytes!` で Rust crate に埋め込み、NSString 変換して newLibraryWithSource
- CUDA: サンプル通り静的リンク(NVCC build-time、extern C 関数を Rust から呼ぶ)。PTX runtime ロードは不要

#### SMART_RENDER_GPU(per-frame、MFR 並列で呼ばれる)
```cpp
// 1. PF_EffectWorld (GPU world) から src_mem, dst_mem を GetGPUWorldData で取得
// 2. AllocateDeviceMemory or CreateGPUWorld で中間バッファ確保
// 3. per-framework で kernel launch:
//    - Metal: MTLCommandBuffer → ComputeEncoder → dispatchThreadgroups → commit
//    - CUDA: 静的リンク関数を call(内部で cudaLaunchKernel 等)
// 4. DisposeGPUWorld で中間バッファ解放
```

**Metal の重要な実装細部**:
```cpp
id<MTLCommandQueue> queue = (id<MTLCommandQueue>)device_info.command_queuePV;  // AE 提供
id<MTLCommandBuffer> commandBuffer = [queue commandBuffer];
id<MTLComputeCommandEncoder> enc = [commandBuffer computeCommandEncoder];
[enc setComputePipelineState:metal_data->invert_pipeline];
[enc setBuffer:src_metal_buffer offset:0 atIndex:0];
[enc setBuffer:dst_metal_buffer offset:0 atIndex:1];
[enc dispatchThreadgroups:numThreadgroups threadsPerThreadgroup:threadsPerGroup];
[enc endEncoding];
[commandBuffer commit];
// 注目: waitUntilCompleted 呼んでいない!
err = NSError2PFErr([commandBuffer error]);
```

**`waitUntilCompleted` 省略の意味**: AE が commit 後の synchronization を自動でハンドルしてくれる。plugin は commit して error ステータスだけチェックすれば OK。

**CUDA の重要な実装細部**:
```cpp
// サンプル: cudaSetDevice/cudaCtxSetCurrent 呼んでいない
Invert_Color_CUDA(...);  // これは .cu ファイルの関数で、中で cudaMemcpy / cudaLaunchKernel
if (cudaPeekAtLastError() != cudaSuccess) { err = PF_Err_INTERNAL_STRUCT_DAMAGED; }
```

**サンプルが push/pop を書かずに動いている**という事実から、AE が entry 前に current context をセット済みである**可能性が高い**(ただし SDK 公式ドキュメントには明記されていない)。smooth の実装では Phase 2-A.3 spike で以下を検証する:
1. SDK サンプル準拠で push/pop 省略 → MFR 並列 render thread で context が正しく current になっているか実機確認
2. もし (1) で問題が出るなら `cuCtxPushCurrent(ae_ctx)` / `cuCtxPopCurrent` で entry/exit を囲む(overhead 想定 < 1 µs/call)

実装初版は (1) の SDK 準拠で書き、spike 結果次第で (2) に切り替える。

### 6.3 SDK 実装と smooth 設計の差分整理

SDK サンプルをベースに smooth 側で追加/変更する項目:

| 項目 | SDK サンプル | smooth で必要な対応 |
|---|---|---|
| CPU アルゴリズム | `FilterImage8` / `FilterImage16` / `FilterImage32` の 3 本(pixel-independent なので画素単位 callback で OK) | smooth の process_row_range は pixel-independent ではない(隣接依存)→ `extraP->cb->iterate` 系ではなく自前 loop |
| 32bpc 対応 | サンプルは 32bpc native 対応済み | Phase 2-A.2 で CPU 側を 32bpc 拡張(Item 2 §2.3) |
| GPU shader 複雑度 | 単純な per-pixel lookup | smooth は 2-pass(検出 → blending)で中間 buffer 必須、shader は長くなる |
| sequence_data | SDK サンプルでは未使用(NULL 返し) | **Phase 2-A.3 で復活、UUID のみを格納**(§4.6 + §5.8 + §6.5)。GPU pipeline と fallen flag は plugin-global 側に分離 |
| GPU error handling | `PF_Err` を return | 我々は catch して once-fallen flag セット、CPU fallback して `PF_Err_NONE` を return(§6.5 参照) |
| GPU checkbox UI | なし(常に GPU 試行) | Item 5 §5.3 の通り追加、`PF_ParamFlag_DISABLED` 機構も入れる |

### 6.4 CUDA 静的リンク方式の詳細(build 戦略確定)

SDK サンプルの CUDA は以下の build 手順:
1. `SDK_Invert_ProcAmp_Kernel.cu` を NVCC でコンパイル → obj/lib
2. plugin の main cpp と同時に link → .aex / .dll に静的リンク
3. runtime: 通常の C++ 関数呼び出しで kernel launch

**smooth で採用する場合**:
- `rust/smooth_core/build.rs` で NVCC を呼ぶ(Windows のみ)
  - NVCC が手元にない場合は feature flag で CUDA backend 全体を無効化
- `.cu` → static lib → Rust の `cc` crate or `build.rs` で cargo 連携
- Rust 側から `extern "C" fn smooth_gpu_cuda_preprocess(...)` を呼ぶ

**代替(NVRTC runtime compile)**:
- NVRTC でソース → PTX を plugin 起動時にコンパイル
- NVCC toolchain 要件がなくなる(ユーザー環境に CUDA driver だけあれば OK、CUDA SDK 不要)
- ただし runtime コンパイルで 1st frame が遅い

**v1.0 判断**: **build-time static link を採用**(SDK サンプルに準拠、signed build の信頼性高)。NVRTC は後日要件変更時に検討。

### 6.5 once-fallen-always-fall の 2 層分離実装案(plugin-global HashMap + sequence_data UUID)

詳細な設計根拠と SDK 制約(render 時 sequence_data read-only、`MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` の span 境界 discard 問題)は §4.6 を参照。本節は C++ / Rust 具体実装の雛形。

**C++ 側 sequence_data 構造**(AE が lifecycle 管理、flattened data で save/load に対応):

```cpp
// 現在 smooth は SUPPORTS_GET_FLATTENED_SEQUENCE_DATA だけ立てて NULL 返し
// → Phase 2-A.3 で以下の構造を復活、ただし中身は UUID のみ(fallen flag は含めない)

struct SmoothSequenceData {
    uint32_t version;          // 1 から開始、schema 変更時に bump
    uint64_t instance_uuid_hi; // §4.6 と統一(u128 を 2 × u64 に分割、C 構造体 align と FFI 互換)
    uint64_t instance_uuid_lo;
};

// SEQUENCE_SETUP: UUID を新規生成、sequence_data に書き込む(render 前なので mutable OK)
// SEQUENCE_RESETUP: ★flattened data の UUID は読み捨てて必ず新 UUID を再生成する★
//   - AE_Effect.h L1094-L1099 / L1112-L1113 の通り RESETUP は save/load・duplicate・in_data 変更
//     の 3 経路で呼ばれ、plugin 側から duplicate かどうかは判別できない。flattened UUID を
//     そのまま復元すると複製元と複製先が同一 UUID を共有し GPU_FALLEN と SETDOWN の remove が
//     衝突するため、再生成方式で duplicate 安全性と "再オープン = fresh retry" を両立する。
// SEQUENCE_FLATTEN: 構造体をそのままコピーして flat data を返す(UUID は保存されるが、
//   RESETUP 側で無視されるので実質 placeholder。フィールドを空にしても良いが version 管理の
//   ため struct layout は維持する)
// GET_FLATTENED_SEQUENCE_DATA: SEQUENCE_FLATTEN と同じ構造を返す
// SEQUENCE_SETDOWN: UUID を読み取り、Rust 側に GPU_FALLEN.remove(uuid) を通知
```

**Rust 側(plugin-global state、GPU_FALLEN HashMap)**:

```rust
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};

static GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>> = Lazy::new(DashMap::new);

// SMART_RENDER_GPU 入口(render 時、sequence_data は read-only でアクセス):
pub unsafe fn smooth_core_try_gpu_render(
    in_data: *const PF_InData,
    uuid_hi: u64,
    uuid_lo: u64,
    // ... GPU 引数
) -> i32 {  // 0 = GPU 成功、非0 = CPU fallback 実行済み
    let uuid: u128 = ((uuid_hi as u128) << 64) | (uuid_lo as u128);

    // 既に fallen?
    if let Some(v) = GPU_FALLEN.get(&uuid) {
        if v.load(Ordering::Relaxed) {
            run_cpu_render(in_data);
            return 1;  // fallen、CPU 実行
        }
    }

    // GPU 試行
    match try_gpu_render_inner(in_data) {
        Ok(()) => 0,
        Err(e) => {
            log::warn!("smooth_core: GPU render failed: {:?}, sticky CPU fallback for uuid {:x}", e, uuid);
            GPU_FALLEN
                .entry(uuid)
                .or_insert_with(|| AtomicBool::new(false))
                .store(true, Ordering::Relaxed);
            run_cpu_render(in_data);
            1
        }
    }
}

// SEQUENCE_SETDOWN 経由で呼び出し:
#[no_mangle]
pub extern "C" fn smooth_core_sequence_setdown(uuid_hi: u64, uuid_lo: u64) {
    let uuid = ((uuid_hi as u128) << 64) | (uuid_lo as u128);
    GPU_FALLEN.remove(&uuid);
}
```

**C++ 側 SMART_RENDER_GPU の骨子**:

```cpp
// render 時の sequence_data アクセス(read-only、suite 経由)
// 実 API は AE_GeneralPlug.h L5713-L5718: PF_EffectSequenceDataSuite1 は
//   PF_GetConstSequenceData(PF_ProgPtr, PF_ConstHandle*) ただ 1 メソッド。
//   PF_ConstHandle = const PF_ConstPtr* (= const (const void*)*) なので
//   受け取ったら 1 段 dereference して void* を取り出す。checkout/checkin ペアは
//   存在しないので注意(round 2 骨子に CheckoutSequenceData/CheckinSequenceData と
//   書いていたが SDK には無く、round 3 で訂正)。
AEFX_SuiteScoper<PF_EffectSequenceDataSuite1> seqSuite(
    in_data, kPFEffectSequenceDataSuite, kPFEffectSequenceDataSuiteVersion1,
    "Couldn't load sequence data suite");
PF_ConstHandle seq_handle = nullptr;
ERR(seqSuite->PF_GetConstSequenceData(in_data->effect_ref, &seq_handle));
const SmoothSequenceData* seq =
    reinterpret_cast<const SmoothSequenceData*>(*seq_handle);  // 1 段 deref
uint64_t uuid_hi = seq->instance_uuid_hi;
uint64_t uuid_lo = seq->instance_uuid_lo;
// 解放は不要(handle の lifetime は AE 側が管理)

// Rust 側に委譲
int fallen = smooth_core_try_gpu_render(in_data, uuid_hi, uuid_lo, /* GPU args */);
// fallen == 1 なら Rust が CPU fallback 実行済み

return PF_Err_NONE;  // 常に成功返却、AE にフレーム失敗と認識させない
```

**保存 / 永続化の扱い**(Medium 1 解消 + round 3 duplicate 対応):
- `instance_uuid` は **SETUP / RESETUP 区間のみの in-memory 識別子**として扱う。SEQUENCE_FLATTEN でバイト列に入ってディスクにも保存されるが、次の SEQUENCE_RESETUP 時に**必ず再生成して上書き**するので実質的に永続化されない
- `gpu_fallen` flag は **DashMap(in-memory)のみ**、flattened data には含めない → プロセス再起動で完全クリア → user がプロジェクト再オープンすれば自動的に GPU retry される
- duplicate(copy & paste / alt-drag): SDK は FLATTEN → 新 instance に RESETUP を発行。RESETUP で新 UUID が入るので複製元と複製先は独立な `GPU_FALLEN` key を持ち、互いの sticky 状態を踏まないし `SEQUENCE_SETDOWN` での remove も干渉しない
- SEQUENCE_FLATTEN で「flat struct をそのままコピー」するのは `SmoothSequenceData`(UUID のみの軽い構造体)であって、fallen 状態ではない

**user からのリセット方法**: 明示的リセット UI は v1.0 に入れない。AE を再起動 or プロジェクト再オープンで自動的にリセット。将来「Reset GPU state」ボタンの検討余地はあるが v1.0 scope 外。

### 6.6 Metal shader の embed 方式確定

SDK サンプル: runtime source コンパイル(MSL 文字列を C 配列として header に埋め込み、`newLibraryWithSource` で実行時コンパイル)。

**smooth で採用**: 同パターン、ただし Rust crate 側で:

```rust
// build.rs で smooth.metal → smooth.metallib をプリコンパイル(xcrun metal + xcrun metallib)
// lib.rs で include_bytes!("../target/gpu/smooth.metallib") → MTLLibrary に newLibraryWithData
// 利点: startup 早い、ユーザー環境に metal toolchain 不要
// 難点: build time に Mac 上で xcrun が動く必要(既存 Xcode ビルド環境に含まれるので問題なし)
```

runtime コンパイル (`newLibraryWithSource`) は開発中の rapid iteration に便利なので、feature flag で切替可能にする(v1.0 は precompile、debug build で runtime compile 切替可)。

### 6.7 Item 6 の結論

- SDK_Invert_ProcAmp.cpp が **smooth Phase 2-A.3 実装の雛形として 80% そのまま流用可能**
- `PF_Cmd_SMART_RENDER_GPU` は **flag ではなく distinct selector**、dispatch は `EffectMain` の switch に追加する形
- PreRender で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を立てる → AE が GPU device あれば SMART_RENDER_GPU を呼ぶ、なければ SMART_RENDER を呼ぶ
- Metal: `newLibraryWithSource` or `newLibraryWithData`(precompile `.metallib`)、CommandBuffer は commit のみで `waitUntilCompleted` 不要
- CUDA: **build-time NVCC static link**(サンプル準拠)、Rust から extern "C" で呼ぶ。context push/pop は SDK サンプルが省略しているためまずは同様に省略、spike で MFR 並列 thread での context current 状態を実機確認し必要なら `cuCtxPushCurrent`/`cuCtxPopCurrent` 追加
- AE の自動 CPU fallback は存在しないので、**once-fallen-always-fall は plugin 独自実装**(sequence_data は UUID のみ格納 + plugin-global `DashMap<UUID, AtomicBool>` で fallen flag、§4.6 / §6.5 参照)
- GPU error 時は内部 catch → CPU path 実行 → `PF_Err_NONE` を AE に返す

### 6.8 まだ残る unknowns(Phase 2-A.3 実装 spike で最終確認)

- AllocateDeviceMemory OOM 時の AE 挙動(エラーを上位に伝播 or そのフレームだけ AE 側で handling)
- MFR 並列度を plugin から減らす API の有無(VRAM 圧迫時の自主制御手段)
- sequence_data を Phase 2-B で実質未使用にしてきた流れからの復活コスト(`SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` と SEQUENCE_SETUP/FLATTEN/RESETUP 全部実装が必要)
- Metal の Managed vs Private storage mode の選択(smooth の 2-pass で中間 buffer は GPU-only なので Private で OK か)

---

(Phase 2-A 設計 RFC まとめは次セクションで)
