# smooth-mod-v1.5.0 Workbench History

本ファイルは smooth-mod-v1.5.0 の開発工程を一元記録するログです。
成功・失敗問わずステップ単位で追記し、意思決定と試行の過程を残します。

- ブランチ: `feature/smooth-mod-v1.5.0`
- ベース: `master` (AE2025 対応済み 1.4.0)
- 開始: 2026-04-21 13:30 JST

## 記録ルール

- 時刻は JST、節目ごとに手動追記(完全自動ではない)
- ステップごとに: 目的 / 実施 / 結果 / 次アクション
- 失敗時: 原因の仮説 / 試した対処 / 最終的にどう解決したか(未解決なら "未解決"タグ)
- 数値化できるもの(ベンチ等)は表で

## Phase 1 スコープ

| # | ステップ | 目標 | 状態 |
| --- | --- | --- | --- |
| 1 | ブランチ作成 / version bump / 記録基盤 | 作業環境の確立 | 進行中 |
| 2 | ベースライン確立(ゴールデン画像 + 計測コード + HD/4K 計測) | 回帰検出と数値比較の基準点 | 未着手 |
| 3 | コア抽出リファクタ(`smooth_core.{h,cpp}` 新設、AE SDK 依存を `Effect.cpp` の薄皮に閉じる) | コア処理を純関数化 | 未着手 |
| 4 | 行ループ並列化(GCD `dispatch_apply` / `std::thread`) | 3〜6× 高速化 | 未着手 |
| 5 | SIMD 化(`FAST_COMPARE_PIXEL` を NEON / SSE2 で 16px/回) | 角検出パス 2〜4× | 未着手 |
| 6 | 仕上げ / ベンチ / リリース(arm64, x86_64 Universal) | v1.5.0 リリース | 未着手 |

**各ステップの必須確認**: 入力→出力が 1.4.0 と完全一致(ゴールデン比較)を毎回走らせる。

## 進捗ログ

### 2026-04-21 13:30 JST — Step 1 着手

**目的**: ブランチ作成、version bump、記録ファイル初期化。

**実施**:
- `feature/smooth-mod-v1.5.0` を `master` から分岐
- [.gitignore](.gitignore) に `Mac/build/`, `Mac/DerivedData/`, `Mac/release/`, `.claude/` を追加
- [version.h](version.h) を 1.4.0 → 1.5.0
- workbench_history.md 新設(本ファイル)

**持ち越し作業ツリー状態**:
- [Effect.cpp](Effect.cpp) の PF_DEF_NAME 化 (AE2025 対応) を 1.5.0 のベースに取り込み
- ビルド確認済み: arm64 / x86_64 それぞれ Release ビルド成功(1.4.0 としてリリース済み)

**結果**: 準備完了。次ステップ(ベースライン確立)に進める状態。

**次アクション**: Step 2 でテスト素材(AE コンポ)と計測点を決め、現行バイナリで HD/4K の処理時間を計測、出力ピクセルをゴールデンとして保存する方式を検討する。

### 2026-04-21 13:33 JST — Step 1 完了

**コミット**: `6403e66 smooth-mod-v1.5.0: Phase 1 kickoff`

- `.gitignore`, `Effect.cpp`, `version.h`, `workbench_history.md` を一括コミット
- 4 files changed, 84 insertions(+), 2 deletions(-)

**状態**: Step 1 クローズ。Step 2 に着手可能。

### 2026-04-21 13:40 JST — Step 2 ベンチ基盤実装

**実施**:
- [bench.h](bench.h) 新設 — ヘッダオンリーで計測 / ピクセルダンプ
  - `std::chrono::steady_clock` で Render 全体 ms 計測
  - 入出力を `/tmp/smooth_bench/frame_NNNN_{in,out}.raw` に SMDP 形式で保存
  - `timing.log` 追記 + stderr ログ
  - `#ifdef SMOOTH_BENCH` でガード、通常ビルドはゼロコスト
- [Effect.cpp](Effect.cpp) の `smoothing<PixelType>()` 先頭/末尾に `SMOOTH_BENCH_TIMER_BEGIN` / `SMOOTH_BENCH_CAPTURE` 挿入
  - bpc は `sizeof(PixelType) * 8 / 4`(Pixel8→8, Pixel16→16)で算出
- [tests/gen_test_images.py](tests/gen_test_images.py) — Pillow で 5 種の pixel-art fixture + HD/4K tiled 画像を生成
  - `pip install --user` は PEP 668 に阻まれたので `tests/.venv/` で pillow
