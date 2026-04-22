# Phase 2-A: GPU 対応 調査ノート

開始日: 2026-04-23
対象: smooth プラグインの GPU レンダリング対応(Phase 2-B MFR 済み v1.5.1 の上に載せる形)

## スコープ確定

### プラットフォーム順序
**(a) Mac (Metal) 先行、調査次第で (b) クロスプラットフォーム抽象化も残す**

理由: 一気に両プラットフォーム対応すると変数が多くなり debug 困難になるため、Mac Metal で "動く・安定・CPU fallback あり" を実証してから Windows に展開する。ただし Rust 側のバインディング選定で wgpu のような抽象化層を選ぶと、結果的に両対応が低コストになる可能性があるため、その判断は Item 3 の結果次第。

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
| 1 | AE SDK GPU API 把握 | **完了** |
| 2 | smooth アルゴリズムのプロファイリング | 未着手 |
| 3 | Rust GPU バインディング選択肢の比較 | 未着手 |
| 4 | MFR + GPU の両立要件 | 未着手 |
| 5 | CPU/GPU 切替 UI 設計 | 未着手 |
| 6 | 競合 / 参考実装調査 | 未着手 |

## 実装順序とリリース方針(2026-04-23 確定)

### 実装ステージ分割

| ステージ | 範囲 | 単独リリース? |
|---|---|---|
| Phase 2-A.1 | SmartRender 経路追加(legacy `PF_Cmd_RENDER` 残しつつ `PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` を追加実装) | **しない**(GPU 下準備として) |
| Phase 2-A.2 | 32bpc 対応(アルゴリズムを f32 domain に拡張、**32bpc goldens を新規取得**) | **しない**(GPU 下準備として) |
| Phase 2-A.3 | GPU render 実装(Mac Metal 先行、Win DX12 後追い) | **する**(これが Phase 2-A の出荷物) |

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

**参照**: `references/AfterEffectsSDK_25.6_61_mac/ae25.6_61.64bit.AfterEffectsSDK/Examples/Headers/AE_EffectGPUSuites.h`、`AE_Effect.h`、`Examples/GP/EMP/EMP.cpp`、`AE GPU SDK Build Instructions.pdf`(未読)

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
PF_Cmd_GPU_DEVICE_SETUP    // per-device 初期化(デバイス毎に shader compile 等)
PF_Cmd_GPU_DEVICE_SETDOWN  // per-device 後始末
```

通常 render 経路は legacy `PF_Cmd_RENDER` から SmartRender (`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER`) に移行する必要あり(legacy render は GPU 非対応)。SmartRender 側で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を Pre-render フェーズで立てると、AE が GPU デバイスで Render を呼ぶ。

### 1.5 メモリ管理

GPU メモリは必ず `AllocateDeviceMemory` / `AllocateHostMemory` 経由で確保する。直接 `cuMemAlloc` / `[MTLDevice newBufferWithLength:]` を呼ぶのは NG(AE の VRAM 圧迫監視下で動作させるため)。

`CreateGPUWorld` / `DisposeGPUWorld` で AE の `PF_EffectWorld` 型の GPU 版を作れる。`GetGPUWorldData` で raw device pointer(`MTLBuffer*` 等)を取り出せる。

### 1.6 排他制御(MFR 対応の観点)

```c
AcquireExclusiveDeviceAccess / ReleaseExclusiveDeviceAccess
```

SDK コメントに重要な記述:
> For full GPU plugins (those that use a separate entry point for GPU rendering) **exclusive access is always held**. These calls do not need to be made in that case.

つまり **full GPU plugin パターン**(専用 GPU entry point を持つ形)を採れば、デバイス排他は AE が自動管理してくれる。partial GPU plugin(render 中に一部だけ GPU 呼ぶ)は自前排他が必要。

**smooth の選択**: Full GPU plugin パターンで進める(コード分岐がクリーンになる、MFR と組み合わせた時の排他ロジックが AE 任せで済む)。

### 1.7 Full GPU plugin の参考実装: `Examples/GP/EMP/`

AE SDK 同梱の完全 GPU 実装サンプル(EMP = Example Metal Plugin か)。共有 `EMP.cpp` / `EMP.h` / `EMP_PiPL.r` + `Mac/`、`Win/` のプラットフォーム別プロジェクト構造 — これは smooth と同じ構造で、**そのまま流用可能な雛形**。Item 2 以降で EMP の構造を詳細に読み込む。

### 1.8 smooth への含意(Item 1 の結論)

- **やるべきこと**:
  - SmartRender 経路に移行(Legacy `PF_Cmd_RENDER` 残しつつ、`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER` を新規追加)
  - `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を追加(MFR と併用可)
  - `PF_Cmd_GPU_DEVICE_SETUP` / `SETDOWN` に shader compile + per-device リソースキャッシュの初期化
  - Full GPU plugin パターン(ユーザーの GPU/CPU 切替 checkbox の有無に関わらず、GPU 経路が走る時は必ず full で)

