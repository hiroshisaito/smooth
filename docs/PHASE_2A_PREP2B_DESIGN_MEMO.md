# Phase 2-A.3 GPU port — line-level blending design memo

Author: design-review pass for Sub-stage C-2.5b.2-prep2b+ scope freeze
Repo state: commit 794772f, `smooth_combined` MSL kernel handling mode_flg=15 only.

---

## 1. Algorithm complexity baseline

Concrete entity counts the prep2b+ work has to cover (CPU side, post-fast-compare gate at `process.rs:82`):

**Scan helpers (count_length family)** — 9 distinct functions:
- `link8.rs:14` `count_length` (currently unused, kept for signature parity)
- `link8.rs:35` `count_length_two_lines` (used by all four `link8_mode_*` and `link8_square_blend_outside`)
- `up_mode.rs:7,64,137,211` `up_mode_{left,right,top,bottom}_count_length`
- `down_mode.rs:7,61,132,186` `down_mode_{left,right,top,bottom}_count_length`

**Blend helpers** — 8 distinct multi-pixel writers + 1 single-pixel writer:
- `blend.rs:62` `blend_line` (writes `ceil(length * line_weight)` pixels along a ray)
- `link8.rs:65` `blend_outside` and `link8.rs:98` `blend_inside` (the latter into local `temp_pixel` then averaged back, so the GPU-relevant write is only `blend_outside` plus the final averaging loop at `link8.rs:270-294`)
- `up_mode.rs:265,287,313,340` and `down_mode.rs:257,279,307,331` — eight `*_blending` helpers, each writes 1..N pixels along one cardinal ray
- `lack.rs:8,37,66` `lack_mode_{01,02,0304}_execute` — single-pixel writes at the centre, but with a *scan* phase that walks up to 3 pixels in two directions

**Maximum write distance per source pixel**:
- `link8_square_execute` writes the centre pixel + up to 4 outside-blend rays of length up to `MAX_LENGTH=128` each (`link8.rs:7`). So one source pixel at (i, j) can write to pixels as far as (i ± 128, j) or (i, j ± 128).
- `up_mode_*` / `down_mode_*` blending writes at most `(start − end) * line_weight` ≤ `core[k].length` pixels per side, bounded by image edges.
- Empirically the worst-case fan-out per source pixel that hits mode_flg=15 is **~512 writes** (4 × 128 outside + 1 centre); for mode_flg ∈ {7, 11, 13} via `link8_execute` it is ~128 writes (one outside ray each side + an inside-temp fold).

**Cross-mode shared state**:
- `BlendingInfo.core[4]: Cinfo` carries `start`, `end`, `length`, `flg` (`types.rs:60-90`). The `CR_FLG_FILL` bit is set inside the count_length helpers (`up_mode.rs:20,94,168,222`) and consumed by the dispatcher at `process.rs:119-124` and `:132-137` to choose `weight = 0.5` vs `line_weight`. **Each `core[k]` is consumed only by mode_flg=3 and =5**, so it does not cross between mode_flg cases — it is purely intra-pixel scratch, which is good news for GPU port.
- `lack_flg` is *cross-pixel*: set at `process.rs:97-99` when the current pixel has `mode_flg & 1` (rightward edge) and is consumed at `process.rs:70-80` by the *next* pixel in the same row. Pure forward dependency along a row.
- `SECOND_COUNT` flag in `up_mode_*_count_length` recursion (`up_mode.rs:31`, etc.): each count_length helper recursively re-invokes itself on a neighbour with `flag = SECOND_COUNT` to look up an "is the line one shorter, then half-pixel adjust" condition. This is a **bounded one-step recursion**, not a chain — no problem for GPU.

---

## 2. Option (a) — algorithm inversion

**Per-thread spec** (the destination pixel `(x', y')` reads anything that could write it):
- Scan a square of radius `MAX_LENGTH+2 = 130` around `(x', y')` for any pixel `(x, y)` whose mode_flg case writes at `(x', y')`.
- For each such candidate source pixel:
  - Recompute the source's full mode_flg.
  - Recompute the source's `core[]` via `up_mode_*_count_length` / `down_mode_*_count_length` (4 helpers each, each is its own up-to-128-pixel scan with one recursive sub-scan).
  - Recompute the source's blend-loop iteration count and its specific contribution at `(x', y')`.
  - Combine contributions deterministically.