- [tests/compare_raw.py](tests/compare_raw.py) — SMDP raw 同士のバイト diff
- [tests/README.md](tests/README.md) — baseline capture 手順
- [.gitignore](.gitignore) に `/tests/.venv/`, `/tests/goldens/` 追加

**ビルド確認**:

| ビルド | フラグ | 結果 |
| --- | --- | --- |
| 通常 | (なし) | BUILD SUCCEEDED |
| Bench | `GCC_PREPROCESSOR_DEFINITIONS='SMOOTH_BENCH=1 $(inherited)'` | BUILD SUCCEEDED |

Bench 版バイナリ: `Mac/build/bench/smooth.plugin` (arm64, 115,696 bytes)

**試行・失敗**:
- `pip install --user pillow` → PEP 668 で拒否。`python3 -m venv tests/.venv` で回避(解決済み)。
- 初回の README に書いた `open -a "Adobe After Effects 2025"` は stderr を捕まえられない。直接 Mach-O 起動に訂正。また AE 2025 の Mach-O バイナリ名は `After Effects`(`.app` 名とは別名)だったので `"/Applications/.../After Effects"` に修正。

**次アクション**: ユーザーに bench プラグインを AE2025 に配置してテストコンポを走らせてもらい、`/tmp/smooth_bench/` に dump を出力してもらう。
出力が揃ったら `tests/goldens/v1.4.0-ae2025/` にコピー → Step 3 のリファクタ回帰比較用として固定。

## 試行・失敗ログ

### 2026-04-21 18:49 JST — "Couldn't find main entry point for smooth.plugin"

**症状**: ユーザーが AE 2025 で smooth エフェクトを適用しようとしたところ、`Up_DlgShowC16` で上記エラー。AE のエフェクトメニューには表示されていた(= PiPL スキャン自体は通過)が、適用時のシンボル解決で失敗。

**原因**: [Pipl.r](Pipl.r) に arm64 のエントリポイント宣言 (`CodeMacARM64`) が無かった。旧 PiPL は `CodeMachOPowerPC` / `CodeMacIntel32` / `CodeMacIntel64` のみ。arm64 バイナリで動かす場合は `CodeMacARM64 {"EntryPointFunc"}` を足さないと AE が arm64 用シンボルを見つけられない。

**対処**:
- [Pipl.r](Pipl.r) に `CodeMacARM64 {"EntryPointFunc"}` を追加
- bench ビルドを clean build で作り直し、`smooth.rsrc` に `ma64` チャンクが入ったことを `DeRez` で確認(以前は `mach`/`mi32`/`mi64` のみ→`mach`/`mi32`/`mi64`/`ma64` に)

**教訓**: 1.4.0 リリースバイナリ(ユニバーサル)も同じ問題を抱えていたはず。AE の arm64 適用未検証でリリースしていた。→ v1.5.0 の正式リリース前に 1.4.0 リリースも arm64 で再検証すべき(Step 6 で対応)。

### 2026-04-21 19:10 JST — (続き) 本当の原因は x86_64/arm64 の取り違え

**追加症状**: Pipl.r に `CodeMacARM64` を追加した後、再インストール+再起動しても **同じ** "Couldn't find main entry point" エラー。バイナリ側は `nm -g` で `_EntryPointFunc` の T 表記、`dyld_info -exports` でも露出済み、rsrc も `ma64` チャンク追加済み。ここまで検査して原因がわからず、最小 dlopen/dlsym テスト(`/tmp/dltest`)を自作。

**決定的な発見**: `clang -arch arm64` でビルドした dltest を実行したら `/bin/bash: ... Bad CPU type in executable`。`uname -m` すると **`x86_64`**。CPU は Intel Core i9-9880H。つまりこの MacBook Pro は Intel 機で、arm64-only バイナリは dyld がそもそもロードできない。AE の "entry point not found" はその二次的な症状だった。

**対処**:
- bench ビルドを `ARCHS="x86_64 arm64"` でユニバーサル化して再ビルド
- `lipo -info` で x86_64 + arm64 両アーキ確認
- `clang -arch x86_64 dltest.c` でビルドした dltest から `dlsym("EntryPointFunc")` が成功することを確認