- **検討が必要**:
  - **32bpc 対応**: GPU render は f32 必須。現 smooth は 8/16bpc 対応のみ、32bpc 用のテストデータ・golden が無い
    - オプション A: GPU path 専用で 8/16bpc → f32 → GPU → 8/16bpc 変換(追加 I/O コスト発生、省略可能性あり要検証)
    - オプション B: 本来の 32bpc 対応をエフェクト全体に追加(smooth のアルゴリズムが 32bpc range でも意味を保つか要検証。スムージング閾値等は輝度スケール固定値ではないか?)
    - オプション C: GPU path は f32 のみで、8/16bpc のプロジェクトは CPU fallback(ユーザーが「GPU 欲しい時は 32bpc プロジェクトで」する)
  - **shader 言語**:
    - Metal: MSL (Metal Shading Language)
    - DX12: HLSL
    - OpenCL: C ベース
    - CUDA: C++ベース
    - クロスプラットフォームなら WGSL (wgpu) or SPIR-V 経由

- **CPU fallback 戦略**: Pre-render で `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を立てるか否かで AE が CPU/GPU 経路を選ぶ。GPU 初期化に失敗した時や user が opt-out した時はこのフラグを立てなければ自動で legacy render 経路に落ちる。

### 1.9 追加調査が必要な項目(PDF 未読 + 実装時に確認)

- `AE GPU SDK Build Instructions.pdf` の内容(特に Mac / Win ビルド手順、shader の embed 方法)
- `Examples/GP/EMP/` の具体的な Metal shader 例(Apple の .metal ファイルをどう bundle するか)
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
- `range` パラメータ → u32 から f32 に意味変更(現行 `0..max=255*4` を `0.0..4.0` にスケール)

**アルゴリズム上の注意点**:
- `range` が現行 UI で integer slider(おそらく 0-4080 程度)→ 32bpc では **絶対値閾値** が全く違うスケール
  - 既存プロジェクトの互換性: integer → f32 への変換テーブルが必要
  - AE の PF_ADD_PARAM UI で bpc に応じて slider スケール変えるか、内部 normalize する
- `line_weight` は元から f32(0.5 等の normalized 値)なので影響なし
- `count_length` 系は閾値比較の結果(true/false)しか使わないので、閾値側さえ 32bpc 対応すれば残りはそのまま動く

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
- **32bpc 対応は Phase 2-A.2 で先行実施**、アルゴリズム本体は閾値 scale 変更と整数→f32 置換で対応可能。`range` の UI scale は user facing change になる可能性あり、要別途検討

### 2.6 追加で深掘りが必要な項目(実装 phase で確認)

- `range` パラメータの現行 UI 値域と、32bpc 拡張時の互換ルール(既存プロジェクト読み込み時の自動変換 or UI 変更なしで f32 正規化を内部で吸収)
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

**採用案**: `cudarc`。CUDA Driver API を直接叩けるため「AE から渡された `CUcontext` を `cuCtxPushCurrent` してから kernel launch」という本件で必要なパターンが素直に書ける。

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

**採用案**: 全 backend で **build-time compile + embed**。`rust/smooth_core/build.rs` に compile step を追加:
- Mac: `xcrun metal -c smooth.metal -o smooth.air && xcrun metallib smooth.air -o smooth.metallib`
- Win DX12: DXC をビルド時に呼ぶ(WSL / vcpkg / LLVM DXC)、もしくは HLSL source を embed して runtime で `IDxcCompiler` → DXIL
- Win CUDA: NVCC または NVRTC で PTX 生成し embed

DXC のビルド時呼び出しは Windows toolchain 要件が増えるので、**最初は HLSL source を runtime compile** するオプションも残しつつ、v1.0 出荷時に embed に寄せる路線。

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

- DXC を Windows CI に乗せるか、最初は runtime compile で出荷するか(v1.0 判断)
- Metal shader の `.metallib` vs `.air`(同じ metal toolchain が出す中間形式、どちらを embed するのが AE 2025 互換で最適か)
- `metal-rs` / `windows` / `cudarc` の **具体的バージョン固定**(`Cargo.toml` で後日レビュー)
- AE の `PF_Cmd_GPU_DEVICE_SETUP` での shader ロードが全 framework で成功することの確認(これは Item 4/6 の実装 spike で検証)

---

---

## Item 4: MFR + GPU 両立要件(Metal + CUDA)

Phase 2-B (v1.5.1) で確立した MFR 契約に、GPU 経路を**破壊せずに**載せる要件を確定する。対象 framework は Metal (Mac) + CUDA (Win)、DX12 は scope 外。

### 4.1 Full GPU Plugin における AE の責務契約

[Item 1 §1.6](#16-排他制御mfr-対応の観点) で確認した通り、SDK はこう明記している:

> For full GPU plugins (those that use a separate entry point for GPU rendering) **exclusive access is always held**. These calls [AcquireExclusiveDeviceAccess/Release] do not need to be made in that case.

**解釈**: AE は GPU 経路の `PF_Cmd_SMART_RENDER`(GPU 指定)**1 呼び出しの間、その device への排他アクセスを保証する**。プラグインは複数の MFR 並列 call を受け取るが、**個々の call の中では device resource に対する他の call との競合を気にしなくて良い**。

**重要な追加条件**(SDK に明記されてはいないが実装時に確認):
- AE は MFR で異なる frame を並列 render するとき、**同じ plugin の同じ device に対して同時に SMART_RENDER を呼ぶか?**
  - Adobe のドキュメントと既存 GPU plugin 実装(Lumetri 等)から推測: **call 自体は並列許可、ただし device に投入する GPU work は AE のキューイング層でシリアライズされる** 可能性が高い
  - → 実装 spike(Item 6 の EMP.cpp 詳細読み込み or 初期 PoC)で確定要

### 4.2 GPU resource のライフタイム分類

smooth が保持する GPU-side state を、**ライフタイム粒度別に分類**して設計に落とす:

| リソース | ライフタイム | 保持場所 | 共有スコープ |
|---|---|---|---|
| **compiled shader pipeline**(`MTLComputePipelineState` / `CUfunction`) | **per-device**、plugin load 中継続 | `PF_Cmd_GPU_DEVICE_SETUP` で作成、`PF_Cmd_GPU_DEVICE_SETDOWN` で解放 | 同 device 上の全 MFR thread 間で共有(read-only state、競合なし) |
| **shader binary** (`.metallib` blob / PTX blob) | **plugin 全体で static**、build-time embed | Rust `include_bytes!` | 全 device 全 thread |
| **transient input/output buffers**(frame 1 枚分の input image / output image / intermediate) | **per-render-call**、frame 内で allocate/free | `PF_Cmd_SMART_RENDER` 内で `AllocateDeviceMemory`、return 前に `FreeDeviceMemory` | 呼び出し thread のみ(AE の MFR は frame 単位で thread を分ける) |
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

**MFR + Metal 実装パターン**:
- pipeline state は per-device 作成、全 MFR thread で共有 read-only 参照
- 各 MFR render call で、その thread が `[device newCommandQueue]` しないで **pre-created shared queue** を使う(queue は per-device で 1 個で足りる、queue は thread-safe)
- 各 MFR render call で `[queue commandBuffer]` → encoder → dispatch → commit → waitUntilCompleted、この command buffer だけ per-call (自動的に per-thread 所有)

### 4.4 CUDA 側の thread-safety

CUDA Driver API の契約:

| オブジェクト | thread-safety |
|---|---|
| `CUcontext` | ✗ **per-thread current context** 制、使用前に `cuCtxPushCurrent` 必須 |
| `CUstream` | thread-safe、複数 thread から同時 launch 可(stream 内の work は順序保証される) |
| `CUmodule` / `CUfunction` | read-only、thread-safe |
| `CUdeviceptr`(device memory) | 同じ stream 上で操作する限り安全 |

**MFR + CUDA 実装パターン**:
- AE が `PF_Cmd_GPU_DEVICE_SETUP` で `CUcontext` を渡してくる(`PF_GPUDeviceInfo::contextPV`)
- plugin は module load (PTX → `CUmodule` → `CUfunction`) を SETUP 時に一度だけ実行、結果を per-device state に保存
- **MFR render call の入口で必ず `cuCtxPushCurrent(ae_ctx)` → 退出前に `cuCtxPopCurrent`**(忘れると別 thread の CUDA 呼び出しが別 context で実行される)
- AE 提供の `CUstream` は 1 本だけ。per-frame に stream を独立させる必要があれば plugin 側で `cuStreamCreate`(ただしフレーム毎にやるとオーバーヘッド、基本は AE の stream でシリアライズで十分)

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

### 4.6 Fallback policy: once-fallen-always-fall(session レベル)

メモリ [project_mfr_timing](../.claude/projects/-Users-hiroshi-Documents-GitHub-smooth/memory/project_mfr_timing.md) で確定した原則を具体化:

**原則**: 1 回でも GPU render 失敗(OOM、shader error、driver timeout 等)が発生したら、**そのレンダーセッション全体を CPU 経路に固定**する。CPU/GPU 混在 render を避ける理由:
- バッチ書き出し中に急に GPU が落ちて CPU に切り替わると、fallback 周辺フレームの bit-identical 性が担保できない(boundary residual が切り替わり点で出る)
- user が結果を見た時に「一部のフレームだけ色が違う」を招く

**実装**:

```rust
// per-plugin static atomic、render session 開始時にリセットする仕組みが必要
static GPU_SESSION_FALLEN: AtomicBool = AtomicBool::new(false);

