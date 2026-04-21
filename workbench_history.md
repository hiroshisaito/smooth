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