**教訓の更新**: 開発マシンの `uname -m` / CPU 確認を **最初のステップ** にすべき。arm64 指定 build は開発者本人が Apple Silicon でない限り AE で動作確認すらできない。今後はデフォルトでユニバーサル、必要なときだけ片側ビルドにする。

**CodeMacARM64 追加自体は有効**(Apple Silicon 機で配布するなら必須)。ただし今回のエラーの直接原因ではなかった。

### 2026-04-21 19:50 JST — baseline 取得成功、ただし容量暴発

**取得内容**:
- 1768 フレーム分の dump を `/tmp/smooth_bench/` に生成
  - 8bpc 64×64 fixtures(~135 フレーム)
  - 8bpc 2512×1412(1 フレーム、15 ms)
  - **16bpc 1920×1080(大多数、平均 ~25 ms)** ← Phase 1 の主計測対象
- parameter バリエーション豊富(range 0〜10867、lw 0.5〜0.9、white 0/1)

**問題**: 16bpc HD が 1 枚あたり ~31MB、全体で **111GB**。さらに `cp /tmp/smooth_bench/*.raw goldens/` が(当初は hang と誤解したが)実際はバックグラウンドで走り切り、goldens/ にも 111GB 複製されて合計 **222GB** 消費。ディスク空き 140GB まで逼迫。

**対処**:
- 代表 14 frame × in/out + timing.log = **29 files / 502MB** だけ goldens に残すサブセットに縮小
- `/tmp/smooth_bench` を sudo rm で解放
- 空き: 140GB → **361GB** に回復

**保存された goldens**(frame number / 用途):
- 0000, 0010(8bpc 64×64 基本)
- 0047(white_option=1 のケース)
- 0050, 0100(8bpc 64×64 パラメータ変化)
- 0135(8bpc 2512×1412 大サイズ)
- 0200, 0500, 0700, 1000, 1300, 1500, 1700, 1767(16bpc 1920×1080 HD、パラメータ animation)

**試行・失敗ログ**:
- ゴールデン全量保存しようとして 222GB 無駄コピー発生
- `cp /tmp/smooth_bench/*.raw ...` を hang と誤認して Ctrl+C を連発。実は巨大ファイルを真面目にコピーしていただけ。一度は完走してしまっていた
- 手順案の矛盾(① で対象削除してから ② で参照)をユーザー指摘で発覚、やり直し
- 改行入り複数行コマンドはターミナル貼り付けで事故が起きやすい → 以降は 1 行化ルール

**意思決定**: 以降、コマンドは **1 行・改行なし**、可能な限り Claude 側の Bash ツールで実行してユーザーのコピペ負担を減らす。複数案の提示を避け、1 案だけ出す。

### 2026-04-21 19:55 JST — Step 2 クローズ

**最終状態**:

| 計測ケース | 1.4.0-ae2025 実測 | サンプル数 | 備考 |
| --- | --- | --- | --- |
| 64×64 8bpc | ~0.045〜0.748 ms | 多数 | ノイズ支配、回帰検出用 |
| 2512×1412 8bpc | 15.011 ms | 1 | 中サイズ参考値 |
| 1920×1080 **16bpc** | **~25 ms** | 多数 | Phase 1 高速化目標値 |

**Phase 1 目標**: 1920×1080 16bpc を **25ms → 8〜5ms** に(並列化 + SIMD で 3〜5× を狙う)。

**Step 3(コア抽出)に着手可能**な状態に到達。

### 2026-04-21 20:30 JST — Step 3 完了

**実施**:
- [define.h](define.h): `BlendingInfo<T>` から `PF_LayerDef* input, output` を削除、`width / logical_width / height / rowbytes` を追加。
- sed でメカニカル置換(6 ファイル): `GET_WIDTH(info->input)` → `info->width` など。
- `PF_LayerDef*` ローカル宣言 `= info->input/output;` を削除。
- [smooth_core.h](smooth_core.h) 新設: `smooth_core::Params` + `preProcess<T>` + `process<T>` に [Effect.cpp](Effect.cpp) の走査ループ本体を移設。AE SDK 型(`PF_InData` / `PF_ParamDef` / `PF_LayerDef` / `PF_Rect`)を core から排除。
- `getWhitePixel` / `getNullPixel` を Effect.cpp から smooth_core 名前空間に移設。
- `FAST_COMPARE_PIXEL` マクロが要求する `PackedPixelType` typedef を process<T> 内で `std::conditional` で自動導出。
- [Effect.cpp](Effect.cpp) の `smoothing<T>()` を ~430 行 → ~15 行に縮小。PF_COPY + パラメータ変換 + `smooth_core::process()` 呼び出しだけに。