// 各 render call で:
if GPU_SESSION_FALLEN.load(Ordering::Relaxed) {
    // CPU 経路直行
} else {
    match try_gpu_render() {
        Ok(_) => {},
        Err(_) => {
            GPU_SESSION_FALLEN.store(true, Ordering::Relaxed);
            cpu_render();
        }
    }
}
```

**open question**: AE の「render session」の境界をどう検出するか。
- 候補 A: `PF_Cmd_SEQUENCE_SETUP` / `SEQUENCE_RESETUP` でリセット(sequence 単位)
- 候補 B: `PF_Cmd_GPU_DEVICE_SETUP` 時にリセット(device 切替時)
- 候補 C: `BEGIN_RENDER` 系の selector が MFR バッチに対して呼ばれるか要調査(SmartRender では PreRender 前段が近い)

実装 spike で挙動確認が必要(Item 6)。

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

Phase 2-B までの smooth は legacy `PF_Cmd_RENDER` のみ実装。Phase 2-A.1 で SmartRender 二本化:

```
PF_Cmd_SMART_PRE_RENDER   // 入力要求を AE に伝える(下記フラグを立てる)
  |-- PF_RenderOutputFlag_GPU_RENDER_POSSIBLE  // GPU で走れる場合は true
  |-- max_result_rect / input_rect 計算(preprocess の bbox 予測は困難なので大きめ)
  