**Read scope per thread**: `(2 × 130 + 1)² ≈ 68k` neighbour reads in the worst case, each of which may itself trigger up to 4 × 128 = 512-pixel scans (the source's count_length). Order **10⁷ reads per output pixel**, ~10¹³ for a 4K frame. This is a non-starter on memory bandwidth alone.

A pruned version (read radius 32, skip count_length recomputation by caching mode_flg in an intermediate) gets back to feasibility but reintroduces option (b)'s intermediate buffers — see §3.

**LOC / sessions**: Even ignoring perf, the inversion logic is *new* code with no CPU counterpart for line-by-line review. Each of the 8 line-blend helpers needs its own inversion. Realistic estimate: **8–14 sessions**, ~1500 LOC of new MSL plus matching Rust dispatch wrappers, plus a substantial new test rig because the CPU code is no longer the line-by-line oracle.

**Specific failure modes**:
- **`lack_flg` propagation across columns** (`process.rs:64-80, 97-99`): The "set in column N affecting column N+1" dependency is intrinsically serial. Inversion has to encode "column N's mode_flg & 1 was set" as a precomputed mask (*another* intermediate buffer, contradicting the prep2a no-intermediates win).
- **`SECOND_COUNT` recursive count_length** (`up_mode.rs:31-46`): each recursion examines a neighbour pixel's count_length result. To invert, thread `(x', y')` may need its source's neighbour's count_length too — a 2-hop scan that is already implicit in the CPU recursion but explicit in the inversion. Bounded but large.
- **Inside-temp averaging in `link8_execute`** (`link8.rs:270-294`): the CPU averages two `temp_pixel[0..2]` arrays computed in one thread; thread (x', y') would need to recompute *both* arrays from the source's perspective and average just the index that lands on (x', y'). Doable but adds another doubling of work per blend write.

**Determinism**: deterministic *if the combine rule is fixed* (e.g., "sum all contributions, divide by count"). However, the CPU's "later writes win" semantics (`process.rs` row-major scan, blend writes overwriting) means **bit-identical CPU↔GPU is unachievable by inversion**. The closest deterministic match is "max-priority source wins, ties broken by (y, x) lexicographic order" — that produces a *different* but consistent image. Visually equivalent? Probably yes for ~99% of pixels; for the 1% on multi-edge intersections, divergence will be visible at ≤2 ULP-magnitude differences but in different *spatial positions* than CPU.

---

## 3. Option (b) — multi-pass with `gpu_suite`-allocated intermediates

**Pass plan** (5 dispatches per frame, plus the existing preprocess+detect):

| # | Kernel | Reads | Writes | Conflicts |
|---|---|---|---|---|
| 1 | preprocess (white-key) | src | src' | none |
| 2 | detect (mode_flg byte) | src' | modes[] | none |
| 3 | mode15 centre (existing `smooth_combined`) | src', modes | dst (own pixel only) | none |
| 4 | mode15 outside-blend × 4 directions | src', modes | dst (line in one direction) | overlapping rays from neighbours |
| 5 | mode {3,5} up/down corner blend | src', modes | dst (4 cardinal rays) | same |
| 6 | mode {7,11,13} link8 outside-blend | src', modes | dst (2 rays + inside temp) | same |
| 7 | mode_flg2==3 突起 + lack_03/04 | src', modes | dst (1 pixel) | minimal |

Per the workbench note (workbench_history.md:1925-1930), these intermediates **must** go through `PF_GPUDeviceSuite1::AllocateDeviceMemory` (commit `c7e164a` regression). Adding 1–2 modes-buffers (`width × height × 1B`) is cheap (≤8 MB at 8000²); adding a full BGRA128 scratch is `width × height × 16B` (≤976 MB at 8000², ≤127 MB at 4K) — exactly the pressure that tripped commit `084b470`. So **the design rule is: scratch must be 1 byte/pixel, never 16**, which is satisfiable for `modes[]` but not for an "accumulator" buffer.

**LOC / sessions**: roughly 1 kernel per existing CPU helper, plus a Rust dispatcher wrapper. **5–7 sessions**, ~700 LOC of MSL + ~200 LOC of Rust. Less new logic than (a) because each kernel can be a near-line-by-line port of a single `*_blending` function.

**Inter-thread write conflicts** (the hard problem):
- Pass 4 (mode15 outside-blend): two source pixels at `(i, j)` and `(i+1, j)` both with mode_flg=15 produce leftward and rightward rays that *overlap* in the strip between them. CPU semantics: the second source's writes overwrite the first's. GPU semantics: undefined unless we serialise.
- Decomposition by direction (e.g., "pass 4a = leftward only, pass 4b = rightward only") still has conflicts: two source pixels in the same row both with leftward rays, separated by < 128, will both write into pixels in between. The CPU "later wins" is `i+1`'s leftward ray overwriting `i`'s leftward ray near `i`.
- **Resolution requires per-direction *priority buffer*** (1 byte/pixel: "what's the lowest source-i index that has written here"). atomicMin over a `uint` or `ushort` gives deterministic CPU-equivalent ordering. This adds 1–2 more 1-byte/pixel intermediates — still within the workbench memory budget.

**Determinism**: With atomic priority buffer, **bit-identical to CPU is achievable** because the "later wins" semantics is reproduced exactly. This is the strongest argument for (b) over (a).

---

## 4. Option (c) — partial GPU implementation

Re-evaluating against Hiroshi's 2026-05-04 hard requirement (no machine-by-machine differences in network rendering, no visible discontinuity at GPU→CPU mid-stream fallback):

**Visual divergence magnitude estimate** at a typical 4K 32bpc frame:
- mode_flg=15 fires only at isolated-pixel-against-different-neighbours configurations — empirically <0.5% of edge pixels.
- mode_flg ∈ {3, 5, 7, 11, 13} fires at **most edge pixels**: a typical 4K vector-graphics frame has ~50k–500k edge pixels, and ~80–95% of those go through these line-level blends.
- Skipping them (current GPU behaviour) leaves the input pixel unsmoothed at every edge. Visible: **yes, glaringly** — this is exactly what the smooth filter is supposed to remove.

So a network render that silently fell back from GPU to CPU mid-Render-Queue would produce frames where the first half has visible jaggies (GPU partial) and the second half is properly smoothed (CPU). **Option (c) violates the hard requirement and is rejected.**

---

## 5. Win CUDA compatibility

- **Option (a)**: translates cleanly. The inversion logic is pure compute, no Metal-specific features. Same SIMT/SIMD divergence concern (count_length scan length is data-dependent, so warp/wavefront divergence is identical on both backends). No fork risk.
- **Option (b)**: also translates. `PF_GPUDeviceSuite1::AllocateDeviceMemory` returns `CUdeviceptr` on Win and `MTLBuffer` on Mac (per `gpu_suite` API spec the workbench cites at workbench_history.md:1928-1930), so the intermediate plumbing is platform-neutral. The atomic-priority pattern uses `atomicMin` (CUDA) ↔ `atomic_min_explicit` (MSL) — both provide 32-bit unsigned atomics with relaxed ordering, semantics compatible. **No fork.**
- **SIMT vs SIMD width difference** (CUDA warp=32, Metal SIMD-group=32 on Apple Silicon, 16 on Intel Mac): only matters for the inner scan loops. Both options have the same warp-divergence pattern (some threads have count_length=2, others=128, lockstep wastes cycles). Optimisation knobs differ but algorithm is shared.
- **Fork risk**: low for both (a) and (b). The risk that *would* force a fork — different memory models — is sidestepped by routing through `PF_GPUDeviceSuite1`.

---

## 6. Recommendation

**Go with option (b): multi-pass + `gpu_suite` intermediates + atomic priority buffer for write-conflict resolution.**

Rationale: (i) it's the only option that achieves bit-identical CPU↔GPU output, which is the strongest possible answer to Hiroshi's multi-machine + fallback-continuity requirement; (ii) per-pass kernels are line-by-line ports of existing CPU helpers, so each kernel has a clear oracle for review; (iii) the memory pressure that killed commit `c7e164a` is avoided by keeping all new intermediates at 1 byte/pixel (modes byte, two 4-byte priority maps), totalling ≤270 MB at 8000² — well below the workbench 4-GB-GPU budget at MFR≤4.

**Concrete next steps (prep2b scope)**:
1. **prep2b.1**: add a per-direction `mode15_outside_priority_v` and `_h` buffer (`uint32`/pixel × 2), allocated through `gpu_suite->AllocateDeviceMemory`. Wire allocation+free into the existing dispatcher next to `modes[]`. Verify on real device that adding 32 MB at 4K does not retrigger the AE warning (this is the one experiment that gates the whole option).
2. **prep2b.2**: implement `smooth_blend_mode15_outside` MSL kernel — port `link8_square_blend_outside` (link8.rs:390-405) directly, using `count_length_two_lines` (already ported), and writing to dst with `atomic_min` on the priority buffer keyed by source linear index `y * width + x`.
   - **2026-05-04 split note**: prep2b.2 was implemented in 2 commits because the wiring + atomic kernel landed at different sessions. **prep2b.2a** (commit `fd2aa05`) added the priority-buffer init kernel + FFI extension + dispatcher 2-pass plumbing (no behaviour change, foundation regression UAT PASS). **prep2b.2b** is the kernel itself per the original §6 specification. After prep2b.2b lands, the pair together fulfils the original prep2b.2 deliverable; subsequent steps (prep2b.3 = link8_01/02/04, etc.) are unchanged from the §6 roadmap below.

**Stop-and-reconsider trigger**: if prep2b.1 (just adding two `uint32`/pixel intermediates) re-triggers the "smooth did not render anything" warning under MFR + 4K 32bpc on real device, then the gpu_suite path is *also* memory-pressure-sensitive, and the only remaining route is option (a) inversion — at which point we accept "visually equivalent, not bit-identical", document the divergence policy, and budget 8–14 sessions instead of 5–7. Specifically: if commit-N's prep2b.1 produces the AE warning on the same 4K 32bpc test that commit `8001aca` failed and `084b470` fixed, switch tracks.

---

## 7. 2026-05-04: option (b) 打ち切り + Path β (option a, 改訂版) pivot

prep2b.2b は 3 連続実機 FAIL で option (b) を打ち切り。外部レビュー受領後、改訂された Path β (per-output writer selection) に pivot 確定。

**FAIL 系譜**:
- commit `ac408f7` (monolithic claim/apply, AllocateDeviceMemory): AE 「smooth did not render anything」+ FrameTask 517、`3cea31b` で revert
- commit `920e80e` (tile-dispatch claim/apply, AllocateDeviceMemory): 同症状再発、`7e4ed29` で revert
- commit `6f3a605` (CreateGPUWorld variant): silent fail で passthrough fallback (GPU では smooth 効果なし)、`fead128` で revert

prep2b.1 自体は通った(allocate + 即 free だけ kernel 未使用)が、prep2b.2b で kernel が atomic を使い始めた途端に FAIL。**§6 stop-and-reconsider trigger は 3 度発動した** と見なす。

**外部レビュー(2026-05-04)で判明した盲点**:
1. command buffer error の async 捕捉漏れ → silent fail bug
2. tile dispatch in single command buffer は driver/AE 視点で 1 cb、watchdog reset しない
3. atomic_min direction は CPU last-writer-wins と逆向き(動いても画ズレリスク)
4. 「AE は multi-pass 非対応」は誤要約。SDK_Invert_ProcAmp が 2 kernel + 1 cb を実装。正確には「smooth の data-dependent atomic chain + 一時 buffer + 非同期完了の組み合わせが AE/Metal の実用 envelope 外」
5. Path β でも bit-identical を完全諦めなくてよい — per-output writer selection で CPU row-major order を再現できる可能性

詳細: memory `feedback_gpu_design_review_lessons.md` + workbench_history.md 2026-05-04 該当節

**Path β v2 設計方針(per-output writer selection、bit-identical 保留)**:

- **核心**: thread = 1 出力 pixel(centre ではない)。各 thread が自分を書きうる候補 centre を限定範囲で gather scan、CPU row-major 順の最終 writer を選び、その writer の blend 値を計算
- **候補空間の刈り込み**: 半径 130 正方形全探索ではなく、**4 cardinal ray + MAX_LENGTH=128 each direction**(line blend が cardinal ray のみの性質を活用、4×128=512 candidates max per pixel)
- **CPU 等価性**: writer key = `(cy * width + cx, block_id, line_position)` の lexicographic max が CPU 順序の最終 writer。block_id 順は inside (0) < block 1 (1) < block 2 (2) < block 3 (3) < block 4 (4) で同 centre 内の overwrite 順を再現
- **早期打ち切り**: ray 上の centre が mode_flg=15 でなければ skip
- **atomic / intermediate buffer なし**: 1-pass dispatch、メモリ問題ゼロ、watchdog 安全
- **silent fail 防止**: command buffer に completed handler を追加して error を Rust→C++ に伝播

**進め方(レビュワー指針)**:
1. **対象を狭く**: まず mode_flg=15 outside だけ per-output 方式で実装(prep2b.2c 相当)
2. **CPU writer-id map 検証**: tiny synthetic fixture で「CPU はどの centre が最後に書いたか」を出す reference を作り、GPU writer-id と比較
3. **bit-identical 諦めは fallback**: writer-id 一致を最初の目標、画素値一致まで届かない場合のみ「視覚的同等」へ後退

**新 roadmap(prep2b.2c〜)**:
- prep2b.2c: per-output kernel for mode_flg=15 outside + writer-id map 検証 fixture
- prep2b.3: 同パターンで mode_flg=15 inside を統合(centre 4-corner avg も per-output 視点で書く)
- prep2b.4〜7: link8_01/02/04 → up_mode_corner → down_mode_corner → lack mode + 突起 mode3 + 32bpc goldens regression
- session 予算: option (b) 打ち切りで失った 3 session の上に Path β v2 で +5〜8 session 想定

**option (b) 復路用の未実施診断**(将来戻る判断が出た場合):
- 診断 A: `waitUntilCompleted` + commandBuffer error logging + free/dispose を completion 後に移す build
- 診断 B: 単純 atomic stress kernel(`count_length` 外す)だけの build
- 診断 C: command buffer を tile/chunk 単位で分割する build

3 つすべて FAIL なら option (b) を確実に打ち切れる根拠になる。今回は 3 連続 UAT FAIL の重みを優先して診断省略で pivot 採用。

---

## 8. 2026-05-05: Path β v2 (prep2c) 連続 FAIL → 判断待ち停止

Path β v2 を 2 variant 実装したが test 3(4400² + 32bpc + GPU ON + transparent ON、19 frames preview)で連続 FAIL。HEAD = `2c85871` で test 3 FAIL のまま判断待ち停止。

### 8.1 FAIL 系譜

**variant v1 — commit `1288bfa`(2 kernel + 2 encoder 構造)**:
- 構造: `smooth_combined`(mode_flg=15 centre 4-corner avg を dst に書く)+ `smooth_blend_mode15_outside_per_output`(outside line blend を dst に書く)を別 compute encoder で sequential dispatch
- 結果: AE 警告「smooth did not render anything」発生。**FrameTask 517 はゼロ**(GPU watchdog 問題は解消、別の理由で AE が dst を不正と判定)
- 仮説: SDK_Invert_ProcAmp.cpp は 1 kernel writes dst または multi-kernel writing **異なる buffer** のパターン。**2 kernel 両方が同じ dst に書き込む**構造が AE の render tracking と相性悪い

**variant v2 — commit `2c85871`(unified `smooth_per_pixel` 1 kernel + 1 encoder + 1 dispatch)**:
- 構造: thread = 1 出力 pixel、Phase 1(Block 2 → Block 1 で LATER outside 探索)/ Phase 2(self mode_flg=15 inside の 4-corner avg)/ Phase 3(Block 3 → Block 4 で EARLIER outside 探索)を 1 kernel に統合、dst には必ず 1 度だけ書き込む(SDK パターン整合)
- 結果: **test 3 FAIL**。AE 警告 + log で `FrameTask threw 517` × 1、12 frames render に 22.7 秒 = ~**1.9 秒/frame**
- 数値根拠: 4400² = 19.4M thread × 4 cardinal × MAX_LENGTH=128 per-pixel scan、各 step ~5 read × 16 byte ≈ **780 GB/frame** の理論メモリ転送量。Apple Silicon ユニファイド帯域 ~400 GB/s でも 1 frame ~2 秒で GPU driver watchdog(~2 秒/dispatch)に断続接触

### 8.2 残存盲点(prep2c でも未解決)

外部レビュー指摘済の以下が prep2c では先行実装できていない:

- 🔴 **silent fail bug**: Rust `dispatch_smooth_chain` は `cb.commit()` 直後に `Ok(())` を返却、command buffer の async error(GPU timeout / fail)を `mark_fallen` に伝播していない。test 3 FAIL の根本診断が困難になる原因の一つ
- 🔴 **memory-bandwidth bound**: 上記 ~780 GB/frame の理論値が watchdog 接触の根本要因。`MAX_LENGTH=128` を kernel 内で削れば watchdog 抜けるが CPU と非 bit-identical になる(Path β v2 の最初の目標 = bit-identical 一致が直接破綻)

### 8.3 選択肢(Hiroshi さん判断待ち)

| 選択肢 | 内容 | bit-identical | 工数感 |
|---|---|---|---|
| A | GPU 用 `SMOOTH_GPU_MAX_LENGTH=16` or `32` を導入(CPU は 128 維持) + cb completed handler で silent fail 解消 | 非 bit-identical(視覚同等狙い、Path β 当初目標から後退) | 1〜2 session |
| B | flat region(全 mode_flg=0 タイル)で early-out するタイル前処理を追加して平均負荷削減 | 維持可能 | 2〜3 session、効果は要実測 |
| C | GPU を mode_flg=15 inside(centre 4-corner avg)のみに rollback、line blend は CPU 経由に戻す部分 GPU 化 | inside のみ bit-identical、line は CPU=CPU 同一 | 1 session、ただし §4「mid-stream fallback continuity」要件との適合は要再評価 |
| D | GPU 経路を v1.6.0 から外す。Phase 2-A は 32bpc CPU only で出荷、GPU は v1.7.0+ に後回し | n/a(GPU 経路削除) | 1 session(削除と doc 整理) |

A〜C いずれを採用しても **silent fail handler(cb completed handler で error→shared atomic→`mark_fallen` 伝播)は必須**。これは選択肢に依存しない先行実装可能項目。

### 8.4 §4(option (c) = 部分 GPU 化)の再評価

§4 では「mode_flg ∈ {3, 5, 7, 11, 13} は edge pixel の大多数を占めるので、これを GPU で skip すると半分は jaggy / 半分は smooth の混在 frame が出る」として option (c) を却下した。一方、選択肢 C は **mode_flg=15 inside だけ GPU**(line blend = mode 3/5/7/11/13/15 outside 全部 CPU)に rollback する形なので、§4 の「mode_flg=15 inside だけ GPU で他を skip」とは異なる。

Hiroshi さん 2026-05-04 の hard requirement(no machine-by-machine differences in network rendering、no visible discontinuity at mid-stream fallback)は GPU↔CPU 切替が **同一 frame に視覚連続**であることを要求している。選択肢 C は GPU 経路でも「inside だけ GPU + line は CPU」の hybrid 出力を返すため、CPU only 経路と画素差があれば連続性は破綻する。**C 採用前に「inside-only GPU が CPU only 経路と bit-identical か」を tiny fixture で確認するのが前提条件**。

### 8.5 §6 stop-and-reconsider trigger は何度発動したか

- §6 trigger 発動 #1〜#3 = prep2b.2b 3 連続 FAIL(2026-05-04)
- §7 で Path β v2 pivot に切替
- §8 で Path β v2 も 2 連続 FAIL = **trigger に相当する事象を計 5 回観測**
- 根本原因 = Path β v2 の per-pixel scan が memory-bandwidth bound で watchdog に届く / silent fail handler 未実装で診断不能の 2 つが残存

判断待ちの位置: bit-identical を目標に持ち続けるか(B が候補、ただし不確実)、後退して視覚同等(A)/ 部分 GPU(C)/ GPU 撤退(D)へ pivot するかの分岐。

---

## 9. 2026-05-05: 外部レビュー第 2 弾 → Hybrid Path β + 4 段優先順位

prep2c v1/v2 連続 FAIL 後に Hiroshi さん経由で受領した外部レビューで、§8 の選択肢 A 直行は時期尚早と判明。**Hybrid Path β を 1 回試す余地が技術的にはまだある**との指摘。

### 9.1 §8 分析の見落とし(レビュワー指摘)

1. **AE 観測 metric の優先**: `22.7s/12 frames = 1.9s/frame` より、log 中の `Frame render avg time(per thread): 2.75s` のほうが危険信号として強い。AE は GPU 同期待ちを含めた per-thread 時間で `FrameTask 517` を判定している
2. **1-pass 寄せすぎ = 平坦領域でも安くならない**: 候補が見つからないピクセルほど Block 2/1/3/4 の MAX_LENGTH=128 走査を全 4 cardinal でやり切る。早期 break は edge 付近にしか効かない構造
3. **MFR queue 滞留も原因**: 単体 kernel 時間がギリギリでも、MFR で複数 frame が同 queue に積まれると timeout 側に倒れやすい
4. **completed handler は当該 frame を救えない**: error→`mark_fallen` 伝播は次 frame 以降の保護用、失敗した当該 frame 自体は AE 側で 517 化されてから retry / abort に入る

### 9.2 Hybrid Path β 設計

「multi-pass だが atomic / BGRA128 scratch / data-dependent writer chain なし」の安全パターン:

- **prepass 1**: 各 pixel が `mode_flg=15` centre かを 1 byte/pixel metadata に書く(必要に応じて `flg` も)
- **optional prepass 2**: 各 centre の 4 方向 line length / ref variant を小さい metadata buffer に書く
- **final pass**: per-output gather。候補ごとに `compute_centre_flg_15` / `count_length_two_lines` を **再計算しない**(prepass 結果を read)、最終 kernel だけが dst を書く

これは multi-pass だが、過去 FAIL の構造(複数 kernel が同 dst / atomic / BGRA128 scratch / data-dependent writer chain)とは別物。**metadata buffer にだけ書き、最終 kernel だけが dst を書く SDK サンプル相当の安全形式**。`prep2b.2a foundation`(commit `fd2aa05`)で AE-managed small buffer は通っているので、完全 1-pass(prep2c)より現実的。

### 9.3 4 段優先順位(2026-05-05 Hiroshi さん承認、優先 1 着手中)

1. **completed handler + error logging + GPU in-flight 1 診断**(本 step、選択肢非依存):
   - Rust 側 `cb.add_completed_handler` で `cb.status` / `cb.error` を捕捉、`uuid` を渡して `mark_fallen` を直接呼ぶ
   - env var `SMOOTH_GPU_INFLIGHT_LIMIT=1` で in-flight 1 制限(Rust Mutex で同期化、`wait_until_completed` で次 dispatch まで block)。再 build 不要、UAT で flip して 517 が消えるかで「kernel 時間 vs queue 滞留」を切り分け
   - **当該 frame は救えない**(AE に既に Ok 返却済み)、目的は ① 診断情報の確保 ② 次 frame 以降の CPU 逃がし ③ MFR 滞留の切り分け
2. **Hybrid Path β prepass 試作**: `mode15_flg` metadata prepass(1 byte/pixel)→ final per-output gather kernel が prepass 結果を read。最終 kernel だけが dst を書く
3. **それでも重い場合は `GPU_MAX_LENGTH=16/32` cap**(視覚同等狙い、bit-identical 後退)
4. **cap 採用時は GPU だけでなく GPU ON プロファイルの CPU fallback も同 cap**:
   - CPU 通常モード(GPU OFF / 8/16bpc)= MAX_LENGTH=128(従来)
   - GPU ON プロファイル(GPU パス成功)= GPU MAX_LENGTH=16/32
   - GPU ON プロファイル(once-fallen で CPU に戻った)= CPU MAX_LENGTH=16/32(GPU と視覚一致)
   - SequenceData に「GPU プロファイル」flag 追加 →`process_row_range` の MAX_LENGTH 選択に反映

### 9.4 §8 選択肢の再評価

レビュワー判断を反映した位置づけ:
- **A 直行**: ❌ 時期尚早、Hybrid Path β を試さずに視覚同等に後退するのはもったいない
- **B(flat region early-out)**: ⚠ 効果不確実、Hybrid Path β に統合すれば prepass で自然に達成できる
- **C(inside-only GPU)**: ⚠ 安定だが smooth の主効果が line blend なので機能として弱い
- **D(GPU 撤退)**: ⚠ リリース判断としてはありだが、技術的にはまだ Hybrid Path β の余地

**確定方針**: 優先 1(silent fail handler + in-flight 1 診断)→ 優先 2(Hybrid Path β prepass 試作)を順次 → 重ければ優先 3+4(cap、CPU fallback も同 cap)。優先 2 で十分なら 3+4 は不要、優先 2 でも Hybrid Path β が無効なら C / D に再 pivot 判断。

---

## 10. 2026-05-05: 優先 1 UAT 結果 + step1/step2 分割設計

優先 1(silent fail handler + in-flight 1 診断)の UAT を実施し、本 FAIL モードの正体が判明:

### 10.1 優先 1 UAT 結果(commit `8866108`)

**Phase 1(default、MFR 並行)**: test 3 FAIL、`[smooth GPU] command buffer FAILED: status=...` 行は**ゼロ**。per-thread 1.03〜2.08s で 517 散発。
**Phase 2(`SMOOTH_GPU_INFLIGHT_LIMIT=1`、serial GPU)**: test 3 FAIL、handler 行依然ゼロ。per-thread 2.27〜2.90s(serial 化で純 kernel 時間が露出)、517 は同程度発生。

**確定**:
1. **GPU watchdog ではない**(Metal が timeout/error を返さず handler 不発火)
2. **MFR queue 滞留ではない**(in-flight 1 でも 517 が同程度)
3. **AE 側「frame ごとの SmartRender 許容時間」がおおむね 2 秒前後**で、kernel 単体時間が 2 秒を超えると AE が当該 frame を 517 化
4. silent fail handler は**装備として正しいが本 FAIL モードでは発火しない**(GPU error として表面化しないため)。次フレーム保護として残置
5. `wait_until_completed()` は production path から外す(AE 時間予算で不利)、`SMOOTH_GPU_INFLIGHT_LIMIT` は診断完了で unset

memory bandwidth bound(~780 GB/frame ÷ ~400 GB/s ≈ 2s)と整合。**workload 削減でしか解決できない**。

### 10.2 step1 / step2 分割

| Step | スコープ | UAT 焦点 | 出荷判断 |
|---|---|---|---|
| **prep2c-step1** | metadata prepass + cap-aware early-out + env-var cap、**GPU 側のみ** | 517 が消えるか + cap 値の sweet spot 探索 | 出荷判断には使わない(CPU fallback 連続性未保証) |
| **prep2c-step2** | CPU `process.rs` に GPU プロファイル(cap + 有効 mode セット)を共有、GPU checkbox ON 時に CPU 側も準拠 | network render / mid-stream fallback 連続性 | step1 PASS 後に独立 commit |

step1 と step2 を **同 commit に混ぜない**(性能問題の切り分けと出力連続性の保証は別レイヤー、結果が読みづらくなる)。

### 10.3 step1 設計詳細

**metadata kernel(`smooth_detect` 既存を活用)**:
- thread = 1 pixel、5 src read、1 byte/pixel write
- output bit layout: bits 0-3 = mode_flg(4 cardinal compare 結果)、bit 7 = fast_compare match
- `mode_flg=15` 判定 = `(metadata & 0x0F) == 0x0F`
- buffer は `PF_GPUDeviceSuite::AllocateDeviceMemory` 経由で C++ 側が確保(prep2b.2a foundation の通り、AE synchronizer 視野内、`device.new_buffer()` は使わない)

**final per-pixel kernel(`smooth_per_pixel` 改修)**:
1. **cap-range early-out scan**: 4 cardinal で `gpu_max_length` 距離まで metadata を走査、`mode_flg=15` 候補がゼロなら **src 同定 copy で return**。`compute_centre_flg_15` の再計算なし、`count_length_two_lines` 呼び出しもなし → 平坦領域は metadata read のみ(~32 byte read)で完了
2. 候補ありなら従来の Phase 1〜3(Block 2/1/3/4 + self-inside)を `gpu_max_length` 範囲で実行。`count_length_two_lines` は MAX_LENGTH=128 ではなく `gpu_max_length` で bound(必要なら関数 signature に limit パラメータ追加)
3. metadata は self の mode_flg=15 inside 判定にも流用(`compute_centre_flg_15` 不要)

**self flat だけでは copy 不可**(Hiroshi さん指摘、最重要):
- 自分が flat でも、cap 距離内の別 mode_flg=15 centre から line blend で書き込まれる可能性がある
- early-out 条件は「**4 cardinal の cap 範囲内に mode_flg=15 候補がゼロ**」(自分の状態だけでは決まらない)
- 高速化の更なる手段(将来案): prepass で「このピクセルへ書きうる mode15 centre が cap 距離内に存在する」coverage / dilated mask を別 metadata として作り、それが 0 なら O(1) copy

**cap 値の実行時切替**:
- env var `SMOOTH_GPU_MAX_LENGTH=16/32/64`(Rust 側で `dispatch_smooth_chain` が読み取り、kernel に uniform で渡す)
- default = 32(起点)
- env var 未設定なら 32、設定されたら parse、parse 失敗なら 32 fallback
- 再 build なしで cap 切替できるので UAT 反復コスト低減

**MSL kernel signature 拡張**:
```msl
kernel void smooth_per_pixel(
    device const float4* src, device float4* dst,
    device const uchar*  metadata,        // NEW
    constant uint& src_pitch, dst_pitch, width, height, logical_width,
    constant float& range,
    constant uint& white_opt,
    constant float& line_weight,
    constant uint& gpu_max_length,        // NEW
    uint2 gid)
```

**FFI 拡張**:
```c
int32_t smooth_core_metal_dispatch_smooth_chain(
    void *handle,
    void *src_buf, void *dst_buf,
    void *metadata_buf,                   // NEW
    uint32_t src_pitch_pixels, dst_pitch_pixels,
    uint32_t width, height, logical_width,
    float range_f32, uint32_t white_opt, float line_weight,
    uint64_t uuid_lo, uuid_hi);
```

`smooth_core_version` bump: 0x0002_000e → 0x0002_000f。

### 10.4 step1 UAT 焦点(出荷判断ではない)

| 観測 | 期待 |
|---|---|
| Phase 1 初動(`SMOOTH_GPU_MAX_LENGTH` 未設定 → cap=32)| test 3 PASS = AE 警告ゼロ + FrameTask 517 ゼロ + 32bpc + GPU ON で smooth 効果適用(出力は CPU と異なる可能性あり、step1 では未保証) |
| `SMOOTH_GPU_MAX_LENGTH=16`(より aggressive) | per-thread 時間が更に短縮、品質劣化が許容内か視覚確認 |
| `SMOOTH_GPU_MAX_LENGTH=64`(より conservative) | 品質は CPU に近づくが 517 再発リスク |

cap sweet spot が確定したら step2 で CPU fallback の同期実装に進む。

### 10.5 step2 設計の前提(本 commit ではなく後続で実装)

**最重要見落とし(Hiroshi さん 2026-05-05 指摘)**: cap だけ揃えても CPU fallback 連続性は担保できない。GPU が現状 `mode_flg=15` のみ処理 → CPU fallback も `mode_flg=15` のみに制限しないと画ズレ。「**有効 mode セット**」を GPU プロファイルとして cap と一緒に共有する必要がある。

**GPU プロファイル定義**(checkbox 由来、SequenceData 状態 update は使わない):
- GPU checkbox OFF → CPU 完全互換、MAX_LENGTH=128、全 mode 処理
- GPU checkbox ON + 32bpc → GPU プロファイル(GPU パスの可用性に関わらず):
  - GPU パス成功時: GPU が cap + 有効 mode セットで処理
  - GPU unavailable / fallen / backend disabled: CPU が cap + 有効 mode セットで処理(network render の他機 fallback で連続性確保)
- SequenceData は read-only 契約を守る(GPU 成功状態を sequence に書き込まない)

**実装場所**:
- `Params` に `gpu_profile: Option<GpuProfile>` を追加、`GpuProfile { cap: u32, allowed_modes: ModeMask }` 等
- `process_row_range` で `Params.gpu_profile` を見て cap / mode セット適用
- Effect.cpp で SmartRender 経路の cb dispatcher が `info->gpu_acceleration && bpc == 32` から `GpuProfile` を派生、CPU fallback path にも渡す

network render UAT: 同一 .aep を Mac(GPU) と Mac(GPU 強制 fallen) で render → frame-by-frame 視覚比較、連続性確認。

**step2 の出荷ゲート**: 「GPU パス出力 == GPU プロファイル CPU fallback 出力」が tiny synthetic fixture で bit-identical(または near-id 政策内)。これが PASS して初めて Sub-stage C-2.5b.2 close を判断する材料が揃う。