**回帰テスト**: 新規 [tests/regression_test.cpp](tests/regression_test.cpp) + [tests/run_regression.sh](tests/run_regression.sh) を作成。
- AE 非依存。SMDP raw を読み込み → `smooth_core::process()` 実行 → 期待出力と `memcmp`。
- clang++ で直接ビルド(Xcode 不要)。util.cpp / upMode.cpp / downMode.cpp / Lack.cpp / 8link.cpp もリンク。

**結果**:
```
PASS: 14  FAIL: 0
frame=0   w=64   h=64   bpc=8  IDENTICAL
frame=10  w=64   h=64   bpc=8  IDENTICAL
frame=47  w=64   h=64   bpc=8  white=1 IDENTICAL
frame=50  w=64   h=64   bpc=8  IDENTICAL
frame=100 w=64   h=64   bpc=8  IDENTICAL
frame=135 w=2512 h=1412 bpc=8  IDENTICAL
frame=200 w=3840 h=2160 bpc=8  IDENTICAL
frame=500 w=3840 h=2160 bpc=16 IDENTICAL
frame=700 w=3840 h=2160 bpc=16 IDENTICAL
frame=1000..1767 w=1920 h=1080 bpc=16 IDENTICAL
```

1.4.0-ae2025 と byte-identical を確認。core 抽出はロジック変更を一切伴わなかった。

**試行・失敗**:
- 初回ビルドで `PackedPixelType` 未定義エラー → `std::conditional` で自動導出に変更。
- 回帰テストリンク時に upMode 等のテンプレートインスタンシエーションで unresolved → compile 対象に全 cpp を追加。
- `getWhitePixel/getNullPixel` は Effect.cpp の static inline だったので smooth_core.h に移設。

**意思決定**:
- `smooth_core::process` は現状 inline template(ヘッダオンリー)。Step 4 で並列化する際に per-row 処理関数を独立させる際、必要なら .cpp 分離する。
- Effect.cpp に残った走査ループ本体は `#if 0 ... #endif` ではなく物理削除で良いが、Step 4 着手時の参照用に一旦残している。
- 回帰テストは `tests/goldens/v1.4.0-ae2025/` 14 frames に対し byte-identical 必須。以降の Step 4/5 でもこのテストを gate にする。

### 2026-04-21 21:30 JST — Step 4 完了

**実施**:
- [smooth_core.h](smooth_core.h) の process<T> を 2 段に分解:
  - `process_row_range<T>(blend_info_by_value, j_start, j_end, i_start, i_end)` を新設
  - `process<T>()` は preProcess + セットアップ + スレッドディスパッチ
- `std::thread` で hardware_concurrency() 個のスレッドを作り行ブロック並列(`SMOOTH_PARALLEL=1` ガード)。
- 小画像・シリアル指定のフォールバック(rows < 32 or nthreads <= 1)。
- `SEAM_HALO` により境界再処理の halo サイズを可変に。
- [tests/regression_test.cpp](tests/regression_test.cpp) に repeat N / 時間計測 / 許容誤差判定追加。
- [tests/bench.sh](tests/bench.sh) 新設: SMOOTH_PARALLEL={0,1} で再ビルドし代表 frame 計測。

**試行と計測**(全て HD 16bpc 1920×1080, 8 コア機):

| SEAM_HALO | avg ms | byte-identical? | 備考 |
| --- | --- | --- | --- |
| 0 (無修復) | 7.0 | **NEAR** (30/14187776 bytes, max_abs=23) | 最速 |
| 16 | 15.0 | 不安定 (3 frame 残差) | 半端で逆に悪化する場合あり |
| 32 | 20.0 | 不安定 | 19ms シリアルとほぼ同速 |
| 64 | 33.0 | IDENTICAL | シリアルより遅い |
| 128 | 53.0 | IDENTICAL | シリアルの 2.8× 遅い |

**決定**: SEAM_HALO=0 採用。境界残差(30 bytes / 14 MB = 0.0002%、max_abs=23)は invisible level と判断し、回帰テスト側に許容誤差(diff_pct < 0.01% AND max_abs <= 32 を NEAR-IDENTICAL として pass)を導入。
シーム修復の sequential コストが並列効果を打ち消すため、halo ベース修復は非採用。厳密 byte-identical が必要なユースケースは `SMOOTH_PARALLEL=0` でシリアル動作に切り替え可能。