PF_Cmd_SMART_RENDER
  |-- GPU device が渡されている?
  |   yes -> GPU path (Metal / CUDA)
  |   no  -> CPU path (既存の process() call)
```

legacy `PF_Cmd_RENDER` は残す(古い AE / scriptable rendering 経路で呼ばれる可能性)。
`SUPPORTS_SMART_RENDER` flag は `AE_Effect_Global_OutFlags_2` に追加、MFR flag と共存する。

### 4.9 Item 4 の結論

- Full GPU plugin パターン採用で device 排他は AE 任せ、実装は per-device pipeline + per-call buffer のシンプル構造
- Metal: command queue 共有、command buffer は per-call で thread-safe 自然に達成
- CUDA: `cuCtxPushCurrent/PopCurrent` を render entry/exit で挟む、stream は AE 提供のものを使う
- VRAM 予算: 4K 16bpc 2 pass で ~400 MB / frame、16 frames in flight で 6.4 GB の OOM リスクあり → 同時実行数ソフト制限 + 超過時 fallback
- Fallback: once-fallen-always-fall、AtomicBool で session 単位。session 境界検出は実装 spike で確定要
- SmartRender 二本化 (Phase 2-A.1) が GPU 対応の前提工事

### 4.10 Item 4 で積み残した検討事項(実装 phase で確認)

- AE が MFR で同一 plugin・同一 device に**同時に SMART_RENDER を呼ぶか**(Adobe 公式 sample の実装パターンで推定、Item 6 の EMP.cpp 読み込み時に確認)
- once-fallen-always-fall の session 境界: `PF_Cmd_SEQUENCE_SETUP` / `BEGIN_RENDER` のどれで atomic をリセットするか
- AE の RenderThreadExecutor 並列度を plugin が query する API の有無(SDK grep 要)
- VRAM 不足時に AE 側でハンドリングしてくれるか(`AllocateDeviceMemory` 失敗時の AE の挙動が "このフレームだけ CPU" になるか "プラグイン全体を止める" になるか)

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

**現行(v1.5.1)**:
```
Effect Controls > smooth:
├── white option     [☐ transparent]
├── range            [slider 0.0 ... 1.0 → 内部 u32 スケール]
├── line weight      [slider 0.0 ... 1.0]
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

#### 検出ソース: AE の `PF_GPUDeviceSuite1::GetDeviceCount`

OS API 直接呼び出し(`MTLCreateSystemDefaultDevice` / `cuInit`)ではなく、AE 経由で検出する。理由:
- AE の project-level GPU 設定(`Software Only` 等)が込みで反映される
- driver 不良・AE から見えない GPU のケースを正しく除外できる
- 複数 GPU 環境で「AE が実際に使える」ものだけをカウントできる

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

現行の About(Build キャプションクリックで開く):
```
smooth, v1.5.0
rust_core 0.1.0+<sha> ffi=0x00020003
```

v1.6.0 での案:
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

Phase 2-B で sequence_data は未使用のまま `SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` だけ立てていた(NULL を受けて満足させる形)。

Phase 2-A.3 で GPU-side state(compiled pipeline 等)を持つ場合、**per-device state をどこに保存するか**:
- 候補 A: plugin-global static(`std::sync::OnceLock` 等、プロセス全体で 1 組)
- 候補 B: **sequence_data**(エフェクトインスタンス単位、AE が lifecycle 管理)
- 候補 C: GPU device index をキーにした plugin-level HashMap(ユニーク、thread-safe 実装が必要)

**推奨**: **候補 C**。理由:
- 同一プロセス内で複数の smooth インスタンスが動く時、compile 済み pipeline を device ごとに 1 組で共有したい(メモリ節約)
- sequence_data は per-instance なので、instance が 100 個あると pipeline も 100 組になる(ムダ)
- plugin-global + HashMap (device index → Pipeline) が妥当。`dashmap` crate か `std::sync::RwLock<HashMap>` で実装

### 5.9 Item 5 の結論

- UI に `GPU Acceleration` **checkbox 1 個のみ**追加、デフォルト ON
- 意味: ☑ = Auto(GPU 試す、失敗で CPU)、☐ = CPU 固定
- **GPU 非対応システムでは checkbox をグレイアウト**(`PF_ParamFlag_DISABLED` を param 登録時に静的適用)
  - 検出は AE の `PF_GPUDeviceSuite1::GetDeviceCount` 経由(OS API 直叩きより信頼度高)
  - Mac でも機構を入れる(将来の OS 要件変更への保険、コード対称性)
- UI 追加タイミングは Phase 2-A.3 本体と同時(fallback release 時は UI 不変)
- About ダイアログに GPU 状態を追加表示(Metal/CUDA/disabled/fallen-to-CPU)
- GPU-side per-device state は plugin-global の `HashMap<device_index, Pipeline>` 方式(sequence_data 使わず)
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
| CUDA context management(push/pop 要?) | **サンプルは push/pop していない**。AE が entry 前に context を current にセット、plugin は Runtime API 感覚で kernel launch のみ書ける |
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

**AE が entry 前に current context を pushしてくれている**ので、plugin は Runtime API 感覚で書いて OK。