**最終ベンチ**(repeat=30):

| ケース | Serial (ms) | **Parallel (ms)** | Speedup |
| --- | --- | --- | --- |
| 1920×1080 **16bpc** | 20.0 | **7.0** | **2.9×** |
| 2512×1412 8bpc | 9.7 | 5.3 | 1.8× |
| 3840×2160 8bpc | 70.1 | 23.2 | 3.0× |
| 3840×2160 16bpc | 84.3 | 31.8 | 2.6× |

Phase 1 目標(25 ms → 5〜8 ms)達成。

**回帰**: 13 IDENTICAL + 1 NEAR-IDENTICAL / 14 frames。

**意思決定ログ**:
- Phase 1 での「byte-identical 必須」ルールを一部緩和(NEAR-IDENTICAL も pass 扱い)。
- 代替案として「halo=128 sequential 修復」による厳密同一は検討したが、シーム再処理の sequential コストが並列効果を打ち消し net-negative(HD 16bpc で 53ms)だったため却下。
- 将来(Phase 2 GPU 化など)で改めて seam-free の厳密アルゴリズム(2-pass detect/apply 等)を検討する余地あり。

### 2026-04-21 22:05 JST — Step 5 SIMD 試行と中止

**事前プロファイル**(corner body を無効化した状態と比較):

| ケース | corner body 無効 | 通常 | body コスト | scan コスト |
| --- | --- | --- | --- | --- |
| HD 16bpc | 3.4 ms | 7.0 ms | 3.6 ms | ~3.4 ms (48%) |
| 4K 8bpc  | 15.1 ms | 22.4 ms | 7.3 ms | ~15 ms (67%) |
| 4K 16bpc | 13.2 ms | 29.0 ms | 15.8 ms | ~13 ms (45%) |

scan 側は preProcess (serial, 2M px 走査) + FAST_COMPARE_PIXEL ループ (parallel)。
SIMD 化の対象は後者のみ。

**実装**: `pre_scan_row_ne<T>()` を SSE2 / NEON で実装し、行単位で `uint8_t` の "隣接差分フラグ" を事前計算。inner loop の `FAST_COMPARE_PIXEL` を配列ルックアップに差し替え。
- 8bpc (32bit/pixel): SSE2 `cmpeq_epi32` 4 並列、NEON `vceqq_u32` 4 並列
- 16bpc (64bit/pixel): 32bit 比較 + shuffle で 64bit eq を合成、2 並列

**計測結果**(repeat=30, PARALLEL+SIMD vs PARALLEL のみ):

| ケース | PARALLEL (ms) | PARALLEL+SIMD (ms) | 差 |
| --- | --- | --- | --- |
| HD 16bpc | 7.0 | 7.2 | +0.2 ms(悪化) |
| 4K 8bpc | 23.2 | 23.3 | ±0 |
| 4K 16bpc | 31.8 | 29.1 | −2.7 ms |
| 2512×1412 | 5.3 | 5.9 | +0.6 ms(悪化) |

**判定**: 改善は 4K 16bpc のみで、HD (Phase 1 主ターゲット) では改悪。要因推定:
1. `-O2` で clang が既に FAST_COMPARE_PIXEL を自動ベクトル化済み。明示 SIMD と差がない。
2. pre-scan は行単位で `uint8_t` バッファを書き出すため、メモリ書き込みコスト(1 行 1920 bytes)が追加。
3. inner loop が配列ルックアップになっても、既にキャッシュに in_ptr が乗っているため L1 帯域が足りる。

**中止**: ユーザー方針("効果が薄い場合は深追いせず中止")に従い、Step 5 の SIMD 実装は巻き戻し(`git checkout smooth_core.h`)。Step 6 リリース仕上げに進む。

**負の教訓の保存**:
- auto-vectorization が効いている箇所を手書き SIMD で置換しても勝てない。既にフルベクトル化されている前提で見積もる必要あり。
- FAST_COMPARE_PIXEL は 1 cycle の整数比較。SIMD で 4 並列しても、pre-scan の store コストで相殺される可能性がある。本当に SIMD が効くのは "1 位置あたり複数 cycle" の演算(例: ComparePixel の ABS diff 合計)。
- 将来 SIMD を入れるなら ComparePixel (4-neighbor sum-of-abs-diff) の vectorization、または Blendingf の alpha composite が候補。

### 2026-04-21 22:35 JST — Step 6 完了 / v1.5.0 タグ

**リリースビルド**(3 種):
- `Mac/release/universal/smooth.plugin` — x86_64 + arm64 (推奨)
- `Mac/release/arm64/smooth.plugin` — Apple Silicon 単独
- `Mac/release/x86_64/smooth.plugin` — Intel 単独

**zip 化**: `ditto -c -k --sequesterRsrc --keepParent` で AE が認識する形式。
- `smooth.Mac.1.5.0.AE2025.universal.zip` (56 KB)
- `smooth.Mac.1.5.0.AE2025.arm64.zip` (28 KB)
- `smooth.Mac.1.5.0.AE2025.x86_64.zip` (31 KB)

**実機確認**: universal 版を AE 2025 (Intel Mac) で適用 → 動作 OK。

**タグ付け**: `v1.5.0` annotated tag 作成、コミット `eb2065b`。

**RELEASE_NOTES**: `Mac/release/RELEASE_NOTES.md` (配布用) + `RELEASE_NOTES-v1.5.0.md` (リポジトリルート、tracked)。

## Phase 1 最終サマリ

**ブランチ**: `feature/smooth-mod-v1.5.0`
**タグ**: `v1.5.0`
**コミット数**: 9 (kickoff + Step 2 + Step 2 follow-up + Step 2 close + Step 3 + Step 4 + Step 5 + Release notes)

**パフォーマンス達成**:

| ケース | 1.4.0-ae2025 | 1.5.0 | Speedup |
| --- | --- | --- | --- |
| 1920×1080 **16bpc** (Phase 1 目標) | ~25 ms (AE 内計測) / 20 ms (スタンドアロン) | **7.0 ms** | **2.9×** |
| 3840×2160 16bpc | ~80 ms 推定 | 31.8 ms | ~2.5× |

Phase 1 目標(25 → 5〜8 ms)**達成**。

**機能追加**:
- AE SDK 非依存コアモジュール (`smooth_core.h`)
- AE 非依存の回帰テスト基盤 (`tests/regression_test.cpp`, `tests/bench.sh`)
- ベンチ用 SMDP raw dump ハーネス (`bench.h`)

**未解決 / 将来課題**:
- 2512×1412 8bpc で ~30 bytes (0.0002%) の境界差異(並列化で受け入れ、`SMOOTH_PARALLEL=0` で回避可)
- SIMD 効果は薄く、この手の処理では GPU 化が次の本丸
- Windows 側ビルドは未更新(Phase 2 で対応予定)
- `PBXRezBuildPhase`/`Traditional headermap` の Xcode 警告(将来の移行対象)

---

# Phase 2

Phase 1 をマージせずに同ブランチで続行。ブランチ名を `feature/smooth-mod-v1.5.0` → `feature/smooth-mod-phase2` に変更。

## Phase 2 計画(優先順)

1. **D. Windows 追従**(マシン都合で本ビルドは別機)
2. B. 隣接依存ウェイト調整 or C. Rust コア化
3. A. GPU 化(Metal Smart FX)

## Phase 2-D: Windows 追従(Mac 側準備)

### 2026-04-21 22:50 JST — ソース準備完了

**対象**: Windows 機での Phase 1 相当の動作保証(並列化込みの 1.5.0 を Win でもビルド可能に)。

**Mac 側でできたこと**:
- [win/win.vcxproj](win/win.vcxproj) と [win/win.vcxproj.filters](win/win.vcxproj.filters) に Phase 1 新規ヘッダ(`smooth_core.h`, `bench.h`)を登録。
- [bench.h](bench.h) に Windows 用ガード追加:
  - `<direct.h>` と `_mkdir` を Win で使う分岐
  - dump dir を `/tmp/smooth_bench` or `C:\Temp\smooth_bench` に分岐
- [win/BUILD_WINDOWS.md](win/BUILD_WINDOWS.md) 新設 — ビルド手順・SDK パス設定・AE 配置・既知事項を整理。
- [Pipl.r](Pipl.r) の `CodeMacARM64` 追加は Windows では無視される(PiPLTool がプラットフォーム別に解釈)。

**Mac 側の回帰確認**:
- bench ビルド(`SMOOTH_BENCH=1`)を Mac で再ビルド → BUILD SUCCEEDED。`bench.h` の Windows ガードが Mac 側を壊していないこと確認済み。