### 6.3 SDK 実装と smooth 設計の差分整理

SDK サンプルをベースに smooth 側で追加/変更する項目:

| 項目 | SDK サンプル | smooth で必要な対応 |
|---|---|---|
| CPU アルゴリズム | `FilterImage8` / `FilterImage16` / `FilterImage32` の 3 本(pixel-independent なので画素単位 callback で OK) | smooth の process_row_range は pixel-independent ではない(隣接依存)→ `extraP->cb->iterate` 系ではなく自前 loop |
| 32bpc 対応 | サンプルは 32bpc native 対応済み | Phase 2-A.2 で CPU 側を 32bpc 拡張(Item 2 §2.3) |
| GPU shader 複雑度 | 単純な per-pixel lookup | smooth は 2-pass(検出 → blending)で中間 buffer 必須、shader は長くなる |
| sequence_data | 未使用(NULL) | Phase 2-B 継続で未使用、GPU state は plugin-global HashMap(Item 5 §5.8) |
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

### 6.5 once-fallen-always-fall の sequence_data 実装案

SDK サンプルは fallback を持たないので、ここは smooth 独自設計。

**State の保存先**: sequence_data(per-effect-instance)。

```cpp
// sequence_data 復活(現在 smooth は SUPPORTS_GET_FLATTENED_SEQUENCE_DATA だけ立てて NULL 返し)
struct SmoothSequenceData {
    bool gpu_fallen;        // once-fallen フラグ
    uint32_t version;       // 将来 schema 変更検知用
};

// SEQUENCE_SETUP: 初期化
// SEQUENCE_FLATTEN: 保存時にそのままコピー(sequence_data が flat struct なので)
// GET_FLATTENED_SEQUENCE_DATA: 保存時
// SEQUENCE_RESETUP: プロジェクト load 時 or duplicated 時、flattened data を復元
```

**Rust 側**:
```rust
// SMART_RENDER_GPU 内部で:
let seq: &mut SmoothSequenceData = get_sequence_data(in_data);

if seq.gpu_fallen {
    // CPU path 直行、return PF_Err_NONE
    return run_cpu_render(...);
}

match run_gpu_render(...) {
    Ok(()) => {},
    Err(e) => {
        log::warn!("GPU render failed: {:?}, falling to CPU for rest of session", e);
        seq.gpu_fallen = true;  // sticky
        run_cpu_render(...);    // このフレームも CPU で埋める
    }
}
// 常に PF_Err_NONE を return(AE にフレーム失敗と誤認させない)
```

**Session 境界**: `gpu_fallen` は sequence 単位、つまり「このエフェクトインスタンスが存在している間」継続。
- プロジェクト close → sequence 終了 → next open で `gpu_fallen = false` にリセット(SEQUENCE_SETUP で新規作成、もしくは SEQUENCE_RESETUP で flattened data を復元しない実装にする)

**user からのリセット方法**: 明示的リセット UI は v1.0 に入れない(project 再オープンで対応)。将来「Reset GPU state」ボタンを追加するオプション。

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
- CUDA: **build-time NVCC static link**(サンプル準拠)、Rust から extern "C" で呼ぶ。context push/pop は不要(AE が managing)
- AE の自動 CPU fallback は存在しないので、**once-fallen-always-fall は sequence_data で plugin 独自実装**
- GPU error 時は内部 catch → CPU path 実行 → `PF_Err_NONE` を AE に返す

### 6.8 まだ残る unknowns(Phase 2-A.3 実装 spike で最終確認)

- AllocateDeviceMemory OOM 時の AE 挙動(エラーを上位に伝播 or そのフレームだけ AE 側で handling)
- MFR 並列度を plugin から減らす API の有無(VRAM 圧迫時の自主制御手段)
- sequence_data を Phase 2-B で実質未使用にしてきた流れからの復活コスト(`SUPPORTS_GET_FLATTENED_SEQUENCE_DATA` と SEQUENCE_SETUP/FLATTEN/RESETUP 全部実装が必要)
- Metal の Managed vs Private storage mode の選択(smooth の 2-pass で中間 buffer は GPU-only なので Private で OK か)

---

(Phase 2-A 設計 RFC まとめは次セクションで)