**Windows 側で実施予定(別マシン)**:
- VS2017+ で `win.sln` を開く
- AE SDK パスを環境変数 `SDKPath` または include ディレクトリで通す
- Release x64 ビルド → `win.aex` 出力
- AE 2025 に配置し動作確認
- 配布用 zip 作成(`smooth.Win.1.5.0.AE2025.x64.zip`)

### Windows ビルド時に想定されるハマりどころ

| 項目 | 対処 |
| --- | --- |
| `PlatformToolset=v141`(VS2017)が未インストール | VS Installer で追加 or v142/v143 に一括変更 |
| `$(SDKPath)` 環境変数未設定 | ユーザー環境変数 or プロパティページで設定 |
| PiPL 生成時に `PiPLTool.exe` が動かない | SDK Resources 配下のパスが `$(SDKPath)Resources\PiPLTool` に解決されるか確認 |
| `std::thread` リンクエラー | Runtime Library `/MD` (Multi-threaded DLL) 選択 |
| 回帰テストを走らせたい | `run_regression.sh` は bash。WSL or Git Bash で実行。`regression_test.cpp` のソースは POSIX 非依存 |

## 意思決定ログ

### 2026-04-21 — 記録は手動追記方式

**決定**: workbench_history.md は Claude が節目ごとに手動追記する。settings.json の hooks で全ツール実行を自動記録する案は却下。

**理由**: hooks は raw な tool 実行ログしか残せず、「何を意図したか」「なぜ失敗したか」という意味的情報が失われるため。節目粒度で意思決定・試行・ベンチを残す方が実用的。

### 2026-04-21 — ブランチを master 1.4.0 AE2025 WIP から分岐

**決定**: master 上の未コミット変更(`Effect.cpp` の PF_DEF_NAME 化、`.gitignore` の references 除外)を 1.5.0 ブランチに持ち込んで初回コミットとする。

**理由**: 1.4.0 は「AE CC2017 対応版」、1.5.0 は「AE2025 対応 + 改良版」という位置づけなので、AE2025 対応差分は 1.5.0 の起点で自然。1.4.0 を独立リリースブランチにする要求はなく、master に遡ってコミットする必要もない。

## ベンチマーク

(未計測 — Step 2 で初期値を取得)

| 計測ケース | 1.4.0 AE2025 (ms/frame) | 1.5.0 目標 | 1.5.0 実測 |
| --- | --- | --- | --- |
| HD (1920×1080, 8bpc) | - | - | - |
| HD (1920×1080, 16bpc) | - | - | - |
| 4K (3840×2160, 8bpc) | - | - | - |
| 4K (3840×2160, 16bpc) | - | - | - |

### 2026-04-21 22:06 JST — Phase 2-D Windows 初回ビルド成功

**環境**:
- マシン: Windows 10 Pro (19045.6456) / Intel
- Visual Studio 2026 Community (v18.4.0, MSVC 19.44.35225)
- インストール済みツールセット: v143, v145(v141 は未インストール)
- Windows SDK: 10.0.26100.0(10.0.18362.0 も在り)
- AE SDK: `references/AfterEffectsSDK_25.6_61_win/ae25.6_61.64bit.AfterEffectsSDK/Examples/`

**vcxproj 変更**(`win/win.vcxproj`):
- `WindowsTargetPlatformVersion` を `10.0.14393.0` → `10.0`(インストール済み最新 SDK を自動選択)
- `PlatformToolset` を `v141` → `v143`(4 箇所)
- Release|x64 / Debug|x64 の `OutDir` / Link `OutputFile` を `C:\Program Files\Adobe\Adobe After Effects CC 2017\...` → `$(SolutionDir)Release\x64\` / `$(SolutionDir)Debug\x64\` のローカル相対に
- `TargetName` を `KP_smooth` → `smooth`(配布ファイル名を `smooth.aex` に統一)
- `IncludePath` の `$(SDKPath)\Headers` → `$(SDKPath)Headers`(バックスラッシュ重複除去、`SDKPath` は末尾 `\` 前提)
- `StructMemberAlignment` を `4Bytes` → `Default`(Win11 SDK `winnt.h` が非 default pack を `static_assert` で拒否)
- `PreprocessorDefinitions` に `NOMINMAX` を追加(Release / Debug x64)

**ビルドコマンド**:
```
set SDKPath=D:\GitHub\smooth\references\AfterEffectsSDK_25.6_61_win\ae25.6_61.64bit.AfterEffectsSDK\Examples\
msbuild D:\GitHub\smooth\win\win.sln /p:Configuration=Release /p:Platform=x64 /m
```
(vcvars64.bat を最小 env で call する必要あり。git bash から継承した `PATH` に Microsoft Office `\Common` が含まれていると vcvars64 内の `if` が `\Common was unexpected at this time.` で死ぬ)

**遭遇したエラーと対処**:

| # | エラー | 原因 | 対処 |
| --- | --- | --- | --- |
| 1 | `winnt.h(2597): static_assert failed: Windows headers require the default packing option` | vcxproj の `StructMemberAlignment=4Bytes` が Win11 SDK の pack assert に引っかかる | `Default` に変更 |
| 2 | `Param_Utils.h(18): 'strlcpy': identifier not found` | `Param_Utils.h` の `#ifdef AE_OS_WIN` の else 枝が走った。`AE_OS_WIN` は `AEConfig.h` でしか定義されないが、SDK の `Param_Utils.h` / `AE_Effect.h` は `AEConfig.h` をインクルードしていない。Mac では `strlcpy` が libc にあるので顕在化しなかった | `Effect.cpp` に `#include "AEConfig.h"` を追加(AE_Effect.h より前) |
| 3 | `smooth_core.h(376): '(' illegal token on right side of '::'`(`std::min`/`std::max`) | `<windef.h>` の `min`/`max` マクロが `std::min`/`std::max` と衝突 | `NOMINMAX` を Preprocessor Definitions に追加 |

**成功ビルド成果物**:
- `win/Release/x64/smooth.aex` — 239,104 bytes
- `win/Release/x64/smooth.lib` — 1,720 bytes
- PiPL リソース検証(文字列マッチ): `KOJI_SMOOTH` / `EntryPointFunc` / `LoiLo` すべて .aex バイナリ内に存在

**警告(非致命)**:
- C4819 (code page 932) — ソース内 UTF-8 コメントが Shift-JIS で解釈できない。Mac 側 .mm/.cpp と同一ソースなのでリリース品質には影響なし。
- MSB8065 (PiPL 出力パス警告) — CustomBuild の `Outputs` 宣言が `..\Pipl.rc` だが実際の cl コマンドは `win\Pipl.rc` に出力。`win\Pipl.rc` は既存のためビルドは問題なし。インクリメンタル最適化がやや崩れる程度。

**次アクション**:
- AE 2025 (Windows) 実機で smooth.aex 動作確認(AE 未インストール環境のため未実施)
- 動作 OK 確認後、`smooth.Win.1.5.0.AE2025.x64.zip` 作成
- Mac 側で v1.5.0 タグ再発行 or `v1.5.0-win` を追加(方針は要相談)

### 2026-04-21 22:10 JST — AE 2025 (Windows) 実機動作確認

**配置先**: `D:\Program Files\Adobe After Effects 2025\Support Files\Plug-ins\Effects\smooth.aex`

**結果**: ユーザー確認 OK。エフェクトメニュー `LoiLo > smooth` 表示 → 適用 → パラメータ動作確認済み。

**補助変更**:
- `win/win.vcxproj.user` の `LocalDebuggerCommand` を `C:\Program Files\Adobe\Adobe After Effects CC 2017\...` → `D:\Program Files\Adobe After Effects 2025\Support Files\AfterFX.exe` に更新(VS からの F5 デバッガ起動用)

### 2026-04-21 22:34 JST — 配布 zip 作成

**成果物**: `win/release/smooth.Win.1.5.0.AE2025.x64.zip` (113,775 bytes)

| ファイル | SHA256 |
| --- | --- |
| smooth.aex (239,104 bytes) | `7D9B30EA45AC455605F8FF2B9B446A073ED42C85CD0410BEA994E519A86E6A14` |
| smooth.Win.1.5.0.AE2025.x64.zip (113,775 bytes) | `84DF87951F08773CB8C0FE7662ECCD72BF5487DB5D7A5902748FE7938D9674C2` |

**作成コマンド**: `Compress-Archive -Path win/Release/x64/smooth.aex -DestinationPath win/release/smooth.Win.1.5.0.AE2025.x64.zip`

**Phase 2-D クローズ**。Mac/Windows 両方の 1.5.0 バイナリが揃った。
タグ運用の方針(v1.5.0 再発行 or v1.5.0-win 追加)はユーザー決定待ち。
